use std::collections::HashMap;
use serde::{Serialize, Deserialize, de::DeserializeOwned};
use serde_json::{to_value, to_vec, from_value, from_slice, Value, Error as JsonError};

use crate::error::{ModuleResult, FnError};


/// Represents the input arguments for a function call
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FnInput {
    /// Positional arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<Value>>,
    /// Keyword arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kwargs: Option<HashMap<String, Value>>,
}

impl FnInput {
    /// Create a new, empty Function input.
    /// 
    /// # Returns
    /// A new [`FnInput`](crate::input::FnInput) instance
    pub fn new() -> Self {
        Self {
            args: None,
            kwargs: None,
        }
    }

    /// Add a positional argument to the function input.
    /// 
    /// # Arguments
    /// * `arg` - The argument to add
    /// 
    /// # Returns
    /// A Result containing the updated [`FnInput`](crate::input::FnInput) instance or an
    /// [`FnError`](crate::error::FnError) if serialization fails
    pub fn with_arg<T>(mut self, arg: T) -> Result<Self, FnError>
    where
        T: Serialize,
    {
        match &mut self.args {
            Some(existing) => existing.push(
                to_value(arg)
                    .map_err(|e| FnError::new("SerializationError", e.to_string()))?
            ),
            None => self.args = Some(vec![
                to_value(arg)
                    .map_err(|e| FnError::new("SerializationError", e.to_string()))?,
            ]),
        }
        Ok(self)
    }

    /// Add multiple positional arguments to the function input.
    /// 
    /// # Arguments
    /// * `args` - An iterable of arguments to add
    /// 
    /// # Returns
    /// A Result containing the updated [`FnInput`](crate::input::FnInput) instance
    /// or an [`FnError`](crate::error::FnError) if serialization fails
    pub fn with_args<I, T>(mut self, args: I) -> Result<Self, FnError>
    where
        I: IntoIterator<Item = T>,
        T: Serialize,
    {
        match &mut self.args {
            Some(existing) => { existing.extend(
                args.into_iter()
                    .map(|arg| to_value(arg))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| FnError::new("SerializationError", e.to_string()))?
            ); },
            None => self.args = Some(
                args.into_iter()
                    .map(|arg| to_value(arg))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| FnError::new("SerializationError", e.to_string()))?,
            ),
        }
        Ok(self)
    }

    /// Add a keyword argument to the function input.
    /// 
    /// # Arguments
    /// * `key` - The name of the keyword argument
    /// * `value` - The value of the keyword argument
    /// 
    /// # Returns
    /// A Result containing the updated [`FnInput`](crate::input::FnInput) instance
    /// or an [`FnError`](crate::error::FnError) if serialization fails
    pub fn with_kwarg<T>(mut self, key: impl Into<String>, value: T) -> Result<Self, FnError>
    where 
        T: Serialize,
    {
        match &mut self.kwargs {
            Some(existing) => { existing.insert(
                key.into(), 
                to_value(value)
                    .map_err(|e| FnError::new("SerializationError", e.to_string()))?
            ); },
            None => self.kwargs = Some(HashMap::from([(
                key.into(),
                to_value(value)
                    .map_err(|e| FnError::new("SerializationError", e.to_string()))?,
            )])),
        }
        Ok(self)
    }

    /// Add multiple keyword arguments to the function input.
    /// 
    /// # Arguments
    /// * `kwargs` - An iterable of (key, value) pairs to add
    /// 
    /// # Returns
    /// A Result containing the updated [`FnInput`](crate::input::FnInput) instance
    /// or an [`FnError`](crate::error::FnError) if serialization fails
    pub fn with_kwargs<I, K, V>(mut self, kwargs: I) -> Result<Self, FnError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Serialize,
    {
        match &mut self.kwargs {
            Some(existing) => { existing.extend(
                kwargs.into_iter()
                    .map(|(k, v)| Ok((k.into(), to_value(v)?)))
                    .collect::<Result<HashMap<_, _>, JsonError>>()
                    .map_err(|e| FnError::new("SerializationError", e.to_string()))?
            ); },
            None => self.kwargs = Some(
                kwargs.into_iter()
                    .map(|(k, v)| Ok((k.into(), to_value(v)?)))
                    .collect::<Result<HashMap<_, _>, JsonError>>()
                    .map_err(|e| FnError::new("SerializationError", e.to_string()))?
            ),
        }
        Ok(self)
    }

    /// Get a positional argument by index.
    /// 
    /// # Arguments
    /// * `index` - The index of the argument to retrieve
    /// 
    /// # Returns
    /// A Result containing the deserialized argument or an [`FnError`](crate::error::FnError)
    /// if the argument is missing or deserialization fails
    pub fn get_arg<T>(&self, index: usize) -> Result<T, FnError>
    where
        T: DeserializeOwned,
    {
        if let Some(args) = &self.args {
            if index < args.len() {
                return from_value(args[index].clone())
                    .map_err(|e| FnError::new("DeserializationError", format!("Failed to parse argument {}: {}", index, e)));
            }
        }

        Err(FnError::new("MissingArg", format!("Missing arg in position {}", index)))
    }

    /// Get a keyword argument by name.
    /// 
    /// # Arguments
    /// * `name` - The name of the keyword argument to retrieve
    /// 
    /// # Returns
    /// A Result containing the deserialized argument or an [`FnError`](crate::error::FnError)
    /// if the argument is missing or deserialization fails
    pub fn get_kwarg<T>(&self, name: &str) -> Result<T, FnError>
    where 
        T: DeserializeOwned,
    {
        if let Some(kwargs) = &self.kwargs {
            if let Some(value) = kwargs.get(name) {
                return from_value(value.clone())
                    .map_err(|e| FnError::new("DeserializationError", format!("Failed to parse kwarg '{}': {}", name, e)));
            }
        }

        Err(FnError::new("MissingKwarg", format!("Missing kwarg: {}", name)))
    }

    /// Convert the positional arguments into a Rust type.
    /// 
    /// # Returns
    /// A Result containing the deserialized arguments or an [`FnError`](crate::error::FnError)
    /// if deserialization fails
    pub fn into_args<T>(self) -> Result<T, FnError>
    where
        T: DeserializeOwned,
    {
        Ok(
            from_value(Value::Array(
                self.args.unwrap_or_default()
            ))
            .map_err(|e| FnError::new("DeserializationError", e.to_string()))?
        )
    }

    /// Convert the keyword arguments into a Rust type.
    /// 
    /// # Returns
    /// A Result containing the deserialized arguments or an [`FnError`](crate::error::FnError)
    /// if deserialization fails
    pub fn into_struct<T>(self) -> Result<T, FnError>
    where
        T: DeserializeOwned,
    {
        Ok(
            from_value(Value::Object(
                self.kwargs
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            ))
            .map_err(|e| FnError::new("DeserializationError", e.to_string()))?
        )
    }

    /// Serialize the Function input to bytes.
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

    /// Deserialize Function input from bytes.
    /// 
    /// # Arguments
    /// * `bytes` - The bytes to deserialize from
    /// 
    /// # Returns
    /// A Result containing the deserialized [`FnInput`](crate::input::FnInput) instance
    /// or an [`FnError`](crate::error::FnError) if deserialization fails
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FnError> {
        Ok(
            from_slice(bytes)
                .map_err(|e| FnError::new("DeserializationError", e.to_string()))?
        )
    }
}

impl Default for FnInput {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for converting function arguments from FnInput
pub trait FromFnInput: Sized {
    fn from_fn_input(input: &FnInput) -> Result<Self, FnError>;
}

/// Trait for converting Rust tuples into FnInput arguments
pub trait IntoFnInput {
    fn into_fn_input(self) -> ModuleResult<FnInput>;
}

impl FromFnInput for () {
    fn from_fn_input(_input: &FnInput) -> Result<Self, FnError> {
        Ok(())
    }
}

macro_rules! impl_from_fn_input {
    ($($T:ident : $idx:tt),*) => {
        #[allow(non_snake_case)]
        impl<$($T: serde::de::DeserializeOwned),*> FromFnInput for ($($T,)*) {
            fn from_fn_input(input: &FnInput) -> Result<Self, FnError> {
                $(
                    let $T = input.get_arg($idx)?;
                )*

                Ok(($($T,)*))
            }
        }
    };
}

impl_from_fn_input!(T1:0);
impl_from_fn_input!(T1:0, T2:1);
impl_from_fn_input!(T1:0, T2:1, T3:2);
impl_from_fn_input!(T1:0, T2:1, T3:2, T4:3);
impl_from_fn_input!(T1:0, T2:1, T3:2, T4:3, T5:4);
impl_from_fn_input!(T1:0, T2:1, T3:2, T4:3, T5:4, T6:5);
impl_from_fn_input!(T1:0, T2:1, T3:2, T4:3, T5:4, T6:5, T7:6);
impl_from_fn_input!(T1:0, T2:1, T3:2, T4:3, T5:4, T6:5, T7:6, T8:7);

impl IntoFnInput for () {
    fn into_fn_input(self) -> ModuleResult<FnInput> {
        Ok(FnInput::default())
    }
}

macro_rules! impl_into_fn_input {
    ($($T:ident : $idx:tt),+) => {
        #[allow(non_snake_case)]
        impl<$($T: serde::Serialize),+> IntoFnInput for ($($T,)+) {
            fn into_fn_input(self) -> ModuleResult<FnInput> {
                Ok(
                    FnInput::default()
                    $(
                        .with_arg(self.$idx)?
                    )+
                )
            }
        }
    };
}

impl_into_fn_input!(T1:0);
impl_into_fn_input!(T1:0, T2:1);
impl_into_fn_input!(T1:0, T2:1, T3:2);
impl_into_fn_input!(T1:0, T2:1, T3:2, T4:3);
impl_into_fn_input!(T1:0, T2:1, T3:2, T4:3, T5:4);
impl_into_fn_input!(T1:0, T2:1, T3:2, T4:3, T5:4, T6:5);
impl_into_fn_input!(T1:0, T2:1, T3:2, T4:3, T5:4, T6:5, T7:6);
impl_into_fn_input!(T1:0, T2:1, T3:2, T4:3, T5:4, T6:5, T7:6, T8:7);