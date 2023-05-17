use std::cell::Ref;

use libxdp_sys::xsk_ring_prod;

use crate::error::CamelliaError;

use self::frame::{AppFrame, Chunk};

pub mod base;
pub mod frame;
pub mod libxdp;
pub mod mmap;
pub mod shared;

pub trait UMemAccessor: Sized {
    type UMemRef;
    type AccessorRef: Clone;

    fn inner(umem_rc: &Self::AccessorRef) -> usize;

    fn fill_inner(umem_rc: &Self::AccessorRef) -> Ref<xsk_ring_prod>;

    fn allocate(
        umem_rc: &Self::AccessorRef,
        size: usize,
    ) -> Result<Vec<AppFrame<Self>>, CamelliaError>;

    fn fill(umem_rc: &Self::AccessorRef, n: usize) -> Result<usize, CamelliaError>;

    fn recycle(umem_rc: &Self::AccessorRef) -> Result<usize, CamelliaError>;

    fn free(umem_rc: &Self::AccessorRef, chunk: Chunk);

    fn register_send(umem_rc: &Self::AccessorRef, chunk: Chunk);

    fn extract_recv(umem_rc: &Self::AccessorRef, xdp_addr: u64) -> Chunk;

    fn equal(umem_rc: &Self::AccessorRef, other: &Self::AccessorRef) -> bool;
}
