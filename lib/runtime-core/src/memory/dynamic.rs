use crate::{
    error::CreationError,
    sys,
    types::MemoryDescriptor,
    units::{Bytes, Pages},
    vm,
};

pub const DYNAMIC_GUARD_SIZE: usize = 4096;

/// This is an internal-only api.
///
/// A Dynamic memory allocates only the minimum amount of memory
/// when first created. Over time, as it grows, it may reallocate to
/// a different location and size.
///
/// Dynamic memories are signifigantly faster to create than static
/// memories and use much less virtual memory, however, they require
/// the webassembly module to bounds-check memory accesses.
///
/// While, a dynamic memory could use a vector of some sort as its
/// backing memory, we use mmap (or the platform-equivalent) to allow
/// us to add a guard-page at the end to help elide some bounds-checks.
pub struct DynamicMemory {
    memory: sys::Memory,
    current: Pages,
    max: Option<Pages>,
}

impl DynamicMemory {
    pub(super) fn new(
        desc: MemoryDescriptor,
        local: &mut vm::LocalMemory,
    ) -> Result<Box<Self>, CreationError> {
        let min_bytes: Bytes = desc.minimum.into();
        let memory = {
            let mut memory = sys::Memory::with_size(min_bytes.0 + DYNAMIC_GUARD_SIZE)
                .map_err(|_| CreationError::UnableToCreateMemory)?;
            if desc.minimum != Pages(0) {
                unsafe {
                    memory
                        .protect(0..min_bytes.0, sys::Protect::ReadWrite)
                        .map_err(|_| CreationError::UnableToCreateMemory)?;
                }
            }

            memory
        };

        let mut storage = Box::new(DynamicMemory {
            memory,
            current: desc.minimum,
            max: desc.maximum,
        });
        let storage_ptr: *mut DynamicMemory = &mut *storage;

        local.base = storage.memory.as_ptr();
        local.bound = min_bytes.0;
        local.memory = storage_ptr as *mut ();

        Ok(storage)
    }

    pub fn size(&self) -> Pages {
        self.current
    }

    pub fn grow(&mut self, delta: Pages, local: &mut vm::LocalMemory) -> Option<Pages> {
        if delta == Pages(0) {
            return Some(self.current);
        }

        let new_pages = self.current.checked_add(delta)?;

        if let Some(max) = self.max {
            if new_pages > max {
                return None;
            }
        }

        let mut new_memory =
            sys::Memory::with_size(new_pages.bytes().0 + DYNAMIC_GUARD_SIZE).ok()?;

        unsafe {
            new_memory
                .protect(0..new_pages.bytes().0, sys::Protect::ReadWrite)
                .ok()?;

            new_memory.as_slice_mut()[..self.current.bytes().0]
                .copy_from_slice(&self.memory.as_slice()[..self.current.bytes().0]);
        }

        self.memory = new_memory; //The old memory gets dropped.

        local.base = self.memory.as_ptr();
        local.bound = new_pages.bytes().0;

        let old_pages = self.current;
        self.current = new_pages;
        Some(old_pages)
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { &self.memory.as_slice()[0..self.current.bytes().0] }
    }

    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { &mut self.memory.as_slice_mut()[0..self.current.bytes().0] }
    }
}
