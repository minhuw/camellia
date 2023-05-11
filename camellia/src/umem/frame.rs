use std::sync::Arc;

use crate::error::CamelliaError;
use crate::umem::mmap::MMapArea;
use crate::umem::UMemAccessor;

#[derive(Debug)]
pub struct Chunk {
    // xdp_address is the offset in XDP UMem, not a valid virtual address
    // valid virtual address = mmap_area.base_address + xdp_address
    pub xdp_address: usize,
    // size of the chunk
    pub size: usize,
    // mmaped memory region backing this chunk
    pub mmap_area: Arc<MMapArea>,
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

pub struct Frame<M>
where
    M: UMemAccessor,
{
    chunk: Option<Chunk>,
    umem: M::AccessorRef,
    offset: usize,
    len: usize,
}

impl<M> Drop for Frame<M>
where
    M: UMemAccessor,
{
    fn drop(&mut self) {
        // panic if RxFrame still contains a chunk
        if let Some(chunk) = self.chunk.take() {
            M::free(&self.umem, chunk);
        }
    }
}

impl<M> Frame<M>
where
    M: UMemAccessor,
{
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

    pub fn umem(&self) -> &M::AccessorRef {
        &self.umem
    }
}

pub struct RxFrame<M: UMemAccessor>(Frame<M>);
pub struct TxFrame<M: UMemAccessor>(Frame<M>);
pub struct AppFrame<M: UMemAccessor>(Frame<M>);

impl<M> AppFrame<M>
where
    M: UMemAccessor,
{
    pub fn from_chunk(chunk: Chunk, umem: M::AccessorRef) -> Self {
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

    pub fn umem(&self) -> &M::AccessorRef {
        self.0.umem()
    }

    pub fn chunk(&self) -> &Chunk {
        self.0.chunk.as_ref().unwrap()
    }
}

impl<M> RxFrame<M>
where
    M: UMemAccessor,
{
    pub fn from_chunk(chunk: Chunk, umem: M::AccessorRef, xdp_addr: usize, xdp_len: usize) -> Self {
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

    pub fn umem(&self) -> &M::AccessorRef {
        self.0.umem()
    }
}

impl<M> TxFrame<M>
where
    M: UMemAccessor,
{
    pub fn from_chunk(chunk: Chunk, umem: M::AccessorRef) -> Self {
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

    pub fn umem(&self) -> &M::AccessorRef {
        self.0.umem()
    }

    pub fn take(self) -> Chunk {
        self.0.take_chunk()
    }
}

impl<M: UMemAccessor> From<AppFrame<M>> for TxFrame<M> {
    fn from(app_frame: AppFrame<M>) -> Self {
        TxFrame(app_frame.0)
    }
}

impl<M: UMemAccessor> From<RxFrame<M>> for TxFrame<M> {
    fn from(rx_frame: RxFrame<M>) -> Self {
        TxFrame(rx_frame.0)
    }
}

impl<M: UMemAccessor> From<RxFrame<M>> for AppFrame<M> {
    fn from(rx_frame: RxFrame<M>) -> Self {
        AppFrame(rx_frame.0)
    }
}
