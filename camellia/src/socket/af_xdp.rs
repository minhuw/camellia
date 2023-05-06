use std::cmp::min;
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;

use libxdp_sys::xsk_ring_cons__peek;
use libxdp_sys::xsk_ring_cons__release;
use libxdp_sys::xsk_ring_cons__rx_desc;
use libxdp_sys::xsk_ring_prod__reserve;
use libxdp_sys::xsk_ring_prod__submit;
use libxdp_sys::xsk_ring_prod__tx_desc;
use libxdp_sys::xsk_socket;
use libxdp_sys::xsk_socket__create;
use libxdp_sys::xsk_socket__delete;
use libxdp_sys::xsk_socket__fd;
use libxdp_sys::{xsk_ring_cons, xsk_ring_prod};
use nix::errno::Errno;

use crate::error::CamelliaError;
use crate::umem::frame::FrameReceive;
use crate::umem::frame::FrameSend;
use crate::umem::frame::UMem;

pub struct RxQueue {
    inner: xsk_ring_cons,
}

pub struct TxQueue {
    inner: xsk_ring_prod,
}

pub struct TxDescriptor {}

pub struct XskSocket<'a> {
    inner: *mut xsk_socket,
    umem: &'a mut UMem,
    rx: RxQueue,
    tx: TxQueue,
}

impl<'a> XskSocket<'a> {
    pub fn new<'b>(
        ifname: &str,
        queue_index: u32,
        umem: &'b mut UMem,
    ) -> Result<Self, CamelliaError>
    where
        'b: 'a,
    {
        let mut raw_socket: *mut xsk_socket = std::ptr::null_mut();
        let mut rx_queue = MaybeUninit::<RxQueue>::zeroed();
        let mut tx_queue = MaybeUninit::<TxQueue>::zeroed();

        let ifname = CString::new(ifname).unwrap();
        unsafe {
            Errno::result(xsk_socket__create(
                &mut raw_socket,
                ifname.as_ptr(),
                queue_index,
                umem.inner(),
                &mut (*rx_queue.as_mut_ptr()).inner,
                &mut (*tx_queue.as_mut_ptr()).inner,
                std::ptr::null(),
            ))?;
        }

        Ok(XskSocket {
            inner: raw_socket,
            umem,
            rx: unsafe { rx_queue.assume_init() },
            tx: unsafe { tx_queue.assume_init() },
        })
    }

    pub fn recv(&mut self) -> Result<Option<FrameReceive>, CamelliaError> {
        let mut frames = Vec::with_capacity(1);

        let received = self.recv_bulk(&mut frames)?;

        assert!(received <= 1);

        if received == 1 {
            Ok(Some(frames.pop().unwrap()))
        } else {
            Ok(None)
        }
    }

    pub fn recv_bulk(&mut self, frames: &mut [FrameReceive]) -> Result<usize, CamelliaError> {
        let mut start_index = 0;
        let received: u32 = unsafe {
            xsk_ring_cons__peek(&mut self.rx.inner, frames.len() as u32, &mut start_index)
        };

        assert!((received as usize) <= frames.len());

        for output_index in 0..received as usize {
            if output_index >= frames.len() {
                break;
            }
            let (addr, len) = unsafe {
                let rx_desp =
                    xsk_ring_cons__rx_desc(&self.rx.inner, start_index + output_index as u32);
                ((*rx_desp).addr, (*rx_desp).len)
            };

            let chunk = self.umem.extract_recv(addr);
            frames[output_index] = FrameReceive::from_chunk(chunk, addr as usize, len as usize);
        }

        unsafe {
            xsk_ring_cons__release(&mut self.rx.inner, received);
        }

        // TODO: add an option controlling whether to fill the umem eagerly
        let filled = self.umem.fill(received as usize)?;

        if filled < (received as usize) {
            log::warn!("fill failed, filled: {}, received: {}", filled, received);
        }

        Ok(received as usize)
    }

    pub fn send(&mut self, frame: FrameSend<'a>) -> Result<Option<FrameSend>, CamelliaError> {
        let mut remaining = self.send_bulk([frame])?;
        assert!(remaining.len() <= 1);

        if remaining.len() == 1 {
            Ok(Some(remaining.pop().unwrap()))
        } else {
            Ok(None)
        }
    }

    pub fn send_bulk<T>(&mut self, frames: T) -> Result<Vec<FrameSend>, CamelliaError>
    where
        T: IntoIterator<Item = FrameSend<'a>>,
        T::IntoIter: ExactSizeIterator,
    {
        let mut start_index = 0;
        let mut remaining = Vec::new();

        self.umem.recycle();

        let iter = frames.into_iter();

        let reserved_desp = unsafe {
            xsk_ring_prod__reserve(&mut self.tx.inner, iter.len() as u32, &mut start_index)
        };

        let actual_sent = min(reserved_desp, iter.len() as u32);

        for (send_index, frame) in iter.enumerate() {
            if (send_index as u32) < actual_sent {
                unsafe {
                    let tx_desc = xsk_ring_prod__tx_desc(
                        &mut self.tx.inner,
                        start_index + (send_index as u32),
                    );
                    (*tx_desc).addr = frame.xdp_address() as u64;
                    (*tx_desc).len = frame.len() as u32;
                    (*tx_desc).options = 0;
                };

                self.umem.register_send(frame.chunk())
            } else {
                remaining.push(frame);
            }
        }

        unsafe {
            xsk_ring_prod__submit(&mut self.tx.inner, actual_sent);
        }

        Ok(remaining)
    }
}

impl Drop for XskSocket<'_> {
    fn drop(&mut self) {
        unsafe { xsk_socket__delete(self.inner) }
    }
}

impl AsRawFd for XskSocket<'_> {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        unsafe { xsk_socket__fd(self.inner) }
    }
}
