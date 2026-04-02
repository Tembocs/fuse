//! C-compatible wrappers around Cranelift's code generation API.
//!
//! These functions let the Stage 2 Fuse compiler (written in Fuse) call
//! Cranelift through FFI to generate native code.
//!
//! All opaque types are passed as `*mut T` pointers. The caller must not
//! free these — use the provided `cl_*_drop` functions.

use std::slice;
use std::str;
use std::collections::HashMap;

use cranelift::prelude::*;
use cranelift::codegen::ir::Function;
use cranelift_module::{Module, Linkage, FuncId, DataDescription, DataId};
use cranelift_object::{ObjectModule, ObjectBuilder};

// ═════════════════════════════════════════════════════════════════════
// Opaque context: holds module + metadata
// ═════════════════════════════════════════════════════════════════════

struct ClContext {
    module: ObjectModule,
    func_ids: Vec<FuncId>,         // index = our handle
    data_ids: Vec<DataId>,         // index = our handle
    str_counter: usize,
}

struct ClFuncContext {
    func: Function,
    builder_ctx: FunctionBuilderContext,
}

// We store builder state in a separate struct that lives on the heap.
struct ClBuilder {
    func: Function,
    builder_ctx: FunctionBuilderContext,
    // The FunctionBuilder borrows func and builder_ctx — we recreate it per call.
    vars: HashMap<u32, Variable>,
    next_var: u32,
}

// ═════════════════════════════════════════════════════════════════════
// Helpers
// ═════════════════════════════════════════════════════════════════════

unsafe fn str_from_raw(ptr: *const u8, len: i64) -> String {
    let bytes = slice::from_raw_parts(ptr, len as usize);
    str::from_utf8(bytes).unwrap_or("").to_string()
}

fn box_ptr<T>(v: T) -> *mut T { Box::into_raw(Box::new(v)) }

// ═════════════════════════════════════════════════════════════════════
// Module management
// ═════════════════════════════════════════════════════════════════════

/// Create a new Cranelift module for the host target.
#[no_mangle]
pub extern "C" fn cl_module_new() -> *mut ClContext {
    let isa_builder = cranelift_native::builder().expect("host ISA");
    let flags = settings::Flags::new(settings::builder());
    let isa = isa_builder.finish(flags).expect("build ISA");
    let obj_builder = ObjectBuilder::new(
        isa, "fuse_program",
        cranelift_module::default_libcall_names(),
    ).expect("object builder");
    box_ptr(ClContext {
        module: ObjectModule::new(obj_builder),
        func_ids: Vec::new(),
        data_ids: Vec::new(),
        str_counter: 0,
    })
}

/// Finish the module and write an object file.
/// Returns 0 on success, -1 on failure.
#[no_mangle]
pub extern "C" fn cl_module_finish(ctx: *mut ClContext, path_ptr: *const u8, path_len: i64) -> i64 {
    let ctx = unsafe { Box::from_raw(ctx) };
    let path = unsafe { str_from_raw(path_ptr, path_len) };
    let product = ctx.module.finish();
    match product.emit() {
        Ok(bytes) => {
            match std::fs::write(&path, &bytes) {
                Ok(_) => 0,
                Err(_) => -1,
            }
        }
        Err(_) => -1,
    }
}

/// Get the default calling convention for the host.
#[no_mangle]
pub extern "C" fn cl_call_conv(ctx: *mut ClContext) -> i64 {
    let ctx = unsafe { &*ctx };
    ctx.module.isa().default_call_conv() as i64
}

// ═════════════════════════════════════════════════════════════════════
// Function declaration and import
// ═════════════════════════════════════════════════════════════════════

/// Declare a local function. Returns a handle (index).
/// `param_count`: number of i64 params. `has_ret`: 1 if returns i64, 0 if void.
#[no_mangle]
pub extern "C" fn cl_declare_func(
    ctx: *mut ClContext, name_ptr: *const u8, name_len: i64,
    param_count: i64, has_ret: i64,
) -> i64 {
    let ctx = unsafe { &mut *ctx };
    let name = unsafe { str_from_raw(name_ptr, name_len) };
    let cc = ctx.module.isa().default_call_conv();
    let mut sig = Signature::new(cc);
    for _ in 0..param_count { sig.params.push(AbiParam::new(types::I64)); }
    if has_ret != 0 { sig.returns.push(AbiParam::new(types::I64)); }
    let id = ctx.module.declare_function(&name, Linkage::Local, &sig).expect("declare func");
    let handle = ctx.func_ids.len() as i64;
    ctx.func_ids.push(id);
    handle
}

/// Import an external function (from a linked library). Returns a handle.
#[no_mangle]
pub extern "C" fn cl_import_func(
    ctx: *mut ClContext, name_ptr: *const u8, name_len: i64,
    param_count: i64, has_ret: i64,
) -> i64 {
    let ctx = unsafe { &mut *ctx };
    let name = unsafe { str_from_raw(name_ptr, name_len) };
    let cc = ctx.module.isa().default_call_conv();
    let mut sig = Signature::new(cc);
    for _ in 0..param_count { sig.params.push(AbiParam::new(types::I64)); }
    if has_ret != 0 { sig.returns.push(AbiParam::new(types::I64)); }
    let id = ctx.module.declare_function(&name, Linkage::Import, &sig).expect("import func");
    let handle = ctx.func_ids.len() as i64;
    ctx.func_ids.push(id);
    handle
}

/// Import with custom signature: params is an array of type codes.
/// Type codes: 0=i64, 1=f64, 2=i8, 3=i32
#[no_mangle]
pub extern "C" fn cl_import_func_sig(
    ctx: *mut ClContext, name_ptr: *const u8, name_len: i64,
    param_types: *const i64, param_count: i64,
    ret_type: i64, // -1=void, 0=i64, 1=f64, 2=i8, 3=i32
) -> i64 {
    let ctx = unsafe { &mut *ctx };
    let name = unsafe { str_from_raw(name_ptr, name_len) };
    let cc = ctx.module.isa().default_call_conv();
    let mut sig = Signature::new(cc);
    let ptypes = unsafe { slice::from_raw_parts(param_types, param_count as usize) };
    for &t in ptypes {
        sig.params.push(AbiParam::new(type_code_to_type(t)));
    }
    if ret_type >= 0 {
        sig.returns.push(AbiParam::new(type_code_to_type(ret_type)));
    }
    let id = ctx.module.declare_function(&name, Linkage::Import, &sig).expect("import func sig");
    let handle = ctx.func_ids.len() as i64;
    ctx.func_ids.push(id);
    handle
}

fn type_code_to_type(code: i64) -> types::Type {
    match code {
        0 => types::I64,
        1 => types::F64,
        2 => types::I8,
        3 => types::I32,
        _ => types::I64,
    }
}

// ═════════════════════════════════════════════════════════════════════
// Function building
// ═════════════════════════════════════════════════════════════════════

/// Begin building a function body. Returns a builder handle.
#[no_mangle]
pub extern "C" fn cl_func_begin(
    ctx: *mut ClContext, func_handle: i64, param_count: i64, has_ret: i64,
) -> *mut ClBuilder {
    let ctx = unsafe { &*ctx };
    let cc = ctx.module.isa().default_call_conv();
    let mut sig = Signature::new(cc);
    for _ in 0..param_count { sig.params.push(AbiParam::new(types::I64)); }
    if has_ret != 0 { sig.returns.push(AbiParam::new(types::I64)); }

    let mut func = Function::new();
    func.signature = sig;

    box_ptr(ClBuilder {
        func,
        builder_ctx: FunctionBuilderContext::new(),
        vars: HashMap::new(),
        next_var: 0,
    })
}

/// Create entry block, append params, switch to it, seal it. Returns block handle (0).
#[no_mangle]
pub extern "C" fn cl_func_entry(bld: *mut ClBuilder) -> i64 {
    let bld = unsafe { &mut *bld };
    let mut builder = FunctionBuilder::new(&mut bld.func, &mut bld.builder_ctx);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    // We store the entry block as block 0.
    std::mem::forget(builder); // We'll recreate the builder each time.
    0
}

/// Finalize and define the function in the module.
#[no_mangle]
pub extern "C" fn cl_func_finish(ctx: *mut ClContext, bld: *mut ClBuilder, func_handle: i64) -> i64 {
    let bld = unsafe { Box::from_raw(bld) };
    let ctx = unsafe { &mut *ctx };
    let func_id = ctx.func_ids[func_handle as usize];
    let mut compile_ctx = cranelift::codegen::Context::for_function(bld.func);
    match ctx.module.define_function(func_id, &mut compile_ctx) {
        Ok(_) => 0,
        Err(e) => { eprintln!("cl_func_finish error: {e:?}"); -1 }
    }
}

// ═════════════════════════════════════════════════════════════════════
// Instructions — emitted via a temporary FunctionBuilder
//
// Since FunctionBuilder borrows the Function mutably and we can't
// store it across FFI calls (lifetime issues), we use a pattern where
// each instruction call creates a temporary builder, emits one
// instruction, and forgets the builder (keeping the Function state).
// ═════════════════════════════════════════════════════════════════════

// Helper: create a temporary builder, run a closure, forget builder.
// This is safe because FunctionBuilder::new just wraps the function reference
// and finalize() is called only once at the end.
macro_rules! with_builder {
    ($bld:expr, |$b:ident| $body:expr) => {{
        let bld = unsafe { &mut *$bld };
        let mut $b = FunctionBuilder::new(&mut bld.func, &mut bld.builder_ctx);
        let result = $body;
        std::mem::forget($b);
        result
    }};
}

/// Get block parameter (function parameter) by index.
#[no_mangle]
pub extern "C" fn cl_block_param(bld: *mut ClBuilder, block: i64, idx: i64) -> i64 {
    with_builder!(bld, |b| {
        let blk = Block::from_u32(block as u32);
        let params = b.block_params(blk);
        params[idx as usize].as_u32() as i64
    })
}

/// Emit iconst instruction. Returns value handle.
#[no_mangle]
pub extern "C" fn cl_iconst(bld: *mut ClBuilder, val: i64) -> i64 {
    with_builder!(bld, |b| b.ins().iconst(types::I64, val).as_u32() as i64)
}

/// Emit f64const instruction.
#[no_mangle]
pub extern "C" fn cl_f64const(bld: *mut ClBuilder, val: f64) -> i64 {
    with_builder!(bld, |b| b.ins().f64const(val).as_u32() as i64)
}

/// Emit iconst i8.
#[no_mangle]
pub extern "C" fn cl_iconst_i8(bld: *mut ClBuilder, val: i64) -> i64 {
    with_builder!(bld, |b| b.ins().iconst(types::I8, val).as_u32() as i64)
}

/// Emit call instruction. args is an array of value handles.
/// Returns the first result value handle, or -1 if void.
#[no_mangle]
pub extern "C" fn cl_call(
    ctx: *mut ClContext, bld: *mut ClBuilder,
    func_handle: i64, args: *const i64, arg_count: i64,
) -> i64 {
    let ctx_ref = unsafe { &mut *ctx };
    let func_id = ctx_ref.func_ids[func_handle as usize];
    let arg_vals: Vec<Value> = unsafe {
        slice::from_raw_parts(args, arg_count as usize)
    }.iter().map(|&v| Value::from_u32(v as u32)).collect();

    with_builder!(bld, |b| {
        let fref = ctx_ref.module.declare_func_in_func(func_id, b.func);
        let call = b.ins().call(fref, &arg_vals);
        let results = b.inst_results(call);
        if results.is_empty() { -1 } else { results[0].as_u32() as i64 }
    })
}

/// Emit return instruction.
#[no_mangle]
pub extern "C" fn cl_return(bld: *mut ClBuilder, val: i64) {
    with_builder!(bld, |b| {
        if val < 0 {
            b.ins().return_(&[]);
        } else {
            b.ins().return_(&[Value::from_u32(val as u32)]);
        }
    })
}

/// Emit jump to block.
#[no_mangle]
pub extern "C" fn cl_jump(bld: *mut ClBuilder, target: i64, args: *const i64, arg_count: i64) {
    let arg_vals: Vec<Value> = if arg_count > 0 && !args.is_null() {
        unsafe { slice::from_raw_parts(args, arg_count as usize) }
            .iter().map(|&v| Value::from_u32(v as u32)).collect()
    } else {
        vec![]
    };
    with_builder!(bld, |b| {
        b.ins().jump(Block::from_u32(target as u32), &arg_vals);
    })
}

/// Emit brif (conditional branch).
#[no_mangle]
pub extern "C" fn cl_brif(
    bld: *mut ClBuilder, cond: i64,
    then_block: i64, else_block: i64,
) {
    with_builder!(bld, |b| {
        b.ins().brif(
            Value::from_u32(cond as u32),
            Block::from_u32(then_block as u32), &[],
            Block::from_u32(else_block as u32), &[],
        );
    })
}

/// Create a new block. Returns block handle.
#[no_mangle]
pub extern "C" fn cl_block_new(bld: *mut ClBuilder) -> i64 {
    with_builder!(bld, |b| b.create_block().as_u32() as i64)
}

/// Switch to a block.
#[no_mangle]
pub extern "C" fn cl_block_switch(bld: *mut ClBuilder, block: i64) {
    with_builder!(bld, |b| b.switch_to_block(Block::from_u32(block as u32)))
}

/// Seal a block.
#[no_mangle]
pub extern "C" fn cl_block_seal(bld: *mut ClBuilder, block: i64) {
    with_builder!(bld, |b| b.seal_block(Block::from_u32(block as u32)))
}

/// Append a block parameter (i64 type). Returns the value handle.
#[no_mangle]
pub extern "C" fn cl_block_append_param(bld: *mut ClBuilder, block: i64) -> i64 {
    with_builder!(bld, |b| {
        b.append_block_param(Block::from_u32(block as u32), types::I64).as_u32() as i64
    })
}

/// Seal all blocks.
#[no_mangle]
pub extern "C" fn cl_seal_all(bld: *mut ClBuilder) {
    with_builder!(bld, |b| b.seal_all_blocks())
}

/// Finalize the builder.
#[no_mangle]
pub extern "C" fn cl_finalize(bld: *mut ClBuilder) {
    let bld = unsafe { &mut *bld };
    let mut b = FunctionBuilder::new(&mut bld.func, &mut bld.builder_ctx);
    b.seal_all_blocks();
    b.finalize();
}

// ═════════════════════════════════════════════════════════════════════
// Variables
// ═════════════════════════════════════════════════════════════════════

/// Declare a variable (i64 type). Returns variable handle.
#[no_mangle]
pub extern "C" fn cl_var_declare(bld: *mut ClBuilder) -> i64 {
    let bld = unsafe { &mut *bld };
    let v = Variable::new(bld.next_var as usize);
    bld.next_var += 1;
    with_builder!(bld, |b| { b.declare_var(v, types::I64); });
    v.index() as i64
}

/// Define a variable's value.
#[no_mangle]
pub extern "C" fn cl_var_def(bld: *mut ClBuilder, var_handle: i64, val: i64) {
    let var = Variable::new(var_handle as usize);
    with_builder!(bld, |b| b.def_var(var, Value::from_u32(val as u32)))
}

/// Use a variable (read its current value). Returns value handle.
#[no_mangle]
pub extern "C" fn cl_var_use(bld: *mut ClBuilder, var_handle: i64) -> i64 {
    let var = Variable::new(var_handle as usize);
    with_builder!(bld, |b| b.use_var(var).as_u32() as i64)
}

// ═════════════════════════════════════════════════════════════════════
// Arithmetic instructions
// ═════════════════════════════════════════════════════════════════════

#[no_mangle]
pub extern "C" fn cl_iadd(bld: *mut ClBuilder, a: i64, b_val: i64) -> i64 {
    with_builder!(bld, |b| b.ins().iadd(Value::from_u32(a as u32), Value::from_u32(b_val as u32)).as_u32() as i64)
}

#[no_mangle]
pub extern "C" fn cl_isub(bld: *mut ClBuilder, a: i64, b_val: i64) -> i64 {
    with_builder!(bld, |b| b.ins().isub(Value::from_u32(a as u32), Value::from_u32(b_val as u32)).as_u32() as i64)
}

#[no_mangle]
pub extern "C" fn cl_imul(bld: *mut ClBuilder, a: i64, b_val: i64) -> i64 {
    with_builder!(bld, |b| b.ins().imul(Value::from_u32(a as u32), Value::from_u32(b_val as u32)).as_u32() as i64)
}

/// Integer comparison. cc: 0=eq, 1=ne, 2=slt, 3=sge, 4=sgt, 5=sle
#[no_mangle]
pub extern "C" fn cl_icmp(bld: *mut ClBuilder, cc: i64, a: i64, b_val: i64) -> i64 {
    let intcc = match cc {
        0 => IntCC::Equal,
        1 => IntCC::NotEqual,
        2 => IntCC::SignedLessThan,
        3 => IntCC::SignedGreaterThanOrEqual,
        4 => IntCC::SignedGreaterThan,
        5 => IntCC::SignedLessThanOrEqual,
        _ => IntCC::Equal,
    };
    with_builder!(bld, |b| b.ins().icmp(intcc, Value::from_u32(a as u32), Value::from_u32(b_val as u32)).as_u32() as i64)
}

#[no_mangle]
pub extern "C" fn cl_iadd_imm(bld: *mut ClBuilder, a: i64, imm: i64) -> i64 {
    with_builder!(bld, |b| b.ins().iadd_imm(Value::from_u32(a as u32), imm).as_u32() as i64)
}

// ═════════════════════════════════════════════════════════════════════
// Data sections (string constants)
// ═════════════════════════════════════════════════════════════════════

/// Create a data section with the given bytes. Returns a data handle.
#[no_mangle]
pub extern "C" fn cl_data_create(
    ctx: *mut ClContext, data_ptr: *const u8, data_len: i64,
) -> i64 {
    let ctx = unsafe { &mut *ctx };
    let name = format!(".data.{}", ctx.str_counter);
    ctx.str_counter += 1;
    let data_id = ctx.module.declare_data(&name, Linkage::Local, false, false).unwrap();
    let bytes = unsafe { slice::from_raw_parts(data_ptr, data_len as usize) };
    let mut desc = DataDescription::new();
    desc.define(bytes.to_vec().into_boxed_slice());
    ctx.module.define_data(data_id, &desc).unwrap();
    let handle = ctx.data_ids.len() as i64;
    ctx.data_ids.push(data_id);
    handle
}

/// Get a pointer to a data section in a function. Returns value handle.
#[no_mangle]
pub extern "C" fn cl_data_addr(ctx: *mut ClContext, bld: *mut ClBuilder, data_handle: i64) -> i64 {
    let ctx = unsafe { &mut *ctx };
    let data_id = ctx.data_ids[data_handle as usize];
    with_builder!(bld, |b| {
        let gv = ctx.module.declare_data_in_func(data_id, b.func);
        b.ins().global_value(types::I64, gv).as_u32() as i64
    })
}

/// Get a function address as a value (for function pointers).
#[no_mangle]
pub extern "C" fn cl_func_addr(ctx: *mut ClContext, bld: *mut ClBuilder, func_handle: i64) -> i64 {
    let ctx = unsafe { &mut *ctx };
    let func_id = ctx.func_ids[func_handle as usize];
    with_builder!(bld, |b| {
        let fref = ctx.module.declare_func_in_func(func_id, b.func);
        b.ins().func_addr(types::I64, fref).as_u32() as i64
    })
}

// ═════════════════════════════════════════════════════════════════════
// Linker invocation
// ═════════════════════════════════════════════════════════════════════

/// Link an object file with libraries into an executable.
/// lib_paths is an array of (ptr, len) pairs.
#[no_mangle]
pub extern "C" fn cl_link(
    obj_path_ptr: *const u8, obj_path_len: i64,
    out_path_ptr: *const u8, out_path_len: i64,
    lib_path_ptr: *const u8, lib_path_len: i64,
) -> i64 {
    let obj_path = unsafe { str_from_raw(obj_path_ptr, obj_path_len) };
    let out_path = unsafe { str_from_raw(out_path_ptr, out_path_len) };
    let lib_path = unsafe { str_from_raw(lib_path_ptr, lib_path_len) };

    // Use rustc as linker driver on MSVC, gcc otherwise.
    if cfg!(target_env = "msvc") {
        let stub_path = format!("{}.stub.rs", obj_path);
        std::fs::write(&stub_path, "#![no_main]").ok();
        let status = std::process::Command::new("rustc")
            .arg("--edition=2021")
            .arg("--crate-type=bin")
            .arg(&stub_path)
            .arg("-o").arg(&out_path)
            .arg("-C").arg(format!("link-arg={}", obj_path))
            .arg("-C").arg(format!("link-arg={}", lib_path))
            .status();
        let _ = std::fs::remove_file(&stub_path);
        match status {
            Ok(s) if s.success() => 0,
            _ => -1,
        }
    } else {
        let linker = if cfg!(windows) { "gcc" } else { "cc" };
        let status = std::process::Command::new(linker)
            .arg(&obj_path).arg(&lib_path)
            .arg("-o").arg(&out_path)
            .arg("-lpthread").arg("-ldl").arg("-lm")
            .status();
        match status {
            Ok(s) if s.success() => 0,
            _ => -1,
        }
    }
}
