use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, Mutex},
};

use libxdp_sys::xsk_ring_prod__needs_wakeup;

use crate::error::CamelliaError;

use super::{
    base::{CompletionQueue, FillQueue, UMem},
    frame::{AppFrame, Chunk},
    libxdp::{populate_fill_ring, recycle_compeletion_ring},
    AccessorRef,
};

#[derive(Debug)]
pub struct SharedAccessor {
    shared_umem: Arc<Mutex<UMem>>,
    cached_chunks: Vec<Chunk>,
    filled_chunks: HashMap<u64, Chunk>,
    tx_chunks: HashMap<u64, Chunk>,
    fill: Pin<Box<FillQueue>>,
    completion: Pin<Box<CompletionQueue>>,
    chunk_size: u32,
}

const SHARED_UMEM_DEFAULT_CHUNK_SIZE: usize = 128;

impl SharedAccessor {
    pub fn new(
        shared_umem: Arc<Mutex<UMem>>,
        fill: Pin<Box<FillQueue>>,
        completion: Pin<Box<CompletionQueue>>,
    ) -> Result<SharedAccessor, CamelliaError> {
        let chunk_size = shared_umem.lock().unwrap().chunk_size;
        Ok(Self {
            shared_umem,
            cached_chunks: Vec::new(),
            filled_chunks: HashMap::new(),
            tx_chunks: HashMap::new(),
            fill,
            completion,
            chunk_size,
        })
    }
    fn pre_alloc(&mut self, n: usize) -> Result<(), CamelliaError> {
        if self.cached_chunks.len() < n {
            self.cached_chunks.append(
                &mut self
                    .shared_umem
                    .lock()
                    .unwrap()
                    .allocate(SHARED_UMEM_DEFAULT_CHUNK_SIZE / 2 + n - self.cached_chunks.len())?,
            )
        }
        Ok(())
    }

    fn after_free(&mut self) {
        if self.cached_chunks.len() > SHARED_UMEM_DEFAULT_CHUNK_SIZE {
            self.shared_umem.lock().unwrap().free(
                self.cached_chunks
                    .drain(0..SHARED_UMEM_DEFAULT_CHUNK_SIZE / 2),
            );
        }
    }

    fn free(&mut self, chunk: Chunk) {
        self.cached_chunks.push(chunk);
        self.after_free();
    }

    fn fill(&mut self, n: usize) -> Result<usize, CamelliaError> {
        self.pre_alloc(n)?;

        let populated = populate_fill_ring(
            &mut self.fill.0,
            n,
            &mut self.cached_chunks,
            &mut self.filled_chunks,
        );
        // chunks may not be consumed if there is no enough room in the free ring,
        // check whether we need to return them to the shared pool
        self.after_free();
        Ok(populated)
    }

    fn recycle(&mut self) -> Result<usize, CamelliaError> {
        let recycled = recycle_compeletion_ring(
            &mut self.completion.0,
            self.tx_chunks.len(),
            self.chunk_size,
            &mut self.cached_chunks,
            &mut self.tx_chunks,
        );
        self.after_free();
        Ok(recycled)
    }

    pub fn extract_recv(&mut self, xdp_addr: u64) -> Chunk {
        // TODO(minhuw): add a helper function to get chunk identifier
        // from xdp address, will be different in aligned and unaligned
        // moode.
        let base_address = xdp_addr - (xdp_addr % (self.chunk_size as u64));
        self.filled_chunks.remove(&base_address).unwrap()
    }

    pub fn register_send(&mut self, chunk: Chunk) {
        self.tx_chunks.insert(chunk.xdp_address() as u64, chunk);
    }
}

pub type SharedAccessorRef = Arc<Mutex<SharedAccessor>>;

impl AccessorRef for SharedAccessorRef {
    type UMemRef = Arc<Mutex<UMem>>;
    fn allocate(&self, n: usize) -> Result<Vec<AppFrame<Self>>, CamelliaError> {
        let mut shared_umem = self.lock().unwrap();
        shared_umem.pre_alloc(n)?;

        Ok(shared_umem
            .cached_chunks
            .drain(0..n)
            .map(|chunk| AppFrame::from_chunk(chunk, self.clone()))
            .collect())
    }

    fn equal(&self, other: &Self) -> bool {
        // We compare address of SharedUMem instead of SharedUMemNode
        Arc::ptr_eq(self, other)
            || Arc::ptr_eq(
                &self.lock().unwrap().shared_umem,
                &other.lock().unwrap().shared_umem,
            )
    }

    fn fill(&self, n: usize) -> Result<usize, CamelliaError> {
        self.lock().unwrap().fill(n)
    }

    fn free(&self, chunk: Chunk) {
        self.lock().unwrap().free(chunk)
    }

    fn extract_recv(&self, xdp_addr: u64) -> Chunk {
        self.lock().unwrap().extract_recv(xdp_addr)
    }

    fn register_send(&self, chunk: Chunk) {
        self.lock().unwrap().register_send(chunk)
    }

    fn inner(&self) -> usize {
        self.lock().unwrap().shared_umem.lock().unwrap().inner() as usize
    }

    fn need_wakeup(&self) -> bool {
        unsafe { xsk_ring_prod__needs_wakeup(&self.lock().unwrap().fill.0) != 0 }
    }

    fn recycle(&self) -> Result<usize, CamelliaError> {
        self.lock().unwrap().recycle()
    }
}
