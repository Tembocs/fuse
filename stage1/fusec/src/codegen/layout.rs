//! Value layout and ABI for Fuse → Cranelift code generation.
//!
//! # Strategy: Boxed FuseValue pointers
//!
//! All Fuse values are passed as opaque pointers to heap-allocated `FuseValue`
//! objects managed by the runtime. This is the simplest correct approach:
//!
//! - Every value is a single `i64` (pointer) at the Cranelift level.
//! - Construction, access, and operations go through `fuse_rt_*` FFI calls.
//! - No unboxing optimization yet — that's a future performance pass.
//!
//! This means the ABI is uniform: every function parameter is `i64`, every
//! return value is `i64`, and the runtime handles all type dispatch.
//!
//! # ABI Summary
//!
//! | Fuse type        | Cranelift type | Representation            |
//! |------------------|---------------|---------------------------|
//! | Int              | i64           | ptr to FuseValue::Int     |
//! | Float            | i64           | ptr to FuseValue::Float   |
//! | Bool             | i64           | ptr to FuseValue::Bool    |
//! | String           | i64           | ptr to FuseValue::Str     |
//! | ()               | i64           | ptr to FuseValue::Unit    |
//! | List<T>          | i64           | ptr to FuseValue::List    |
//! | Struct           | i64           | ptr to FuseValue::Struct  |
//! | Enum             | i64           | ptr to FuseValue::Enum    |
//! | Fn               | i64           | ptr to FuseValue::Fn      |
//! | Lambda           | i64           | ptr to FuseValue::Lambda  |
//!
//! # Calling convention
//!
//! - All parameters: `i64` (pointer to FuseValue)
//! - Return value: `i64` (pointer to FuseValue)
//! - `mutref` parameters: the caller passes a pointer. After the call returns,
//!   the caller reads back the (possibly modified) value from the same pointer.
//!   The runtime handles mutref writeback internally.
//! - `move` parameters: ownership transfers. The caller must not use the
//!   value after the call. The callee is responsible for dropping it.
//!
//! # Entry point
//!
//! The compiled binary's `main()` function:
//! 1. Calls the `@entrypoint` Fuse function.
//! 2. If the result is `Err`, prints the error and exits with code 1.
//! 3. Otherwise exits with code 0.

use cranelift::prelude::*;

/// The Cranelift type used for all Fuse values: a 64-bit pointer.
pub const FUSE_VALUE_TYPE: types::Type = types::I64;

/// The Cranelift pointer type (same as FUSE_VALUE_TYPE on 64-bit).
pub const PTR_TYPE: types::Type = types::I64;

/// Build a Cranelift function signature where all parameters and the
/// return value are `i64` (pointer to FuseValue).
pub fn fuse_fn_sig(param_count: usize, call_conv: isa::CallConv) -> Signature {
    let mut sig = Signature::new(call_conv);
    for _ in 0..param_count {
        sig.params.push(AbiParam::new(FUSE_VALUE_TYPE));
    }
    sig.returns.push(AbiParam::new(FUSE_VALUE_TYPE));
    sig
}

/// Build a Cranelift signature for a void-returning runtime function
/// (e.g., `fuse_rt_println`).
pub fn void_fn_sig(param_count: usize, call_conv: isa::CallConv) -> Signature {
    let mut sig = Signature::new(call_conv);
    for _ in 0..param_count {
        sig.params.push(AbiParam::new(FUSE_VALUE_TYPE));
    }
    sig
}

/// Build a signature for a runtime function with a specific return type.
pub fn rt_sig(params: &[types::Type], ret: Option<types::Type>, call_conv: isa::CallConv) -> Signature {
    let mut sig = Signature::new(call_conv);
    for &p in params {
        sig.params.push(AbiParam::new(p));
    }
    if let Some(r) = ret {
        sig.returns.push(AbiParam::new(r));
    }
    sig
}

/// The number of runtime function parameters for common operations.
/// Used to generate import declarations.
pub struct RtFuncInfo {
    pub name: &'static str,
    pub params: &'static [types::Type],
    pub ret: Option<types::Type>,
}

/// Catalog of all `fuse_rt_*` functions that compiled code may call.
/// Each entry specifies parameter types and return type at the Cranelift level.
pub static RT_FUNCTIONS: &[RtFuncInfo] = &[
    // Value construction
    RtFuncInfo { name: "fuse_rt_int",       params: &[types::I64], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_float",     params: &[types::F64], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_bool",      params: &[types::I8],  ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str",       params: &[PTR_TYPE, types::I64], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_unit",      params: &[],           ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_new",  params: &[],           ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_none",      params: &[],           ret: Some(PTR_TYPE) },

    // Value access
    RtFuncInfo { name: "fuse_rt_as_int",    params: &[PTR_TYPE],   ret: Some(types::I64) },
    RtFuncInfo { name: "fuse_rt_as_float",  params: &[PTR_TYPE],   ret: Some(types::F64) },
    RtFuncInfo { name: "fuse_rt_as_bool",   params: &[PTR_TYPE],   ret: Some(types::I8) },
    RtFuncInfo { name: "fuse_rt_is_truthy", params: &[PTR_TYPE],   ret: Some(types::I8) },
    RtFuncInfo { name: "fuse_rt_type_name", params: &[PTR_TYPE],   ret: Some(PTR_TYPE) },

    // Arithmetic
    RtFuncInfo { name: "fuse_rt_add",       params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_sub",       params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_mul",       params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_div",       params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_mod",       params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_neg",       params: &[PTR_TYPE],           ret: Some(PTR_TYPE) },

    // Comparison
    RtFuncInfo { name: "fuse_rt_eq",        params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_ne",        params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_lt",        params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_gt",        params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_le",        params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_ge",        params: &[PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },

    // I/O
    RtFuncInfo { name: "fuse_rt_println",   params: &[PTR_TYPE],   ret: None },
    RtFuncInfo { name: "fuse_rt_eprintln",  params: &[PTR_TYPE],   ret: None },

    // Enum constructors / predicates
    RtFuncInfo { name: "fuse_rt_ok",        params: &[PTR_TYPE],   ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_err",       params: &[PTR_TYPE],   ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_some",      params: &[PTR_TYPE],   ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_is_ok",     params: &[PTR_TYPE],   ret: Some(types::I8) },
    RtFuncInfo { name: "fuse_rt_is_err",    params: &[PTR_TYPE],   ret: Some(types::I8) },
    RtFuncInfo { name: "fuse_rt_is_some",   params: &[PTR_TYPE],   ret: Some(types::I8) },
    RtFuncInfo { name: "fuse_rt_is_none",   params: &[PTR_TYPE],   ret: Some(types::I8) },
    RtFuncInfo { name: "fuse_rt_unwrap_enum", params: &[PTR_TYPE], ret: Some(PTR_TYPE) },

    // Enum variant construction
    RtFuncInfo { name: "fuse_rt_enum_variant", params: &[PTR_TYPE, PTR_TYPE, PTR_TYPE, PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },

    // Struct construction and field access
    RtFuncInfo { name: "fuse_rt_struct_new",       params: &[PTR_TYPE, types::I64], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_struct_set_field",  params: &[PTR_TYPE, PTR_TYPE, types::I64, PTR_TYPE], ret: None },
    RtFuncInfo { name: "fuse_rt_field",            params: &[PTR_TYPE, PTR_TYPE, types::I64], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_set_field",        params: &[PTR_TYPE, PTR_TYPE, types::I64, PTR_TYPE], ret: None },

    // Clone and drop
    RtFuncInfo { name: "fuse_rt_clone",     params: &[PTR_TYPE],   ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_drop",      params: &[PTR_TYPE],   ret: None },

    // String methods
    RtFuncInfo { name: "fuse_rt_str_len",        params: &[PTR_TYPE],                       ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_char_at",    params: &[PTR_TYPE, PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_substring",  params: &[PTR_TYPE, PTR_TYPE, PTR_TYPE],    ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_starts_with", params: &[PTR_TYPE, PTR_TYPE],             ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_contains",   params: &[PTR_TYPE, PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_char_code_at", params: &[PTR_TYPE, PTR_TYPE],            ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_split",      params: &[PTR_TYPE, PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_trim",       params: &[PTR_TYPE],                       ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_replace",    params: &[PTR_TYPE, PTR_TYPE, PTR_TYPE],    ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_to_upper",   params: &[PTR_TYPE],                       ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_str_to_lower",   params: &[PTR_TYPE],                       ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_from_char_code",  params: &[types::I64],                    ret: Some(PTR_TYPE) },

    // List methods
    RtFuncInfo { name: "fuse_rt_list_len",       params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_get",       params: &[PTR_TYPE, PTR_TYPE],     ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_set",       params: &[PTR_TYPE, PTR_TYPE, PTR_TYPE], ret: None },
    RtFuncInfo { name: "fuse_rt_list_push",      params: &[PTR_TYPE, PTR_TYPE],     ret: None },
    RtFuncInfo { name: "fuse_rt_list_contains",  params: &[PTR_TYPE, PTR_TYPE],     ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_first",     params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_last",      params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_sum",       params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_sorted",    params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_is_empty",  params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },

    // Int/Float methods
    RtFuncInfo { name: "fuse_rt_int_to_float",   params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_int_to_string",  params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_int_is_even",    params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_float_to_string", params: &[PTR_TYPE],             ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_to_display_string", params: &[PTR_TYPE],           ret: Some(PTR_TYPE) },

    // Lambda-based list operations (fn_ptr is i64 on 64-bit, env is PTR)
    RtFuncInfo { name: "fuse_rt_list_map_fn",    params: &[PTR_TYPE, PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_filter_fn", params: &[PTR_TYPE, PTR_TYPE, PTR_TYPE], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_list_retain_fn", params: &[PTR_TYPE, PTR_TYPE, PTR_TYPE], ret: None },

    // Generic method dispatch
    RtFuncInfo { name: "fuse_rt_struct_set_del",  params: &[PTR_TYPE, PTR_TYPE, types::I64],  ret: None },
    RtFuncInfo { name: "fuse_rt_safe_field",     params: &[PTR_TYPE, PTR_TYPE, types::I64], ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_len",            params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_contains",       params: &[PTR_TYPE, PTR_TYPE],     ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_to_string",      params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_variant_name",   params: &[PTR_TYPE],              ret: Some(PTR_TYPE) },

    // System
    RtFuncInfo { name: "fuse_rt_read_file",      params: &[PTR_TYPE, types::I64],  ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_args",           params: &[],                      ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_exit",           params: &[types::I64],            ret: None },
    RtFuncInfo { name: "fuse_rt_parse_int",      params: &[PTR_TYPE, types::I64],  ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_parse_float",    params: &[PTR_TYPE, types::I64],  ret: Some(PTR_TYPE) },
    RtFuncInfo { name: "fuse_rt_panic",          params: &[PTR_TYPE, types::I64],  ret: None },
];
