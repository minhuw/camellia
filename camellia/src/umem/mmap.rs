use crate::error::CamelliaError;
use nix::sys::mman::{mmap_anonymous, munmap, MapFlags, ProtFlags};
use std::num::NonZeroUsize;
use std::ptr::NonNull;

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
            mmap_anonymous(
                None,
                NonZeroUsize::new_unchecked(size),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED | MapFlags::MAP_ANONYMOUS,
            )?
        };

        let mmap_area = Self {
            base_address: mmap_base.as_ptr() as usize,
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
        if let Err(e) = unsafe {
            munmap(
                NonNull::new(self.base_address as *mut std::ffi::c_void).unwrap(),
                self.length,
            )
        } {
            eprintln!(
                "unable to munmap memory region (base address: {}, length: {}) due to {}",
                self.base_address, self.length, e
            );
        }
    }
}

#[cfg(test)]
mod test {
    use crate::umem::mmap::MMapArea;

    #[test]
    fn test_mmap() {
        let mmap_area = MMapArea::new(4096).unwrap();
        assert_ne!(mmap_area.base_address(), 0);
    }
}
