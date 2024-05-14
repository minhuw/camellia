use std::{
    cell::{Ref, RefCell},
    cmp::min,
    collections::HashMap,
    fmt::Display,
    ops::{AddAssign, SubAssign},
    os::{fd::AsRawFd, raw::c_void},
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex},
};

use libxdp_sys::{
    xsk_ring_cons, xsk_ring_cons__comp_addr, xsk_ring_cons__peek, xsk_ring_cons__release,
    xsk_ring_prod, xsk_umem, xsk_umem__create, xsk_umem__delete, xsk_umem__fd, xsk_umem_config,
    XSK_RING_CONS__DEFAULT_NUM_DESCS, XSK_RING_PROD__DEFAULT_NUM_DESCS,
    XSK_UMEM__DEFAULT_FRAME_HEADROOM, XSK_UMEM__DEFAULT_FRAME_SIZE,
};
use nix::errno::Errno;

use crate::error::CamelliaError;

use super::{
    frame::{AppFrame, Chunk},
    libxdp::populate_fill_ring,
    mmap::MMapArea,
    UMemAccessor,
};

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
pub struct FillQueue(pub xsk_ring_prod);

impl Default for FillQueue {
    fn default() -> Self {
        FillQueue(xsk_ring_prod {
            cached_prod: 0,
            cached_cons: 0,
            mask: 0,
            size: 0,
            producer: std::ptr::null_mut(),
            consumer: std::ptr::null_mut(),
            ring: std::ptr::null_mut(),
            flags: std::ptr::null_mut(),
        })
    }
}

#[derive(Debug)]
pub struct CompletionQueue(pub xsk_ring_cons);

impl Default for CompletionQueue {
    fn default() -> Self {
        CompletionQueue(xsk_ring_cons {
            cached_prod: 0,
            cached_cons: 0,
            mask: 0,
            size: 0,
            producer: std::ptr::null_mut(),
            consumer: std::ptr::null_mut(),
            ring: std::ptr::null_mut(),
            flags: std::ptr::null_mut(),
        })
    }
}

#[derive(Debug)]
pub struct UMem {
    area: Arc<MMapArea>,
    pub chunks: Vec<Chunk>,
    // We need to Pin rings because their addresses are stored in libxdp code
    pub fill: Pin<Box<FillQueue>>,
    pub completion: Pin<Box<CompletionQueue>>,
    pub chunk_size: u32,
    _num_chunks: u32,
    pub inner: *mut xsk_umem,
}

static LOCKED_IO_MEMORY: Mutex<u64> = Mutex::new(0);

impl UMem {
    fn new(
        chunk_size: u32,
        num_chunks: u32,
        config: xsk_umem_config,
    ) -> Result<Self, CamelliaError> {
        let mmap_size = chunk_size * num_chunks;
        let mut umem_inner: *mut xsk_umem = std::ptr::null_mut();
        let area = Arc::new(MMapArea::new((chunk_size * num_chunks) as usize)?);
        let mut fill_queue = Box::pin(FillQueue::default());
        let mut completion_queue = Box::pin(CompletionQueue::default());

        let mut locked_memory = LOCKED_IO_MEMORY.lock().unwrap();
        locked_memory.add_assign(mmap_size as u64);

        rlimit::Resource::MEMLOCK
            .get()
            .and_then(|(soft, hard)| {
                if min(soft, hard) < *locked_memory {
                    rlimit::Resource::MEMLOCK.set(*locked_memory, *locked_memory)
                } else {
                    Ok(())
                }
            })
            .unwrap();

        unsafe {
            match xsk_umem__create(
                &mut umem_inner,
                area.base_address() as *mut c_void,
                mmap_size as u64,
                &mut fill_queue.as_mut().0,
                &mut completion_queue.as_mut().0,
                &config,
            ) {
                0 => {}
                errno => return Err(Errno::from_raw(-errno).into()),
            }
        }

        let mut umem = UMem {
            area,
            chunks: Vec::new(),
            fill: fill_queue,
            completion: completion_queue,
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

    pub fn inner(&self) -> *mut xsk_umem {
        self.inner
    }

    pub fn allocate(&mut self, n: usize) -> Result<Vec<Chunk>, CamelliaError> {
        if self.chunks.len() < n {
            return Err(CamelliaError::InvalidArgument(format!(
                "SharedUMem::allocate: {} chunks requested, but only {} chunks available",
                n,
                self.chunks.len()
            )));
        }
        Ok(self.chunks.drain(0..n).collect())
    }

    pub fn free(&mut self, chunks: impl IntoIterator<Item = Chunk>) {
        self.chunks.extend(chunks);
    }
}

impl Display for UMem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{fill: {:?}, completion: {:?}}}",
            self.fill, self.completion,
        )
    }
}

impl Drop for UMem {
    fn drop(&mut self) {
        let errno = unsafe { xsk_umem__delete(self.inner) };
        if errno < 0 {
            eprintln!("failed to delete xsk umem: {}", Errno::from_raw(-errno));
        }
        let mut locked_memory = LOCKED_IO_MEMORY.lock().unwrap();
        locked_memory.sub_assign(self._num_chunks as u64 * self.chunk_size as u64);
    }
}

impl AsRawFd for UMem {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        unsafe { xsk_umem__fd(self.inner) }
    }
}

#[derive(Debug)]
pub struct DedicatedAccessor {
    filled_chunks: HashMap<u64, Chunk>,
    tx_chunks: HashMap<u64, Chunk>,
    base: UMem,
}

impl DedicatedAccessor {
    pub fn new(base: UMem) -> Result<Self, CamelliaError> {
        let umem = DedicatedAccessor {
            filled_chunks: HashMap::new(),
            tx_chunks: HashMap::new(),
            base,
        };

        Ok(umem)
    }

    pub fn inner(&self) -> *mut xsk_umem {
        self.base.inner()
    }

    pub fn fill(&mut self, n: usize) -> Result<usize, CamelliaError> {
        let actual_filled = populate_fill_ring(
            &mut self.base.fill.0,
            n,
            &mut self.base.chunks,
            &mut self.filled_chunks,
        );
        Ok(actual_filled)
    }

    pub fn allocate(
        umem_rc: &Rc<RefCell<Self>>,
        n: usize,
    ) -> Result<Vec<AppFrame<Self>>, CamelliaError> {
        let mut umem = umem_rc.borrow_mut();
        if umem.base.chunks.len() < n {
            return Err(CamelliaError::ResourceExhausted(format!(
                "request {} frames, but only {} frames are available",
                n,
                umem.base.chunks.len()
            )));
        }

        Ok(umem
            .base
            .chunks
            .drain(0..n)
            .map(|chunk| AppFrame::from_chunk(chunk, umem_rc.clone()))
            .collect())
    }

    pub fn free(&mut self, chunk: Chunk) {
        self.base.chunks.push(chunk);
    }

    pub fn recycle(&mut self) -> Result<usize, CamelliaError> {
        let mut start_index = 0;
        let completed = unsafe {
            xsk_ring_cons__peek(
                &mut self.base.completion.0,
                self.tx_chunks.len() as u32,
                &mut start_index,
            )
        };

        for complete_index in 0..completed {
            let xdp_addr = unsafe {
                *xsk_ring_cons__comp_addr(&self.base.completion.0, start_index + complete_index)
            };

            self.base
                .chunks
                .push(self.tx_chunks.remove(&xdp_addr).unwrap());
        }

        unsafe {
            xsk_ring_cons__release(&mut self.base.completion.0, completed);
        }

        Ok(completed as usize)
    }

    pub fn extract_recv(&mut self, xdp_addr: u64) -> Chunk {
        let base_address = xdp_addr - (xdp_addr % (self.base.chunk_size as u64));
        // The chunk must be filled before

        self.filled_chunks.remove(&base_address).unwrap()
    }

    pub fn register_send(&mut self, chunk: Chunk) {
        self.tx_chunks.insert(chunk.xdp_address() as u64, chunk);
    }
}

impl From<UMem> for Rc<RefCell<DedicatedAccessor>> {
    fn from(value: UMem) -> Self {
        Rc::new(RefCell::new(DedicatedAccessor {
            filled_chunks: HashMap::new(),
            tx_chunks: HashMap::new(),
            base: value,
        }))
    }
}

impl UMemAccessor for DedicatedAccessor {
    type UMemRef = UMem;
    type AccessorRef = Rc<RefCell<DedicatedAccessor>>;

    fn allocate(
        umem_rc: &Self::AccessorRef,
        n: usize,
    ) -> Result<Vec<AppFrame<Self>>, CamelliaError> {
        DedicatedAccessor::allocate(umem_rc, n)
    }

    fn free(umem_rc: &Self::AccessorRef, chunk: Chunk) {
        umem_rc.borrow_mut().free(chunk)
    }

    fn fill(umem_rc: &Self::AccessorRef, n: usize) -> Result<usize, CamelliaError> {
        umem_rc.borrow_mut().fill(n)
    }

    fn fill_inner(umem_rc: &Self::AccessorRef) -> Ref<xsk_ring_prod> {
        Ref::map(umem_rc.borrow(), |umem| &umem.base.fill.0)
    }

    fn recycle(umem_rc: &Self::AccessorRef) -> Result<usize, CamelliaError> {
        umem_rc.borrow_mut().recycle()
    }

    fn extract_recv(umem_rc: &Self::AccessorRef, xdp_addr: u64) -> Chunk {
        umem_rc.borrow_mut().extract_recv(xdp_addr)
    }

    fn equal(umem_rc: &Self::AccessorRef, other: &Self::AccessorRef) -> bool {
        Rc::ptr_eq(umem_rc, other)
    }

    fn register_send(umem_rc: &Self::AccessorRef, chunk: Chunk) {
        umem_rc.borrow_mut().register_send(chunk)
    }

    fn inner(umem_rc: &Self::AccessorRef) -> usize {
        umem_rc.borrow().inner() as usize
    }
}

#[cfg(test)]
mod test {
    use std::{cell::RefCell, ffi::CStr, io::Write, rc::Rc};

    use super::*;

    #[test]
    fn test_umem_create() {
        let umem = UMemBuilder::new().num_chunks(1024).build().unwrap();
        assert_eq!(umem.chunks.len(), 1024);
    }

    #[test]
    fn test_frame_allocate() {
        let mut umem = UMemBuilder::new().num_chunks(1024).build().unwrap();

        let frames = umem.allocate(1024).unwrap();
        assert_eq!(frames.len(), 1024);
        assert_eq!(umem.chunks.len(), 0);
    }

    #[test]
    fn test_frame_write() {
        let umem = UMemBuilder::new().num_chunks(1024).build().unwrap();

        let accessor = Rc::new(RefCell::new(DedicatedAccessor::new(umem).unwrap()));

        let mut frame = DedicatedAccessor::allocate(&accessor, 1)
            .unwrap()
            .pop()
            .unwrap();

        let mut buffer = frame.raw_buffer_append(1024).unwrap();

        assert_eq!(buffer.len(), 1024);

        buffer.write_all(b"hello, world\0").unwrap();

        let chunk = frame.chunk();
        unsafe {
            assert_eq!(
                CStr::from_ptr(chunk.address() as *const i8),
                CStr::from_bytes_with_nul_unchecked(b"hello, world\0")
            );
        }
    }
}
