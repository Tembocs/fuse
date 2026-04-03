//! HIR → Cranelift IR translation and native binary emission.
//!
//! All Fuse values are boxed FuseValue pointers (i64) at the Cranelift level.
//! Operations go through fuse_rt_* FFI calls in fuse-runtime/src/ffi.rs.

use std::collections::HashMap;

use cranelift::prelude::*;
use cranelift::codegen::ir::Function;
use cranelift_module::{Module, Linkage, FuncId, DataDescription, DataId};
use cranelift_object::{ObjectModule, ObjectBuilder};

use crate::hir::nodes::*;
use crate::ast::nodes::{BinOp, UnaryOp};
use super::layout::*;

// ═════════════════════════════════════════════════════════════════════
// Codegen
// ═════════════════════════════════════════════════════════════════════

pub struct Codegen {
    module: ObjectModule,
    fuse_fns: HashMap<String, FuncId>,
    rt_fns: HashMap<String, FuncId>,
    string_data: HashMap<String, DataId>,
    str_counter: usize,
    lambda_counter: usize,
    /// Lambdas to compile (collected during function codegen, compiled after).
    pending_lambdas: Vec<(String, Vec<String>, Vec<HirStmt>)>,
    /// Type name → mangled __del__ function name (for ASAP destruction).
    del_fns: HashMap<String, String>,
    /// Extern function name → (param types, has return).
    extern_sigs: HashMap<String, (Vec<HirType>, HirType)>,
}

impl Codegen {
    pub fn new() -> Self {
        let isa_builder = cranelift_native::builder().expect("host ISA");
        let flags = settings::Flags::new(settings::builder());
        let isa = isa_builder.finish(flags).expect("build ISA");
        let obj_builder = ObjectBuilder::new(
            isa, "fuse_program",
            cranelift_module::default_libcall_names(),
        ).expect("object builder");
        Self {
            module: ObjectModule::new(obj_builder),
            fuse_fns: HashMap::new(),
            rt_fns: HashMap::new(),
            string_data: HashMap::new(),
            str_counter: 0,
            lambda_counter: 0,
            pending_lambdas: Vec::new(),
            del_fns: HashMap::new(),
            extern_sigs: HashMap::new(),
        }
    }

    pub fn compile(mut self, program: &HirProgram, output_path: &str) {
        self.import_runtime_fns();
        self.declare_fuse_fns(program);
        self.import_extern_fns(program);

        // Build del_fns map: type_name → mangled __del__ name.
        for d in &program.decls {
            match d {
                HirDecl::Struct(s) => {
                    if s.del_method.is_some() || s.methods.iter().any(|m| m.name == "__del__") {
                        self.del_fns.insert(s.name.clone(), format!("fuse_ext_{}___del__", s.name));
                    }
                }
                HirDecl::DataClass(dc) => {
                    if dc.del_method.is_some() || dc.methods.iter().any(|m| m.name == "__del__") {
                        self.del_fns.insert(dc.name.clone(), format!("fuse_ext_{}___del__", dc.name));
                    }
                }
                _ => {}
            }
        }

        // Collect functions to compile.
        let fn_decls: Vec<HirFnDecl> = program.decls.iter().filter_map(|d| {
            if let HirDecl::Fn(f) = d { Some(f.clone()) } else { None }
        }).collect();
        // Also collect struct/DC methods — set ext_type so they get the right mangled name.
        let mut method_decls: Vec<HirFnDecl> = Vec::new();
        for d in &program.decls {
            match d {
                HirDecl::Struct(s) => {
                    for m in &s.methods {
                        let mut mc = m.clone();
                        mc.ext_type = Some(s.name.clone());
                        method_decls.push(mc);
                    }
                }
                HirDecl::DataClass(dc) => {
                    for m in &dc.methods {
                        let mut mc = m.clone();
                        mc.ext_type = Some(dc.name.clone());
                        method_decls.push(mc);
                    }
                }
                _ => {}
            }
        }

        for f in fn_decls.iter().chain(method_decls.iter()) {
            self.codegen_fn(f);
        }

        // Compile any pending lambdas (may produce more lambdas, so loop).
        while !self.pending_lambdas.is_empty() {
            let lambdas = std::mem::take(&mut self.pending_lambdas);
            for (name, params, body) in lambdas {
                self.compile_lambda(&name, &params, &body);
            }
        }

        self.generate_main(program);

        let product = self.module.finish();
        let obj_bytes = product.emit().expect("emit object");
        let obj_path = format!("{output_path}.o");
        std::fs::write(&obj_path, &obj_bytes).expect("write object file");
        link(&obj_path, output_path);
        let _ = std::fs::remove_file(&obj_path);
    }

    fn import_runtime_fns(&mut self) {
        let cc = self.module.isa().default_call_conv();
        for info in RT_FUNCTIONS {
            let sig = rt_sig(info.params, info.ret, cc);
            let id = self.module.declare_function(info.name, Linkage::Import, &sig)
                .expect(&format!("declare {}", info.name));
            self.rt_fns.insert(info.name.to_string(), id);
        }
    }

    fn import_extern_fns(&mut self, program: &HirProgram) {
        let cc = self.module.isa().default_call_conv();
        for d in &program.decls {
            if let HirDecl::ExternFn(ef) = d {
                // Skip if already imported by the runtime.
                if self.rt_fns.contains_key(&ef.name) {
                    let id = *self.rt_fns.get(&ef.name).unwrap();
                    self.fuse_fns.insert(format!("fuse_{}", ef.name), id);
                    let param_types: Vec<HirType> = ef.params.iter().map(|p| p.ty.clone()).collect();
                    self.extern_sigs.insert(ef.name.clone(), (param_types, ef.ret_ty.clone()));
                    continue;
                }
                // Map FFI param types to Cranelift types.
                let mut sig = Signature::new(cc);
                for p in &ef.params {
                    let ct = match &p.ty {
                        HirType::Int => types::I64,
                        HirType::Float => types::F64,
                        HirType::Bool => types::I8,
                        _ => PTR_TYPE,
                    };
                    sig.params.push(AbiParam::new(ct));
                }
                match &ef.ret_ty {
                    HirType::Unit => {}
                    HirType::Int => { sig.returns.push(AbiParam::new(types::I64)); }
                    HirType::Float => { sig.returns.push(AbiParam::new(types::F64)); }
                    HirType::Bool => { sig.returns.push(AbiParam::new(types::I8)); }
                    _ => { sig.returns.push(AbiParam::new(PTR_TYPE)); }
                }
                let id = self.module.declare_function(&ef.name, Linkage::Import, &sig)
                    .expect(&format!("import extern fn {}", ef.name));
                let param_types: Vec<HirType> = ef.params.iter().map(|p| p.ty.clone()).collect();
                self.extern_sigs.insert(ef.name.clone(), (param_types, ef.ret_ty.clone()));
                self.fuse_fns.insert(format!("fuse_{}", ef.name), id);
                self.rt_fns.insert(ef.name.clone(), id);
            }
        }
    }

    fn declare_fuse_fns(&mut self, program: &HirProgram) {
        let cc = self.module.isa().default_call_conv();
        for d in &program.decls {
            if let HirDecl::Fn(f) = d {
                self.declare_one_fn(f, cc);
            }
        }
        for d in &program.decls {
            match d {
                HirDecl::Struct(s) => {
                    for m in &s.methods { self.declare_method(&s.name, m, cc); }
                }
                HirDecl::DataClass(dc) => {
                    for m in &dc.methods { self.declare_method(&dc.name, m, cc); }
                }
                _ => {}
            }
        }
    }

    fn declare_method(&mut self, type_name: &str, f: &HirFnDecl, cc: isa::CallConv) {
        let mangled = format!("fuse_ext_{}_{}", type_name, f.name);
        let pc = f.params.iter().filter(|p| p.name != "self").count() + 1; // +1 for self
        let sig = fuse_fn_sig(pc, cc);
        let id = self.module.declare_function(&mangled, Linkage::Local, &sig)
            .expect(&format!("declare method {}.{}", type_name, f.name));
        self.fuse_fns.insert(mangled, id);
    }

    fn declare_one_fn(&mut self, f: &HirFnDecl, cc: isa::CallConv) {
        let mangled = mangle(&f);
        let pc = f.params.iter().filter(|p| p.name != "self").count()
            + if f.ext_type.is_some() { 1 } else { 0 };
        let sig = fuse_fn_sig(pc, cc);
        let id = self.module.declare_function(&mangled, Linkage::Local, &sig)
            .expect(&format!("declare {}", f.name));
        self.fuse_fns.insert(mangled, id);
    }

    /// Compile a lambda as a standalone Cranelift function.
    /// Signature: fn(env: *mut FuseValue, arg: *mut FuseValue) -> *mut FuseValue
    fn compile_lambda(&mut self, name: &str, params: &[String], body: &[HirStmt]) {
        let func_id = *self.fuse_fns.get(name).unwrap();
        let cc = self.module.isa().default_call_conv();
        // Lambda signature: (env, arg) -> result
        let sig = fuse_fn_sig(2, cc);

        let mut func = Function::new();
        func.signature = sig;
        let mut bctx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut func, &mut bctx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            b.seal_block(entry);

            {
                let mut ctx = FnGen::new(&mut b, &mut self.module, &mut self.fuse_fns, &self.rt_fns, &mut self.string_data, &mut self.str_counter, &mut self.lambda_counter, &mut self.pending_lambdas, &self.del_fns, &self.extern_sigs);
                let bp: Vec<Value> = ctx.b.block_params(entry).to_vec();
                let _env = bp[0]; // captured env (unused for now)
                let arg = bp[1];  // the argument value

                // Bind the first param name to the argument.
                if let Some(p) = params.first() {
                    ctx.def(p, arg);
                }

                let result = ctx.stmts(body);
                if !ctx.terminated {
                    let rv = result.unwrap_or_else(|| ctx.rt("fuse_rt_unit", &[]));
                    ctx.b.ins().return_(&[rv]);
                }
            }
            b.seal_all_blocks();
            b.finalize();
        }
        let mut cctx = cranelift::codegen::Context::for_function(func);
        self.module.define_function(func_id, &mut cctx).unwrap();
    }

    fn intern_string(&mut self, s: &str) -> DataId {
        if let Some(&id) = self.string_data.get(s) { return id; }
        let name = format!(".str.{}", self.str_counter);
        self.str_counter += 1;
        let id = self.module.declare_data(&name, Linkage::Local, false, false).unwrap();
        let mut desc = DataDescription::new();
        desc.define(s.as_bytes().to_vec().into_boxed_slice());
        self.module.define_data(id, &desc).unwrap();
        self.string_data.insert(s.to_string(), id);
        id
    }

    // ── Compile a single function ──────────────────────────────────

    fn codegen_fn(&mut self, f: &HirFnDecl) {
        let mangled = mangle(f);
        let func_id = *self.fuse_fns.get(&mangled).unwrap();
        let pc = f.params.iter().filter(|p| p.name != "self").count()
            + if f.ext_type.is_some() { 1 } else { 0 };
        let cc = self.module.isa().default_call_conv();
        let sig = fuse_fn_sig(pc, cc);

        let mut func = Function::new();
        func.signature = sig;

        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut func, &mut builder_ctx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            b.seal_block(entry);

            {
                let mut ctx = FnGen::new(&mut b, &mut self.module, &mut self.fuse_fns, &self.rt_fns, &mut self.string_data, &mut self.str_counter, &mut self.lambda_counter, &mut self.pending_lambdas, &self.del_fns, &self.extern_sigs);
                let params: Vec<Value> = ctx.b.block_params(entry).to_vec();

                let mut pi = 0;
                if f.ext_type.is_some() && pi < params.len() {
                    ctx.def("self", params[pi]); pi += 1;
                }
                ctx.mutref_cells = Vec::new();
                let mut mutref_cells: Vec<(String, Value)> = Vec::new();
                for p in &f.params {
                    if p.name == "self" {
                        if f.ext_type.is_some() { continue; }
                        if pi < params.len() { ctx.def("self", params[pi]); pi += 1; }
                    } else if pi < params.len() {
                        let param_val = params[pi]; pi += 1;
                        if p.convention == Convention::Mutref {
                            // Mutref param is a ref cell — unwrap it.
                            let cell = param_val;
                            let actual = ctx.rt("fuse_rt_ref_get", &[cell]);
                            ctx.def(&p.name, actual);
                            mutref_cells.push((p.name.clone(), cell));
                        } else {
                            ctx.def(&p.name, param_val);
                        }
                    }
                    // Track owned parameters of destructible types for ASAP.
                    if p.convention == Convention::Owned {
                        let type_name = match &p.ty {
                            HirType::Struct(n) | HirType::DataClass(n) => Some(n.clone()),
                            _ => None,
                        };
                        if let Some(tn) = type_name {
                            if ctx.del_fns.contains_key(&tn) {
                                ctx.destructibles.push((p.name.clone(), tn));
                            }
                        }
                    }
                }

                // Set mutref_cells before body so explicit `return` can write back.
                ctx.mutref_cells = mutref_cells;

                let result = match &f.body {
                    HirFnBody::Block(stmts) => ctx.stmts_asap(stmts),
                    HirFnBody::Expr(e) => Some(ctx.expr(e)),
                };
                if !ctx.terminated {
                    // Write mutref params back to their ref cells.
                    for (name, cell) in &ctx.mutref_cells.clone() {
                        if let Some(&var) = ctx.vars.get(name) {
                            let val = ctx.b.use_var(var);
                            ctx.rt_void("fuse_rt_ref_set", &[*cell, val]);
                        }
                    }
                    // Emit deferred expressions before final return.
                    ctx.emit_defers();
                    let rv = result.unwrap_or_else(|| ctx.rt("fuse_rt_unit", &[]));
                    ctx.b.ins().return_(&[rv]);
                }
            }
            // FnGen dropped — b is no longer borrowed.
            b.seal_all_blocks();
            b.finalize();
        }
        let mut cctx = cranelift::codegen::Context::for_function(func);
        self.module.define_function(func_id, &mut cctx).expect("define fn");
    }

    // ── Generate C main ────────────────────────────────────────────

    fn generate_main(&mut self, program: &HirProgram) {
        let entry = program.decls.iter().find_map(|d| {
            if let HirDecl::Fn(f) = d { if f.is_entrypoint { return Some(f); } }
            None
        });
        let entry = match entry { Some(f) => f, None => return };
        let cc = self.module.isa().default_call_conv();

        let mut main_sig = Signature::new(cc);
        main_sig.returns.push(AbiParam::new(types::I32));
        let main_id = self.module.declare_function("main", Linkage::Export, &main_sig).unwrap();

        let mut func = Function::new();
        func.signature = main_sig;
        let mut bctx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut func, &mut bctx);
            let blk = b.create_block();
            b.switch_to_block(blk);
            b.seal_block(blk);

            let mangled = mangle(entry);
            let cid = *self.fuse_fns.get(&mangled).unwrap();
            let cref = self.module.declare_func_in_func(cid, b.func);
            let call = b.ins().call(cref, &[]);
            let result = b.inst_results(call)[0];

            // Check Err.
            let ie_id = *self.rt_fns.get("fuse_rt_is_err").unwrap();
            let ie_ref = self.module.declare_func_in_func(ie_id, b.func);
            let ie_call = b.ins().call(ie_ref, &[result]);
            let is_err = b.inst_results(ie_call)[0];

            let err_blk = b.create_block();
            let ok_blk = b.create_block();

            // brif(cond, then_block, then_args, else_block, else_args)
            b.ins().brif(is_err, err_blk, &[], ok_blk, &[]);

            b.switch_to_block(err_blk);
            b.seal_block(err_blk);
            let uw_id = *self.rt_fns.get("fuse_rt_unwrap_enum").unwrap();
            let uw_ref = self.module.declare_func_in_func(uw_id, b.func);
            let uw_call = b.ins().call(uw_ref, &[result]);
            let err_val = b.inst_results(uw_call)[0];
            let ep_id = *self.rt_fns.get("fuse_rt_eprintln").unwrap();
            let ep_ref = self.module.declare_func_in_func(ep_id, b.func);
            b.ins().call(ep_ref, &[err_val]);
            let one = b.ins().iconst(types::I32, 1);
            b.ins().return_(&[one]);

            b.switch_to_block(ok_blk);
            b.seal_block(ok_blk);
            let zero = b.ins().iconst(types::I32, 0);
            b.ins().return_(&[zero]);

            b.finalize();
        }
        let mut cctx = cranelift::codegen::Context::for_function(func);
        self.module.define_function(main_id, &mut cctx).unwrap();
    }
}

// ═════════════════════════════════════════════════════════════════════
// Per-function code generation context
// ═════════════════════════════════════════════════════════════════════

/// Borrows the FunctionBuilder and module independently to avoid
/// the borrow-checker conflict of having &mut Codegen + &mut FunctionBuilder.
struct FnGen<'a, 'b> {
    b: &'a mut FunctionBuilder<'b>,
    module: &'a mut ObjectModule,
    fuse_fns: &'a mut HashMap<String, FuncId>,
    rt_fns: &'a HashMap<String, FuncId>,
    string_data: &'a mut HashMap<String, DataId>,
    str_counter: &'a mut usize,
    vars: HashMap<String, Variable>,
    next_var: usize,
    /// Track whether the current block has been terminated.
    terminated: bool,
    /// Deferred expressions to execute before function return (LIFO order).
    defers: Vec<HirExpr>,
    /// Lambda counter (shared with Codegen via pointer).
    lambda_counter: &'a mut usize,
    /// Pending lambdas to compile after the current function.
    pending_lambdas: &'a mut Vec<(String, Vec<String>, Vec<HirStmt>)>,
    /// Type name → mangled __del__ function name.
    del_fns: &'a HashMap<String, String>,
    /// Extern function sigs for unboxing args.
    extern_sigs: &'a HashMap<String, (Vec<HirType>, HirType)>,
    /// Variables that have __del__ methods: (var_name, type_name).
    destructibles: Vec<(String, String)>,
    /// Variables already destroyed by ASAP (don't double-destroy).
    destroyed: std::collections::HashSet<String>,
    /// Mutref ref cells: (param_name, cell_value) for writeback before return.
    mutref_cells: Vec<(String, Value)>,
}

impl<'a, 'b> FnGen<'a, 'b> {
    fn new(
        b: &'a mut FunctionBuilder<'b>,
        module: &'a mut ObjectModule,
        fuse_fns: &'a mut HashMap<String, FuncId>,
        rt_fns: &'a HashMap<String, FuncId>,
        string_data: &'a mut HashMap<String, DataId>,
        str_counter: &'a mut usize,
        lambda_counter: &'a mut usize,
        pending_lambdas: &'a mut Vec<(String, Vec<String>, Vec<HirStmt>)>,
        del_fns: &'a HashMap<String, String>,
        extern_sigs: &'a HashMap<String, (Vec<HirType>, HirType)>,
    ) -> Self {
        Self { b, module, fuse_fns, rt_fns, string_data, str_counter,
               vars: HashMap::new(), next_var: 0, terminated: false,
               defers: Vec::new(), lambda_counter, pending_lambdas,
               del_fns, extern_sigs, destructibles: Vec::new(), destroyed: std::collections::HashSet::new(),
               mutref_cells: Vec::new() }
    }

    fn def(&mut self, name: &str, val: Value) {
        let v = Variable::new(self.next_var);
        self.next_var += 1;
        self.b.declare_var(v, FUSE_VALUE_TYPE);
        self.b.def_var(v, val);
        self.vars.insert(name.to_string(), v);
    }

    fn rt(&mut self, name: &str, args: &[Value]) -> Value {
        let id = *self.rt_fns.get(name).expect(name);
        let r = self.module.declare_func_in_func(id, self.b.func);
        let c = self.b.ins().call(r, args);
        let res = self.b.inst_results(c);
        if res.is_empty() { self.rt("fuse_rt_unit", &[]) } else { res[0] }
    }

    fn rt_void(&mut self, name: &str, args: &[Value]) {
        let id = *self.rt_fns.get(name).expect(name);
        let r = self.module.declare_func_in_func(id, self.b.func);
        self.b.ins().call(r, args);
    }

    fn call_fuse(&mut self, mangled: &str, args: &[Value]) -> Value {
        if let Some(&id) = self.fuse_fns.get(mangled) {
            let r = self.module.declare_func_in_func(id, self.b.func);
            let c = self.b.ins().call(r, args);
            let results = self.b.inst_results(c);
            if results.is_empty() {
                self.rt("fuse_rt_unit", &[])
            } else {
                results[0]
            }
        } else {
            self.rt("fuse_rt_unit", &[])
        }
    }

    /// Try to infer the type name from a construction expression.
    fn infer_type_from_expr(&self, e: &HirExpr) -> Option<String> {
        match &e.kind {
            HirExprKind::StructConstruct { type_name, .. } => Some(type_name.clone()),
            _ => match &e.ty {
                HirType::Struct(n) | HirType::DataClass(n) => Some(n.clone()),
                _ => None,
            }
        }
    }

    fn cg_lambda_id(&mut self) -> usize {
        let id = *self.lambda_counter;
        *self.lambda_counter += 1;
        id
    }

    /// Emit all deferred expressions in reverse (LIFO) order.
    fn emit_defers(&mut self) {
        let defers: Vec<HirExpr> = self.defers.clone();
        for d in defers.iter().rev() {
            self.expr(d);
        }
    }

    fn str_const(&mut self, s: &str) -> (Value, Value) {
        let did = intern_str(self.module, self.string_data, self.str_counter, s);
        let gv = self.module.declare_data_in_func(did, self.b.func);
        let ptr = self.b.ins().global_value(PTR_TYPE, gv);
        let len = self.b.ins().iconst(types::I64, s.len() as i64);
        (ptr, len)
    }

    fn make_str(&mut self, s: &str) -> Value {
        let (p, l) = self.str_const(s);
        self.rt("fuse_rt_str", &[p, l])
    }

    // ── Statements ─────────────────────────────────────────────────

    fn stmts(&mut self, ss: &[HirStmt]) -> Option<Value> {
        self.stmts_inner(ss, false)
    }

    fn stmts_asap(&mut self, ss: &[HirStmt]) -> Option<Value> {
        self.stmts_inner(ss, true)
    }

    fn stmts_inner(&mut self, ss: &[HirStmt], is_fn_body: bool) -> Option<Value> {
        // Compute last-use for ASAP destruction.
        let last_use = self.compute_last_use_map(ss);
        // For function bodies, include params in ASAP. For nested blocks, only new vars.
        let destr_start = if is_fn_body { 0 } else { self.destructibles.len() };

        let mut last = None;
        for (i, s) in ss.iter().enumerate() {
            if self.terminated { break; }
            last = self.stmt(s);

            // ASAP destruction: call __del__ for variables defined in this scope
            // whose last use was this statement.
            if !self.terminated {
                let to_destroy: Vec<(String, String)> = self.destructibles[destr_start..].iter()
                    .filter(|(name, _)| last_use.get(name).copied() == Some(i) && !self.destroyed.contains(name))
                    .cloned()
                    .collect();
                for (var_name, type_name) in &to_destroy {
                    if let Some(del_mangled) = self.del_fns.get(type_name) {
                        if let Some(&var) = self.vars.get(var_name) {
                            if self.fuse_fns.contains_key(del_mangled) {
                                let val = self.b.use_var(var);
                                self.call_fuse(del_mangled, &[val]);
                                self.destroyed.insert(var_name.clone());
                            }
                        }
                    }
                }
            }
        }
        last
    }

    /// Compute last-use index for each variable referenced in a statement list.
    fn compute_last_use_map(&self, ss: &[HirStmt]) -> HashMap<String, usize> {
        let mut map: HashMap<String, usize> = HashMap::new();
        for (i, s) in ss.iter().enumerate() {
            // For defer statements, push last-use beyond the block so destruction
            // happens after all non-deferred statements.
            let idx = if matches!(s, HirStmt::Defer(..)) { ss.len() } else { i };
            for name in self.collect_stmt_names(s) {
                let e = map.entry(name).or_insert(0);
                if idx > *e { *e = idx; }
            }
        }
        map
    }

    fn collect_stmt_names(&self, s: &HirStmt) -> Vec<String> {
        let mut names = Vec::new();
        match s {
            HirStmt::Val { name: _, value, .. } | HirStmt::Var { name: _, value, .. } =>
                self.collect_expr_names(value, &mut names),
            HirStmt::ValTuple { value, .. } =>
                self.collect_expr_names(value, &mut names),
            HirStmt::Assign { target, value, .. } => {
                self.collect_expr_names(target, &mut names);
                self.collect_expr_names(value, &mut names);
            }
            HirStmt::Expr(e) => self.collect_expr_names(e, &mut names),
            HirStmt::Return(Some(e), _) | HirStmt::Defer(e, _) =>
                self.collect_expr_names(e, &mut names),
            HirStmt::If { cond, then_body, else_body, .. } => {
                self.collect_expr_names(cond, &mut names);
                for s in then_body { names.extend(self.collect_stmt_names(s)); }
                if let Some(eb) = else_body {
                    match eb {
                        HirElseBody::ElseIf(s) => names.extend(self.collect_stmt_names(s)),
                        HirElseBody::Block(ss) => { for s in ss { names.extend(self.collect_stmt_names(s)); } }
                    }
                }
            }
            HirStmt::For { iter, body, .. } => {
                self.collect_expr_names(iter, &mut names);
                for s in body { names.extend(self.collect_stmt_names(s)); }
            }
            HirStmt::Loop(body, _) => {
                for s in body { names.extend(self.collect_stmt_names(s)); }
            }
            _ => {}
        }
        names
    }

    fn collect_expr_names(&self, e: &HirExpr, names: &mut Vec<String>) {
        match &e.kind {
            HirExprKind::Ident(n) => names.push(n.clone()),
            HirExprKind::SelfExpr => names.push("self".into()),
            HirExprKind::Binary(l, _, r) | HirExprKind::Elvis(l, r) => {
                self.collect_expr_names(l, names);
                self.collect_expr_names(r, names);
            }
            HirExprKind::Unary(_, inner) | HirExprKind::Move(inner) |
            HirExprKind::MutrefE(inner) | HirExprKind::RefE(inner) |
            HirExprKind::Question(inner) | HirExprKind::Await(inner) |
            HirExprKind::Spawn(inner, _) | HirExprKind::Field(inner, _) |
            HirExprKind::OptChain(inner, _) => {
                self.collect_expr_names(inner, names);
            }
            HirExprKind::Call(callee, args) => {
                self.collect_expr_names(callee, names);
                for a in args { self.collect_expr_names(a, names); }
            }
            HirExprKind::MethodCall { receiver, args, .. } => {
                self.collect_expr_names(receiver, names);
                for a in args { self.collect_expr_names(a, names); }
            }
            HirExprKind::FStr(parts) | HirExprKind::List(parts) | HirExprKind::Tuple(parts) => {
                for p in parts { self.collect_expr_names(p, names); }
            }
            HirExprKind::Match(subj, arms) => {
                self.collect_expr_names(subj, names);
                for a in arms { self.collect_expr_names(&a.body, names); }
            }
            HirExprKind::When(arms) => {
                for a in arms {
                    if let Some(c) = &a.cond { self.collect_expr_names(c, names); }
                    self.collect_expr_names(&a.body, names);
                }
            }
            HirExprKind::StructConstruct { args, .. } | HirExprKind::PathCall { args, .. } => {
                for a in args { self.collect_expr_names(a, names); }
            }
            HirExprKind::EnumConstruct { value, .. } => {
                if let Some(v) = value { self.collect_expr_names(v, names); }
            }
            HirExprKind::Block(ss) => {
                for s in ss { names.extend(self.collect_stmt_names(s)); }
            }
            _ => {}
        }
    }

    fn stmt(&mut self, s: &HirStmt) -> Option<Value> {
        match s {
            HirStmt::Val { name, value, ty, .. } | HirStmt::Var { name, value, ty, .. } => {
                let v = self.expr(value);
                // Track destructible variables for ASAP.
                let mut type_name = match ty {
                    HirType::Struct(n) | HirType::DataClass(n) => Some(n.clone()),
                    _ => self.infer_type_from_expr(value),
                };
                // If value is from another variable, check if source is destructible.
                if type_name.is_none() {
                    if let HirExprKind::Ident(src_name) = &value.kind {
                        if let Some((_, src_type)) = self.destructibles.iter().find(|(n, _)| n == src_name) {
                            type_name = Some(src_type.clone());
                        }
                    }
                }
                // Clone if assigning a destructible value from another variable
                // (ensures independent lifecycle for @value copies).
                let v = if let Some(ref tn) = type_name {
                    if self.del_fns.contains_key(tn) {
                        if matches!(&value.kind, HirExprKind::Ident(_) | HirExprKind::SelfExpr) {
                            self.rt("fuse_rt_clone", &[v])
                        } else { v }
                    } else { v }
                } else { v };
                self.def(name, v);
                if let Some(tn) = type_name {
                    if self.del_fns.contains_key(&tn) {
                        self.destructibles.push((name.clone(), tn));
                    }
                }
                None
            }
            HirStmt::ValTuple { names, value, .. } => {
                let v = self.expr(value);
                for (i, n) in names.iter().enumerate() {
                    let idx = self.b.ins().iconst(types::I64, i as i64);
                    let idx_v = self.rt("fuse_rt_int", &[idx]);
                    let elem = self.rt("fuse_rt_list_get", &[v, idx_v]);
                    self.def(n, elem);
                }
                None
            }
            HirStmt::Assign { target, value, .. } => {
                let v = self.expr(value);
                match &target.kind {
                    HirExprKind::Ident(n) => {
                        if let Some(&var) = self.vars.get(n) {
                            self.b.def_var(var, v);
                        }
                    }
                    HirExprKind::Field(obj, f) => {
                        let o = self.expr(obj);
                        let (fp, fl) = self.str_const(f);
                        self.rt_void("fuse_rt_set_field", &[o, fp, fl, v]);
                    }
                    _ => {}
                }
                None
            }
            HirStmt::Expr(e) => Some(self.expr(e)),
            HirStmt::Return(e, _) => {
                let v = match e {
                    Some(ex) => self.expr(ex),
                    None => self.rt("fuse_rt_unit", &[]),
                };
                // Write mutref params back before returning.
                for (name, cell) in &self.mutref_cells.clone() {
                    if let Some(&var) = self.vars.get(name) {
                        let val = self.b.use_var(var);
                        self.rt_void("fuse_rt_ref_set", &[*cell, val]);
                    }
                }
                // Emit deferred expressions in reverse (LIFO) before returning.
                self.emit_defers();
                self.b.ins().return_(&[v]);
                self.terminated = true;
                let nb = self.b.create_block();
                self.b.switch_to_block(nb);
                self.b.seal_block(nb);
                self.terminated = false;
                None
            }
            HirStmt::If { cond, then_body, else_body, .. } => {
                let cv = self.expr(cond);
                let tr = self.rt("fuse_rt_is_truthy", &[cv]);

                let tb = self.b.create_block();
                let eb = self.b.create_block();
                let mb = self.b.create_block();
                self.b.append_block_param(mb, FUSE_VALUE_TYPE);

                self.b.ins().brif(tr, tb, &[], eb, &[]);

                self.b.switch_to_block(tb);
                self.b.seal_block(tb);
                self.terminated = false;
                let tv = self.stmts(then_body).unwrap_or_else(|| self.rt("fuse_rt_unit", &[]));
                if !self.terminated { self.b.ins().jump(mb, &[tv]); }
                self.terminated = false;

                self.b.switch_to_block(eb);
                self.b.seal_block(eb);
                let ev = match else_body {
                    Some(HirElseBody::Block(ss)) => self.stmts(ss).unwrap_or_else(|| self.rt("fuse_rt_unit", &[])),
                    Some(HirElseBody::ElseIf(s)) => self.stmt(s).unwrap_or_else(|| self.rt("fuse_rt_unit", &[])),
                    None => self.rt("fuse_rt_unit", &[]),
                };
                if !self.terminated { self.b.ins().jump(mb, &[ev]); }
                self.terminated = false;

                self.b.switch_to_block(mb);
                self.b.seal_block(mb);
                Some(self.b.block_params(mb)[0])
            }
            HirStmt::For { var, iter, body, .. } => {
                let list = self.expr(iter);
                let ll = self.rt("fuse_rt_list_len", &[list]);
                let len = self.rt("fuse_rt_as_int", &[ll]);

                let cv = Variable::new(self.next_var); self.next_var += 1;
                self.b.declare_var(cv, types::I64);
                let zero_i = self.b.ins().iconst(types::I64, 0);
                self.b.def_var(cv, zero_i);

                let hdr = self.b.create_block();
                let bdy = self.b.create_block();
                let ext = self.b.create_block();

                self.b.ins().jump(hdr, &[]);

                self.b.switch_to_block(hdr);
                let i = self.b.use_var(cv);
                let cmp = self.b.ins().icmp(IntCC::SignedLessThan, i, len);
                self.b.ins().brif(cmp, bdy, &[], ext, &[]);

                self.b.switch_to_block(bdy);
                self.b.seal_block(bdy);
                self.terminated = false;
                let idx = self.rt("fuse_rt_int", &[i]);
                let elem = self.rt("fuse_rt_list_get", &[list, idx]);
                self.def(var, elem);
                self.stmts(body);
                if !self.terminated {
                    let i2 = self.b.use_var(cv);
                    let inc = self.b.ins().iadd_imm(i2, 1);
                    self.b.def_var(cv, inc);
                    self.b.ins().jump(hdr, &[]);
                }
                self.terminated = false;

                self.b.seal_block(hdr);
                self.b.switch_to_block(ext);
                self.b.seal_block(ext);
                None
            }
            HirStmt::Loop(body, _) => {
                let lb = self.b.create_block();
                let ext = self.b.create_block();
                self.b.ins().jump(lb, &[]);
                self.b.switch_to_block(lb);
                self.terminated = false;
                self.stmts(body);
                if !self.terminated { self.b.ins().jump(lb, &[]); }
                self.terminated = false;
                self.b.seal_block(lb);
                self.b.switch_to_block(ext);
                self.b.seal_block(ext);
                None
            }
            HirStmt::Defer(e, _) => {
                self.defers.push(e.clone());
                None
            }
        }
    }

    // ── Expressions ────────────────────────────────────────────────

    fn expr(&mut self, e: &HirExpr) -> Value {
        match &e.kind {
            HirExprKind::IntLit(v) => { let r = self.b.ins().iconst(types::I64, *v); self.rt("fuse_rt_int", &[r]) }
            HirExprKind::FloatLit(v) => { let r = self.b.ins().f64const(*v); self.rt("fuse_rt_float", &[r]) }
            HirExprKind::StrLit(s) => self.make_str(s),
            HirExprKind::BoolLit(b) => { let r = self.b.ins().iconst(types::I8, if *b {1} else {0}); self.rt("fuse_rt_bool", &[r]) }
            HirExprKind::Unit => self.rt("fuse_rt_unit", &[]),
            HirExprKind::Ident(n) => {
                if let Some(&v) = self.vars.get(n) { self.b.use_var(v) }
                else { self.rt("fuse_rt_unit", &[]) }
            }
            HirExprKind::SelfExpr => {
                if let Some(&v) = self.vars.get("self") { self.b.use_var(v) }
                else { self.rt("fuse_rt_unit", &[]) }
            }
            HirExprKind::Binary(l, op, r) => {
                match op {
                    BinOp::And => return self.and_expr(l, r),
                    BinOp::Or => return self.or_expr(l, r),
                    _ => {}
                }
                let lv = self.expr(l); let rv = self.expr(r);
                let f = match op {
                    BinOp::Add=>"fuse_rt_add", BinOp::Sub=>"fuse_rt_sub", BinOp::Mul=>"fuse_rt_mul",
                    BinOp::Div=>"fuse_rt_div", BinOp::Mod=>"fuse_rt_mod",
                    BinOp::Eq=>"fuse_rt_eq", BinOp::Ne=>"fuse_rt_ne",
                    BinOp::Lt=>"fuse_rt_lt", BinOp::Gt=>"fuse_rt_gt",
                    BinOp::Le=>"fuse_rt_le", BinOp::Ge=>"fuse_rt_ge",
                    _ => unreachable!(),
                };
                self.rt(f, &[lv, rv])
            }
            HirExprKind::Unary(op, inner) => {
                let v = self.expr(inner);
                match op {
                    UnaryOp::Neg => self.rt("fuse_rt_neg", &[v]),
                    UnaryOp::Not => {
                        let tr = self.rt("fuse_rt_is_truthy", &[v]);
                        let z = self.b.ins().iconst(types::I8, 0);
                        let inv = self.b.ins().icmp(IntCC::Equal, tr, z);
                        self.rt("fuse_rt_bool", &[inv])
                    }
                }
            }
            HirExprKind::FStr(parts) => {
                let mut r = self.make_str("");
                for p in parts {
                    let pv = self.expr(p);
                    let ps = self.rt("fuse_rt_to_display_string", &[pv]);
                    r = self.rt("fuse_rt_add", &[r, ps]);
                }
                r
            }
            HirExprKind::List(elems) => {
                let l = self.rt("fuse_rt_list_new", &[]);
                for e in elems { let v = self.expr(e); self.rt_void("fuse_rt_list_push", &[l, v]); }
                l
            }
            HirExprKind::Tuple(elems) => {
                let l = self.rt("fuse_rt_list_new", &[]);
                for e in elems { let v = self.expr(e); self.rt_void("fuse_rt_list_push", &[l, v]); }
                l
            }
            HirExprKind::Call(callee, args) => self.call_expr(callee, args),
            HirExprKind::MethodCall { receiver, method, args, receiver_type } =>
                self.method_call(receiver, method, args, receiver_type),
            HirExprKind::Field(obj, name) => {
                let o = self.expr(obj);
                let (fp, fl) = self.str_const(name);
                self.rt("fuse_rt_field", &[o, fp, fl])
            }
            HirExprKind::StructConstruct { type_name, args, field_names } => {
                let (np, nl) = self.str_const(type_name);
                let obj = self.rt("fuse_rt_struct_new", &[np, nl]);
                for (i, a) in args.iter().enumerate() {
                    let v = self.expr(a);
                    if let Some(fn_) = field_names.get(i) {
                        let (fp, fl) = self.str_const(fn_);
                        self.rt_void("fuse_rt_struct_set_field", &[obj, fp, fl, v]);
                    }
                }
                // Set del_fn on structs that have __del__.
                if self.del_fns.contains_key(type_name) {
                    let (dp, dl) = self.str_const("__del__");
                    self.rt_void("fuse_rt_struct_set_del", &[obj, dp, dl]);
                }
                obj
            }
            HirExprKind::EnumConstruct { enum_name, variant, value } => {
                let (ep, el) = self.str_const(enum_name);
                let (vp, vl) = self.str_const(variant);
                let payload = match value {
                    Some(v) => self.expr(v),
                    None => self.b.ins().iconst(types::I64, 0),
                };
                self.rt("fuse_rt_enum_variant", &[ep, el, vp, vl, payload])
            }
            HirExprKind::Question(inner) => self.question_expr(inner),
            HirExprKind::OptChain(obj, field) => self.opt_chain_expr(obj, field),
            HirExprKind::Elvis(l, r) => {
                let lv = self.expr(l);
                let is_none = self.rt("fuse_rt_is_none", &[lv]);
                let rb = self.b.create_block();
                let mb = self.b.create_block();
                self.b.append_block_param(mb, FUSE_VALUE_TYPE);
                self.b.ins().brif(is_none, rb, &[], mb, &[lv]);
                self.b.switch_to_block(rb); self.b.seal_block(rb);
                let rv = self.expr(r);
                self.b.ins().jump(mb, &[rv]);
                self.b.switch_to_block(mb); self.b.seal_block(mb);
                self.b.block_params(mb)[0]
            }
            HirExprKind::Match(subj, arms) => self.match_expr(subj, arms),
            HirExprKind::When(arms) => self.when_expr(arms),
            HirExprKind::Move(inner) => {
                if let HirExprKind::Ident(name) = &inner.kind {
                    self.destroyed.insert(name.clone());
                }
                self.expr(inner)
            }
            HirExprKind::MutrefE(inner) => {
                // Wrap in ref cell for mutref passing.
                let v = self.expr(inner);
                self.rt("fuse_rt_ref_new", &[v])
            }
            HirExprKind::RefE(inner) =>
                self.expr(inner),
            HirExprKind::Block(ss) => self.stmts(ss).unwrap_or_else(|| self.rt("fuse_rt_unit", &[])),
            HirExprKind::Lambda { params, body, .. } => {
                // Create a lambda function and return its pointer.
                let lam_name = format!("fuse_lambda_{}", self.cg_lambda_id());
                let cc = self.module.isa().default_call_conv();
                let sig = fuse_fn_sig(2, cc); // (env, arg) -> result
                let func_id = self.module.declare_function(&lam_name, Linkage::Local, &sig).unwrap();
                self.fuse_fns.insert(lam_name.clone(), func_id);
                self.pending_lambdas.push((lam_name.clone(), params.clone(), body.clone()));
                // Return the function pointer as an i64 value.
                let fref = self.module.declare_func_in_func(func_id, self.b.func);
                let ptr = self.b.ins().func_addr(PTR_TYPE, fref);
                ptr
            }
            HirExprKind::PathCall { .. } => self.rt("fuse_rt_unit", &[]), // TODO
            HirExprKind::Spawn(_, _) | HirExprKind::Await(_) => self.rt("fuse_rt_unit", &[]), // TODO
        }
    }

    fn and_expr(&mut self, l: &HirExpr, r: &HirExpr) -> Value {
        let lv = self.expr(l);
        let lt = self.rt("fuse_rt_is_truthy", &[lv]);
        let rb = self.b.create_block();
        let mb = self.b.create_block();
        self.b.append_block_param(mb, FUSE_VALUE_TYPE);
        let fv = { let z = self.b.ins().iconst(types::I8, 0); self.rt("fuse_rt_bool", &[z]) };
        self.b.ins().brif(lt, rb, &[], mb, &[fv]);
        self.b.switch_to_block(rb); self.b.seal_block(rb);
        let rv = self.expr(r);
        let rt = self.rt("fuse_rt_is_truthy", &[rv]);
        let res = self.rt("fuse_rt_bool", &[rt]);
        self.b.ins().jump(mb, &[res]);
        self.b.switch_to_block(mb); self.b.seal_block(mb);
        self.b.block_params(mb)[0]
    }

    fn or_expr(&mut self, l: &HirExpr, r: &HirExpr) -> Value {
        let lv = self.expr(l);
        let lt = self.rt("fuse_rt_is_truthy", &[lv]);
        let rb = self.b.create_block();
        let mb = self.b.create_block();
        self.b.append_block_param(mb, FUSE_VALUE_TYPE);
        let tv = { let o = self.b.ins().iconst(types::I8, 1); self.rt("fuse_rt_bool", &[o]) };
        self.b.ins().brif(lt, mb, &[tv], rb, &[]);
        self.b.switch_to_block(rb); self.b.seal_block(rb);
        let rv = self.expr(r);
        let rt = self.rt("fuse_rt_is_truthy", &[rv]);
        let res = self.rt("fuse_rt_bool", &[rt]);
        self.b.ins().jump(mb, &[res]);
        self.b.switch_to_block(mb); self.b.seal_block(mb);
        self.b.block_params(mb)[0]
    }

    fn call_expr(&mut self, callee: &HirExpr, args: &[HirExpr]) -> Value {
        // Track mutref args for writeback.
        let mut mutref_writebacks: Vec<(String, Value)> = Vec::new();
        let avs: Vec<Value> = args.iter().map(|a| {
            let v = self.expr(a);
            // If this arg is a mutref wrapping a variable, track it for writeback.
            if let HirExprKind::MutrefE(inner) = &a.kind {
                if let HirExprKind::Ident(name) = &inner.kind {
                    mutref_writebacks.push((name.clone(), v));
                }
            }
            v
        }).collect();
        if let HirExprKind::Ident(name) = &callee.kind {
            match name.as_str() {
                "println" => { let v = avs.into_iter().next().unwrap_or_else(|| self.rt("fuse_rt_unit", &[])); self.rt_void("fuse_rt_println", &[v]); return self.rt("fuse_rt_unit", &[]); }
                "eprintln" => { let v = avs.into_iter().next().unwrap_or_else(|| self.rt("fuse_rt_unit", &[])); self.rt_void("fuse_rt_eprintln", &[v]); return self.rt("fuse_rt_unit", &[]); }
                "exit" => { let v = avs.into_iter().next().unwrap_or_else(|| { let z = self.b.ins().iconst(types::I64, 0); self.rt("fuse_rt_int", &[z]) }); let ci = self.rt("fuse_rt_as_int", &[v]); self.rt_void("fuse_rt_exit", &[ci]); return self.rt("fuse_rt_unit", &[]); }
                "panic" => { let v = avs.into_iter().next().unwrap_or_else(|| self.make_str("")); self.rt_void("fuse_rt_eprintln", &[v]); let o = self.b.ins().iconst(types::I64, 1); self.rt_void("fuse_rt_exit", &[o]); return self.rt("fuse_rt_unit", &[]); }
                "args" => { return self.rt("fuse_rt_args", &[]); }
                "readFile" => {
                    return self.rt("fuse_rt_read_file_val", &[avs[0]]);
                }
                "fromCharCode" => {
                    let code = self.rt("fuse_rt_as_int", &[avs[0]]);
                    return self.rt("fuse_rt_from_char_code", &[code]);
                }
                "parseInt" => {
                    let s = avs[0];
                    let slen = self.rt("fuse_rt_len", &[s]);
                    let len = self.rt("fuse_rt_as_int", &[slen]);
                    return self.rt("fuse_rt_parse_int", &[s, len]);
                }
                "parseFloat" => {
                    let s = avs[0];
                    let slen = self.rt("fuse_rt_len", &[s]);
                    let len = self.rt("fuse_rt_as_int", &[slen]);
                    return self.rt("fuse_rt_parse_float", &[s, len]);
                }
                _ => {}
            }
            // Check if this is an extern function — needs arg unboxing.
            if let Some((param_types, ret_type)) = self.extern_sigs.get(name).cloned() {
                let mut raw_args = Vec::new();
                for (i, av) in avs.iter().enumerate() {
                    let pt = param_types.get(i).unwrap_or(&HirType::Ptr);
                    let raw = match pt {
                        HirType::Int => self.rt("fuse_rt_as_int", &[*av]),
                        HirType::Float => self.rt("fuse_rt_as_float", &[*av]),
                        HirType::Bool => self.rt("fuse_rt_as_bool", &[*av]),
                        _ => *av, // Ptr types pass through as-is
                    };
                    raw_args.push(raw);
                }
                let m = format!("fuse_{name}");
                let raw_result = self.call_fuse(&m, &raw_args);
                // Box the return value.
                return match ret_type {
                    HirType::Int => self.rt("fuse_rt_int", &[raw_result]),
                    HirType::Float => self.rt("fuse_rt_float", &[raw_result]),
                    HirType::Bool => self.rt("fuse_rt_bool", &[raw_result]),
                    HirType::Unit => self.rt("fuse_rt_unit", &[]),
                    _ => raw_result, // Ptr returns are already boxed
                };
            }
            let m = format!("fuse_{name}");
            if self.fuse_fns.contains_key(&m) {
                let result = self.call_fuse(&m, &avs);
                // Mutref writeback: read ref cells back to caller variables.
                for (var_name, cell) in &mutref_writebacks {
                    let new_val = self.rt("fuse_rt_ref_get", &[*cell]);
                    if let Some(&var) = self.vars.get(var_name) {
                        self.b.def_var(var, new_val);
                    }
                }
                return result;
            }
        }
        self.rt("fuse_rt_unit", &[])
    }

    fn method_call(&mut self, recv: &HirExpr, method: &str, args: &[HirExpr], rtype: &str) -> Value {
        let obj = self.expr(recv);
        let avs: Vec<Value> = args.iter().map(|a| self.expr(a)).collect();
        match method {
            "len" => return self.rt("fuse_rt_len", &[obj]),
            "get" if !avs.is_empty() => return self.rt("fuse_rt_list_get", &[obj, avs[0]]),
            "push" if !avs.is_empty() => { self.rt_void("fuse_rt_list_push", &[obj, avs[0]]); return self.rt("fuse_rt_unit", &[]); }
            "set" if avs.len()>=2 => { self.rt_void("fuse_rt_list_set", &[obj, avs[0], avs[1]]); return self.rt("fuse_rt_unit", &[]); }
            "contains" if !avs.is_empty() => return self.rt("fuse_rt_contains", &[obj, avs[0]]),
            "first" => return self.rt("fuse_rt_list_first", &[obj]),
            "last" => return self.rt("fuse_rt_list_last", &[obj]),
            "sum" => return self.rt("fuse_rt_list_sum", &[obj]),
            "sorted" => return self.rt("fuse_rt_list_sorted", &[obj]),
            "isEmpty" => return self.rt("fuse_rt_list_is_empty", &[obj]),
            "charAt" if !avs.is_empty() => return self.rt("fuse_rt_str_char_at", &[obj, avs[0]]),
            "substring" if avs.len()>=2 => return self.rt("fuse_rt_str_substring", &[obj, avs[0], avs[1]]),
            "startsWith" if !avs.is_empty() => return self.rt("fuse_rt_str_starts_with", &[obj, avs[0]]),
            "charCodeAt" if !avs.is_empty() => return self.rt("fuse_rt_str_char_code_at", &[obj, avs[0]]),
            "split" if !avs.is_empty() => return self.rt("fuse_rt_str_split", &[obj, avs[0]]),
            "trim" => return self.rt("fuse_rt_str_trim", &[obj]),
            "replace" if avs.len()>=2 => return self.rt("fuse_rt_str_replace", &[obj, avs[0], avs[1]]),
            "toUpper" => return self.rt("fuse_rt_str_to_upper", &[obj]),
            "toLower" => return self.rt("fuse_rt_str_to_lower", &[obj]),
            "toFloat" => return self.rt("fuse_rt_int_to_float", &[obj]),
            "toString" => return self.rt("fuse_rt_to_display_string", &[obj]),
            "isEven" => return self.rt("fuse_rt_int_is_even", &[obj]),
            "map" if !avs.is_empty() => {
                // avs[0] is a lambda function pointer.
                let null_env = self.b.ins().iconst(types::I64, 0);
                return self.rt("fuse_rt_list_map_fn", &[obj, avs[0], null_env]);
            }
            "filter" if !avs.is_empty() => {
                let null_env = self.b.ins().iconst(types::I64, 0);
                return self.rt("fuse_rt_list_filter_fn", &[obj, avs[0], null_env]);
            }
            "retainWhere" if !avs.is_empty() => {
                let null_env = self.b.ins().iconst(types::I64, 0);
                self.rt_void("fuse_rt_list_retain_fn", &[obj, avs[0], null_env]);
                return self.rt("fuse_rt_unit", &[]);
            }
            _ => {}
        }
        // Extension function lookup.
        let mut all = vec![obj]; all.extend(avs);
        if !rtype.is_empty() {
            let m = format!("fuse_ext_{}_{}", rtype, method);
            if self.fuse_fns.contains_key(&m) { return self.call_fuse(&m, &all); }
        }
        for prefix in &["String","Int","Float","Bool","List"] {
            let m = format!("fuse_ext_{}_{}", prefix, method);
            if self.fuse_fns.contains_key(&m) { return self.call_fuse(&m, &all); }
        }
        // Search all fuse_ext_*_{method} names with runtime type dispatch.
        let suffix = format!("_{}", method);
        let candidates: Vec<String> = self.fuse_fns.keys()
            .filter(|k| k.starts_with("fuse_ext_") && k.ends_with(&suffix))
            .cloned().collect();
        if candidates.len() == 1 {
            return self.call_fuse(&candidates[0], &all);
        } else if candidates.len() > 1 {
            // Multiple candidates — emit runtime type dispatch.
            let type_name = self.rt("fuse_rt_type_name", &[obj]);
            let merge = self.b.create_block();
            self.b.append_block_param(merge, FUSE_VALUE_TYPE);

            for (i, cand) in candidates.iter().enumerate() {
                // Extract type name from "fuse_ext_TypeName_method"
                let prefix = "fuse_ext_";
                let type_part = &cand[prefix.len()..cand.len() - suffix.len()];
                let expected = self.make_str(type_part);
                let eq = self.rt("fuse_rt_eq", &[type_name, expected]);
                let eq_bool = self.rt("fuse_rt_is_truthy", &[eq]);

                let match_block = self.b.create_block();
                let next_block = self.b.create_block();

                self.b.ins().brif(eq_bool, match_block, &[], next_block, &[]);

                self.b.switch_to_block(match_block);
                self.b.seal_block(match_block);
                let result = self.call_fuse(cand, &all);
                self.b.ins().jump(merge, &[result]);

                self.b.switch_to_block(next_block);
                self.b.seal_block(next_block);
            }
            // Fallback: return unit.
            let unit = self.rt("fuse_rt_unit", &[]);
            self.b.ins().jump(merge, &[unit]);
            self.b.switch_to_block(merge);
            self.b.seal_block(merge);
            return self.b.block_params(merge)[0];
        }
        self.rt("fuse_rt_unit", &[])
    }

    fn question_expr(&mut self, inner: &HirExpr) -> Value {
        let v = self.expr(inner);
        let ok = self.rt("fuse_rt_is_ok", &[v]);
        let sm = self.rt("fuse_rt_is_some", &[v]);
        let success = self.b.ins().bor(ok, sm);
        let ub = self.b.create_block();
        let rb = self.b.create_block();
        self.b.ins().brif(success, ub, &[], rb, &[]);
        self.b.switch_to_block(rb); self.b.seal_block(rb);
        self.b.ins().return_(&[v]);
        self.terminated = true;
        let nb = self.b.create_block();
        self.b.switch_to_block(nb); self.b.seal_block(nb);
        self.terminated = false;
        self.b.switch_to_block(ub); self.b.seal_block(ub);
        self.rt("fuse_rt_unwrap_enum", &[v])
    }

    fn opt_chain_expr(&mut self, obj: &HirExpr, field: &str) -> Value {
        let v = self.expr(obj);
        let is_none = self.rt("fuse_rt_is_none", &[v]);
        let ab = self.b.create_block();
        let mb = self.b.create_block();
        self.b.append_block_param(mb, FUSE_VALUE_TYPE);
        let nv = self.rt("fuse_rt_none", &[]);
        self.b.ins().brif(is_none, mb, &[nv], ab, &[]);
        self.b.switch_to_block(ab); self.b.seal_block(ab);
        // Unwrap Some if needed.
        let is_some = self.rt("fuse_rt_is_some", &[v]);
        let ub = self.b.create_block();
        let db = self.b.create_block();
        let fmb = self.b.create_block();
        self.b.append_block_param(fmb, FUSE_VALUE_TYPE);
        self.b.ins().brif(is_some, ub, &[], db, &[]);
        self.b.switch_to_block(ub); self.b.seal_block(ub);
        let uw = self.rt("fuse_rt_unwrap_enum", &[v]);
        self.b.ins().jump(fmb, &[uw]);
        self.b.switch_to_block(db); self.b.seal_block(db);
        self.b.ins().jump(fmb, &[v]);
        self.b.switch_to_block(fmb); self.b.seal_block(fmb);
        let inner = self.b.block_params(fmb)[0];
        let (fp, fl) = self.str_const(field);
        let res = self.rt("fuse_rt_safe_field", &[inner, fp, fl]);
        self.b.ins().jump(mb, &[res]);
        self.b.switch_to_block(mb); self.b.seal_block(mb);
        self.b.block_params(mb)[0]
    }

    fn match_expr(&mut self, subj: &HirExpr, arms: &[HirMatchArm]) -> Value {
        let sv = self.expr(subj);
        let mb = self.b.create_block();
        self.b.append_block_param(mb, FUSE_VALUE_TYPE);
        let mut next = self.b.create_block();
        for (i, arm) in arms.iter().enumerate() {
            let last = i == arms.len() - 1;
            let bb = self.b.create_block();
            if !last { next = self.b.create_block(); }
            match &arm.pattern {
                HirPattern::Wildcard(_) => {
                    self.b.ins().jump(bb, &[]);
                }
                HirPattern::Ident(n, _, _) if n.contains('.') => {
                    // Dotted name like "FetchResult.NotFound" — treat as enum variant check.
                    let variant_str = self.rt("fuse_rt_variant_name", &[sv]);
                    let exp = self.make_str(n);
                    let eq = self.rt("fuse_rt_eq", &[variant_str, exp]);
                    let eqb = self.rt("fuse_rt_is_truthy", &[eq]);
                    if last { self.b.ins().jump(bb, &[]); }
                    else { self.b.ins().brif(eqb, bb, &[], next, &[]); }
                }
                HirPattern::Ident(n, _, _) if n == "None" => {
                    // Special case: bare "None" is Option.None.
                    let variant_str = self.rt("fuse_rt_variant_name", &[sv]);
                    let exp = self.make_str("Option.None");
                    let eq = self.rt("fuse_rt_eq", &[variant_str, exp]);
                    let eqb = self.rt("fuse_rt_is_truthy", &[eq]);
                    if last { self.b.ins().jump(bb, &[]); }
                    else { self.b.ins().brif(eqb, bb, &[], next, &[]); }
                }
                HirPattern::Ident(n, _, _) => {
                    // Regular identifier — binds subject to variable, always matches.
                    self.def(n, sv);
                    self.b.ins().jump(bb, &[]);
                }
                HirPattern::Constructor(name, sub_pats, _, _) => {
                    // Get the full variant name (e.g., "Result.Ok", "Option.Some", "MathError.DivisionByZero").
                    let variant_str = self.rt("fuse_rt_variant_name", &[sv]);
                    // The pattern name may be short ("Ok") or qualified ("MathError.DivisionByZero").
                    // Try matching against full name and common prefixes.
                    let candidates = match name.as_str() {
                        "Ok" => vec!["Result.Ok".to_string()],
                        "Err" => vec!["Result.Err".to_string()],
                        "Some" => vec!["Option.Some".to_string()],
                        "None" => vec!["Option.None".to_string()],
                        _ => vec![name.clone()],
                    };
                    let exp = self.make_str(&candidates[0]);
                    let eq = self.rt("fuse_rt_eq", &[variant_str, exp]);
                    let eqb = self.rt("fuse_rt_is_truthy", &[eq]);
                    if last { self.b.ins().jump(bb, &[]); }
                    else { self.b.ins().brif(eqb, bb, &[], next, &[]); }
                }
                HirPattern::Literal(lit, _) => {
                    let lv = match lit {
                        HirLit::Int(v) => { let r = self.b.ins().iconst(types::I64, *v); self.rt("fuse_rt_int", &[r]) }
                        HirLit::Float(v) => { let r = self.b.ins().f64const(*v); self.rt("fuse_rt_float", &[r]) }
                        HirLit::Str(s) => self.make_str(s),
                        HirLit::Bool(b) => { let r = self.b.ins().iconst(types::I8, if *b {1} else {0}); self.rt("fuse_rt_bool", &[r]) }
                    };
                    let eq = self.rt("fuse_rt_eq", &[sv, lv]);
                    let eqb = self.rt("fuse_rt_is_truthy", &[eq]);
                    if last { self.b.ins().jump(bb, &[]); }
                    else { self.b.ins().brif(eqb, bb, &[], next, &[]); }
                }
                _ => { self.b.ins().jump(bb, &[]); }
            }
            self.b.switch_to_block(bb); self.b.seal_block(bb);
            // Bind sub-patterns for constructors (recursive for nested patterns).
            if let HirPattern::Constructor(_, sub_pats, _, _) = &arm.pattern {
                if !sub_pats.is_empty() {
                    let inner = self.rt("fuse_rt_unwrap_enum", &[sv]);
                    self.bind_sub_patterns(&sub_pats, inner);
                }
            }
            let bv = self.expr(&arm.body);
            if !self.terminated { self.b.ins().jump(mb, &[bv]); }
            self.terminated = false;
            if !last {
                self.b.switch_to_block(next); self.b.seal_block(next);
            }
        }
        self.b.switch_to_block(mb); self.b.seal_block(mb);
        self.b.block_params(mb)[0]
    }

    /// Recursively bind sub-patterns from a constructor match.
    fn bind_sub_patterns(&mut self, pats: &[HirPattern], val: Value) {
        for pat in pats {
            match pat {
                HirPattern::Ident(n, _, _) => { self.def(n, val); }
                HirPattern::Constructor(name, inner_pats, _, _) => {
                    // Nested constructor: unwrap and recurse.
                    // The val should be an enum variant matching `name`.
                    if !inner_pats.is_empty() {
                        let inner = self.rt("fuse_rt_unwrap_enum", &[val]);
                        self.bind_sub_patterns(inner_pats, inner);
                    }
                }
                HirPattern::Wildcard(_) => {} // nothing to bind
                HirPattern::Tuple(inner_pats, _) => {
                    // Destructure tuple elements.
                    for (i, p) in inner_pats.iter().enumerate() {
                        let idx = self.b.ins().iconst(types::I64, i as i64);
                        let idx_v = self.rt("fuse_rt_int", &[idx]);
                        let elem = self.rt("fuse_rt_list_get", &[val, idx_v]);
                        self.bind_sub_patterns(&[p.clone()], elem);
                    }
                }
                _ => {}
            }
        }
    }

    fn when_expr(&mut self, arms: &[HirWhenArm]) -> Value {
        let mb = self.b.create_block();
        self.b.append_block_param(mb, FUSE_VALUE_TYPE);
        for arm in arms {
            match &arm.cond {
                None => {
                    let v = self.expr(&arm.body);
                    if !self.terminated { self.b.ins().jump(mb, &[v]); self.terminated = true; }
                }
                Some(c) => {
                    let cv = self.expr(c);
                    let tr = self.rt("fuse_rt_is_truthy", &[cv]);
                    let bb = self.b.create_block();
                    let nb = self.b.create_block();
                    self.b.ins().brif(tr, bb, &[], nb, &[]);
                    self.b.switch_to_block(bb); self.b.seal_block(bb);
                    let v = self.expr(&arm.body);
                    if !self.terminated { self.b.ins().jump(mb, &[v]); }
                    self.terminated = false;
                    self.b.switch_to_block(nb); self.b.seal_block(nb);
                }
            }
        }
        if !self.terminated { let u = self.rt("fuse_rt_unit", &[]); self.b.ins().jump(mb, &[u]); }
        self.terminated = false;
        self.b.switch_to_block(mb); self.b.seal_block(mb);
        self.b.block_params(mb)[0]
    }
}

// ═════════════════════════════════════════════════════════════════════
// Helpers
// ═════════════════════════════════════════════════════════════════════

fn mangle(f: &HirFnDecl) -> String {
    match &f.ext_type {
        Some(ext) => format!("fuse_ext_{}_{}", ext, f.name),
        None => format!("fuse_{}", f.name),
    }
}

fn intern_str(module: &mut ObjectModule, cache: &mut HashMap<String, DataId>, counter: &mut usize, s: &str) -> DataId {
    if let Some(&id) = cache.get(s) { return id; }
    let name = format!(".str.{}", *counter);
    *counter += 1;
    let id = module.declare_data(&name, Linkage::Local, false, false).unwrap();
    let mut desc = DataDescription::new();
    desc.define(s.as_bytes().to_vec().into_boxed_slice());
    module.define_data(id, &desc).unwrap();
    cache.insert(s.to_string(), id);
    id
}

fn link(obj_path: &str, output_path: &str) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let base = std::path::Path::new(manifest_dir).parent().unwrap();

    // Find runtime library — check both MSVC (.lib) and GNU (.a) naming.
    let candidates = [
        base.join("target/release/fuse_runtime.lib"),
        base.join("target/release/libfuse_runtime.a"),
        base.join("target/debug/fuse_runtime.lib"),
        base.join("target/debug/libfuse_runtime.a"),
    ];
    let rt = candidates.iter().find(|p| p.exists())
        .expect("fuse-runtime static library not found — run `cargo build -p fuse-runtime` first");

    // Also find cranelift-ffi library if it exists (needed for Stage 2 compiler).
    let cl_ffi_candidates = [
        base.join("target/release/cranelift_ffi.lib"),
        base.join("target/release/libcranelift_ffi.a"),
        base.join("target/debug/cranelift_ffi.lib"),
        base.join("target/debug/libcranelift_ffi.a"),
    ];
    let cl_ffi = cl_ffi_candidates.iter().find(|p| p.exists());

    let out = if cfg!(windows) && !output_path.ends_with(".exe") {
        format!("{output_path}.exe")
    } else {
        output_path.to_string()
    };

    if cfg!(target_env = "msvc") {
        link_msvc(obj_path, rt.to_str().unwrap(), cl_ffi.map(|p| p.to_str().unwrap()), &out);
    } else {
        link_gcc(obj_path, rt.to_str().unwrap(), cl_ffi.map(|p| p.to_str().unwrap()), &out);
    }
}

fn link_msvc(obj_path: &str, rt_path: &str, cl_ffi: Option<&str>, output_path: &str) {
    // Use `rustc -C linker-flavor=msvc` as a linker driver.
    // We create a trivial .rs file whose only purpose is to give rustc
    // something to compile, then inject our object file and runtime lib
    // as extra link arguments. rustc handles finding link.exe, the
    // Windows SDK, and the CRT — we don't have to locate any of it.
    let stub_dir = std::path::Path::new(obj_path).parent()
        .unwrap_or(std::path::Path::new("."));
    let stub_path = stub_dir.join("_fuse_stub.rs");
    // The stub uses #![no_main] so rustc doesn't generate its own main().
    // Our Cranelift-generated main() becomes the real entry point.
    std::fs::write(&stub_path, "#![no_main]").expect("write stub");

    let obj_abs = std::fs::canonicalize(obj_path).unwrap_or_else(|_| obj_path.into());
    let rt_abs = std::fs::canonicalize(rt_path).unwrap_or_else(|_| rt_path.into());

    let mut cmd = std::process::Command::new("rustc");
    cmd.arg("--edition=2021")
        .arg("--crate-type=bin")
        .arg(stub_path.to_str().unwrap())
        .arg("-o").arg(output_path)
        .arg("-C").arg(format!("link-arg={}", obj_abs.display()))
        .arg("-C").arg(format!("link-arg={}", rt_abs.display()));
    if let Some(cl) = cl_ffi {
        let cl_abs = std::fs::canonicalize(cl).unwrap_or_else(|_| cl.into());
        cmd.arg("-C").arg(format!("link-arg={}", cl_abs.display()));
    }
    // 8 MB stack — compilers have deep call stacks.
    cmd.arg("-C").arg("link-arg=/STACK:8388608");
    let result = cmd.status();

    let _ = std::fs::remove_file(&stub_path);

    match result {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("linker (via rustc) failed: exit {}", s.code().unwrap_or(-1));
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("linker error: {e}");
            std::process::exit(1);
        }
    }
}

fn link_gcc(obj_path: &str, rt_path: &str, cl_ffi: Option<&str>, output_path: &str) {
    let linker = if cfg!(windows) { "gcc" } else { "cc" };
    let mut cmd = std::process::Command::new(linker);
    cmd.arg(obj_path).arg(rt_path);
    if let Some(cl) = cl_ffi { cmd.arg(cl); }
    cmd.arg("-o").arg(output_path);

    if cfg!(target_os = "linux") {
        cmd.arg("-lpthread").arg("-ldl").arg("-lm");
        cmd.arg("-Wl,-z,stacksize=8388608");
    } else if cfg!(target_os = "macos") {
        cmd.arg("-lpthread").arg("-lm");
        cmd.arg("-Wl,-stack_size,0x800000");
    } else if cfg!(windows) {
        cmd.arg("-lws2_32").arg("-luserenv").arg("-ladvapi32").arg("-lbcrypt").arg("-lntdll");
    }

    let status = cmd.status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => { eprintln!("linker failed: exit {}", s.code().unwrap_or(-1)); std::process::exit(1); }
        Err(e) => { eprintln!("linker error: {e}"); std::process::exit(1); }
    }
}
