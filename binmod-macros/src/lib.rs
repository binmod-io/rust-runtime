#[allow(unused_extern_crates)]
extern crate self as binmod_macros;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};
