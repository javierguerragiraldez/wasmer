use crate::{
    error::CreationError,
    export::Export,
    import::IsExport,
    memory::dynamic::DYNAMIC_GUARD_SIZE,
    memory::static_::{SAFE_STATIC_GUARD_SIZE, SAFE_STATIC_HEAP_SIZE},
    types::{MemoryDescriptor, ValueType},
    units::Pages,
    vm,
};
use std::{cell::RefCell, fmt, mem, ptr, rc::Rc, slice};

pub use self::dynamic::DynamicMemory;
pub use self::static_::{SharedStaticMemory, StaticMemory};

mod dynamic;
mod static_;

pub struct Memory {
    desc: MemoryDescriptor,
    storage: Rc<RefCell<(MemoryStorage, Box<vm::LocalMemory>)>>,
}

impl Memory {
    /// Create a new `Memory` from a [`MemoryDescriptor`]
    ///
    /// [`MemoryDescriptor`]: struct.MemoryDescriptor.html
    ///
    /// Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::types::MemoryDescriptor;
    /// # use wasmer_runtime_core::memory::Memory;
    /// # use wasmer_runtime_core::error::Result;
    /// # use wasmer_runtime_core::units::Pages;
    /// # fn create_memory() -> Result<()> {
    /// let descriptor = MemoryDescriptor {
    ///     minimum: Pages(10),
    ///     maximum: None,
    ///     shared: false,
    /// };
    ///
    /// let memory = Memory::new(descriptor)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(desc: MemoryDescriptor) -> Result<Self, CreationError> {
        let mut vm_local_memory = Box::new(vm::LocalMemory {
            base: ptr::null_mut(),
            bound: 0,
            memory: ptr::null_mut(),
        });

        let memory_storage = match desc.memory_type() {
            MemoryType::Dynamic => {
                MemoryStorage::Dynamic(DynamicMemory::new(desc, &mut vm_local_memory)?)
            }
            MemoryType::Static => {
                MemoryStorage::Static(StaticMemory::new(desc, &mut vm_local_memory)?)
            }
            MemoryType::SharedStatic => unimplemented!("shared memories are not yet implemented"),
        };

        Ok(Memory {
            desc,
            storage: Rc::new(RefCell::new((memory_storage, vm_local_memory))),
        })
    }

    /// Return the [`MemoryDescriptor`] that this memory
    /// was created with.
    ///
    /// [`MemoryDescriptor`]: struct.MemoryDescriptor.html
    pub fn descriptor(&self) -> MemoryDescriptor {
        self.desc
    }

    /// Grow this memory by the specfied number of pages.
    pub fn grow(&mut self, delta: Pages) -> Option<Pages> {
        match &mut *self.storage.borrow_mut() {
            (MemoryStorage::Dynamic(ref mut dynamic_memory), ref mut local) => {
                dynamic_memory.grow(delta, local)
            }
            (MemoryStorage::Static(ref mut static_memory), ref mut local) => {
                static_memory.grow(delta, local)
            }
            (MemoryStorage::SharedStatic(_), _) => unimplemented!(),
        }
    }

    /// The size, in wasm pages, of this memory.
    pub fn size(&self) -> Pages {
        match &*self.storage.borrow() {
            (MemoryStorage::Dynamic(ref dynamic_memory), _) => dynamic_memory.size(),
            (MemoryStorage::Static(ref static_memory), _) => static_memory.size(),
            (MemoryStorage::SharedStatic(_), _) => unimplemented!(),
        }
    }

    pub fn read<T: ValueType>(&self, offset: u32) -> Result<T, ()> {
        let offset = offset as usize;
        let borrow_ref = self.storage.borrow();
        let memory_storage = &borrow_ref.0;

        let mem_slice = match memory_storage {
            MemoryStorage::Dynamic(ref dynamic_memory) => dynamic_memory.as_slice(),
            MemoryStorage::Static(ref static_memory) => static_memory.as_slice(),
            MemoryStorage::SharedStatic(_) => panic!("cannot slice a shared memory"),
        };

        if offset + mem::size_of::<T>() <= mem_slice.len() {
            T::from_le(&mem_slice[offset..]).map_err(|_| ())
        } else {
            Err(())
        }
    }

    pub fn write<T: ValueType>(&self, offset: u32, value: T) -> Result<(), ()> {
        let offset = offset as usize;
        let mut borrow_ref = self.storage.borrow_mut();
        let memory_storage = &mut borrow_ref.0;

        let mem_slice = match memory_storage {
            MemoryStorage::Dynamic(ref mut dynamic_memory) => dynamic_memory.as_slice_mut(),
            MemoryStorage::Static(ref mut static_memory) => static_memory.as_slice_mut(),
            MemoryStorage::SharedStatic(_) => panic!("cannot slice a shared memory"),
        };

        if offset + mem::size_of::<T>() <= mem_slice.len() {
            value.into_le(&mut mem_slice[offset..]);
            Ok(())
        } else {
            Err(())
        }
    }

    pub fn read_many<T: ValueType>(&self, offset: u32, count: usize) -> Result<Vec<T>, ()> {
        let offset = offset as usize;
        let borrow_ref = self.storage.borrow();
        let memory_storage = &borrow_ref.0;

        let mem_slice = match memory_storage {
            MemoryStorage::Dynamic(ref dynamic_memory) => dynamic_memory.as_slice(),
            MemoryStorage::Static(ref static_memory) => static_memory.as_slice(),
            MemoryStorage::SharedStatic(_) => panic!("cannot slice a shared memory"),
        };

        let bytes_size = count * mem::size_of::<T>();

        if offset + bytes_size <= mem_slice.len() {
            let buffer = &mem_slice[offset..offset + bytes_size];
            let value_type_buffer = unsafe {
                slice::from_raw_parts(
                    buffer.as_ptr() as *const T,
                    buffer.len() / mem::size_of::<T>(),
                )
            };
            Ok(value_type_buffer.to_vec())
        } else {
            Err(())
        }
    }

    pub fn write_many<T: ValueType>(&self, offset: u32, values: &[T]) -> Result<(), ()> {
        let offset = offset as usize;
        let mut borrow_ref = self.storage.borrow_mut();
        let memory_storage = &mut borrow_ref.0;

        let mem_slice = match memory_storage {
            MemoryStorage::Dynamic(ref mut dynamic_memory) => dynamic_memory.as_slice_mut(),
            MemoryStorage::Static(ref mut static_memory) => static_memory.as_slice_mut(),
            MemoryStorage::SharedStatic(_) => panic!("cannot slice a shared memory"),
        };

        let bytes_size = values.len() * mem::size_of::<T>();

        if offset + bytes_size <= mem_slice.len() {
            let u8_buffer =
                unsafe { slice::from_raw_parts(values.as_ptr() as *const u8, bytes_size) };
            mem_slice[offset..offset + bytes_size].copy_from_slice(u8_buffer);
            Ok(())
        } else {
            Err(())
        }
    }

    pub fn direct_access<T: ValueType, F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[T]) -> R,
    {
        let borrow_ref = self.storage.borrow();
        let memory_storage = &borrow_ref.0;

        let mem_slice = match memory_storage {
            MemoryStorage::Dynamic(ref dynamic_memory) => dynamic_memory.as_slice(),
            MemoryStorage::Static(ref static_memory) => static_memory.as_slice(),
            MemoryStorage::SharedStatic(_) => panic!("cannot slice a shared memory"),
        };

        let t_buffer = unsafe {
            slice::from_raw_parts(
                mem_slice.as_ptr() as *const T,
                mem_slice.len() / mem::size_of::<T>(),
            )
        };

        f(t_buffer)
    }

    pub fn direct_access_mut<T: ValueType, F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut [T]) -> R,
    {
        let mut borrow_ref = self.storage.borrow_mut();
        let memory_storage = &mut borrow_ref.0;

        let mem_slice = match memory_storage {
            MemoryStorage::Dynamic(ref mut dynamic_memory) => dynamic_memory.as_slice_mut(),
            MemoryStorage::Static(ref mut static_memory) => static_memory.as_slice_mut(),
            MemoryStorage::SharedStatic(_) => panic!("cannot slice a shared memory"),
        };

        let t_buffer = unsafe {
            slice::from_raw_parts_mut(
                mem_slice.as_mut_ptr() as *mut T,
                mem_slice.len() / mem::size_of::<T>(),
            )
        };

        f(t_buffer)
    }

    pub(crate) fn vm_local_memory(&mut self) -> *mut vm::LocalMemory {
        &mut *self.storage.borrow_mut().1
    }
}

impl IsExport for Memory {
    fn to_export(&mut self) -> Export {
        Export::Memory(self.clone())
    }
}

impl Clone for Memory {
    fn clone(&self) -> Self {
        Self {
            desc: self.desc,
            storage: Rc::clone(&self.storage),
        }
    }
}

pub enum MemoryStorage {
    Dynamic(Box<DynamicMemory>),
    Static(Box<StaticMemory>),
    SharedStatic(Box<SharedStaticMemory>),
}

impl MemoryStorage {
    pub fn to_type(&self) -> MemoryType {
        match self {
            MemoryStorage::Dynamic(_) => MemoryType::Dynamic,
            MemoryStorage::Static(_) => MemoryType::Static,
            MemoryStorage::SharedStatic(_) => MemoryType::SharedStatic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    Dynamic,
    Static,
    SharedStatic,
}

impl MemoryType {
    #[doc(hidden)]
    pub fn guard_size(self) -> u64 {
        match self {
            MemoryType::Dynamic => DYNAMIC_GUARD_SIZE as u64,
            MemoryType::Static => SAFE_STATIC_GUARD_SIZE as u64,
            MemoryType::SharedStatic => SAFE_STATIC_GUARD_SIZE as u64,
        }
    }

    #[doc(hidden)]
    pub fn bounds(self) -> Option<u64> {
        match self {
            MemoryType::Dynamic => None,
            MemoryType::Static => Some(SAFE_STATIC_HEAP_SIZE as u64),
            MemoryType::SharedStatic => Some(SAFE_STATIC_HEAP_SIZE as u64),
        }
    }
}

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Memory")
            .field("desc", &self.desc)
            .field("size", &self.size())
            .finish()
    }
}
