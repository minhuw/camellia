use crate::error::CamelliaError;

use self::frame::{AppFrame, Chunk};

pub mod base;
pub mod frame;
pub mod libxdp;
pub mod mmap;
pub mod shared;

pub trait AccessorRef: Sized + Clone {
    type UMemRef;

    fn inner(&self) -> usize;

    fn need_wakeup(&self) -> bool;

    fn allocate(&self, size: usize) -> Result<Vec<AppFrame<Self>>, CamelliaError>;

    fn fill(&self, n: usize) -> Result<usize, CamelliaError>;

    fn recycle(&self) -> Result<usize, CamelliaError>;

    fn free(&self, chunk: Chunk);

    fn register_send(&self, chunk: Chunk);

    fn extract_recv(&self, xdp_addr: u64) -> Chunk;

    fn equal(&self, other: &Self) -> bool;
}
