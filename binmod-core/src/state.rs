use wasmtime::StoreLimits;
use wasmtime_wasi::p1::WasiP1Ctx;


pub struct ModuleState {
    pub wasi: WasiP1Ctx,
    pub limits: StoreLimits,
}
