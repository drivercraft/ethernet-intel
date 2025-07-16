use core::cell::RefCell;

use dma_api::{DVec, Direction};

use crate::{descriptor::Descriptor, err::DError, mac::Mac};

pub const DEFAULT_RING_SIZE: usize = 256;

pub struct Ring<D: Descriptor> {
    pub descriptors: DVec<D>,
}

impl<D: Descriptor> Ring<D> {
    pub fn new(size: usize) -> Result<Self, DError> {
        let descriptors =
            DVec::zeros(size, 0x1000, Direction::Bidirectional).ok_or(DError::NoMemory)?;

        Ok(Self { descriptors })
    }

    pub fn init(&mut self) {
        
    }
}
