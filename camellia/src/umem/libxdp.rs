use std::{
    cmp::min,
    collections::HashMap,
    os::fd::{AsRawFd, BorrowedFd},
};

use libc::{recvfrom, sendto, MSG_DONTWAIT};
use libxdp_sys::{
    xsk_ring_cons, xsk_ring_cons__comp_addr, xsk_ring_cons__peek, xsk_ring_cons__release,
    xsk_ring_prod, xsk_ring_prod__fill_addr, xsk_ring_prod__needs_wakeup, xsk_ring_prod__reserve,
    xsk_ring_prod__submit,
};
use nix::poll::{poll, PollFd};
use nix::{errno::Errno, poll::PollTimeout};

use crate::error::CamelliaError;

use super::frame::Chunk;

pub fn populate_fill_ring(
    ring: &mut xsk_ring_prod,
    n: usize,
    chunks: &mut Vec<Chunk>,
    filled_chunks: &mut HashMap<u64, Chunk>,
) -> usize {
    let mut start_index = 0;
    let reserved = unsafe { xsk_ring_prod__reserve(ring, n as u32, &mut start_index) };
    let actual_filled = min(chunks.len(), reserved as usize);

    for (fill_index, chunk) in chunks.drain(0..actual_filled).enumerate() {
        unsafe {
            let fill_addr = xsk_ring_prod__fill_addr(ring, start_index + fill_index as u32);
            *fill_addr = chunk.xdp_address() as u64;
        }

        filled_chunks.insert(chunk.xdp_address() as u64, chunk);
    }

    unsafe {
        xsk_ring_prod__submit(ring, actual_filled as u32);
    }

    actual_filled
}

pub fn recycle_compeletion_ring(
    ring: &mut xsk_ring_cons,
    n: usize,
    chunk_size: u32,
    chunks: &mut Vec<Chunk>,
    tx_chunks: &mut HashMap<u64, Chunk>,
) -> usize {
    let mut start_index = 0;
    let completed = unsafe { xsk_ring_cons__peek(ring, n as u32, &mut start_index) };

    for complete_index in 0..completed {
        let xdp_addr = unsafe { *xsk_ring_cons__comp_addr(ring, start_index + complete_index) };
        let base_address = xdp_addr - (xdp_addr % chunk_size as u64);
        chunks.push(tx_chunks.remove(&base_address).unwrap());
    }

    unsafe {
        xsk_ring_cons__release(ring, completed);
    }

    completed as usize
}

pub fn wakeup_rx(fd: BorrowedFd) -> Result<(), CamelliaError> {
    unsafe {
        Errno::result(recvfrom(
            fd.as_raw_fd(),
            std::ptr::null_mut(),
            0,
            MSG_DONTWAIT,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ))?;
    }
    Ok(())
}

pub fn need_wakeup(ring: &xsk_ring_prod) -> bool {
    unsafe { xsk_ring_prod__needs_wakeup(ring) != 0 }
}

pub fn wakeup_tx(fd: BorrowedFd) -> Result<(), CamelliaError> {
    unsafe {
        Errno::result(sendto(
            fd.as_raw_fd(),
            std::ptr::null(),
            0,
            MSG_DONTWAIT,
            std::ptr::null(),
            0,
        ))
        .or_else(|e| match e {
            Errno::EAGAIN | Errno::EBUSY | Errno::ENETDOWN | Errno::ENOBUFS => Ok(0),
            _ => Err(e),
        })?;
    }
    Ok(())
}

pub fn wakeup_rxtx(fd: BorrowedFd) -> Result<(), CamelliaError> {
    let mut fds = [PollFd::new(fd, nix::poll::PollFlags::POLLOUT)];
    poll(&mut fds, PollTimeout::ZERO)?;
    Ok(())
}

pub fn wakeup_fill_if_necessary(
    ring: &mut xsk_ring_prod,
    xsk_fd: BorrowedFd,
) -> Result<(), CamelliaError> {
    unsafe {
        if xsk_ring_prod__needs_wakeup(ring) != 0 {
            wakeup_rx(xsk_fd)?;
        }
    }
    Ok(())
}

pub fn wakeup_tx_if_necessary(
    ring: &mut xsk_ring_prod,
    xsk_fd: BorrowedFd,
) -> Result<(), CamelliaError> {
    unsafe {
        if xsk_ring_prod__needs_wakeup(ring) != 0 {
            wakeup_tx(xsk_fd)?;
        }
    }
    Ok(())
}
