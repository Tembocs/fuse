//! Fuse runtime support library.
//!
//! Compiled Fuse programs link against this library. It provides:
//! - `FuseValue`: the tagged union representing all Fuse values at runtime
//! - Built-in functions (println, eprintln)
//! - List, String, Int, Float operations
//! - Enum variant construction and matching
//! - Struct construction and field access
//! - ASAP destruction support
//! - Defer support

mod value;
mod builtins;
mod list_ops;
mod string_ops;

pub use value::*;
pub use builtins::*;
pub use list_ops::*;
pub use string_ops::*;
