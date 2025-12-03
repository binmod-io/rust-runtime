use std::{collections::HashMap, path::Path, fs};
use wasmtime::{Engine, Instance, InstancePre, Store, Module as WasmModule, Caller, Linker, Config, AsContextMut, AsContext};
use wasmtime_wasi::p1;
use serde::de::DeserializeOwned;

use crate::{
    input::{FnInput, IntoFnInput},
    result::FnResult,
    state::ModuleState,
    host_fns::{HostFn, HostFnCallable, HostFnWrapper},
    memory::{MemoryOps, AsyncMemoryOps, unpack_ptr},
    config::{ModuleEnv, ModuleConfig, ModuleLimits},
    error::{ModuleResult, ModuleError},
};


/// Represents a Binmod Module with host functions, and provides methods
/// to instantiate and call functions within the module.
/// 
/// # Examples
/// ```rust
/// use binmod::{module::Module, config::ModuleEnv};
/// 
/// let mut module = Module::builder()
///     .from_file("path/to/module.wasm")
///     .unwrap()
///     .with_name("example_module")
///     .with_namespace("env")
///     .with_environment(
///         ModuleEnv::default()
///             .arg("path/to/module.wasm")
///             .inherit_env()
///             .inherit_network()
///     )
///     .host_fn("host_function", |arg1: i32, arg2: String| -> Result<String, String> {
///         Ok(format!("Received: {} and {}", arg1, arg2))
///     })
///     .build()?
///     .instantiate()?;
/// 
/// let result: String = module
///     .typed_call("guest_function", (42, "Hello".to_string()))?;
/// println!("Result from guest function: {}", result);
/// ```
pub struct Module {
    name: String,
    namespace: String,
    binary: Vec<u8>,
    environment: ModuleEnv,
    config: ModuleConfig,
    limits: ModuleLimits,
    host_fns: HashMap<String, HostFn>,
    engine: Option<Engine>,
    store: Option<Store<ModuleState>>,
    linker: Option<Linker<ModuleState>>,
    instance_pre: Option<InstancePre<ModuleState>>,
    instance: Option<Instance>,
}

impl Module {
    /// Create a new Binmod Module.
    /// 
    /// # Arguments
    /// * `binary` - The WebAssembly binary code of the module
    /// * `name` - The name of the module
    /// * `namespace` - The namespace for the module's host functions
    /// * `environment` - The environment configuration for the module
    /// * `config` - The configuration for the module
    /// * `limits` - The resource limits for the module
    /// * `host_fns` - A map of host function names to HostFn instances
    ///
    /// # Returns
    /// A new [`Module`](crate::module::Module) instance
    pub fn new(
        binary: Vec<u8>,
        name: impl Into<String>,
        namespace: impl Into<String>,
        environment: ModuleEnv,
        config: ModuleConfig,
        limits: ModuleLimits,
        host_fns: HashMap<String, HostFn>,
    ) -> Self {
        Self {
            name: name.into(),
            namespace: namespace.into(),
            binary,
            environment,
            config,
            limits,
            host_fns,
            engine: None,
            store: None,
            linker: None,
            instance_pre: None,
            instance: None,
        }
    }

    /// Create a new [`ModuleBuilder`](crate::module::ModuleBuilder)
    /// for constructing a [`Module`](crate::module::Module).
    pub fn builder() -> ModuleBuilder {
        ModuleBuilder::new()
    }

    /// Get the name of the module.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the namespace of the module.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Get the binary code of the module.
    pub fn binary(&self) -> &[u8] {
        &self.binary
    }

    /// Get the environment configuration of the module.
    pub fn environment(&self) -> &ModuleEnv {
        &self.environment
    }

    /// Check if the module has been instantiated.
    pub fn is_instantiated(&self) -> bool {
        self.instance.is_some()
    }

    /// Set the fuel for the module's store.
    /// 
    /// # Arguments
    /// * `fuel` - The amount of fuel to set
    /// 
    /// # Returns
    /// A result indicating success or an error
    /// if fuel is not enabled or the module is not instantiated
    pub fn set_fuel(&mut self, fuel: u64) -> ModuleResult<()> {
        self.store
            .as_mut()
            .ok_or(ModuleError::NotInstantiated)?
            .set_fuel(fuel)
            .map_err(|_| ModuleError::FuelNotEnabled)?;

        Ok(())
    }

    /// Get the remaining fuel for the module's store.
    /// 
    /// # Returns
    /// A result containing the remaining fuel or an error
    /// if fuel is not enabled or the module is not instantiated
    pub fn get_fuel(&mut self) -> ModuleResult<u64> {
        Ok(
            self.store
                .as_mut()
                .ok_or(ModuleError::NotInstantiated)?
                .get_fuel()
                .map_err(|_| ModuleError::FuelNotEnabled)?
        )
    }

    /// Set the epoch deadline for the module's store.
    /// 
    /// # Arguments
    /// * `deadline` - The epoch deadline to set
    /// 
    /// # Returns
    /// A result indicating success or an error
    /// if the module is not instantiated
    pub fn set_epoch_deadline(&mut self, deadline: u64) -> ModuleResult<()> {
        self.store
            .as_mut()
            .ok_or(ModuleError::NotInstantiated)?
            .set_epoch_deadline(deadline);
    
        Ok(())
    }

    /// Increment the epoch for the module's store.
    /// 
    /// # Returns
    /// A result indicating success or an error
    /// if the module is not instantiated
    pub fn increment_epoch(&mut self) -> ModuleResult<()> {
        self.engine
            .as_mut()
            .ok_or(ModuleError::NotInstantiated)?
            .increment_epoch();

        Ok(())
    }

    /// Instantiate the module.
    /// 
    /// # Returns
    /// A result containing the instantiated module or an error
    /// if instantiation fails or the module is already instantiated
    pub fn instantiate(mut self) -> ModuleResult<Self> {
        if self.is_instantiated() {
            return Err(ModuleError::AlreadyInstantiated);
        }

        if !self.engine.is_some() {
            let engine = Engine::new(&self.config.clone().into())?;
            let mut linker = Linker::<ModuleState>::new(&engine);

            linker.func_wrap(
                "binmod",
                "host_alloc",
                |mut caller: Caller<ModuleState>, size: u32| -> u32 {
                    caller
                        .get_export("guest_alloc")
                        .and_then(|e| e.into_func())
                        .ok_or_else(|| anyhow::anyhow!("failed to find guest_alloc"))
                        .unwrap()
                        .typed::<u32, u32>(&caller)
                        .unwrap()
                        .call(&mut caller, size)
                        .unwrap()
                }
            )?;
            linker.func_wrap(
                "binmod",
                "host_dealloc",
                |mut caller: Caller<ModuleState>, ptr: u32, size: u32| {
                    caller
                        .get_export("guest_dealloc")
                        .and_then(|e| e.into_func())
                        .ok_or_else(|| anyhow::anyhow!("failed to find guest_dealloc"))
                        .unwrap()
                        .typed::<(u32, u32), ()>(&caller)
                        .unwrap()
                        .call(&mut caller, (ptr, size))
                        .unwrap();
                }
            )?;

            for (name, host_fn) in &self.host_fns {
                linker.func_wrap(
                    &self.namespace,
                    name,
                    host_fn.clone().into_func(),
                )?;
            }

            self.engine = Some(engine);
            self.linker = Some(linker);
        }

        if !self.instance_pre.is_some() {
            p1::add_to_linker_sync(
                self.linker
                    .as_mut()
                    .expect("linker should be initialized"),
                |state| &mut state.wasi,
            )?;

            self.instance_pre = Some(
                self.linker
                    .as_mut()
                    .expect("linker should be initialized")
                    .instantiate_pre(
                        &WasmModule::from_binary(self.engine.as_ref().expect("engine should be intialized"), &self.binary)
                            .map_err(|e| ModuleError::InstantiationError(format!("failed to compile module: {}", e)))?
                    )
                    .map_err(|e| ModuleError::InstantiationError(format!("failed to create instance pre: {}", e)))?
            )
        }

        let mut store = Store::new(
            self.engine
                .as_ref()
                .expect("engine should be intialized"),
            ModuleState {
                wasi: self.environment
                    .clone()
                    .into(),
                limits: self.limits
                    .clone()
                    .into(),
            }
        );
        store.limiter(|s| &mut s.limits);

        self.instance = Some(
            self.instance_pre
                .as_ref()
                .expect("instance_pre should be initialized")
                .instantiate(&mut store)
                .map_err(|e| ModuleError::InstantiationError(format!("failed to instantiate module: {}", e)))?
        );

        self.store = Some(store);

        // Invoke method `_initialize` directly through wasmtime's API
        // instead of relying on invoking via binmod because binmod will try to invoke guest exported
        // methods and crash if the initializer haven't been called yet.
        if let Some(initialize_func) = self.instance
            .as_ref()
            .unwrap()
            .get_func(
                self.store
                    .as_mut()
                    .unwrap()
                    .as_context_mut(),
                "_initialize"
            )
        {
            initialize_func
                .typed::<(), ()>(
                    self.store
                        .as_ref()
                        .unwrap()
                        .as_context()
                )?
                .call(
                    self.store
                        .as_mut()
                        .unwrap()
                        .as_context_mut(),
                    ()
                )
                .map_err(|e| ModuleError::InstantiationError(format!("failed to call _initialize: {}", e)))?;
        }

        // Now we invoke the binmod initializer `initialize` if it exists.
        if let Err(e) = self.typed_call::<()>("initialize", ()) {
            if !matches!(e, ModuleError::FunctionNotFound(_)) {
                return Err(e);
            }
        }

        Ok(self)
    }

    /// Call a function within the module with typed arguments and return value.
    /// 
    /// # Arguments
    /// * `name` - The name of the function to call
    /// * `args` - The arguments to pass to the function
    /// 
    /// # Returns
    /// A result containing the return value of the function or an error
    /// if the call fails or the module is not instantiated
    pub fn typed_call<R>(&mut self, name: impl AsRef<str>, args: impl IntoFnInput) -> ModuleResult<R>
    where
        R: DeserializeOwned,
    {
        Ok(
            self
            .call(name.as_ref(), args.into_fn_input()?)?
            .into_result::<R>()?
        )
    }

    /// Call a function within the module.
    /// 
    /// # Arguments
    /// * `name` - The name of the function to call
    /// * `input` - The input to pass to the function
    /// 
    /// # Returns
    /// A result containing the [`FnResult`](crate::result::FnResult) of the function call or an error
    /// if the call fails or the module is not instantiated
    pub fn call(&mut self, name: impl AsRef<str>, input: FnInput) -> ModuleResult<FnResult> {
        let store = self.store.as_mut().ok_or(ModuleError::NotInstantiated)?;
        let instance = self.instance.as_ref().ok_or(ModuleError::NotInstantiated)?;
        let memory = MemoryOps::from_instance(instance, store)?;

        let func = instance
            .get_typed_func::<(u32, u32), u64>(store.as_context_mut(), name.as_ref())
            .map_err(|e| ModuleError::FunctionNotFound(format!("failed to get function '{}': {}", name.as_ref(), e)))?;

        let (input_ptr, input_len) = memory.write(
            store.as_context_mut(),
            &input
                .to_bytes()?
        )?;
        let (result_ptr, result_len) = unpack_ptr(
            func.call(store.as_context_mut(), (input_ptr as u32, input_len as u32))?,
        );

        Ok(FnResult::from_bytes(
            &memory.read(
                store.as_context_mut(),
                result_ptr,
                result_len,
            )?
        )?)
    }
}

impl Clone for Module {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            namespace: self.namespace.clone(),
            binary: self.binary.clone(),
            environment: self.environment.clone(),
            config: self.config.clone(),
            limits: self.limits.clone(),
            host_fns: self.host_fns.clone(),
            engine: self.engine.clone(),
            store: None,
            linker: self.linker.clone(),
            instance_pre: self.instance_pre.clone(),
            instance: None,
        }
    }
}

/// Represents a Binmod Module with host functions, and provides methods
/// to instantiate and call functions within the module asynchronously.
/// 
/// # Examples
/// ```rust
/// use binmod::{module::AsyncModule, config::ModuleEnv};
/// 
/// let mut module = AsyncModule::builder()
///     .from_file("path/to/module.wasm")
///     .with_name("my_module")
///     .with_namespace("binmod")
///     .with_environment(
///         ModuleEnv::default()
///             .arg("path/to/module.wasm")
///             .inherit_env()
///             .inherit_network()
///     )
///     .host_fn("host_function", |arg1: i32, arg2: String| -> Result<String, String> {
///         Ok(format!("Host function called with args: {}, {}", arg1, arg2))
///     })
///     .build()?
///     .instantiate()
///     .await?;
/// 
/// let result: String = module
///     .typed_call("guest_function", (42, "Hello".to_string()))
///     .await?;
/// println!("Result from guest function: {}", result);
/// ```
/// 
/// # Note
/// 
/// The async module API is experimental and may have performance implications and limited support.
/// Do not use in production environments without thorough testing.
pub struct AsyncModule {
    name: String,
    namespace: String,
    binary: Vec<u8>,
    environment: ModuleEnv,
    config: ModuleConfig,
    limits: ModuleLimits,
    fuel_yield_interval: Option<u64>,
    host_fns: HashMap<String, HostFn>,
    engine: Option<Engine>,
    store: Option<Store<ModuleState>>,
    linker: Option<Linker<ModuleState>>,
    instance_pre: Option<InstancePre<ModuleState>>,
    instance: Option<Instance>,
}

impl AsyncModule {
    /// Create a new Binmod Async Module.
    /// 
    /// # Arguments
    /// * `binary` - The WebAssembly binary code of the module
    /// * `name` - The name of the module
    /// * `namespace` - The namespace for the module's host functions
    /// * `environment` - The environment configuration for the module
    /// * `config` - The configuration for the module
    /// * `limits` - The resource limits for the module
    /// * `host_fns` - A map of host function names to HostFn instances
    ///
    /// # Returns
    /// A new [`AsyncModule`](crate::module::AsyncModule) instance
    pub fn new(
        binary: Vec<u8>,
        name: impl Into<String>,
        namespace: impl Into<String>,
        environment: ModuleEnv,
        config: ModuleConfig,
        limits: ModuleLimits,
        fuel_yield_interval: Option<u64>,
        host_fns: HashMap<String, HostFn>,
    ) -> Self {
        Self {
            name: name.into(),
            namespace: namespace.into(),
            binary,
            environment,
            config,
            limits,
            fuel_yield_interval,
            host_fns,
            engine: None,
            store: None,
            linker: None,
            instance_pre: None,
            instance: None,
        }
    }

    /// Create a new [`ModuleBuilder`](crate::module::ModuleBuilder)
    /// for constructing a [`AsyncModule`](crate::module::AsyncModule).
    pub fn builder() -> ModuleBuilder {
        ModuleBuilder::new()
    }

    /// Get the name of the module.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the namespace of the module.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Get the binary code of the module.
    pub fn binary(&self) -> &[u8] {
        &self.binary
    }

    /// Get the environment configuration of the module.
    pub fn environment(&self) -> &ModuleEnv {
        &self.environment
    }

    /// Check if the module has been instantiated.
    pub fn is_instantiated(&self) -> bool {
        self.instance.is_some()
    }

    /// Set the fuel for the module's store.
    /// 
    /// # Arguments
    /// * `fuel` - The amount of fuel to set
    /// 
    /// # Returns
    /// A result indicating success or an error
    /// if fuel is not enabled or the module is not instantiated
    pub fn set_fuel(&mut self, fuel: u64) -> ModuleResult<()> {
        self.store
            .as_mut()
            .ok_or(ModuleError::NotInstantiated)?
            .set_fuel(fuel)
            .map_err(|_| ModuleError::FuelNotEnabled)?;

        Ok(())
    }

    /// Get the remaining fuel for the module's store.
    /// 
    /// # Returns
    /// A result containing the remaining fuel or an error
    /// if fuel is not enabled or the module is not instantiated
    pub fn get_fuel(&mut self) -> ModuleResult<u64> {
        Ok(
            self.store
                .as_mut()
                .ok_or(ModuleError::NotInstantiated)?
                .get_fuel()
                .map_err(|_| ModuleError::FuelNotEnabled)?
        )
    }

    /// Set the epoch deadline for the module's store.
    /// 
    /// # Arguments
    /// * `deadline` - The epoch deadline to set
    /// 
    /// # Returns
    /// A result indicating success or an error
    /// if the module is not instantiated
    pub fn set_epoch_deadline(&mut self, deadline: u64) -> ModuleResult<()> {
        self.store
            .as_mut()
            .ok_or(ModuleError::NotInstantiated)?
            .set_epoch_deadline(deadline);
    
        Ok(())
    }

    /// Increment the epoch for the module's store.
    /// 
    /// # Returns
    /// A result indicating success or an error
    /// if the module is not instantiated
    pub fn increment_epoch(&mut self) -> ModuleResult<()> {
        self.engine
            .as_mut()
            .ok_or(ModuleError::NotInstantiated)?
            .increment_epoch();

        Ok(())
    }

    /// Instantiate the module.
    /// 
    /// # Returns
    /// A result containing the instantiated module or an error
    /// if instantiation fails or the module is already instantiated
    pub async fn instantiate(mut self) -> ModuleResult<Self> {
        if self.is_instantiated() {
            return Err(ModuleError::AlreadyInstantiated);
        }

        if !self.engine.is_some() {
            let mut config: Config = self.config
                .clone()
                .into();

            // Async requires fuel to be enabled
            config.async_support(true);
            config.consume_fuel(true);

            let engine = Engine::new(&config)?;
            let mut linker = Linker::<ModuleState>::new(&engine);

            // All hosts expect a host_alloc and host_dealloc function in
            // the `binmod` namespace to manage memory between host and guest.
            linker.func_wrap(
                "binmod",
                "host_alloc",
                |mut caller: Caller<ModuleState>, size: u32| -> u32 {
                    futures::executor::block_on(async {
                        caller
                            .get_export("guest_alloc")
                            .and_then(|e| e.into_func())
                            .ok_or_else(|| anyhow::anyhow!("failed to find guest_alloc"))
                            .unwrap()
                            .typed::<u32, u32>(&caller)
                            .unwrap()
                            .call_async(&mut caller, size)
                            .await
                            .unwrap()
                    })
                }
            )?;
            linker.func_wrap(
                "binmod",
                "host_dealloc",
                |mut caller: Caller<ModuleState>, ptr: u32, size: u32| {
                    futures::executor::block_on(async {
                        caller
                            .get_export("guest_dealloc")
                            .and_then(|e| e.into_func())
                            .ok_or_else(|| anyhow::anyhow!("failed to find guest_dealloc"))
                            .unwrap()
                            .typed::<(u32, u32), ()>(&caller)
                            .unwrap()
                            .call_async(&mut caller, (ptr, size))
                            .await
                            .unwrap();
                    });
                }
            )?;

            for (name, host_fn) in &self.host_fns {
                linker.func_wrap(
                    &self.namespace,
                    &name,
                    host_fn.clone().into_func_async(),
                )?;
            }

            self.engine = Some(engine);
            self.linker = Some(linker);
        }

        if !self.instance_pre.is_some() {
            p1::add_to_linker_async(
                self.linker
                    .as_mut()
                    .expect("linker should be initialized"),
                |state| &mut state.wasi,
            )?;

            self.instance_pre = Some(
                self.linker
                    .as_mut()
                    .expect("linker should be initialized")
                    .instantiate_pre(
                        &WasmModule::from_binary(self.engine.as_ref().expect("engine should be intialized"), &self.binary)
                            .map_err(|e| ModuleError::InstantiationError(format!("failed to compile module: {}", e)))?
                    )
                    .map_err(|e| ModuleError::InstantiationError(format!("failed to create instance pre: {}", e)))?
            )
        }

        let mut store = Store::new(
            self.engine
                .as_ref()
                .expect("engine should be intialized"),
            ModuleState {
                wasi: self.environment
                    .clone()
                    .into(),
                limits: self.limits
                    .clone()
                    .into(),
            }
        );

        // We start with unlimited fuel for async modules
        // and ensure execution is paused for an async yield
        // everytime it consumes `n` units of fuel.
        store.set_fuel(u64::MAX)
            .map_err(|_| ModuleError::FuelNotEnabled)?;
        store.fuel_async_yield_interval(Some(self.fuel_yield_interval.unwrap_or(10000)))?;
        store.limiter(|s| &mut s.limits);

        self.instance = Some(
            self.instance_pre
                .as_ref()
                .expect("instance_pre should be initialized")
                .instantiate_async(&mut store)
                .await
                .map_err(|e| ModuleError::InstantiationError(format!("failed to instantiate module: {}", e)))?
        );

        self.store = Some(store);

        // Invoke method `_initialize` directly through wasmtime's API
        // instead of relying on invoking via binmod because binmod will try to invoke guest exported
        // methods and crash if the initializer haven't been called yet.
        if let Some(initialize_func) = self.instance
            .as_ref()
            .unwrap()
            .get_func(
                self.store
                    .as_mut()
                    .unwrap()
                    .as_context_mut(),
                "_initialize"
            )
        {
            initialize_func
                .typed::<(), ()>(
                    self.store
                        .as_ref()
                        .unwrap()
                        .as_context()
                )?
                .call_async(
                    self.store
                        .as_mut()
                        .unwrap()
                        .as_context_mut(),
                    ()
                )
                .await
                .map_err(|e| ModuleError::InstantiationError(format!("failed to call _initialize: {}", e)))?;
        }

        // Now we invoke the binmod initializer `initialize` if it exists.
        if let Err(e) = self.typed_call::<()>("initialize", ()).await {
            if !matches!(e, ModuleError::FunctionNotFound(_)) {
                return Err(e);
            }
        }

        Ok(self)
    }

    /// Call a function within the module with typed arguments and return value.
    /// 
    /// # Arguments
    /// * `name` - The name of the function to call
    /// * `args` - The arguments to pass to the function
    /// 
    /// # Returns
    /// A result containing the return value of the function or an error
    /// if the call fails or the module is not instantiated
    pub async fn typed_call<R>(&mut self, name: impl AsRef<str>, args: impl IntoFnInput) -> ModuleResult<R>
    where
        R: DeserializeOwned,
    {
        Ok(
            self
                .call(name.as_ref(), args.into_fn_input()?)
                .await?
                .into_result::<R>()?
        )
    }

    /// Call a function within the module.
    /// 
    /// # Arguments
    /// * `name` - The name of the function to call
    /// * `input` - The input to pass to the function
    /// 
    /// # Returns
    /// A result containing the [`FnResult`](crate::result::FnResult) of the
    /// function call or an error if the call fails or the module is not instantiated
    pub async fn call(&mut self, name: impl AsRef<str>, input: FnInput) -> ModuleResult<FnResult> {
        let store = self.store.as_mut().ok_or(ModuleError::NotInstantiated)?;
        let instance = self.instance.as_ref().ok_or(ModuleError::NotInstantiated)?;
        let memory = AsyncMemoryOps::from_instance(instance, store)?;

        let func = instance
            .get_typed_func::<(u32, u32), u64>(store.as_context_mut(), name.as_ref())
            .map_err(|e| ModuleError::FunctionNotFound(format!("failed to get function '{}': {}", name.as_ref(), e)))?;

        let (input_ptr, input_len) = memory
            .write(
                store.as_context_mut(),
                &input.to_bytes()?
            )
            .await?;
        let (result_ptr, result_len) = unpack_ptr(
            func
                .call_async(store.as_context_mut(), (input_ptr as u32, input_len as u32))
                .await?,
        );

        Ok(FnResult::from_bytes(
            &memory
                .read(
                    store.as_context_mut(),
                    result_ptr,
                    result_len,
                )
                .await?
        )?)
    }
}

impl Clone for AsyncModule {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            namespace: self.namespace.clone(),
            binary: self.binary.clone(),
            environment: self.environment.clone(),
            config: self.config.clone(),
            limits: self.limits.clone(),
            fuel_yield_interval: self.fuel_yield_interval.clone(),
            host_fns: self.host_fns.clone(),
            engine: self.engine.clone(),
            store: None,
            linker: self.linker.clone(),
            instance_pre: self.instance_pre.clone(),
            instance: None,
        }
    }
}

/// Builder for constructing a [`Module`](crate::module::Module)
/// or an [`AsyncModule`](crate::module::AsyncModule).
#[derive(Clone)]
pub struct ModuleBuilder {
    name: Option<String>,
    namespace: Option<String>,
    binary: Option<Vec<u8>>,
    config: Option<ModuleConfig>,
    limits: Option<ModuleLimits>,
    environment: Option<ModuleEnv>,
    host_fns: HashMap<String, HostFn>,
    fuel_yield_interval: Option<u64>,
}

impl ModuleBuilder {
    /// Create a new [`ModuleBuilder`](crate::module::ModuleBuilder) instance.
    pub fn new() -> Self {
        Self {
            name: None,
            namespace: None,
            binary: None,
            config: None,
            limits: None,
            environment: None,
            host_fns: HashMap::new(),
            fuel_yield_interval: None,
        }
    }

    /// Set the binary code for the module.
    /// 
    /// # Arguments
    /// * `binary` - The WebAssembly binary code
    /// 
    /// # Returns
    /// The updated ModuleBuilder instance
    pub fn with_binary(mut self, binary: Vec<u8>) -> Self {
        self.binary = Some(binary);
        self
    }

    /// Set the binary code for the module from a file.
    /// 
    /// # Arguments
    /// * `path` - The path to the WebAssembly binary file
    /// 
    /// # Returns
    /// A result containing the updated ModuleBuilder instance or an error
    pub fn from_file(mut self, path: impl AsRef<Path>) -> ModuleResult<Self> {
        self.binary = Some(fs::read(path)?);
        Ok(self)
    }

    /// Set the name for the module.
    /// 
    /// # Arguments
    /// * `name` - The name to set
    /// 
    /// # Returns
    /// The updated ModuleBuilder instance
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the namespace for the module.
    /// 
    /// # Arguments
    /// * `namespace` - The namespace to set
    /// 
    /// # Returns
    /// The updated ModuleBuilder instance
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Set the environment for the module.
    /// 
    /// # Arguments
    /// * `environment` - The environment to set
    /// 
    /// # Returns
    /// The updated ModuleBuilder instance
    pub fn with_environment(mut self, environment: ModuleEnv) -> Self {
        self.environment = Some(environment);
        self
    }

    /// Set the configuration for the module.
    /// 
    /// # Arguments
    /// * `config` - The configuration to set
    /// 
    /// # Returns
    /// The updated ModuleBuilder instance
    pub fn with_config(mut self, config: ModuleConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the resource limits for the module.
    /// 
    /// # Arguments
    /// * `limits` - The resource limits to set
    /// 
    /// # Returns
    /// The updated ModuleBuilder instance
    pub fn with_limits(mut self, limits: ModuleLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Add a host function to the module.
    /// 
    /// # Arguments
    /// * `name` - The name of the host function
    /// * `func` - The Rust function or closure to be wrapped as a Host Function
    /// 
    /// # Returns
    /// The updated ModuleBuilder instance
    pub fn host_fn<F, Args>(mut self, name: impl Into<String>, func: F) -> Self
    where
        HostFnWrapper<F, Args>: HostFnCallable + 'static,
    {
        self.host_fns.insert(name.into(), HostFn::new(func));
        self
    }

    /// Set the fuel yield interval for async modules.
    /// 
    /// # Arguments
    /// * `interval` - The fuel yield interval to set
    /// 
    /// # Returns
    /// The updated ModuleBuilder instance
    pub fn with_fuel_yield_interval(mut self, interval: u64) -> Self {
        self.fuel_yield_interval = Some(interval);
        self
    }

    /// Build a [`Module`](crate::module::Module) from the builder configuration.
    /// 
    /// # Returns
    /// A result containing the constructed Module or an error
    pub fn build(self) -> ModuleResult<Module> {
        Ok(Module::new(
            self.binary.ok_or_else(|| ModuleError::InvalidModuleConfig("Binary not provided".into()))?,
            self.name.ok_or_else(|| ModuleError::InvalidModuleConfig("Name not provided".into()))?,
            self.namespace.unwrap_or("env".into()),
            self.environment.unwrap_or(ModuleEnv::default()),
            self.config.unwrap_or(ModuleConfig::default()),
            self.limits.unwrap_or(ModuleLimits::default()),
            self.host_fns,
        ))
    }

    /// Build an [`AsyncModule`](crate::module::AsyncModule) from the builder configuration.
    /// 
    /// # Returns
    /// A result containing the constructed AsyncModule or an error
    pub fn build_async(self) -> ModuleResult<AsyncModule> {
        Ok(AsyncModule::new(
            self.binary.ok_or_else(|| ModuleError::InvalidModuleConfig("Binary not provided".into()))?,
            self.name.ok_or_else(|| ModuleError::InvalidModuleConfig("Name not provided".into()))?,
            self.namespace.unwrap_or("env".into()),
            self.environment.unwrap_or(ModuleEnv::default()),
            self.config.unwrap_or(ModuleConfig::default()),
            self.limits.unwrap_or(ModuleLimits::default()),
            self.fuel_yield_interval,
            self.host_fns,
        ))
    }
}