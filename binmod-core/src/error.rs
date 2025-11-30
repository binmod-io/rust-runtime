use thiserror::Error;
use serde::{Serialize, Deserialize};

/// Errors that can occur in module operations
#[derive(Error, Debug)]
pub enum ModuleError {
    /// Errors related to function calls
    #[error("Function error: {0}")]
    FunctionError(#[from] FnError),

    /// Errors related to serialization/deserialization
    #[error("Failed to serialize/deserialize: {0}")]
    SerializeError(#[from] serde_json::Error),

    /// Error when module is not instantiated
    #[error("Module not instantiated")]
    NotInstantiated,

    /// Error when fuel is not enabled and
    /// fuel-related operations are attempted
    #[error("Fuel not enabled")]
    FuelNotEnabled,

    /// Error when a requested function is not found
    #[error("Function not found: {0}")]
    FunctionNotFound(String),

    /// Error when a function signature is invalid
    #[error("Invalid function signature")]
    InvalidFunctionSignature,

    /// Errors related to memory operations
    #[error("Memory operation failed: {0}")]
    MemoryError(String),

    /// General runtime errors
    #[error("Module runtime error: {0}")]
    RuntimeError(String),

    /// Trap errors from Wasmtime
    #[error("WASM trap: {0}")]
    Trap(#[from] wasmtime::Trap),

    /// Wasmtime-specific errors
    #[error("Wasmtime error: {0}")]
    WasmtimeError(#[from] wasmtime::Error),

    /// I/O related errors
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Error when module is already instantiated
    #[error("Module already instantiated")]
    AlreadyInstantiated,

    /// Errors during module instantiation
    #[error("Instantiation failed: {0}")]
    InstantiationError(String),
    
    /// Error when a module is not found during linking
    #[error("Module not found during linking: {0}")]
    ModuleNotFound(String),

    /// Error for invalid module configuration
    #[error("Invalid module configuration: {0}")]
    InvalidModuleConfig(String),
}

pub type ModuleResult<T> = Result<T, ModuleError>;

/// Represents an error that occurs within an invoked function
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FnError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

impl FnError {
    /// Create a new Function error instance.
    /// 
    /// # Arguments
    /// * `error_type` - The type/category of the error
    /// * `message` - A descriptive message about the error
    /// 
    /// # Returns
    /// A new [`FnError`](crate::error::FnError) instance
    pub fn new(error_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error_type: error_type.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.error_type, self.message)
    }
}

impl std::error::Error for FnError {}