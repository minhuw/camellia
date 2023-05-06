use std::num::NonZeroUsize;

use crate::error::CamelliaError;
use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};

#[derive(Debug)]
pub struct MMapArea {
    base_address: usize,
    length: usize,
}

impl MMapArea {
    pub fn new(size: usize) -> Result<Self, CamelliaError> {
        if size == 0 {
            return Err(CamelliaError::InvalidArgument(
                "mmap size could not be zero".into(),
            ));
        }
        let mmap_base = unsafe {
            mmap(
                None,
                NonZeroUsize::new_unchecked(size),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::empty(),
                0,
                0,
            )?
        };

        let mmap_area = Self {
            base_address: mmap_base as usize,
            length: size,
        };

        Ok(mmap_area)
    }

    pub fn base_address(&self) -> usize {
        self.base_address
    }
}

impl Drop for MMapArea {
    fn drop(&mut self) {
        if let Err(e) = unsafe { munmap(self.base_address as *mut std::ffi::c_void, self.length) } {
            eprintln!(
                "unable to munmap memory region (base address: {}, length: {}) due to {}",
                self.base_address, self.length, e
            );
        }
    }
}
