//! String operations for the Fuse runtime.

use crate::value::FuseValue;

pub fn fuse_string_to_upper(s: &FuseValue) -> FuseValue {
    FuseValue::Str(s.as_str().to_uppercase())
}

pub fn fuse_string_to_lower(s: &FuseValue) -> FuseValue {
    FuseValue::Str(s.as_str().to_lowercase())
}

pub fn fuse_string_len(s: &FuseValue) -> FuseValue {
    FuseValue::Int(s.as_str().len() as i64)
}

pub fn fuse_int_to_float(v: &FuseValue) -> FuseValue {
    FuseValue::Float(v.as_int() as f64)
}

pub fn fuse_int_is_even(v: &FuseValue) -> FuseValue {
    FuseValue::Bool(v.as_int() % 2 == 0)
}
