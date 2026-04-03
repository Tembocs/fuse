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

pub fn fuse_string_char_at(s: &FuseValue, idx: &FuseValue) -> FuseValue {
    let s = s.as_str();
    let i = idx.as_int() as usize;
    if i >= s.len() {
        return FuseValue::Str(String::new());
    }
    // If i is not a char boundary, find the char that contains this byte.
    if s.is_char_boundary(i) {
        let ch = &s[i..].chars().next().unwrap();
        FuseValue::Str(ch.to_string())
    } else {
        // Inside a multi-byte char — return it as a replacement character.
        FuseValue::Str("\u{FFFD}".to_string())
    }
}

pub fn fuse_string_substring(s: &FuseValue, start: &FuseValue, end: &FuseValue) -> FuseValue {
    let s = s.as_str();
    let len = s.len();
    let mut a = (start.as_int() as usize).min(len);
    let mut b = (end.as_int() as usize).min(len);
    // Snap to nearest char boundaries.
    while a < len && !s.is_char_boundary(a) { a += 1; }
    while b < len && !s.is_char_boundary(b) { b += 1; }
    if b < a { b = a; }
    FuseValue::Str(s[a..b].to_string())
}

pub fn fuse_string_starts_with(s: &FuseValue, prefix: &FuseValue) -> FuseValue {
    FuseValue::Bool(s.as_str().starts_with(prefix.as_str()))
}

pub fn fuse_string_contains(s: &FuseValue, needle: &FuseValue) -> FuseValue {
    FuseValue::Bool(s.as_str().contains(needle.as_str()))
}

pub fn fuse_string_char_code_at(s: &FuseValue, idx: &FuseValue) -> FuseValue {
    let s = s.as_str();
    let i = idx.as_int() as usize;
    if i >= s.len() {
        return FuseValue::Int(-1);
    }
    // Return the byte value at position i (used for ASCII classification).
    FuseValue::Int(s.as_bytes()[i] as i64)
}

pub fn fuse_string_from_char_code(code: &FuseValue) -> FuseValue {
    let c = code.as_int() as u8 as char;
    FuseValue::Str(c.to_string())
}

pub fn fuse_int_to_string(v: &FuseValue) -> FuseValue {
    FuseValue::Str(format!("{}", v.as_int()))
}
