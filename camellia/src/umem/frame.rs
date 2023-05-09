use std::cell::RefCell;
use std::cmp::min;
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::sync::Arc;

use libc::{c_void, recvfrom, MSG_DONTWAIT};
use libxdp_sys::{
    xsk_ring_cons, xsk_ring_cons__comp_addr, xsk_ring_cons__peek, xsk_ring_cons__release,
    xsk_ring_prod, xsk_ring_prod__fill_addr, xsk_ring_prod__needs_wakeup, xsk_ring_prod__reserve,
    xsk_ring_prod__submit, xsk_umem, xsk_umem__create, xsk_umem__delete, xsk_umem__fd,
    xsk_umem_config, XSK_RING_CONS__DEFAULT_NUM_DESCS, XSK_RING_PROD__DEFAULT_NUM_DESCS,
    XSK_UMEM__DEFAULT_FRAME_HEADROOM, XSK_UMEM__DEFAULT_FRAME_SIZE,
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

pub struct Frame {
    chunk: Option<Chunk>,
    umem: Rc<RefCell<UMem>>,
    offset: usize,
    len: usize,
}

impl Drop for Frame {
    fn drop(&mut self) {
        // panic if RxFrame still contains a chunk
        if let Some(chunk) = self.chunk.take() {
            self.umem.borrow_mut().free(chunk);
        }
    }
}

impl Frame {
    pub fn raw_buffer(&self) -> &[u8] {
        let chunk = self.chunk.as_ref().unwrap();
        let base_address = chunk.address() + self.offset;
        unsafe { std::slice::from_raw_parts(base_address as *const u8, self.len) }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn xdp_address(&self) -> usize {
        self.chunk.as_ref().unwrap().xdp_address() + self.offset
    }

    pub fn raw_buffer_mut(&mut self) -> &mut [u8] {
        let chunk = self.chunk.as_ref().unwrap();
        let base_address = chunk.address() + self.offset;
        unsafe { std::slice::from_raw_parts_mut(base_address as *mut u8, self.len) }
    }

    pub fn raw_buffer_resize(&mut self, size: usize) -> Result<&mut [u8], CamelliaError> {
        let chunk = self.chunk.as_ref().unwrap();

        if size > chunk.size {
            return Err(CamelliaError::InvalidArgument(format!(
                "request size {} is larger than chunk size {}",
                size, chunk.size
            )));
        }
        self.len = size;
        let base_address = chunk.address();
        Ok(unsafe { std::slice::from_raw_parts_mut(base_address as *mut u8, size) })
    }

    pub fn raw_buffer_append(&mut self, size: usize) -> Result<&mut [u8], CamelliaError> {
        let chunk = self.chunk.as_ref().unwrap();
        if self.len + size > chunk.size {
            return Err(CamelliaError::InvalidArgument(format!(
                "request size {} is larger than available size (total: {}, used: {})",
                size, chunk.size, self.len
            )));
        }
        let base_address = chunk.address() + self.len;
        self.len += size;
        Ok(unsafe { std::slice::from_raw_parts_mut(base_address as *mut u8, size) })
    }

    pub fn take_chunk(mut self) -> Chunk {
        self.chunk.take().unwrap()
    }
}

pub struct RxFrame(Frame);
pub struct TxFrame(Frame);
pub struct AppFrame(Frame);

impl AppFrame {
    fn from_chunk(chunk: Chunk, umem: Rc<RefCell<UMem>>) -> Self {
        AppFrame(Frame {
            chunk: Some(chunk),
            offset: 0,
            len: 0,
            umem,
        })
    }

    pub fn raw_buffer(&self) -> &[u8] {
        self.0.raw_buffer()
    }

    pub fn raw_buffer_mut(&mut self) -> &mut [u8] {
        self.0.raw_buffer_mut()
    }

    pub fn raw_buffer_resize(&mut self, size: usize) -> Result<&mut [u8], CamelliaError> {
        self.0.raw_buffer_resize(size)
    }

    pub fn raw_buffer_append(&mut self, size: usize) -> Result<&mut [u8], CamelliaError> {
        self.0.raw_buffer_append(size)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl RxFrame {
    pub fn from_chunk(
        chunk: Chunk,
        umem: Rc<RefCell<UMem>>,
        xdp_addr: usize,
        xdp_len: usize,
    ) -> Self {
        if !chunk.is_xdp_array_valid(xdp_addr, xdp_len) {
            panic!(
                "{}",
                format!(
                    "invalid xdp address: {} or length: {} for chunk: {:?}",
                    xdp_addr, xdp_len, chunk
                )
            )
        }

        RxFrame(Frame {
            offset: xdp_addr - chunk.xdp_address(),
            chunk: Some(chunk),
            umem,
            len: xdp_len,
        })
    }

    pub fn raw_buffer(&self) -> &[u8] {
        self.0.raw_buffer()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl TxFrame {
    pub fn from_chunk(chunk: Chunk, umem: Rc<RefCell<UMem>>) -> Self {
        TxFrame(Frame {
            chunk: Some(chunk),
            umem,
            offset: 0,
            len: 0,
        })
    }

    pub fn xdp_address(&self) -> usize {
        self.0.xdp_address()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<AppFrame> for TxFrame {
    fn from(app_frame: AppFrame) -> Self {
        TxFrame(app_frame.0)
    }
}

impl From<RxFrame> for TxFrame {
    fn from(rx_frame: RxFrame) -> Self {
        TxFrame(rx_frame.0)
    }
}

impl From<RxFrame> for AppFrame {
    fn from(rx_frame: RxFrame) -> Self {
        AppFrame(rx_frame.0)
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

pub struct UMemBuilder {
    chunk_size: u32,
    num_chunks: Option<u32>,
    frame_headroom: u32,
    fill_queue_size: u32,
    completion_queue_size: u32,
}

impl Default for UMemBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl UMemBuilder {
    pub fn new() -> Self {
        UMemBuilder {
            chunk_size: XSK_UMEM__DEFAULT_FRAME_SIZE,
            num_chunks: None,
            frame_headroom: XSK_UMEM__DEFAULT_FRAME_HEADROOM,
            fill_queue_size: XSK_RING_PROD__DEFAULT_NUM_DESCS,
            completion_queue_size: XSK_RING_CONS__DEFAULT_NUM_DESCS,
        }
    }

    pub fn chunk_size(mut self, chunk_size: u32) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    pub fn num_chunks(mut self, num_chunks: u32) -> Self {
        self.num_chunks.replace(num_chunks);
        self
    }

    pub fn frame_headroom(mut self, frame_headroom: u32) -> Self {
        self.frame_headroom = frame_headroom;
        self
    }

    pub fn fill_queue_size(mut self, fill_queue_size: u32) -> Self {
        self.fill_queue_size = fill_queue_size;
        self
    }

    pub fn completion_queue_size(mut self, completion_queue_size: u32) -> Self {
        self.completion_queue_size = completion_queue_size;
        self
    }

    pub fn build(self) -> Result<UMem, CamelliaError> {
        if self.num_chunks.is_none() {
            return Err(CamelliaError::InvalidArgument(
                "number of chunks must be specified".to_string(),
            ));
        }

        let xsk_config = xsk_umem_config {
            frame_size: self.chunk_size,
            frame_headroom: self.frame_headroom,
            fill_size: self.fill_queue_size,
            comp_size: self.completion_queue_size,
            flags: 0,
        };

        UMem::new(self.chunk_size, self.num_chunks.unwrap(), xsk_config)
    }
}

#[derive(Debug)]
pub struct UMem {
    area: Arc<MMapArea>,
    chunks: Vec<Chunk>,
    filled_chunks: HashMap<u64, Chunk>,
    tx_chunks: HashMap<u64, Chunk>,
    fill: FillQueue,
    completion: CompletionQueue,
    chunk_size: u32,
    _num_chunks: u32,
    inner: *mut xsk_umem,
}

impl UMem {
    fn new(
        chunk_size: u32,
        num_chunks: u32,
        config: xsk_umem_config,
    ) -> Result<Self, CamelliaError> {
        let mmap_size = chunk_size * num_chunks;
        let mut umem_inner: *mut xsk_umem = std::ptr::null_mut();
        let area = Arc::new(MMapArea::new((chunk_size * num_chunks) as usize)?);
        let mut fill_queue = MaybeUninit::<FillQueue>::zeroed();
        let mut completion_queue = MaybeUninit::<CompletionQueue>::zeroed();

        unsafe {
            match xsk_umem__create(
                &mut umem_inner,
                area.base_address() as *mut c_void,
                mmap_size as u64,
                &mut (*fill_queue.as_mut_ptr()).inner,
                &mut (*completion_queue.as_mut_ptr()).inner,
                &config,
            ) {
                0 => {}
                errno => return Err(Errno::from_i32(-errno).into()),
            }
        }

        let mut umem = UMem {
            area,
            chunks: Vec::new(),
            fill: unsafe { fill_queue.assume_init() },
            completion: unsafe { completion_queue.assume_init() },
            filled_chunks: HashMap::new(),
            tx_chunks: HashMap::new(),
            chunk_size,
            _num_chunks: num_chunks,
            inner: umem_inner,
        };
        for i in 0..num_chunks {
            umem.chunks.push(Chunk {
                xdp_address: (i * chunk_size) as usize,
                size: chunk_size as usize,
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
            unsafe { xsk_ring_prod__reserve(&mut self.fill.inner, n as u32, &mut start_index) };

        let actual_filled = min(self.chunks.len(), reserved as usize);

        for (fill_index, chunk) in self.chunks.drain(0..actual_filled).enumerate() {
            unsafe {
                *xsk_ring_prod__fill_addr(&mut self.fill.inner, start_index + fill_index as u32) =
                    chunk.xdp_address() as u64;
            }
            self.filled_chunks.insert(chunk.xdp_address() as u64, chunk);
        }

        unsafe {
            xsk_ring_prod__submit(&mut self.fill.inner, actual_filled as u32);
        }

        unsafe {
            if xsk_ring_prod__needs_wakeup(&self.fill.inner) > 0 {
                Errno::result(recvfrom(
                    self.as_raw_fd(),
                    std::ptr::null_mut(),
                    0,
                    MSG_DONTWAIT,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ))?;
            }
        }

        Ok(actual_filled)
    }

    pub fn allocate(
        umem_rc: &mut Rc<RefCell<Self>>,
        n: usize,
    ) -> Result<Vec<AppFrame>, CamelliaError> {
        let mut umem = umem_rc.borrow_mut();
        if umem.chunks.len() < n {
            return Err(CamelliaError::ResourceExhausted(format!(
                "request {} frames, but only {} frames are available",
                n,
                umem.chunks.len()
            )));
        }

        Ok(umem
            .chunks
            .drain(0..n)
            .map(|chunk| AppFrame::from_chunk(chunk, umem_rc.clone()))
            .collect())
    }

    pub fn free(&mut self, chunk: Chunk) {
        self.chunks.push(chunk);
    }

    pub fn recycle(&mut self) {
        let mut start_index = 0;
        let completed = unsafe {
            xsk_ring_cons__peek(
                &mut self.completion.inner,
                self.tx_chunks.len() as u32,
                &mut start_index,
            )
        };

        for complete_index in 0..completed {
            let xdp_addr = unsafe {
                *xsk_ring_cons__comp_addr(&self.completion.inner, start_index + complete_index)
            };

            self.chunks.push(self.tx_chunks.remove(&xdp_addr).unwrap());
        }
        unsafe {
            xsk_ring_cons__release(&mut self.completion.inner, completed);
        }
    }

    pub fn extract_recv(&mut self, xdp_addr: u64) -> Chunk {
        let base_address = xdp_addr - (xdp_addr % (self.chunk_size as u64));
        // The chunk must be filled before

        self.filled_chunks.remove(&base_address).unwrap()
    }

    pub fn register_send(&mut self, frame: TxFrame) {
        self.tx_chunks
            .insert(frame.xdp_address() as u64, frame.0.take_chunk());
    }
}

impl Drop for UMem {
    fn drop(&mut self) {
        let errno = unsafe { xsk_umem__delete(self.inner) };
        if errno < 0 {
            eprintln!("failed to delete xsk umem: {}", Errno::from_i32(-errno));
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
    use std::{ffi::CStr, io::Write};

    use super::*;

    #[test]
    fn test_umem_create() {
        let umem = UMemBuilder::new().num_chunks(1024).build().unwrap();
        assert_eq!(umem.chunks.len(), 1024);
    }

    #[test]
    fn test_frame_allocate() {
        let mut umem = Rc::new(RefCell::new(
            UMemBuilder::new().num_chunks(1024).build().unwrap(),
        ));
        let umem_clone = umem.clone();
        let frames = UMem::allocate(&mut umem, 1024).unwrap();
        assert_eq!(frames.len(), 1024);
        assert_eq!(umem_clone.borrow_mut().chunks.len(), 0);
    }

    #[test]
    fn test_frame_write() {
        let mut umem = Rc::new(RefCell::new(
            UMemBuilder::new().num_chunks(1024).build().unwrap(),
        ));
        let mut frame = UMem::allocate(&mut umem, 1).unwrap().pop().unwrap();
        let mut buffer = frame.raw_buffer_append(1024).unwrap();

        assert_eq!(buffer.len(), 1024);

        buffer.write(b"hello, world\0").unwrap();

        let chunk = frame.0.chunk.as_ref().unwrap();
        unsafe {
            assert_eq!(
                CStr::from_ptr(chunk.address() as *const i8),
                CStr::from_bytes_with_nul_unchecked(b"hello, world\0")
            );
        }
    }
}
