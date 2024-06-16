use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};

use libxdp_sys::xsk_ring_prod__needs_wakeup;

use crate::error::CamelliaError;

use super::{
    base::{CompletionQueue, FillQueue, UMem},
    frame::{AppFrame, Chunk},
    libxdp::{populate_fill_ring, recycle_compeletion_ring},
    mmap::MMapArea,
    AccessorRef,
};

#[derive(Debug)]
pub struct SharedAccessor {
    shared_umem: Arc<Mutex<UMem>>,
    umem_id: usize,
    mmap_area: Arc<MMapArea>,
    cached_chunks: Vec<usize>,
    fill: Pin<Box<FillQueue>>,
    completion: Pin<Box<CompletionQueue>>,
    chunk_size: u32,
    tx_issued_num: usize,
}

const SHARED_UMEM_DEFAULT_CHUNK_SIZE: usize = 128;

impl SharedAccessor {
    pub fn new(
        shared_umem: Arc<Mutex<UMem>>,
        fill: Pin<Box<FillQueue>>,
        completion: Pin<Box<CompletionQueue>>,
    ) -> Result<SharedAccessor, CamelliaError> {
        let chunk_size = shared_umem.lock().unwrap().chunk_size;
        let mmap_area = shared_umem.lock().unwrap().area.clone();
        let umem_id = shared_umem.lock().unwrap().inner() as usize;
        Ok(Self {
            shared_umem,
            umem_id,
            mmap_area,
            cached_chunks: Vec::new(),
            fill,
            completion,
            chunk_size,
            tx_issued_num: 0,
        })
    }

    fn pre_alloc(&mut self, n: usize) -> Result<(), CamelliaError> {
        if self.cached_chunks.len() < n {
            self.cached_chunks.append(
                &mut self.shared_umem.lock().unwrap().allocate_raw(
                    SHARED_UMEM_DEFAULT_CHUNK_SIZE / 2 + n - self.cached_chunks.len(),
                )?,
            )
        }
        Ok(())
    }

    fn after_free(&mut self) {
        if self.cached_chunks.len() > SHARED_UMEM_DEFAULT_CHUNK_SIZE {
            self.shared_umem.lock().unwrap().free_raw(
                self.cached_chunks
                    .drain(0..SHARED_UMEM_DEFAULT_CHUNK_SIZE / 2),
            );
        }
    }

    fn free(&mut self, chunk: Chunk) {
        self.cached_chunks.push(chunk.xdp_address);
        self.after_free();
    }

    fn fill(&mut self, n: usize) -> Result<usize, CamelliaError> {
        self.pre_alloc(n)?;

        let populated = populate_fill_ring(&mut self.fill.0, n, &mut self.cached_chunks);
        // chunks may not be consumed if there is no enough room in the free ring,
        // check whether we need to return them to the shared pool
        self.after_free();
        Ok(populated)
    }

    fn recycle(&mut self) -> Result<usize, CamelliaError> {
        let recycled = recycle_compeletion_ring(
            &mut self.completion.0,
            self.tx_issued_num,
            self.chunk_size,
            &mut self.cached_chunks,
        );
        self.tx_issued_num -= recycled;

        self.after_free();
        Ok(recycled)
    }

    pub fn extract_recv(&mut self, xdp_addr: u64) -> Chunk {
        // TODO(minhuw): add a helper function to get chunk identifier
        // from xdp address, will be different in aligned and unaligned
        // moode.
        let base_address = xdp_addr - (xdp_addr % (self.chunk_size as u64));
        Chunk {
            xdp_address: base_address as usize,
            size: self.chunk_size as usize,
            mmap_area: self.mmap_area.clone(),
        }
    }

    pub fn register_send(&mut self, _chunk: Chunk) {
        self.tx_issued_num += 1;
    }
}

#[derive(Clone, Debug)]
pub struct SharedAccessorRef {
    inner: Arc<Mutex<SharedAccessor>>,
    id: usize,
}

impl SharedAccessorRef {
    pub fn new(inner: Arc<Mutex<SharedAccessor>>) -> Self {
        Self {
            inner: inner.clone(),
            id: inner.lock().unwrap().umem_id,
        }
    }
}

impl AccessorRef for SharedAccessorRef {
    type UMemRef = Arc<Mutex<UMem>>;
    fn allocate(&self, n: usize) -> Result<Vec<AppFrame<Self>>, CamelliaError> {
        let mut shared_umem = self.inner.lock().unwrap();
        shared_umem.pre_alloc(n)?;
        let chunk_size = shared_umem.chunk_size as usize;
        let mmap_area = shared_umem.mmap_area.clone();

        Ok(shared_umem
            .cached_chunks
            .drain(0..n)
            .map(|address| {
                AppFrame::from_chunk(
                    Chunk {
                        xdp_address: address,
                        size: chunk_size,
                        mmap_area: mmap_area.clone(),
                    },
                    self.clone(),
                )
            })
            .collect())
    }

    fn equal(&self, other: &Self) -> bool {
        // We compare address of SharedUMem instead of SharedUMemNode
        self.id == other.id
    }

    fn fill(&self, n: usize) -> Result<usize, CamelliaError> {
        self.inner.lock().unwrap().fill(n)
    }

    fn free(&self, chunk: Chunk) {
        self.inner.lock().unwrap().free(chunk)
    }

    fn extract_recv(&self, xdp_addr: u64) -> Chunk {
        self.inner.lock().unwrap().extract_recv(xdp_addr)
    }

    fn register_send(&self, chunk: Chunk) {
        self.inner.lock().unwrap().register_send(chunk)
    }

    fn inner(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .shared_umem
            .lock()
            .unwrap()
            .inner() as usize
    }

    fn need_wakeup(&self) -> bool {
        unsafe { xsk_ring_prod__needs_wakeup(&self.inner.lock().unwrap().fill.0) != 0 }
    }

    fn recycle(&self) -> Result<usize, CamelliaError> {
        self.inner.lock().unwrap().recycle()
    }
}
