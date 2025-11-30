use std::{marker::PhantomData, sync::Arc};
use anyhow::Result;
use serde::de::DeserializeOwned;
use wasmtime::{AsContextMut, Caller};

use crate::{
    state::ModuleState,
    memory::{unpack_ptr, pack_ptr, MemoryOps, AsyncMemoryOps},
    input::{FromFnInput, FnInput},
    result::{FnResult, IntoFnResult}
};


/// Trait for host functions that can be called
pub trait HostFnCallable: Send + Sync + 'static {
    fn call(&self, input: &FnInput) -> FnResult;
}

/// Wrapper for host functions that implements HostFnCallable
#[derive(Clone)]
pub struct HostFnWrapper<F, Args> {
    func: F,
    _marker: PhantomData<Args>,
}

impl<F, Args> HostFnWrapper<F, Args> {
    pub fn new(func: F) -> Self {
        Self {
            func,
            _marker: PhantomData,
        }
    }
}

impl<F, R> HostFnCallable for HostFnWrapper<F, ()>
where
    F: Fn() -> R + Send + Sync + 'static,
    R: IntoFnResult,
{
    fn call(&self, _input: &FnInput) -> FnResult {
        (self.func)().into_fn_result()   
    }
}

macro_rules! impl_host_fn_callable {
    ($($T:ident),+) => {
        #[allow(non_snake_case)]
        impl<F, R, $($T),*> HostFnCallable for HostFnWrapper<F, ($($T,)*)>
        where
            F: Fn($($T),*) -> R + Send + Sync + 'static,
            ($($T,)*): FromFnInput,
            $($T: DeserializeOwned + Send + Sync + 'static),+,
            R: IntoFnResult,
        {
            fn call(&self, input: &FnInput) -> FnResult {
                let ($($T,)*) = match <($($T,)*)>::from_fn_input(input) {
                    Ok(args) => args,
                    Err(e) => return FnResult::err(&e),
                };
                (self.func)($($T),*).into_fn_result()   
            }
        }
    };
}

impl_host_fn_callable!(A1);
impl_host_fn_callable!(A1, A2);
impl_host_fn_callable!(A1, A2, A3);
impl_host_fn_callable!(A1, A2, A3, A4);
impl_host_fn_callable!(A1, A2, A3, A4, A5);
impl_host_fn_callable!(A1, A2, A3, A4, A5, A6);
impl_host_fn_callable!(A1, A2, A3, A4, A5, A6, A7);
impl_host_fn_callable!(A1, A2, A3, A4, A5, A6, A7, A8);

/// Represents a Host Function that can be called from a Wasmtime module
#[derive(Clone)]
pub struct HostFn {
    func: Arc<dyn HostFnCallable>,
}

impl HostFn {
    /// Create a new Host Function from a Rust function or closure.
    /// 
    /// # Arguments
    /// * `func` - The Rust function or closure to be wrapped as a Host Function
    ///
    /// # Returns
    /// A new HostFn instance
    pub fn new<F, Args>(func: F) -> Self
    where
        HostFnWrapper<F, Args>: HostFnCallable + 'static,
    {
        Self {
            func: Arc::new(HostFnWrapper::new(func)),
        }
    }

    /// Convert the Host Function into a Wasmtime function.
    /// 
    /// # Returns
    /// A closure that can be used as a Wasmtime host function
    pub fn into_func(self) -> impl Fn(Caller<ModuleState>, u64) -> Result<u64> {
        move |mut caller: Caller<ModuleState>, ptr: u64| -> Result<u64> {
            let memory = MemoryOps::from_caller(&mut caller)?;
            let (input_ptr, input_len) = unpack_ptr(ptr);
            let input = FnInput::from_bytes(
                &memory.read(
                    caller.as_context_mut(),
                    input_ptr,
                    input_len,
                )?
            )?;
            let (result_ptr, result_len) = memory.write(
                caller.as_context_mut(),
                &self.func
                    .call(&input)
                    .to_bytes()?,
            )?;

            Ok(pack_ptr(result_ptr, result_len))
        }
    }

    /// Convert the Host Function into a Wasmtime function, handling
    /// asynchronous memory operations for input and output when using
    /// an [`AsyncModule`](crate::module::AsyncModule).
    /// 
    /// # Returns
    /// A closure that can be used as a Wasmtime host function
    pub fn into_func_async(self) -> impl Fn(Caller<ModuleState>, u64) -> Result<u64> {
        move |mut caller: Caller<ModuleState>, ptr: u64| -> Result<u64> {
            futures::executor::block_on(async {
                let memory = AsyncMemoryOps::from_caller(&mut caller)?;
                let (input_ptr, input_len) = unpack_ptr(ptr);
                let input = FnInput::from_bytes(
                    &memory
                        .read(
                            caller.as_context_mut(),
                            input_ptr,
                            input_len,
                        )
                        .await?
                )?;
                let (result_ptr, result_len) = memory
                    .write(
                        caller.as_context_mut(),
                        &self.func
                            .call(&input)
                            .to_bytes()?,
                    )
                    .await?;

                Ok(pack_ptr(result_ptr, result_len))
            })
        }
    }
}