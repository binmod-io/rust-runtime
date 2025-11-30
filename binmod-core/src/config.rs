use std::{env, sync::Arc, collections::HashMap, path::PathBuf, pin::Pin, net::SocketAddr, future::Future};
use serde::{Serialize, Deserialize};
use wasmtime::{Config, Strategy, Cache, CacheConfig, OptLevel, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, p1::WasiP1Ctx, DirPerms, FilePerms, sockets::SocketAddrUse};


/// Enum for selecting the module compiler strategy.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ModuleCompiler {
    Auto,
    Cranelift,
    Winch,
}

impl From<ModuleCompiler> for Strategy {
    fn from(compiler: ModuleCompiler) -> Self {
        match compiler {
            ModuleCompiler::Auto => Strategy::Auto,
            ModuleCompiler::Cranelift => Strategy::Cranelift,
            ModuleCompiler::Winch => Strategy::Winch,
        }
    }
}

/// Struct for configuring a module.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ModuleConfig {
    /// The compiler strategy to use.
    /// 
    /// `Auto` lets Wasmtime choose the best strategy for the host machine.
    /// `Cranelift` uses the Cranelift compiler.
    /// `Winch` uses the Winch compiler.
    /// 
    /// Default is `Winch`.
    pub compiler: ModuleCompiler,
    /// Whether to enable epoch-based interruption.
    /// 
    /// This allows interrupting running WebAssembly code by incrementing the epoch counter.
    /// This is useful for implementing timeouts or cooperative multitasking.
    /// 
    /// Default is `false`.
    pub epoch_interruption: bool,
    /// Whether to enable fuel consumption.
    /// 
    /// This allows limiting the amount of computation a WebAssembly module can perform
    /// by consuming "fuel" for each instruction executed.
    /// This is useful for preventing infinite loops or excessive resource usage.
    /// 
    /// Default is `false`.
    pub consume_fuel: bool,
    /// Whether to enable caching of compiled modules.
    /// 
    /// This can improve performance by reusing previously compiled code.
    /// However, it may increase disk usage and complexity.
    /// 
    /// Default is `false`.
    pub cache: bool,
    /// Whether to enable WebAssembly threads.
    /// 
    /// This allows WebAssembly modules to use shared memory and spawn threads.
    /// 
    /// Default is `true`.
    pub threads: bool,
    /// Whether to enable WebAssembly tail calls.
    /// 
    /// This allows WebAssembly modules to perform tail calls, which can improve performance
    /// by avoiding stack growth during recursive calls.
    /// 
    /// Default is `false`.
    pub tail_call: bool,
    /// Whether to enable WebAssembly SIMD.
    /// 
    /// This allows WebAssembly modules to use SIMD instructions for parallel data processing,
    /// which can improve performance for certain workloads.
    /// 
    /// Default is `true`.
    pub simd: bool,
    /// Whether to enable WebAssembly relaxed SIMD.
    /// 
    /// This allows WebAssembly modules to use relaxed SIMD instructions, which can improve
    /// performance for certain workloads at the cost of strict IEEE compliance.
    /// 
    /// Default is `false`.
    pub relaxed_simd: bool,
    /// Whether to enable deterministic behavior for relaxed SIMD.
    /// 
    /// This ensures that relaxed SIMD operations produce consistent results across different platforms
    /// and executions, at the cost of some performance.
    /// 
    /// Default is `false`.
    pub relaxed_simd_deterministic: bool,
    /// Whether to enable WebAssembly 64-bit memory.
    /// 
    /// This allows WebAssembly modules to use 64-bit memory addressing,
    /// which can support larger memory sizes.
    /// 
    /// Default is `false`.
    pub memory64: bool,
}

impl ModuleConfig {
    /// Create a new [`ModuleConfig`](crate::config::ModuleConfig) with default settings.
    pub fn new() -> Self {
        Self {
            compiler: ModuleCompiler::Winch,
            epoch_interruption: false,
            consume_fuel: false,
            cache: false,
            threads: true,
            tail_call: false,
            simd: true,
            relaxed_simd: false,
            relaxed_simd_deterministic: false,
            memory64: false,
        }
    }

    /// Set the compiler strategy.
    /// 
    /// # Arguments
    /// * `compiler` - The compiler strategy to set
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_compiler(mut self, compiler: ModuleCompiler) -> Self {
        self.compiler = compiler;
        self
    }

    /// Enable or disable epoch-based interruption.
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable epoch interruption
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_epoch_interruption(mut self, enabled: bool) -> Self {
        self.epoch_interruption = enabled;
        self
    }

    /// Enable or disable fuel consumption.
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable fuel consumption
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_consume_fuel(mut self, enabled: bool) -> Self {
        self.consume_fuel = enabled;
        self
    }

    /// Enable or disable module caching.
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable caching
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_cache(mut self, enabled: bool) -> Self {
        self.cache = enabled;
        self
    }

    /// Enable or disable WebAssembly threads.
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable threads
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_threads(mut self, enabled: bool) -> Self {
        self.threads = enabled;
        self
    }

    /// Enable or disable WebAssembly tail calls.
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable tail calls
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_tail_call(mut self, enabled: bool) -> Self {
        self.tail_call = enabled;
        self
    }

    /// Enable or disable SIMD support.
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable SIMD
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_simd(mut self, enabled: bool) -> Self {
        self.simd = enabled;
        self
    }

    /// Enable or disable relaxed SIMD support.
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable relaxed SIMD
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_relaxed_simd(mut self, enabled: bool) -> Self {
        self.relaxed_simd = enabled;
        self
    }

    /// Enable or disable deterministic behavior for relaxed SIMD.
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable deterministic relaxed SIMD
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_relaxed_simd_deterministic(mut self, enabled: bool) -> Self {
        self.relaxed_simd_deterministic = enabled;
        self
    }

    /// Enable or disable use of 64-bit memory.
    /// 
    /// # Arguments
    /// * `enabled` - Whether to enable 64-bit memory
    /// 
    /// # Returns
    /// The updated ModuleFeatureFlags instance
    pub fn with_memory64(mut self, enabled: bool) -> Self {
        self.memory64 = enabled;
        self
    }
}

impl Default for ModuleConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ModuleConfig> for Config {
    fn from(features: ModuleConfig) -> Self {
        let mut config = Config::new();

        if features.compiler == ModuleCompiler::Cranelift {
            config.cranelift_opt_level(OptLevel::Speed);
        }

        if features.cache {
            config.cache(Some(Cache::new(CacheConfig::default()).unwrap()));
        }

        config
            .strategy(features.compiler.into())
            .epoch_interruption(features.epoch_interruption)
            .consume_fuel(features.consume_fuel)
            .wasm_threads(features.threads)
            .wasm_tail_call(features.tail_call)
            .wasm_simd(features.simd)
            .wasm_relaxed_simd(features.relaxed_simd)
            .relaxed_simd_deterministic(features.relaxed_simd_deterministic)
            .wasm_memory64(features.memory64)
            .wasm_multi_value(true)
            .parallel_compilation(true);

        config
    }
}

/// Struct for configuring module limits.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ModuleLimits {
    /// The maximum number of bytes a linear memory can grow to.
    pub memory_size: i32,
}

impl ModuleLimits {
    /// Create a new ModuleLimits with default settings.
    pub fn new() -> Self {
        Self {
            memory_size: -1,
        }
    }
}

impl Default for ModuleLimits {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ModuleLimits> for StoreLimits {
    fn from(limits: ModuleLimits) -> Self {
        let mut builder = StoreLimitsBuilder::new();

        if limits.memory_size >= 0 {
            builder = builder.memory_size(limits.memory_size as usize);
        }

        builder.build()
    }
}

/// Enum representing the use of a socket address.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ModuleSocketAddrAction {
    /// Binding TCP socket
    TcpBind,
    /// Connecting TCP socket
    TcpConnect,
    /// Binding UDP socket
    UdpBind,
    /// Connecting UDP socket
    UdpConnect,
    /// Sending datagram on non-connected UDP socket
    UdpOutgoingDatagram,
}

impl From<ModuleSocketAddrAction> for SocketAddrUse {
    fn from(action: ModuleSocketAddrAction) -> Self {
        match action {
            ModuleSocketAddrAction::TcpBind => SocketAddrUse::TcpBind,
            ModuleSocketAddrAction::TcpConnect => SocketAddrUse::TcpConnect,
            ModuleSocketAddrAction::UdpBind => SocketAddrUse::UdpBind,
            ModuleSocketAddrAction::UdpConnect => SocketAddrUse::UdpConnect,
            ModuleSocketAddrAction::UdpOutgoingDatagram => SocketAddrUse::UdpOutgoingDatagram,
        }
    }
}

impl From<SocketAddrUse> for ModuleSocketAddrAction {
    fn from(action: SocketAddrUse) -> Self {
        match action {
            SocketAddrUse::TcpBind => ModuleSocketAddrAction::TcpBind,
            SocketAddrUse::TcpConnect => ModuleSocketAddrAction::TcpConnect,
            SocketAddrUse::UdpBind => ModuleSocketAddrAction::UdpBind,
            SocketAddrUse::UdpConnect => ModuleSocketAddrAction::UdpConnect,
            SocketAddrUse::UdpOutgoingDatagram => ModuleSocketAddrAction::UdpOutgoingDatagram,
        }
    }
}

/// Struct representing network configuration for a module environment.
#[derive(Clone)]
pub struct ModuleNetwork {
    /// Allow TCP connections.
    pub allow_tcp: bool,
    /// Allow UDP connections.
    pub allow_udp: bool,
    /// Allow DNS resolution.
    pub allow_dns: bool,
    /// A function to check whether a socket address and action is permitted.
    pub socket_check: Arc<
        dyn Fn(SocketAddr, ModuleSocketAddrAction) -> Pin<Box<dyn Future<Output = bool> + Send + Sync>>
            + Send
            + Sync
            + 'static
    >,
}

impl ModuleNetwork {
    /// Create a new ModuleNetwork with default settings.
    pub fn new() -> Self {
        Self {
            allow_tcp: true,
            allow_udp: true,
            allow_dns: true,
            socket_check: Arc::new(|_, _| Box::pin(async { false })),
        }
    }

    /// Inherit the same network permissions as the host.
    pub fn inherit(mut self) -> Self {
        self.allow_tcp = true;
        self.allow_udp = true;
        self.allow_dns = true;
        self.socket_check = Arc::new(|_, _| Box::pin(async { true }));
        self
    }
}

impl Default for ModuleNetwork {
    fn default() -> Self {
        Self::new()
    }
}

/// Struct representing the environment configuration for a module
/// such as arguments, environment variables, and mounted paths.
#[derive(Clone)]
pub struct ModuleEnv {
    /// Arguments to pass to the module.
    pub args: Option<Vec<String>>,
    /// Environment variables to set for the module.
    pub env: Option<HashMap<String, String>>,
    /// Host paths to mount into the module's filesystem.
    pub mount: Option<HashMap<String, PathBuf>>,
    /// Network configuration for the module.
    pub network: ModuleNetwork,
}

impl ModuleEnv {
    /// Create a new, empty ModuleEnv.
    pub fn new() -> Self {
        Self {
            args: None,
            env: None,
            mount: None,
            network: ModuleNetwork::default(),
        }
    }

    /// Inherit the current process's environment.
    pub fn inherit(mut self) -> Self {
        self.args = Some(env::args().skip(1).collect());
        self.env = Some(env::vars().collect());
        self.network = self.network.inherit();
        self
    }

    /// Inherit the current process's arguments.
    pub fn inherit_args(mut self) -> Self {
        self.args = Some(env::args().skip(1).collect());
        self
    }

    /// Inherit the current process's environment variables.
    pub fn inherit_env(mut self) -> Self {
        self.env = Some(env::vars().collect());
        self
    }

    /// Inherit the same network permissions as the host.
    pub fn inherit_network(mut self) -> Self {
        self.network = self.network.inherit();
        self
    }

    /// Add a single argument to the module environment.
    /// 
    /// # Arguments
    /// * `arg` - The argument to add
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        match &mut self.args {
            Some(args) => args.push(arg.into()),
            None => self.args = Some(vec![arg.into()]),
        }
        self
    }

    /// Add multiple arguments to the module environment.
    /// 
    /// # Arguments
    /// * `args` - An iterator of arguments to add
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        match &mut self.args {
            Some(existing) => existing.extend(args.into_iter().map(Into::into)),
            None => self.args = Some(args.into_iter().map(Into::into).collect()),
        }
        self
    }

    /// Add a single environment variable to the module environment.
    /// 
    /// # Arguments
    /// * `key` - The environment variable key
    /// * `value` - The environment variable value
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn env_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        match &mut self.env {
            Some(env_vars) => { env_vars.insert(key.into(), value.into()); },
            None => self.env = Some(HashMap::from([(key.into(), value.into())])),
        }
        self
    }

    /// Add multiple environment variables to the module environment.
    /// 
    /// # Arguments
    /// * `vars` - An iterator of (key, value) tuples representing environment variables
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn env_vars<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        match &mut self.env {
            Some(env_vars) => { env_vars.extend(
                vars.into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
            ); },
            None => self.env = Some(
                vars.into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect()
            )
        }
        self
    }

    /// Mount a host path into the module's filesystem.
    /// 
    /// # Arguments
    /// * `host_path` - The host path to mount
    /// * `guest_path` - The guest path inside the module
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn mount_path(mut self, host_path: impl Into<PathBuf>, guest_path: impl Into<String>) -> Self {
        match &mut self.mount {
            Some(mounts) => { mounts.insert(guest_path.into(), host_path.into()); },
            None => self.mount = Some(HashMap::from([(guest_path.into(), host_path.into())])),
        }
        self
    }

    /// Mount multiple host paths into the module's filesystem.
    /// 
    /// # Arguments
    /// * `paths` - An iterator of (host_path, guest_path) tuples to mount
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn mount_paths<I, HP, GP>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = (HP, GP)>,
        HP: Into<PathBuf>,
        GP: Into<String>,
    {
        match &mut self.mount {
            Some(mounts) => { mounts.extend(
                paths.into_iter()
                    .map(|(hp, gp)| (gp.into(), hp.into()))
            ); },
            None => self.mount = Some(
                paths.into_iter()
                    .map(|(hp, gp)| (gp.into(), hp.into()))
                    .collect()
            )
        }
        self
    }

    /// Allow or disallow TCP connections.
    /// 
    /// # Arguments
    /// * `allow` - Whether to allow TCP connections
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn allow_tcp(mut self, allow: bool) -> Self {
        self.network.allow_tcp = allow;
        self
    }

    /// Allow or disallow UDP connections.
    /// 
    /// # Arguments
    /// * `allow` - Whether to allow UDP connections
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn allow_udp(mut self, allow: bool) -> Self {
        self.network.allow_udp = allow;
        self
    }

    /// Allow or disallow DNS resolution.
    /// 
    /// # Arguments
    /// * `allow` - Whether to allow DNS resolution
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn allow_dns(mut self, allow: bool) -> Self {
        self.network.allow_dns = allow;
        self
    }

    /// Set a custom socket address check function.
    /// 
    /// # Arguments
    /// * `f` - The function to use for checking socket addresses
    /// 
    /// # Returns
    /// The updated ModuleEnv instance
    pub fn socket_check<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(SocketAddr, ModuleSocketAddrAction) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + Sync + 'static,
    {
        self.network.socket_check = Arc::new(move |addr, action| {
            Box::pin(f(addr, action))
        });
        self
    }
}

impl Default for ModuleEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ModuleEnv> for WasiP1Ctx {
    fn from(env: ModuleEnv) -> Self {
        let mut builder = WasiCtx::builder();

        if let Some(args) = env.args {
            builder.args(&args);
        }

        if let Some(env_vars) = env.env {
            for (key, value) in env_vars {
                builder.env(&key, &value);
            }
        }

        if let Some(mounts) = env.mount {
            for (guest_path, host_path) in mounts {
                builder
                    .preopened_dir(
                        host_path,
                        &guest_path,
                        // TODO: Support read only mounts
                        DirPerms::all(),
                        FilePerms::all(),
                    )
                    .expect(&format!("failed to preopen dir {}", guest_path));
            }
        }

        builder.allow_tcp(env.network.allow_tcp);
        builder.allow_udp(env.network.allow_udp);
        builder.allow_ip_name_lookup(env.network.allow_dns);
        builder.socket_addr_check(move |addr, action| {
            (env.network.socket_check)(addr, action.into())
        });

        builder.build_p1()
    }
}