use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
    sync::{Arc, Condvar, Mutex},
    future::Future,
};
use futures::stream::{self, StreamExt, TryStreamExt};
use mea::{condvar::{Condvar as AsyncCondvar}, mutex::{Mutex as AsyncMutex}};

use crate::{module::{Module, AsyncModule, ModuleBuilder}, error::{ModuleResult, ModuleError}};


/// A pool of pre-instantiated modules for reuse.
/// 
/// This struct allows for leasing and returning modules in a thread-safe manner.
/// 
/// # Examples
/// 
/// ```rust
/// use binmod::{module::ModuleBuilder, pool::ModulePool};
/// 
/// // Create a module
/// let module = Module::builder()
///     .from_file("my_module.wasm")?
///     .with_name("my_module")
///     .with_namespace("binmod")
///     .host_fn("host_add", |a: i64, b: i64| -> Result<i64> { Ok(a + b) })
///     .build()?
///     .instantiate()?;
/// 
/// // Create a module pool with 4 instances of the module
/// let pool = ModulePool::builder()
///     .with_module(module)
///     .with_count(4)
///     .build()?;
/// 
/// // Lease a module from the pool
/// let mut leased_module = pool.lease();
/// leased_module.typed_call::<i64>("add", (2, 3))?;
/// // The module is automatically returned to the pool when `leased_module` goes out of scope
/// // via `Drop` or can be manually released using `leased_module.release()`.
/// leased_module.release();
/// 
/// // Or use `scoped` to automatically manage the lease
/// pool.scoped(|module| {
///     // Use the leased module here
///     let result = module.typed_call::<i64>("add", (5, 7));
///     result
/// })?;
/// ```
#[derive(Clone)]
pub struct ModulePool {
    modules: Arc<(Mutex<VecDeque<Module>>, Condvar)>,
}

impl ModulePool {
    /// Creates a new ModulePool with the given modules.
    /// 
    /// # Arguments
    /// * `modules` - A vector of pre-instantiated modules to populate the pool.
    /// 
    /// # Returns
    /// A new ModulePool instance.
    pub fn new(modules: Vec<Module>) -> Self {
        Self {
            modules: Arc::new((Mutex::new(VecDeque::from(modules)), Condvar::new())),
        }
    }

    /// Creates a new ModulePoolBuilder.
    /// 
    /// # Returns
    /// A new ModulePoolBuilder instance.
    pub fn builder() -> ModulePoolBuilder {
        ModulePoolBuilder::new()
    }

    /// Leases a module from the pool, blocking if necessary until one is available.
    /// 
    /// # Returns
    /// A ModuleLease representing the leased module.
    pub fn lease(&self) -> ModuleLease<'_> {
        let (lock, cvar) = &*self.modules;
        let mut modules = lock.lock().unwrap();
        
        while modules.is_empty() {
            modules = cvar.wait(modules).unwrap();
        }

        ModuleLease {
            pool: self,
            module: Some(modules.pop_front().unwrap()),
        }
    }

    /// Attempts to lease a module from the pool without blocking.
    /// 
    /// # Returns
    /// An Option containing a ModuleLease if a module was available, or None otherwise.
    pub fn try_lease(&self) -> Option<ModuleLease<'_>> {
        let (lock, _) = &*self.modules;
        let mut modules = lock.lock().unwrap();
        
        if modules.is_empty() {
            None
        } else {
            Some(ModuleLease {
                pool: self,
                module: Some(modules.pop_front().unwrap()),
            })
        }
    }

    /// Returns a module to the pool.
    /// 
    /// # Arguments
    /// * `module` - The module to return to the pool.
    pub fn return_module(&self, module: Module) {
        let (lock, cvar) = &*self.modules;
        let mut modules = lock.lock().unwrap();
        
        modules.push_back(module);
        cvar.notify_one();
    }

    /// Executes a function with a leased module from the pool.
    /// The module is automatically returned to the pool after the function completes.
    /// 
    /// # Arguments
    /// * `func` - The function to execute with the leased module.
    /// 
    /// # Returns
    /// The result of the function.
    pub fn scoped<F, R>(&self, func: F) -> R
    where
        F: FnOnce(&mut Module) -> R,
    {
        let mut lease = self.lease();
        let result = (func)(&mut lease);
        lease.release();
        result
    }
}

/// A lease on a module from a ModulePool.
pub struct ModuleLease<'a> {
    pool: &'a ModulePool,
    module: Option<Module>,
}

impl<'a> ModuleLease<'a> {
    /// Releases the leased module back to the pool.
    pub fn release(&mut self) {
        if let Some(module) = self.module.take() {
            self.pool.return_module(module);
        }
    }
}

impl Deref for ModuleLease<'_> {
    type Target = Module;

    fn deref(&self) -> &Self::Target {
        self.module.as_ref().unwrap()
    }
}

impl DerefMut for ModuleLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.module.as_mut().unwrap()
    }
}

impl Drop for ModuleLease<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

/// A builder for creating ModulePool instances.
pub struct ModulePoolBuilder {
    template: Option<Module>,
    builder: Option<ModuleBuilder>,
    count: usize,
}

impl ModulePoolBuilder {
    /// Creates a new ModulePoolBuilder.
    /// 
    /// # Returns
    /// A new ModulePoolBuilder instance.
    pub fn new() -> Self {
        Self {
            template: None,
            builder: None,
            count: 0,
        }
    }

    /// Sets the module template to use for instantiation.
    /// 
    /// # Arguments
    /// * `module` - The module template to use.
    /// 
    /// # Returns
    /// The updated ModulePoolBuilder instance.
    pub fn with_module(mut self, module: Module) -> Self {
        self.template = Some(module);
        self
    }

    /// Sets the module builder to use for instantiation.
    /// 
    /// # Arguments
    /// * `builder` - The module builder to use.
    /// 
    /// # Returns
    /// The updated ModulePoolBuilder instance.
    pub fn with_builder(mut self, builder: ModuleBuilder) -> Self {
        self.builder = Some(builder);
        self
    }

    /// Sets the number of modules to instantiate in the pool.
    /// 
    /// # Arguments
    /// * `count` - The number of modules to instantiate.
    /// 
    /// # Returns
    /// The updated ModulePoolBuilder instance.
    pub fn with_count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    /// Build a ModulePool instance from the provided configuration.
    pub fn build(mut self) -> ModuleResult<ModulePool> {
        if self.count == 0 {
            return Err(ModuleError::InstantiationError(
                "ModulePool must have a count greater than zero".to_string(),
            ));
        }

        let mut modules = vec![];

        if let Some(mut template) = self.template.take() {
            if !template.is_instantiated() {
                template = template.instantiate()?;
            }

            modules.extend(
                (0..self.count-1)
                    .map(|_| template.clone().instantiate())
                    .collect::<Result<Vec<Module>, _>>()?
                    .into_iter()
                    .chain(vec![template].into_iter())
                    .collect::<Vec<_>>()
            );
        } else if let Some(builder) = self.builder.take() {
            modules.extend(
                (0..self.count)
                    .map(|_| builder.clone().build()?.instantiate())
                    .collect::<Result<Vec<Module>, _>>()?
            );
        } else {
            return Err(ModuleError::InstantiationError(
                "Either a module or a builder must be provided to build a ModulePool".to_string(),
            ));
        }

        Ok(ModulePool::new(modules))
    }
}

impl Default for ModulePoolBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A pool of pre-instantiated asynchronous modules for reuse.
/// 
/// This struct allows for leasing and returning asynchronous modules in an async thread-safe manner.
/// 
/// # Examples
/// 
/// ```rust
/// use binmod::{module::AsyncModule, pool::AsyncModulePool};
/// 
/// // Create an asynchronous module
/// let module = AsyncModule::builder()
///     .from_file("my_module.wasm")?
///     .with_name("my_module")
///     .with_namespace("binmod")
///     .host_fn("host_add", |a: i64, b: i64| -> Result<i64> { Ok(a + b) })
///     .build_async()
///     .await?
///     .instantiate()
///     .await?;
/// 
/// // Create an asynchronous module pool with 4 instances of the module
/// let pool = AsyncModulePool::builder()
///     .with_module(module)
///     .with_count(4)
///     .build()
///     .await?;
/// 
/// // Lease an asynchronous module from the pool
/// let mut leased_module = pool.lease().await;
/// leased_module.typed_call::<i64>("add", (2, 3)).await?;
/// // The module is NOT automatically returned to the pool when `leased_module` goes out of scope
/// // unlike the synchronous ModulePool. It must be manually released using `leased_module.release().await`.
/// leased_module.release().await;
/// 
/// // Or use `scoped` to automatically manage the lease
/// pool.scoped(|module| {
///     // Use the leased module here
///     let result = module.typed_call::<i64>("add", (5, 7)).await;
///     result
/// }).await?;
/// ```
#[derive(Clone)]
pub struct AsyncModulePool {
    modules: Arc<(AsyncMutex<VecDeque<AsyncModule>>, AsyncCondvar)>,
}

impl AsyncModulePool {
    /// Creates a new AsyncModulePool with the given modules.
    /// 
    /// # Arguments
    /// * `modules` - A vector of pre-instantiated asynchronous modules to populate the pool
    /// 
    /// # Returns
    /// A new AsyncModulePool instance.
    pub fn new(modules: Vec<AsyncModule>) -> Self {
        Self {
            modules: Arc::new((AsyncMutex::new(VecDeque::from(modules)), AsyncCondvar::new())),
        }
    }

    /// Creates a new AsyncModulePoolBuilder.
    /// 
    /// # Returns
    /// A new AsyncModulePoolBuilder instance.
    pub fn builder() -> AsyncModulePoolBuilder {
        AsyncModulePoolBuilder::new()
    }

    /// Leases a module from the pool, asynchronously blocking if necessary until one is available.
    /// 
    /// # Returns
    /// An AsyncModuleLease representing the leased module.
    pub async fn lease(&self) -> AsyncModuleLease<'_> {
        let (lock, cvar) = &*self.modules;
        let mut modules = lock.lock().await;
        
        while modules.is_empty() {
            modules = cvar.wait(modules).await;
        }

        AsyncModuleLease {
            pool: self,
            module: Some(modules.pop_front().unwrap()),
        }
    }

    /// Returns a module to the pool.
    /// 
    /// # Arguments
    /// * `module` - The asynchronous module to return to the pool.
    pub async fn return_module(&self, module: AsyncModule) {
        let (lock, cvar) = &*self.modules;
        let mut modules = lock.lock().await;
        
        modules.push_back(module);
        cvar.notify_one();
    }

    /// Executes a function with a leased asynchronous module from the pool.
    /// The module is automatically returned to the pool after the function completes.
    /// 
    /// # Arguments
    /// * `func` - The function to execute with the leased asynchronous module.
    /// 
    /// # Returns
    /// The result of the function.
    pub async fn scoped<F, Fut, R>(&self, func: F) -> R
    where
        F: FnOnce(&mut AsyncModule) -> Fut,
        Fut: Future<Output = R>,
    {
        let mut lease = self.lease().await;
        let result = (func)(&mut lease).await;
        lease.release().await;
        result
    }
}

/// A lease on an asynchronous module from an AsyncModulePool.
pub struct AsyncModuleLease<'a> {
    pool: &'a AsyncModulePool,
    module: Option<AsyncModule>,
}

impl<'a> AsyncModuleLease<'a> {
    /// Releases the leased asynchronous module back to the pool.
    pub async fn release(&mut self) {
        if let Some(module) = self.module.take() {
            self.pool.return_module(module).await;
        }
    }
}

impl Deref for AsyncModuleLease<'_> {
    type Target = AsyncModule;

    fn deref(&self) -> &Self::Target {
        self.module.as_ref().unwrap()
    }
}

impl DerefMut for AsyncModuleLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.module.as_mut().unwrap()
    }
}

/// A builder for creating AsyncModulePool instances.
pub struct AsyncModulePoolBuilder {
    template: Option<AsyncModule>,
    builder: Option<ModuleBuilder>,
    count: usize,
}

impl AsyncModulePoolBuilder {
    /// Creates a new AsyncModulePoolBuilder.
    /// 
    /// # Returns
    /// A new AsyncModulePoolBuilder instance.
    pub fn new() -> Self {
        Self {
            template: None,
            builder: None,
            count: 0,
        }
    }

    /// Sets the asynchronous module template to use for instantiation.
    /// 
    /// # Arguments
    /// * `module` - The asynchronous module template to use.
    /// 
    /// # Returns
    /// The updated AsyncModulePoolBuilder instance.
    pub fn with_module(mut self, module: AsyncModule) -> Self {
        self.template = Some(module);
        self
    }

    /// Sets the module builder to use for instantiation.
    /// 
    /// # Arguments
    /// * `builder` - The module builder to use.
    /// 
    /// # Returns
    /// The updated AsyncModulePoolBuilder instance.
    pub fn with_builder(mut self, builder: ModuleBuilder) -> Self {
        self.builder = Some(builder);
        self
    }

    /// Sets the number of asynchronous modules to instantiate in the pool.
    /// 
    /// # Arguments
    /// * `count` - The number of asynchronous modules to instantiate.
    /// 
    /// # Returns
    /// The updated AsyncModulePoolBuilder instance.
    pub fn with_count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    /// Build an AsyncModulePool instance from the provided configuration.
    pub async fn build(mut self) -> ModuleResult<AsyncModulePool> {
        if self.count == 0 {
            return Err(ModuleError::InstantiationError(
                "AsyncModulePool must have a count greater than zero".to_string(),
            ));
        }

        let mut modules = vec![];

        if let Some(mut template) = self.template.take() {
            if !template.is_instantiated() {
                template = template.instantiate().await?;
            }

            modules.extend(
                stream::iter(0..self.count-1)
                    .then(|_| template.clone().instantiate())
                    .try_collect::<Vec<_>>()
                    .await?
                    .into_iter()
                    .chain(vec![template].into_iter())
                    .collect::<Vec<_>>()
            );

        } else if let Some(builder) = self.builder.take() {
            modules.extend(
                stream::iter(0..self.count)
                    .then(|_| async { builder.clone().build_async()?.instantiate().await })
                    .try_collect::<Vec<_>>()
                    .await?
            );
        } else {
            return Err(ModuleError::InstantiationError(
                "Either an async module or a builder must be provided to build an AsyncModulePool".to_string(),
            ));
        }

        Ok(AsyncModulePool::new(modules))
    }
}

impl Default for AsyncModulePoolBuilder {
    fn default() -> Self {
        Self::new()
    }
}
