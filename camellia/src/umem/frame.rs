use std::cmp::min;
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::sync::Arc;

use libc::c_void;
use libxdp_sys::{
    xsk_ring_cons, xsk_ring_cons__comp_addr, xsk_ring_cons__peek, xsk_ring_prod,
    xsk_ring_prod__fill_addr, xsk_ring_prod__reserve, xsk_ring_prod__submit, xsk_umem,
    xsk_umem__create, xsk_umem__delete, xsk_umem__fd,
};
use nix::errno::Errno;

use crate::error::CamelliaError;
use crate::umem::mmap::MMapArea;

#[derive(Debug)]
pub struct Chunk {
    // xdp_address is the offset in XDP UMem, not a valid virtual address
    // valid virtual address = mmap_area.base_address + xdp_address
    xdp_address: usize,
    // size of the chunk
    size: usize,
    // mmaped memory region backing this chunk
    mmap_area: Arc<MMapArea>,
}

impl Chunk {
    pub fn xdp_address(&self) -> usize {
        self.xdp_address
    }

    pub fn address(&self) -> usize {
        self.mmap_area.as_ref().base_address() + self.xdp_address
    }

    pub fn is_xdp_addr_valid(&self, xdp_address: usize) -> bool {
        (xdp_address >= self.xdp_address) && (xdp_address < self.xdp_address + self.size)
    }

    pub fn is_xdp_array_valid(&self, xdp_address: usize, len: usize) -> bool {
        (xdp_address >= self.xdp_address) && (xdp_address + len <= self.xdp_address + self.size)
    }

    pub fn is_addr_valid(&self, address: usize) -> bool {
        let base_address = self.mmap_area.as_ref().base_address();
        (address >= (base_address + self.xdp_address))
            && (address < (base_address + self.xdp_address + self.size))
    }

    pub fn is_array_valid(&self, address: usize, len: usize) -> bool {
        let base_address = self.mmap_area.as_ref().base_address();
        (address >= (base_address + self.xdp_address))
            && (address + len <= (base_address + self.xdp_address + self.size))
    }

    pub fn xdp_to_addr(&self, xdp_address: usize) -> usize {
        if !self.is_xdp_addr_valid(xdp_address) {
            panic!("invalid xdp address: {} for chunk: {:?}", xdp_address, self)
        }

        self.mmap_area.as_ref().base_address() + xdp_address
    }
}

pub struct FrameReceive<'a> {
    _chunk: Chunk,
    raw_buffer: &'a [u8],
}

pub struct FrameSend<'a> {
    chunk: Chunk,
    raw_buffer: &'a mut [u8],
    len: usize,
}

pub enum Frame<'a> {
    Receive(FrameReceive<'a>),
    Send(FrameSend<'a>),
}

impl<'a> FrameReceive<'a> {
    pub fn from_chunk(chunk: Chunk, xdp_addr: usize, xdp_len: usize) -> Self {
        if !chunk.is_xdp_array_valid(xdp_addr, xdp_len) {
            panic!(
                "{}",
                format!(
                    "invalid xdp address: {} or length: {} for chunk: {:?}",
                    xdp_addr, xdp_len, chunk
                )
            )
        }

        let array_address = chunk.xdp_to_addr(xdp_addr);
        FrameReceive {
            _chunk: chunk,
            raw_buffer: unsafe { std::slice::from_raw_parts(array_address as *const u8, xdp_len) },
        }
    }

    pub fn raw_buffer(&self) -> &[u8] {
        self.raw_buffer
    }
}

impl<'a> FrameSend<'a> {
    pub fn from_chunk(chunk: Chunk) -> Self {
        let base_address = chunk.address();
        let size = chunk.size;
        FrameSend {
            chunk,
            raw_buffer: unsafe { std::slice::from_raw_parts_mut(base_address as *mut u8, size) },
            len: 0,
        }
    }

    pub fn append_buffer(&'a mut self, size: usize) -> Result<&'a mut [u8], CamelliaError> {
        if (self.len + size) > self.chunk.size {
            return Err(CamelliaError::InvalidArgument(format!(
                "buffer size is exhausted, request: {}, total: {}, allocated: {}",
                size, self.chunk.size, self.len
            )));
        }
        let raw_buffer = &mut self.raw_buffer[self.len..self.len + size];
        self.len += size;
        Ok(raw_buffer)
    }

    pub fn xdp_address(&self) -> usize {
        self.chunk.xdp_address()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn chunk(self) -> Chunk {
        self.chunk
    }
}

#[derive(Debug)]
pub struct FillQueue {
    inner: xsk_ring_prod,
}

#[derive(Debug)]
pub struct CompletionQueue {
    inner: xsk_ring_cons,
}

#[derive(Debug)]
pub struct UMem {
    area: Arc<MMapArea>,
    chunks: Vec<Chunk>,
    filled_chunks: HashMap<u64, Chunk>,
    tx_chunks: HashMap<u64, Chunk>,
    _fill: FillQueue,
    _completion: CompletionQueue,
    inner: *mut xsk_umem,
}

impl UMem {
    pub fn new(chunk_size: usize, chunks: usize) -> Result<Self, CamelliaError> {
        let mmap_size = chunk_size * chunks;
        let mut umem_inner: *mut xsk_umem = std::ptr::null_mut();
        let area = Arc::new(MMapArea::new(chunk_size * chunks)?);
        let mut fill_queue = MaybeUninit::<FillQueue>::zeroed();
        let mut completion_queue = MaybeUninit::<CompletionQueue>::zeroed();

        unsafe {
            Errno::result(xsk_umem__create(
                &mut umem_inner,
                area.base_address() as *mut c_void,
                mmap_size as u64,
                &mut (*fill_queue.as_mut_ptr()).inner,
                &mut (*completion_queue.as_mut_ptr()).inner,
                std::ptr::null(),
            ))?;
        }

        let mut umem = UMem {
            area,
            chunks: Vec::new(),
            _fill: unsafe { fill_queue.assume_init() },
            _completion: unsafe { completion_queue.assume_init() },
            filled_chunks: HashMap::new(),
            tx_chunks: HashMap::new(),
            inner: std::ptr::null_mut(),
        };
        for i in 0..chunks {
            umem.chunks.push(Chunk {
                xdp_address: i * chunk_size,
                size: chunk_size,
                mmap_area: umem.area.clone(),
            });
        }

        Ok(umem)
    }

    pub fn inner(&mut self) -> *mut xsk_umem {
        self.inner
    }

    pub fn fill(&mut self, n: usize) -> Result<usize, CamelliaError> {
        let mut start_index = 0;
        let reserved =
            unsafe { xsk_ring_prod__reserve(&mut self._fill.inner, n as u32, &mut start_index) };

        let actual_filled = min(self.chunks.len(), reserved as usize);

        for (fill_index, chunk) in self.chunks.drain(0..actual_filled).enumerate() {
            unsafe {
                *xsk_ring_prod__fill_addr(&mut self._fill.inner, start_index + fill_index as u32) =
                    chunk.xdp_address() as u64;
            }
            start_index += 1;
            self.filled_chunks.insert(chunk.address() as u64, chunk);
        }

        unsafe {
            xsk_ring_prod__submit(&mut self._fill.inner, actual_filled as u32);
        }

        Ok(actual_filled)
    }

    pub fn allocate(&mut self, frames: &mut [FrameSend]) -> Result<usize, CamelliaError> {
        if self.chunks.len() < frames.len() {
            return Err(CamelliaError::ResourceExhausted(format!(
                "request {} frames, but only {} frames are available",
                frames.len(),
                self.chunks.len()
            )));
        }

        self.chunks
            .drain(0..frames.len())
            .enumerate()
            .for_each(|(i, chunk)| {
                frames[i] = FrameSend::from_chunk(chunk);
            });

        Ok(frames.len())
    }

    pub fn recycle(&mut self) {
        let mut start_index = 0;
        let completed = unsafe {
            xsk_ring_cons__peek(
                &mut self._completion.inner,
                self.tx_chunks.len() as u32,
                &mut start_index,
            )
        };

        for complete_index in 0..completed {
            let xdp_addr = unsafe {
                *xsk_ring_cons__comp_addr(&self._completion.inner, start_index + complete_index)
            };

            self.chunks.push(self.tx_chunks.remove(&xdp_addr).unwrap());
        }
    }

    pub fn extract_recv(&mut self, xdp_addr: u64) -> Chunk {
        // The chunk must be filled before
        self.filled_chunks.remove(&xdp_addr).unwrap()
    }

    pub fn register_send(&mut self, chunk: Chunk) {
        self.tx_chunks.insert(chunk.xdp_address() as u64, chunk);
    }
}

impl Drop for UMem {
    fn drop(&mut self) {
        if let Err(e) = unsafe { Errno::result(xsk_umem__delete(self.inner)) } {
            eprintln!("failed to delete xsk umem: {}", e);
        }
    }
}

impl AsRawFd for UMem {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        unsafe { xsk_umem__fd(self.inner) }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_umem_create() {
        let umem = UMem::new(2048, 1024).unwrap();
        assert_eq!(umem.chunks.len(), 1024);
    }
}
