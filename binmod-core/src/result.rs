use std::any;
use serde::{Serialize, Deserialize};
use serde_json::{to_value, to_vec, from_value, from_slice, Value};

use crate::error::FnError;


/// Result type for function calls
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "object")]
pub enum FnResult {
    #[serde(rename = "data")]
    Data { value: Option<Value> },
    #[serde(rename = "error")]
    Error { #[serde(flatten)] error: FnError },
}

impl FnResult {
    /// Create a successful function result with the given value.
    /// 
    /// # Arguments
    /// * `value` - The value to include in the result
    /// 
    /// # Returns
    /// A Result containing the [`FnResult`](crate::result::FnResult) instance
    /// or an [`FnError`](crate::error::FnError) if serialization fails
    pub fn ok<T: Serialize>(value: T) -> Result<Self, FnError> {
        Ok(Self::Data {
            value: Some(
                to_value(value)
                    .map_err(|e| FnError::new("SerializationError", e.to_string()))?
            ),
        })
    }

    /// Create an error function result with the given error.
    /// 
    /// # Arguments
    /// * `error` - The error to include in the result
    /// 
    /// # Returns
    /// A [`FnResult`](crate::result::FnResult) instance representing the error
    pub fn err<E: ToString>(error: &E) -> Self {
        Self::Error {
            error: FnError::new(
                any::type_name::<E>(),
                error.to_string(),
            ),
        }
    }

    /// Create a function result representing no return value.
    /// 
    /// # Returns
    /// A [`FnResult`](crate::result::FnResult) instance with no data
    pub fn none() -> Self {
        Self::Data { value: None }
    }

    /// Convert the function result into a Rust type.
    /// 
    /// # Returns
    /// A Result containing the deserialized value or an [`FnError`](crate::error::FnError)
    /// if deserialization fails or if the result is an error
    pub fn into_result<T: for<'de> Deserialize<'de>>(self) -> Result<T, FnError> {
        match self {
            Self::Data { value } => {
                from_value(value.unwrap_or(Value::Null))
                    .map_err(|e| FnError::new("DeserializationError", e.to_string()))
            },
            Self::Error { error } => Err(error),
        }
    }

    /// Check if the result is an error.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Check if the result is data.
    pub fn is_data(&self) -> bool {
        matches!(self, Self::Data { .. })
    }

    /// Serialize the Function result to bytes.
    /// 
    /// # Returns
    /// A Result containing the serialized bytes or an [`FnError`](crate::error::FnError)
    /// if serialization fails
    pub fn to_bytes(&self) -> Result<Vec<u8>, FnError> {
        Ok(
            to_vec(self)
                .map_err(|e| FnError::new("SerializationError", e.to_string()))?
        )
    }

    /// Deserialize Function result from bytes.
    /// 
    /// # Arguments
    /// * `bytes` - The bytes to deserialize from
    /// 
    /// # Returns
    /// A Result containing the deserialized [`FnResult`](crate::result::FnResult) instance
    /// or an [`FnError`](crate::error::FnError) if deserialization fails
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FnError> {
        Ok(
            from_slice(bytes)
                .map_err(|e| FnError::new("DeserializationError", e.to_string()))?
        )
    }
}

/// Trait for converting function results to FnResult
pub trait IntoFnResult {
    fn into_fn_result(self) -> FnResult;
}

impl<T: Serialize, E: ToString> IntoFnResult for Result<T, E> {
    fn into_fn_result(self) -> FnResult {
        match self {
            Ok(value) => FnResult::ok(value)
                .unwrap_or_else(|e| FnResult::err(&e)),
            Err(e) => FnResult::err(&e),
        }
    }
}

impl IntoFnResult for () {
    fn into_fn_result(self) -> FnResult {
        FnResult::none()
    }
}
