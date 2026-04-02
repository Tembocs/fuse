//! C-compatible FFI surface for the Fuse runtime.
//!
//! Compiled Fuse programs call these functions to create, inspect, and
//! manipulate FuseValue objects. All values are heap-allocated and passed
//! as raw pointers (`*mut FuseValue`).
//!
//! # Memory model
//!
//! - `fuse_rt_*` constructors return owned `*mut FuseValue` (caller must drop).
//! - `fuse_rt_clone` creates an independent copy.
//! - `fuse_rt_drop` frees a value.
//! - Operations that return new values (add, sub, etc.) allocate fresh values.

use crate::value::*;
use std::slice;
use std::str;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn box_val(v: FuseValue) -> *mut FuseValue {
    Box::into_raw(Box::new(v))
}

unsafe fn ref_val<'a>(ptr: *mut FuseValue) -> &'a FuseValue {
    &*ptr
}

unsafe fn mut_val<'a>(ptr: *mut FuseValue) -> &'a mut FuseValue {
    &mut *ptr
}

unsafe fn str_from_raw(ptr: *const u8, len: i64) -> String {
    let bytes = slice::from_raw_parts(ptr, len as usize);
    str::from_utf8(bytes).unwrap_or("").to_string()
}

// ═══════════════════════════════════════════════════════════════════════
// Value construction
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_int(v: i64) -> *mut FuseValue {
    box_val(FuseValue::Int(v))
}

#[no_mangle]
pub extern "C" fn fuse_rt_float(v: f64) -> *mut FuseValue {
    box_val(FuseValue::Float(v))
}

#[no_mangle]
pub extern "C" fn fuse_rt_bool(v: i8) -> *mut FuseValue {
    box_val(FuseValue::Bool(v != 0))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str(ptr: *const u8, len: i64) -> *mut FuseValue {
    let s = unsafe { str_from_raw(ptr, len) };
    box_val(FuseValue::Str(s))
}

#[no_mangle]
pub extern "C" fn fuse_rt_unit() -> *mut FuseValue {
    box_val(FuseValue::Unit)
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_new() -> *mut FuseValue {
    box_val(FuseValue::List(Vec::new()))
}

#[no_mangle]
pub extern "C" fn fuse_rt_none() -> *mut FuseValue {
    box_val(FuseValue::none())
}

// ═══════════════════════════════════════════════════════════════════════
// Value access
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_as_int(ptr: *mut FuseValue) -> i64 {
    unsafe { ref_val(ptr).as_int() }
}

#[no_mangle]
pub extern "C" fn fuse_rt_as_float(ptr: *mut FuseValue) -> f64 {
    unsafe { ref_val(ptr).as_float() }
}

#[no_mangle]
pub extern "C" fn fuse_rt_as_bool(ptr: *mut FuseValue) -> i8 {
    if unsafe { ref_val(ptr).as_bool() } { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn fuse_rt_is_truthy(ptr: *mut FuseValue) -> i8 {
    if unsafe { ref_val(ptr).is_truthy() } { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn fuse_rt_type_name(ptr: *mut FuseValue) -> *mut FuseValue {
    let name = unsafe { ref_val(ptr).type_name().to_string() };
    box_val(FuseValue::Str(name))
}

// ═══════════════════════════════════════════════════════════════════════
// Arithmetic
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_add(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).add(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_sub(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).sub(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_mul(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).mul(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_div(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).div(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_mod(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).modulo(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_neg(a: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).neg() };
    box_val(result)
}

// ═══════════════════════════════════════════════════════════════════════
// Comparison
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_eq(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).eq(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_ne(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).ne(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_lt(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).lt(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_gt(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).gt(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_le(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).le(ref_val(b)) };
    box_val(result)
}

#[no_mangle]
pub extern "C" fn fuse_rt_ge(a: *mut FuseValue, b: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(a).ge(ref_val(b)) };
    box_val(result)
}

// ═══════════════════════════════════════════════════════════════════════
// I/O
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_println(val: *mut FuseValue) {
    let v = unsafe { ref_val(val) };
    println!("{v}");
}

#[no_mangle]
pub extern "C" fn fuse_rt_eprintln(val: *mut FuseValue) {
    let v = unsafe { ref_val(val) };
    eprintln!("{v}");
}

// ═══════════════════════════════════════════════════════════════════════
// Enum constructors and predicates
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_ok(val: *mut FuseValue) -> *mut FuseValue {
    let v = unsafe { ref_val(val).clone() };
    box_val(FuseValue::ok(v))
}

#[no_mangle]
pub extern "C" fn fuse_rt_err(val: *mut FuseValue) -> *mut FuseValue {
    let v = unsafe { ref_val(val).clone() };
    box_val(FuseValue::err(v))
}

#[no_mangle]
pub extern "C" fn fuse_rt_some(val: *mut FuseValue) -> *mut FuseValue {
    let v = unsafe { ref_val(val).clone() };
    box_val(FuseValue::some(v))
}

#[no_mangle]
pub extern "C" fn fuse_rt_is_ok(val: *mut FuseValue) -> i8 {
    if unsafe { ref_val(val).is_ok() } { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn fuse_rt_is_err(val: *mut FuseValue) -> i8 {
    if unsafe { ref_val(val).is_err() } { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn fuse_rt_is_some(val: *mut FuseValue) -> i8 {
    if unsafe { ref_val(val).is_some() } { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn fuse_rt_is_none(val: *mut FuseValue) -> i8 {
    if unsafe { ref_val(val).is_none() } { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn fuse_rt_unwrap_enum(val: *mut FuseValue) -> *mut FuseValue {
    let v = unsafe { ref_val(val).unwrap_enum_value() };
    box_val(v)
}

#[no_mangle]
pub extern "C" fn fuse_rt_enum_variant(
    enum_name: *const u8, enum_len: i64,
    var_name: *const u8, var_len: i64,
    payload: *mut FuseValue,
) -> *mut FuseValue {
    let en = unsafe { str_from_raw(enum_name, enum_len) };
    let vn = unsafe { str_from_raw(var_name, var_len) };
    let val = if payload.is_null() {
        None
    } else {
        Some(unsafe { ref_val(payload).clone() })
    };
    box_val(FuseValue::enum_variant(&en, &vn, val))
}

// ═══════════════════════════════════════════════════════════════════════
// Struct construction and field access
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_struct_new(name: *const u8, name_len: i64) -> *mut FuseValue {
    let type_name = unsafe { str_from_raw(name, name_len) };
    box_val(FuseValue::new_struct(&type_name, vec![], None))
}

#[no_mangle]
pub extern "C" fn fuse_rt_struct_set_field(
    obj: *mut FuseValue, field_name: *const u8, field_len: i64, val: *mut FuseValue,
) {
    let name = unsafe { str_from_raw(field_name, field_len) };
    let v = unsafe { ref_val(val).clone() };
    let s = unsafe { mut_val(obj) };
    if let FuseValue::Struct(ref mut st) = s {
        st.fields.push((name, v));
    }
}

#[no_mangle]
pub extern "C" fn fuse_rt_field(
    obj: *mut FuseValue, field_name: *const u8, field_len: i64,
) -> *mut FuseValue {
    let name = unsafe { str_from_raw(field_name, field_len) };
    let v = unsafe { ref_val(obj).field(&name) };
    box_val(v)
}

#[no_mangle]
pub extern "C" fn fuse_rt_set_field(
    obj: *mut FuseValue, field_name: *const u8, field_len: i64, val: *mut FuseValue,
) {
    let name = unsafe { str_from_raw(field_name, field_len) };
    let v = unsafe { ref_val(val).clone() };
    unsafe { mut_val(obj) }.set_field(&name, v);
}

// ═══════════════════════════════════════════════════════════════════════
// Clone and drop
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_clone(ptr: *mut FuseValue) -> *mut FuseValue {
    let v = unsafe { ref_val(ptr).clone() };
    box_val(v)
}

#[no_mangle]
pub extern "C" fn fuse_rt_drop(ptr: *mut FuseValue) {
    if !ptr.is_null() {
        unsafe { let _ = Box::from_raw(ptr); }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// String methods
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_str_len(s: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_string_len(unsafe { ref_val(s) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_char_at(s: *mut FuseValue, idx: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_string_char_at(unsafe { ref_val(s) }, unsafe { ref_val(idx) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_substring(s: *mut FuseValue, start: *mut FuseValue, end: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_string_substring(unsafe { ref_val(s) }, unsafe { ref_val(start) }, unsafe { ref_val(end) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_starts_with(s: *mut FuseValue, prefix: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_string_starts_with(unsafe { ref_val(s) }, unsafe { ref_val(prefix) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_contains(s: *mut FuseValue, needle: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_string_contains(unsafe { ref_val(s) }, unsafe { ref_val(needle) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_char_code_at(s: *mut FuseValue, idx: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_string_char_code_at(unsafe { ref_val(s) }, unsafe { ref_val(idx) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_split(s: *mut FuseValue, delim: *mut FuseValue) -> *mut FuseValue {
    let sv = unsafe { ref_val(s) };
    let dv = unsafe { ref_val(delim) };
    let parts: Vec<FuseValue> = sv.as_str().split(dv.as_str())
        .map(|p| FuseValue::Str(p.to_string()))
        .collect();
    box_val(FuseValue::List(parts))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_trim(s: *mut FuseValue) -> *mut FuseValue {
    let trimmed = unsafe { ref_val(s) }.as_str().trim().to_string();
    box_val(FuseValue::Str(trimmed))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_replace(s: *mut FuseValue, find: *mut FuseValue, rep: *mut FuseValue) -> *mut FuseValue {
    let result = unsafe { ref_val(s) }.as_str()
        .replace(unsafe { ref_val(find) }.as_str(), unsafe { ref_val(rep) }.as_str());
    box_val(FuseValue::Str(result))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_to_upper(s: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_string_to_upper(unsafe { ref_val(s) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_str_to_lower(s: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_string_to_lower(unsafe { ref_val(s) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_from_char_code(code: i64) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_string_from_char_code(&FuseValue::Int(code)))
}

// ═══════════════════════════════════════════════════════════════════════
// List methods
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_list_len(list: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::list_ops::fuse_list_len(unsafe { ref_val(list) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_get(list: *mut FuseValue, idx: *mut FuseValue) -> *mut FuseValue {
    let l = unsafe { ref_val(list) };
    let i = unsafe { ref_val(idx) }.as_int() as usize;
    let items = l.as_list();
    if i < items.len() {
        box_val(items[i].clone())
    } else {
        box_val(FuseValue::none())
    }
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_set(list: *mut FuseValue, idx: *mut FuseValue, val: *mut FuseValue) {
    let i = unsafe { ref_val(idx) }.as_int() as usize;
    let v = unsafe { ref_val(val) }.clone();
    let l = unsafe { mut_val(list) };
    if let FuseValue::List(ref mut items) = l {
        if i < items.len() {
            items[i] = v;
        }
    }
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_push(list: *mut FuseValue, val: *mut FuseValue) {
    let v = unsafe { ref_val(val) }.clone();
    crate::list_ops::fuse_list_push(unsafe { mut_val(list) }, v);
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_contains(list: *mut FuseValue, val: *mut FuseValue) -> *mut FuseValue {
    let l = unsafe { ref_val(list) };
    let v = unsafe { ref_val(val) };
    let found = l.as_list().iter().any(|item| item.fuse_eq(v));
    box_val(FuseValue::Bool(found))
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_first(list: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::list_ops::fuse_list_first(unsafe { ref_val(list) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_last(list: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::list_ops::fuse_list_last(unsafe { ref_val(list) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_sum(list: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::list_ops::fuse_list_sum(unsafe { ref_val(list) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_sorted(list: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::list_ops::fuse_list_sorted(unsafe { ref_val(list) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_list_is_empty(list: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::list_ops::fuse_list_is_empty(unsafe { ref_val(list) }))
}

// ═══════════════════════════════════════════════════════════════════════
// Int / Float methods
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_int_to_float(v: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_int_to_float(unsafe { ref_val(v) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_int_to_string(v: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_int_to_string(unsafe { ref_val(v) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_int_is_even(v: *mut FuseValue) -> *mut FuseValue {
    box_val(crate::string_ops::fuse_int_is_even(unsafe { ref_val(v) }))
}

#[no_mangle]
pub extern "C" fn fuse_rt_float_to_string(v: *mut FuseValue) -> *mut FuseValue {
    let s = format!("{}", unsafe { ref_val(v) });
    box_val(FuseValue::Str(s))
}

#[no_mangle]
pub extern "C" fn fuse_rt_to_display_string(v: *mut FuseValue) -> *mut FuseValue {
    let s = format!("{}", unsafe { ref_val(v) });
    box_val(FuseValue::Str(s))
}

// ═══════════════════════════════════════════════════════════════════════
// Lambda-based list operations
// ═══════════════════════════════════════════════════════════════════════

/// Type alias for a compiled Fuse function that takes (env, arg) and returns a value.
type FuseFnPtr = extern "C" fn(*mut FuseValue, *mut FuseValue) -> *mut FuseValue;

/// Map a compiled function over a list.
/// `fn_ptr` is a compiled Fuse lambda function.
/// `env` is the captured environment (or null).
#[no_mangle]
pub extern "C" fn fuse_rt_list_map_fn(
    list: *mut FuseValue, fn_ptr: FuseFnPtr, env: *mut FuseValue,
) -> *mut FuseValue {
    let items = unsafe { ref_val(list) }.as_list().clone();
    let mut result = Vec::new();
    for item in &items {
        let arg = box_val(item.clone());
        let mapped = fn_ptr(env, arg);
        result.push(unsafe { ref_val(mapped).clone() });
    }
    box_val(FuseValue::List(result))
}

/// Filter a list using a compiled predicate function.
#[no_mangle]
pub extern "C" fn fuse_rt_list_filter_fn(
    list: *mut FuseValue, fn_ptr: FuseFnPtr, env: *mut FuseValue,
) -> *mut FuseValue {
    let items = unsafe { ref_val(list) }.as_list().clone();
    let mut result = Vec::new();
    for item in &items {
        let arg = box_val(item.clone());
        let keep = fn_ptr(env, arg);
        if unsafe { ref_val(keep).is_truthy() } {
            result.push(item.clone());
        }
    }
    box_val(FuseValue::List(result))
}

/// Retain elements in-place using a compiled predicate function.
#[no_mangle]
pub extern "C" fn fuse_rt_list_retain_fn(
    list: *mut FuseValue, fn_ptr: FuseFnPtr, env: *mut FuseValue,
) {
    let items = unsafe { ref_val(list) }.as_list().clone();
    let mut result = Vec::new();
    for item in &items {
        let arg = box_val(item.clone());
        let keep = fn_ptr(env, arg);
        if unsafe { ref_val(keep).is_truthy() } {
            result.push(item.clone());
        }
    }
    if let FuseValue::List(ref mut l) = unsafe { mut_val(list) } {
        *l = result;
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Generic method dispatch (type-aware)
// ═══════════════════════════════════════════════════════════════════════

/// Generic `.len()` — works on both List and String.
#[no_mangle]
pub extern "C" fn fuse_rt_len(v: *mut FuseValue) -> *mut FuseValue {
    let val = unsafe { ref_val(v) };
    match val {
        FuseValue::List(items) => box_val(FuseValue::Int(items.len() as i64)),
        FuseValue::Str(s) => box_val(FuseValue::Int(s.len() as i64)),
        _ => box_val(FuseValue::Int(0)),
    }
}

/// Generic `.contains()` — works on both List and String.
#[no_mangle]
pub extern "C" fn fuse_rt_contains(v: *mut FuseValue, needle: *mut FuseValue) -> *mut FuseValue {
    let val = unsafe { ref_val(v) };
    let n = unsafe { ref_val(needle) };
    match val {
        FuseValue::List(items) => {
            let found = items.iter().any(|item| item.fuse_eq(n));
            box_val(FuseValue::Bool(found))
        }
        FuseValue::Str(s) => {
            let found = s.contains(n.as_str());
            box_val(FuseValue::Bool(found))
        }
        _ => box_val(FuseValue::Bool(false)),
    }
}

/// Generic `.toString()` — works on any type.
#[no_mangle]
pub extern "C" fn fuse_rt_to_string(v: *mut FuseValue) -> *mut FuseValue {
    let s = format!("{}", unsafe { ref_val(v) });
    box_val(FuseValue::Str(s))
}

/// Enum variant name — returns the variant name as a string.
#[no_mangle]
pub extern "C" fn fuse_rt_variant_name(v: *mut FuseValue) -> *mut FuseValue {
    let val = unsafe { ref_val(v) };
    match val {
        FuseValue::Enum(e) => {
            let full = format!("{}.{}", e.enum_name, e.variant);
            box_val(FuseValue::Str(full))
        }
        _ => box_val(FuseValue::Str(val.type_name().to_string())),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// System
// ═══════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn fuse_rt_read_file(path: *const u8, path_len: i64) -> *mut FuseValue {
    let p = unsafe { str_from_raw(path, path_len) };
    match std::fs::read_to_string(&p) {
        Ok(contents) => box_val(FuseValue::ok(FuseValue::Str(contents))),
        Err(e) => box_val(FuseValue::err(FuseValue::Str(e.to_string()))),
    }
}

#[no_mangle]
pub extern "C" fn fuse_rt_args() -> *mut FuseValue {
    let args: Vec<FuseValue> = std::env::args()
        .map(|a| FuseValue::Str(a))
        .collect();
    box_val(FuseValue::List(args))
}

#[no_mangle]
pub extern "C" fn fuse_rt_exit(code: i64) {
    std::process::exit(code as i32);
}

#[no_mangle]
pub extern "C" fn fuse_rt_parse_int(ptr: *const u8, len: i64) -> *mut FuseValue {
    let s = unsafe { str_from_raw(ptr, len) };
    match s.parse::<i64>() {
        Ok(v) => box_val(FuseValue::ok(FuseValue::Int(v))),
        Err(e) => box_val(FuseValue::err(FuseValue::Str(e.to_string()))),
    }
}

#[no_mangle]
pub extern "C" fn fuse_rt_parse_float(ptr: *const u8, len: i64) -> *mut FuseValue {
    let s = unsafe { str_from_raw(ptr, len) };
    match s.parse::<f64>() {
        Ok(v) => box_val(FuseValue::ok(FuseValue::Float(v))),
        Err(e) => box_val(FuseValue::err(FuseValue::Str(e.to_string()))),
    }
}

#[no_mangle]
pub extern "C" fn fuse_rt_panic(msg: *const u8, len: i64) {
    let s = unsafe { str_from_raw(msg, len) };
    eprintln!("Fuse panic: {s}");
    std::process::exit(1);
}
