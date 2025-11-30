# Binmod Runtime for Rust

Rust runtime for loading and executing Binmod WebAssembly modules in your applications.

## Overview

The Binmod Rust Runtime allows you to embed WebAssembly-based plugins into your Rust applications. Load modules written in any language with a Binmod MDK (Rust, Python, and more coming soon).

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
binmod = "0.1"
```

## Load a Module

```rust
use binmod::prelude::*;
use anyhow::Result;

let mut module = Module::builder()
    .from_file("my_calculator.wasm")?
    .with_name("my_calculator")
    .with_namespace("calculator")
    .with_environment(
        ModuleEnv::default()
            .arg("my_calculator.wasm")
            .inherit_env()
            .inherit_network()
    )
    .host_fn("log", |msg: String| -> Result<()> {
        println!("[Module] {}", msg);
        Ok(())
    })
    .host_fn("get_pi", || -> Result<f64> {
        Ok(3.14159)
    })
    .build()?
    .instantiate()?;
```

**Key points:**
- **name**: Identifier for your module
- **namespace**: Groups related host functions together
- **host_fn**: Adds host functions that the module can invoke
- All host function parameters and return values must be serializable (via `serde`) and return Results

## Call Module Functions

**Synchronous API:**
```rust
use binmod::prelude::*;

let area: f64 = module.typed_call("circle_area", (5.0,))?;
println!("Circle area: {}", area);

let sum: i64 = module.typed_call("add", (10, 20))?;
println!("Sum: {}", sum);
```

**Asynchronous API:**

> [!NOTE] Async modules are experimental and may have performance implications and limited support.
> Do not use in production without thorough testing.

```rust
use binmod::prelude::*;

let mut module = AsyncModule::builder()
    .from_file("my_calculator.wasm")?
    .with_name("my_calculator")
    .with_namespace("calculator")
    .with_environment(
        ModuleEnv::default()
            .arg("my_calculator.wasm")
            .inherit_env()
            .inherit_network()
    )
    .host_fn("log", |msg: String| -> Result<()> {
        println!("[Module] {}", msg);
        Ok(())
    })
    .host_fn("get_pi", || -> Result<f64> {
        Ok(3.14159)
    })
    .build_async()?
    .instantiate()
    .await?;

let area: f64 = module.typed_call("circle_area", (5.0,)).await?;
println!("Circle area: {}", area);

let sum: i64 = module.typed_call("add", (10, 20)).await?;
println!("Sum: {}", sum);
```

## Complete Example

```rust
use binmod::prelude::*;
use anyhow::Result;

fn main() -> Result<()> {
    // Load and configure the module
    let mut module = Module::builder()
        .from_file("my_calculator.wasm")?
        .with_name("my_calculator")
        .with_namespace("calculator")
        .with_config(
            ModuleConfig::default()
                .with_consume_fuel(true)
        )
        .with_environment(
            ModuleEnv::default()
                .arg("my_calculator.wasm")
                .inherit_env()
                .inherit_network()
        )
        .host_fn("log", |msg: String| -> Result<()> {
            println!("[Module] {}", msg);
            Ok(())
        })
        .host_fn("get_pi", || -> Result<f64> {
            Ok(3.14159)
        })
        .build()?
        .instantiate()?;

    // Set max fuel for execution
    module.set_fuel(1_000_000)?;

    // Call module functions
    let area: f64 = module.typed_call("circle_area", (5.0,))?;
    println!("Circle area: {}", area);

    let sum: i64 = module.typed_call("add", (10, 20))?;
    println!("10 + 20 = {}", sum);

    Ok(())
}
```

## Module Pooling

Modules are *not* thread-safe. In a multi-threaded context use a module pool to manage multiple instances
that can be leased and returned safely.

```rust
use binmod::prelude::*;
use anyhow::Result;

// Synchronous pool
let template = Module::builder()
    .from_file("my_calculator.wasm")?
    .with_name("my_calculator")
    .with_namespace("calculator")
    .with_environment(
        ModuleEnv::default()
            .arg("my_calculator.wasm")
            .inherit_env()
            .inherit_network()
    )
    .host_fn("log", |msg: String| -> Result<()> {
        println!("[Module] {}", msg);
        Ok(())
    })
    .host_fn("get_pi", || -> Result<f64> {
        Ok(3.14159)
    })
    .build()?
    .instantiate()?;

let pool = ModulePool::builder()
    .with_module(template)
    .with_count(4)  // Create 4 instances (3 + template)
    .build()?;

// Lease a module from the pool
let mut module = pool.lease();
let result: f64 = module.typed_call("circle_area", (5.0,))?;
module.release();  // Return to pool

// Or use scoped to automatically manage leasing and release
pool.scoped(|module| {
    let result: f64 = module.typed_call("circle_area", (5.0,))?;
    Ok(())
})?;

// Asynchronous pool
let template = AsyncModule::builder()
    .from_file("my_calculator.wasm")?
    .with_name("my_calculator")
    .with_namespace("calculator")
    .with_environment(
        ModuleEnv::default()
            .arg("my_calculator.wasm")
            .inherit_env()
            .inherit_network()
    )
    .host_fn("log", |msg: String| -> Result<()> {
        println!("[Module] {}", msg);
        Ok(())
    })
    .host_fn("get_pi", || -> Result<f64> {
        Ok(3.14159)
    })
    .build_async()?
    .instantiate()
    .await?;

let pool = AsyncModulePool::builder()
    .with_module(template)
    .with_count(4)
    .build()
    .await?;

// Lease a module from the pool
let mut module = pool.lease().await;
let result: f64 = module.typed_call("circle_area", (5.0,)).await?;
module.release().await;

// Or use scoped to automatically manage leasing and release
pool.scoped(|module| async {
    let result: f64 = module.typed_call("circle_area", (5.0,)).await?;
    Ok(())
}).await?;
```

## Error Handling

```rust
use binmod::prelude::*;

match module.typed_call::<f64>("circle_area", (5.0,)) {
    Ok(result) => println!("Result: {}", result),
    Err(e) => eprintln!("Module execution error: {}", e),
}
```

## Advanced Configuration

### Module Limits

Control resource usage with module limits:

```rust
use binmod::prelude::*;

let module = Module::builder()
    .from_file("my_calculator.wasm")?
    .with_name("my_calculator")
    .with_limits(ModuleLimits {
        memory_size: 64 * 1024 * 1024, // 64 MB
    })
    .build()?
    .instantiate()?;
```

## Module Compatibility

WebAssembly modules must be compiled with the WASI Preview 1 target. Modules created with any Binmod MDK are compatible with this runtime.

## License

MIT License

## Support

If you encounter issues or have questions, please open an issue on GitHub.