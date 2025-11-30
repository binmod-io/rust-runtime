use wasmtime::{AsContextMut, AsContext, Caller, Instance, Memory, Store, TypedFunc};

use crate::{state::ModuleState, error::{ModuleError, ModuleResult}};


/// Pack a pointer and length into a single u64 value
/// 
/// The higher 32 bits contain the pointer, and the lower 32 bits contain the length.
/// Format: (ptr << 32) | len
/// 
/// # Arguments
/// * `ptr` - The pointer value (u32)
/// * `len` - The length value (u32)
/// 
/// # Returns
/// A u64 value combining the pointer and length
pub fn pack_ptr(ptr: u32, len: usize) -> u64 {
    ((ptr as u64) << 32) | (len as u64)
}


/// Unpack a u64 value into a pointer and length
/// 
/// # Arguments
/// * `packed` - The packed u64 value
/// 
/// # Returns
/// A tuple containing the pointer (u32) and length (usize)
pub fn unpack_ptr(packed: u64) -> (u32, usize) {
    ((packed >> 32) as u32, (packed & 0xFFFFFFFF) as usize)
}

#[derive(Clone)]
pub struct MemoryOps {
    memory: Memory,
    alloc_fn: TypedFunc<u32, u32>,
    dealloc_fn: TypedFunc<(u32, u32), ()>,
}

impl MemoryOps {
    pub fn new(
        memory: Memory,
        alloc_fn: TypedFunc<u32, u32>,
        dealloc_fn: TypedFunc<(u32, u32), ()>,
    ) -> Self {
        Self {
            memory,
            alloc_fn,
            dealloc_fn,
        }
    }

    pub fn from_instance(instance: &Instance, store: &mut Store<ModuleState>) -> ModuleResult<Self> {
        Ok(Self {
            memory: instance
                .get_memory(store.as_context_mut(), "memory")
                .ok_or_else(|| ModuleError::MemoryError("failed to find memory export".to_string()))?,
            alloc_fn: instance
                .get_func(store.as_context_mut(), "guest_alloc")
                .ok_or_else(|| ModuleError::MemoryError("failed to find guest_alloc".to_string()))?
                .typed::<u32, u32>(store.as_context())
                .map_err(|e| ModuleError::MemoryError(format!("failed to type guest_alloc: {}", e)))?,
            dealloc_fn: instance
                .get_func(store.as_context_mut(), "guest_dealloc")
                .ok_or_else(|| ModuleError::MemoryError("failed to find guest_dealloc".to_string()))?
                .typed::<(u32, u32), ()>(store.as_context())
                .map_err(|e| ModuleError::MemoryError(format!("failed to type guest_dealloc: {}", e)))?
        })
    }

    pub fn from_caller(caller: &mut Caller<'_, ModuleState>) -> ModuleResult<Self> {
        Ok(Self {
            memory: caller
                .get_export("memory")
                .and_then(|e| e.into_memory())
                .ok_or_else(|| ModuleError::MemoryError("failed to find memory export".to_string()))?,
            alloc_fn: caller
                .get_export("guest_alloc")
                .and_then(|e| e.into_func())
                .ok_or_else(|| ModuleError::MemoryError("failed to find guest_alloc".to_string()))?
                .typed::<u32, u32>(&caller)
                .map_err(|e| ModuleError::MemoryError(format!("failed to type guest_alloc: {}", e)))?,
            dealloc_fn: caller
                .get_export("guest_dealloc")
                .and_then(|e| e.into_func())
                .ok_or_else(|| ModuleError::MemoryError("failed to find guest_dealloc".to_string()))?
                .typed::<(u32, u32), ()>(&caller)
                .map_err(|e| ModuleError::MemoryError(format!("failed to type guest_dealloc: {}", e)))?,
        })
    }

    /// Allocate memory in the guest module
    /// 
    /// # Arguments
    /// 
    /// * `store` - The mutable store context
    /// * `size` - The size of memory to allocate
    /// 
    /// # Returns
    /// 
    /// A pointer to the allocated memory in the guest module
    pub fn alloc(&self, mut ctx: impl AsContextMut, size: usize) -> ModuleResult<u32> {
        Ok(
            self.alloc_fn
                .call(ctx.as_context_mut(), size as u32)
                .map_err(|e| ModuleError::MemoryError(format!("Guest alloc failed: {}", e)))?
        )
    }

    /// Deallocate memory in the guest module
    /// 
    /// # Arguments
    /// 
    /// * `store` - The mutable store context
    /// * `ptr` - The pointer to the memory to deallocate
    /// * `size` - The size of the memory to deallocate
    /// 
    /// # Returns
    /// 
    /// A result indicating success or failure
    pub fn dealloc(&self, mut ctx: impl AsContextMut, ptr: u32, size: usize) -> ModuleResult<()> {
        self.dealloc_fn
            .call(ctx.as_context_mut(), (ptr, size as u32))
            .map_err(|e| ModuleError::MemoryError(format!("Guest dealloc failed: {}", e)))?;

        Ok(())
    }

    /// Write data to the guest module's memory
    /// 
    /// # Arguments
    /// 
    /// * `store` - The mutable store context
    /// * `data` - The data to write
    /// 
    /// # Returns
    /// 
    /// A tuple containing the pointer to the written data and its size
    pub fn write(&self, mut ctx: impl AsContextMut, data: &[u8]) -> ModuleResult<(u32, usize)> {
        let size = data.len();
        let ptr = self.alloc(ctx.as_context_mut(), size)?;

        self.memory
            .write(ctx.as_context_mut(), ptr as usize, data)
            .map_err(|e| ModuleError::MemoryError(format!("Memory write failed: {}", e)))?;

        Ok((ptr, size))
    }

    /// Read data from the guest module's memory
    /// 
    /// # Arguments
    /// 
    /// * `ctx` - The store context
    /// * `ptr` - The pointer to the data to read
    /// * `len` - The length of the data to read
    /// 
    /// # Returns
    /// 
    /// A vector containing the read data
    pub fn read(&self, mut ctx: impl AsContextMut, ptr: u32, len: usize) -> ModuleResult<Vec<u8>> {
        if ptr == 0 || len == 0 {
            return Err(ModuleError::MemoryError(
                "Null pointer or zero length".to_string(),
            ));
        }

        let mut buffer = vec![0u8; len];
        self.memory
            .read(ctx.as_context_mut(), ptr as usize, &mut buffer)
            .map_err(|e| ModuleError::MemoryError(format!("Memory read failed: {}", e)))?;

        self.dealloc(ctx.as_context_mut(), ptr, len)?;

        Ok(buffer)
    }
}

pub struct AsyncMemoryOps {
    memory: Memory,
    alloc_fn: TypedFunc<u32, u32>,
    dealloc_fn: TypedFunc<(u32, u32), ()>,
}

impl AsyncMemoryOps {
    pub fn new(
        memory: Memory,
        alloc_fn: TypedFunc<u32, u32>,
        dealloc_fn: TypedFunc<(u32, u32), ()>,
    ) -> Self {
        Self {
            memory,
            alloc_fn,
            dealloc_fn,
        }
    }

    pub fn from_instance(instance: &Instance, store: &mut Store<ModuleState>) -> ModuleResult<Self> {
        Ok(Self {
            memory: instance
                .get_memory(store.as_context_mut(), "memory")
                .ok_or_else(|| ModuleError::MemoryError("failed to find memory export".to_string()))?,
            alloc_fn: instance
                .get_func(store.as_context_mut(), "guest_alloc")
                .ok_or_else(|| ModuleError::MemoryError("failed to find guest_alloc".to_string()))?
                .typed::<u32, u32>(store.as_context())
                .map_err(|e| ModuleError::MemoryError(format!("failed to type guest_alloc: {}", e)))?,
            dealloc_fn: instance
                .get_func(store.as_context_mut(), "guest_dealloc")
                .ok_or_else(|| ModuleError::MemoryError("failed to find guest_dealloc".to_string()))?
                .typed::<(u32, u32), ()>(store.as_context())
                .map_err(|e| ModuleError::MemoryError(format!("failed to type guest_dealloc: {}", e)))?
        })
    }

    pub fn from_caller(caller: &mut Caller<'_, ModuleState>) -> ModuleResult<Self> {
        Ok(Self {
            memory: caller
                .get_export("memory")
                .and_then(|e| e.into_memory())
                .ok_or_else(|| ModuleError::MemoryError("failed to find memory export".to_string()))?,
            alloc_fn: caller
                .get_export("guest_alloc")
                .and_then(|e| e.into_func())
                .ok_or_else(|| ModuleError::MemoryError("failed to find guest_alloc".to_string()))?
                .typed::<u32, u32>(&caller)
                .map_err(|e| ModuleError::MemoryError(format!("failed to type guest_alloc: {}", e)))?,
            dealloc_fn: caller
                .get_export("guest_dealloc")
                .and_then(|e| e.into_func())
                .ok_or_else(|| ModuleError::MemoryError("failed to find guest_dealloc".to_string()))?
                .typed::<(u32, u32), ()>(&caller)
                .map_err(|e| ModuleError::MemoryError(format!("failed to type guest_dealloc: {}", e)))?,
        })
    }

    /// Allocate memory in the guest module
    /// 
    /// # Arguments
    /// 
    /// * `store` - The mutable store context
    /// * `size` - The size of memory to allocate
    /// 
    /// # Returns
    /// 
    /// A pointer to the allocated memory in the guest module
    pub async fn alloc<T>(&self, mut ctx: impl AsContextMut<Data = T>, size: usize) -> ModuleResult<u32>
    where
        T: Send + 'static,
    {
        Ok(
            self.alloc_fn
                .call_async(ctx.as_context_mut(), size as u32)
                .await
                .map_err(|e| ModuleError::MemoryError(format!("Guest alloc failed: {}", e)))?
        )
    }

    /// Deallocate memory in the guest module
    /// 
    /// # Arguments
    /// 
    /// * `store` - The mutable store context
    /// * `ptr` - The pointer to the memory to deallocate
    /// * `size` - The size of the memory to deallocate
    /// 
    /// # Returns
    /// 
    /// A result indicating success or failure
    pub async fn dealloc<T>(&self, mut ctx: impl AsContextMut<Data = T>, ptr: u32, size: usize) -> ModuleResult<()>
    where
        T: Send + 'static,
    {
        self.dealloc_fn
            .call_async(ctx.as_context_mut(), (ptr, size as u32))
            .await
            .map_err(|e| ModuleError::MemoryError(format!("Guest dealloc failed: {}", e)))?;

        Ok(())
    }

    /// Write data to the guest module's memory
    /// 
    /// # Arguments
    /// 
    /// * `store` - The mutable store context
    /// * `data` - The data to write
    /// 
    /// # Returns
    /// 
    /// A tuple containing the pointer to the written data and its size
    pub async fn write<T>(&self, mut ctx: impl AsContextMut<Data = T>, data: &[u8]) -> ModuleResult<(u32, usize)>
    where
        T: Send + 'static,
    {
        let size = data.len();
        let ptr = self.alloc(ctx.as_context_mut(), size).await?;

        self.memory
            .write(ctx.as_context_mut(), ptr as usize, data)
            .map_err(|e| ModuleError::MemoryError(format!("Memory write failed: {}", e)))?;

        Ok((ptr, size))
    }

    /// Read data from the guest module's memory
    /// 
    /// # Arguments
    /// 
    /// * `ctx` - The store context
    /// * `ptr` - The pointer to the data to read
    /// * `len` - The length of the data to read
    /// 
    /// # Returns
    /// 
    /// A vector containing the read data
    pub async fn read(&self, mut ctx: impl AsContextMut<Data: Send>, ptr: u32, len: usize) -> ModuleResult<Vec<u8>> {
        if ptr == 0 || len == 0 {
            return Err(ModuleError::MemoryError(
                "Null pointer or zero length".to_string(),
            ));
        }

        let mut buffer = vec![0u8; len];
        self.memory
            .read(ctx.as_context_mut(), ptr as usize, &mut buffer)
            .map_err(|e| ModuleError::MemoryError(format!("Memory read failed: {}", e)))?;

        self.dealloc(ctx.as_context_mut(), ptr, len).await?;
        
        Ok(buffer)
    }
}