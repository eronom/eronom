use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use cranelift_codegen::ir::{
    types, AbiParam, Block, InstBuilder, Signature, UserExternalName, UserFuncName,
};
use cranelift_codegen::isa;
use cranelift_codegen::settings::{self, Configurable, Flags};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{default_libcall_names, DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use target_lexicon::Triple;

use super::ir::{EirBinaryOp, EirFunction, EirInst, EirModule, EirType, EirUnaryOp};
use crate::frontend::parse_and_resolve_imports;
use crate::vm::Compiler;

pub fn map_eir_type_to_clif(ty: EirType) -> types::Type {
    match ty {
        EirType::F64 => types::F64,
        _ => types::I64,
    }
}

pub struct CraneliftBackend {
    pub module: ObjectModule,
    pub target_triple: Triple,
    declared_funcs: HashMap<String, (FuncId, Signature)>,
    runtime_funcs: HashMap<String, FuncId>,
    printf_fid: Option<FuncId>,
    printf_f64_fid: Option<FuncId>,
    fmt_i64_id: Option<DataId>,
    fmt_f64_id: Option<DataId>,
    fmt_str_id: Option<DataId>,
    fflush_fid: Option<FuncId>,
    global_data_ids: HashMap<String, DataId>,
    string_literals: HashMap<String, DataId>,
}

impl CraneliftBackend {
    pub fn new(module_name: &str, target_triple_str: Option<&str>) -> anyhow::Result<Self> {
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed").unwrap();
        flag_builder.set("is_pic", "true").unwrap();
        let flags = Flags::new(flag_builder);

        let target_triple = if let Some(t_str) = target_triple_str {
            Triple::from_str(t_str)
                .map_err(|e| anyhow::anyhow!("Invalid target triple '{}': {}", t_str, e))?
        } else {
            Triple::host()
        };

        let isa = isa::lookup(target_triple.clone())
            .map_err(|e| anyhow::anyhow!("Unsupported target architecture '{}': {}", target_triple, e))?
            .finish(flags)?;

        let obj_builder = ObjectBuilder::new(isa, module_name.to_string(), default_libcall_names())
            .map_err(|e| anyhow::anyhow!("Failed to create Cranelift ObjectBuilder: {}", e))?;
        let clif_module = ObjectModule::new(obj_builder);

        Ok(Self {
            module: clif_module,
            target_triple,
            declared_funcs: HashMap::new(),
            runtime_funcs: HashMap::new(),
            printf_fid: None,
            printf_f64_fid: None,
            fmt_i64_id: None,
            fmt_f64_id: None,
            fmt_str_id: None,
            fflush_fid: None,
            global_data_ids: HashMap::new(),
            string_literals: HashMap::new(),
        })
    }

    fn declare_runtime_fn(&mut self, name: &str, params: &[types::Type], ret: Option<types::Type>) -> anyhow::Result<FuncId> {
        if let Some(&fid) = self.runtime_funcs.get(name) {
            return Ok(fid);
        }
        let mut sig = self.module.make_signature();
        for &p in params {
            sig.params.push(AbiParam::new(p));
        }
        if let Some(r) = ret {
            sig.returns.push(AbiParam::new(r));
        }
        let fid = self.module.declare_function(name, Linkage::Import, &sig)?;
        self.runtime_funcs.insert(name.to_string(), fid);
        Ok(fid)
    }

    pub fn compile_module(mut self, eir: &EirModule) -> anyhow::Result<Vec<u8>> {
        // 1. Declare libc printf and format strings
        let mut printf_sig = self.module.make_signature();
        printf_sig.params.push(AbiParam::new(types::I64));
        printf_sig.params.push(AbiParam::new(types::I64));
        printf_sig.returns.push(AbiParam::new(types::I32));
        let printf_fid = self.module.declare_function("printf", Linkage::Import, &printf_sig)?;
        self.printf_fid = Some(printf_fid);

        let mut dprintf_sig = self.module.make_signature();
        dprintf_sig.params.push(AbiParam::new(types::I32));
        dprintf_sig.params.push(AbiParam::new(types::I64));
        dprintf_sig.params.push(AbiParam::new(types::F64));
        dprintf_sig.returns.push(AbiParam::new(types::I32));
        let printf_f64_fid = self.module.declare_function("dprintf", Linkage::Import, &dprintf_sig)?;
        self.printf_f64_fid = Some(printf_f64_fid);

        let mut d_i64 = DataDescription::new();
        d_i64.define(b"%ld\n\0".to_vec().into_boxed_slice());
        let fmt_i64_id = self.module.declare_data("__fmt_i64", Linkage::Local, false, false)?;
        self.module.define_data(fmt_i64_id, &d_i64)?;
        self.fmt_i64_id = Some(fmt_i64_id);

        let mut d_f64 = DataDescription::new();
        d_f64.define(b"%f\n\0".to_vec().into_boxed_slice());
        let fmt_f64_id = self.module.declare_data("__fmt_f64", Linkage::Local, false, false)?;
        self.module.define_data(fmt_f64_id, &d_f64)?;
        self.fmt_f64_id = Some(fmt_f64_id);

        let mut d_str = DataDescription::new();
        d_str.define(b"%s\n\0".to_vec().into_boxed_slice());
        let fmt_str_id = self.module.declare_data("__fmt_str", Linkage::Local, false, false)?;
        self.module.define_data(fmt_str_id, &d_str)?;
        self.fmt_str_id = Some(fmt_str_id);

        let mut fflush_sig = self.module.make_signature();
        fflush_sig.params.push(AbiParam::new(types::I64));
        fflush_sig.returns.push(AbiParam::new(types::I32));
        let fflush_fid = self.module.declare_function("fflush", Linkage::Import, &fflush_sig)?;
        self.fflush_fid = Some(fflush_fid);

        // 2. Declare standard runtime helper imports
        self.declare_runtime_fn("er_runtime_init", &[], None)?;
        self.declare_runtime_fn("er_runtime_cleanup", &[], None)?;
        self.declare_runtime_fn("er_println_i64", &[types::I64], None)?;
        self.declare_runtime_fn("er_println_f64", &[types::F64], None)?;
        self.declare_runtime_fn("er_println_bool", &[types::I64], None)?;
        self.declare_runtime_fn("er_print_string", &[types::I64, types::I64], None)?;
        self.declare_runtime_fn("er_println_string", &[types::I64, types::I64], None)?;
        self.declare_runtime_fn("er_alloc_string", &[types::I64, types::I64], Some(types::I64))?;
        self.declare_runtime_fn("er_alloc_array", &[types::I64], Some(types::I64))?;
        self.declare_runtime_fn("er_alloc_object", &[], Some(types::I64))?;
        self.declare_runtime_fn("er_alloc_struct", &[types::I64, types::I64], Some(types::I64))?;
        self.declare_runtime_fn("er_aot_array_push", &[types::I64, types::I64], Some(types::I64))?;
        self.declare_runtime_fn("er_aot_array_get", &[types::I64, types::I64], Some(types::I64))?;
        self.declare_runtime_fn("er_aot_array_set", &[types::I64, types::I64, types::I64], None)?;

        // Pre-scan and declare global variables and string literals
        for func in &eir.functions {
            for inst in &func.instructions {
                match inst {
                    EirInst::DefineGlobal { name, .. } | EirInst::GetGlobal { name, .. } => {
                        if name != "print" && name != "println" && !self.global_data_ids.contains_key(name) {
                            let data_id = self.module.declare_data(&format!("__er_g_{}", name), Linkage::Local, true, false)?;
                            let mut desc = DataDescription::new();
                            desc.define_zeroinit(8);
                            self.module.define_data(data_id, &desc)?;
                            self.global_data_ids.insert(name.clone(), data_id);
                        }
                    }
                    EirInst::ConstString { val, .. } => {
                        if !self.string_literals.contains_key(val) {
                            let mut desc = DataDescription::new();
                            let mut bytes = val.as_bytes().to_vec();
                            bytes.push(0);
                            desc.define(bytes.into_boxed_slice());
                            let str_id = self.module.declare_data(&format!("__er_str_{}", self.string_literals.len()), Linkage::Local, false, false)?;
                            self.module.define_data(str_id, &desc)?;
                            self.string_literals.insert(val.clone(), str_id);
                        }
                    }
                    _ => {}
                }
            }
        }

        // 3. Declare all E-IR functions
        for func in &eir.functions {
            let mut sig = self.module.make_signature();
            for pty in &func.param_types {
                sig.params.push(AbiParam::new(map_eir_type_to_clif(*pty)));
            }
            if func.name == "main" {
                sig.returns.push(AbiParam::new(types::I64));
            } else {
                match func.return_type {
                    EirType::Void => {}
                    rty => sig.returns.push(AbiParam::new(map_eir_type_to_clif(rty))),
                }
            }
            let fid = self.module.declare_function(&func.name, Linkage::Export, &sig)?;
            self.declared_funcs.insert(func.name.clone(), (fid, sig));
        }

        // 4. Compile each function body
        let mut ctx = self.module.make_context();
        let mut fn_builder_ctx = FunctionBuilderContext::new();

        for func in &eir.functions {
            let (fid, ref sig) = self.declared_funcs[&func.name];
            ctx.func.signature = sig.clone();
            ctx.func.name = UserFuncName::User(UserExternalName::new(fid.as_u32(), 0));

            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fn_builder_ctx);
            self.compile_function_body(&mut builder, func, eir)?;
            builder.finalize(self.module.isa().frontend_config());

            self.module.define_function(fid, &mut ctx)?;
            self.module.clear_context(&mut ctx);
        }

        // 5. Emit finalized native object code
        let product = self.module.finish();
        let bytes = product.emit()
            .map_err(|e| anyhow::anyhow!("Failed to serialize Cranelift object: {}", e))?;
        Ok(bytes)
    }

    fn compile_function_body(
        &mut self,
        builder: &mut FunctionBuilder,
        func: &EirFunction,
        _eir: &EirModule,
    ) -> anyhow::Result<()> {
        let mut int_vars = Vec::with_capacity(func.reg_count);
        let mut float_vars = Vec::with_capacity(func.reg_count);

        for _ in 0..func.reg_count {
            int_vars.push(builder.declare_var(types::I64));
            float_vars.push(builder.declare_var(types::F64));
        }

        // Identify branch targets to create Cranelift basic blocks
        let mut block_map: HashMap<usize, Block> = HashMap::new();
        for inst in &func.instructions {
            match inst {
                EirInst::Jump { target } => {
                    block_map.entry(*target).or_insert_with(|| builder.create_block());
                }
                EirInst::BranchIfFalse { target, .. } => {
                    block_map.entry(*target).or_insert_with(|| builder.create_block());
                }
                _ => {}
            }
        }

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);

        // Bind incoming arguments to initial register variables
        let params = builder.block_params(entry_block).to_vec();
        for (i, &param_val) in params.iter().enumerate() {
            if i < func.arity {
                if i < func.param_types.len() && func.param_types[i] == EirType::F64 {
                    builder.def_var(float_vars[i], param_val);
                } else {
                    builder.def_var(int_vars[i], param_val);
                }
            }
        }

        let mut terminated = false;

        for (idx, inst) in func.instructions.iter().enumerate() {
            if let Some(&target_block) = block_map.get(&idx) {
                if !terminated {
                    builder.ins().jump(target_block, &[]);
                }
                builder.switch_to_block(target_block);
                terminated = false;
            }

            if terminated {
                continue;
            }

            match inst {
                EirInst::ConstI64 { dest, val } => {
                    let v = builder.ins().iconst(types::I64, *val);
                    builder.def_var(int_vars[*dest], v);
                }
                EirInst::ConstF64 { dest, val } => {
                    let v = builder.ins().f64const(*val);
                    builder.def_var(float_vars[*dest], v);
                }
                EirInst::ConstBool { dest, val } => {
                    let v = builder.ins().iconst(types::I64, if *val { 1 } else { 0 });
                    builder.def_var(int_vars[*dest], v);
                }
                EirInst::ConstNull { dest } => {
                    let v = builder.ins().iconst(types::I64, 0);
                    builder.def_var(int_vars[*dest], v);
                }
                EirInst::Move { dest, src, ty } => {
                    if *ty == EirType::F64 {
                        let v = builder.use_var(float_vars[*src]);
                        builder.def_var(float_vars[*dest], v);
                    } else {
                        let v = builder.use_var(int_vars[*src]);
                        builder.def_var(int_vars[*dest], v);
                    }
                }
                EirInst::Binary { dest, op, ty, left, right } => {
                    if *ty == EirType::F64 {
                        let l = builder.use_var(float_vars[*left]);
                        let r = builder.use_var(float_vars[*right]);
                        let res = match op {
                            EirBinaryOp::Add => builder.ins().fadd(l, r),
                            EirBinaryOp::Sub => builder.ins().fsub(l, r),
                            EirBinaryOp::Mul => builder.ins().fmul(l, r),
                            EirBinaryOp::Div => builder.ins().fdiv(l, r),
                            EirBinaryOp::Lt => {
                                let c = builder.ins().fcmp(cranelift_codegen::ir::condcodes::FloatCC::LessThan, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Le => {
                                let c = builder.ins().fcmp(cranelift_codegen::ir::condcodes::FloatCC::LessThanOrEqual, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Gt => {
                                let c = builder.ins().fcmp(cranelift_codegen::ir::condcodes::FloatCC::GreaterThan, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Ge => {
                                let c = builder.ins().fcmp(cranelift_codegen::ir::condcodes::FloatCC::GreaterThanOrEqual, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Eq => {
                                let c = builder.ins().fcmp(cranelift_codegen::ir::condcodes::FloatCC::Equal, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Ne => {
                                let c = builder.ins().fcmp(cranelift_codegen::ir::condcodes::FloatCC::NotEqual, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            _ => builder.ins().fadd(l, r),
                        };
                        if matches!(op, EirBinaryOp::Lt | EirBinaryOp::Le | EirBinaryOp::Gt | EirBinaryOp::Ge | EirBinaryOp::Eq | EirBinaryOp::Ne) {
                            builder.def_var(int_vars[*dest], res);
                        } else {
                            builder.def_var(float_vars[*dest], res);
                        }
                    } else {
                        let l = builder.use_var(int_vars[*left]);
                        let r = builder.use_var(int_vars[*right]);
                        let res = match op {
                            EirBinaryOp::Add => builder.ins().iadd(l, r),
                            EirBinaryOp::Sub => builder.ins().isub(l, r),
                            EirBinaryOp::Mul => builder.ins().imul(l, r),
                            EirBinaryOp::Div => builder.ins().sdiv(l, r),
                            EirBinaryOp::Mod => builder.ins().srem(l, r),
                            EirBinaryOp::BitAnd => builder.ins().band(l, r),
                            EirBinaryOp::BitOr => builder.ins().bor(l, r),
                            EirBinaryOp::BitXor => builder.ins().bxor(l, r),
                            EirBinaryOp::Shl => builder.ins().ishl(l, r),
                            EirBinaryOp::Shr => builder.ins().sshr(l, r),
                            EirBinaryOp::Lt => {
                                let c = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::SignedLessThan, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Le => {
                                let c = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::SignedLessThanOrEqual, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Gt => {
                                let c = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThan, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Ge => {
                                let c = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Eq => {
                                let c = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                            EirBinaryOp::Ne => {
                                let c = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::NotEqual, l, r);
                                builder.ins().uextend(types::I64, c)
                            }
                        };
                        builder.def_var(int_vars[*dest], res);
                    }
                }
                EirInst::Unary { dest, op, ty, src } => {
                    if *ty == EirType::F64 {
                        let s = builder.use_var(float_vars[*src]);
                        let res = match op {
                            EirUnaryOp::Neg => builder.ins().fneg(s),
                            _ => s,
                        };
                        builder.def_var(float_vars[*dest], res);
                    } else {
                        let s = builder.use_var(int_vars[*src]);
                        let res = match op {
                            EirUnaryOp::Neg => builder.ins().ineg(s),
                            EirUnaryOp::BitNot => builder.ins().bnot(s),
                            EirUnaryOp::Not => {
                                let c = builder.ins().icmp_imm_s(cranelift_codegen::ir::condcodes::IntCC::Equal, s, 0);
                                builder.ins().uextend(types::I64, c)
                            }
                        };
                        builder.def_var(int_vars[*dest], res);
                    }
                }
                EirInst::Call { dest, func_name, args, ret_ty } => {
                    if let Some(&(target_fid, _)) = self.declared_funcs.get(func_name) {
                        let target_ref = self.module.declare_func_in_func(target_fid, builder.func);
                        let mut arg_vals = Vec::new();
                        for &arg_reg in args {
                            arg_vals.push(builder.use_var(int_vars[arg_reg]));
                        }
                        let call_inst = builder.ins().call(target_ref, &arg_vals);
                        let results = builder.inst_results(call_inst);
                        if !results.is_empty() {
                            let res_val = results[0];
                            if *ret_ty == EirType::F64 {
                                builder.def_var(float_vars[*dest], res_val);
                            } else {
                                builder.def_var(int_vars[*dest], res_val);
                            }
                        }
                    }
                }
                EirInst::Jump { target } => {
                    let target_block = block_map[target];
                    builder.ins().jump(target_block, &[]);
                    terminated = true;
                }
                EirInst::BranchIfFalse { cond, target } => {
                    let cond_val = builder.use_var(int_vars[*cond]);
                    let is_false = builder.ins().icmp_imm_s(cranelift_codegen::ir::condcodes::IntCC::Equal, cond_val, 0);
                    let target_block = block_map[target];
                    let next_block = builder.create_block();
                    builder.ins().brif(is_false, target_block, &[], next_block, &[]);
                    builder.switch_to_block(next_block);
                    terminated = false;
                }
                EirInst::Return { val } => {
                    if let Some((reg, ty)) = val {
                        if *ty == EirType::F64 {
                            let v = builder.use_var(float_vars[*reg]);
                            builder.ins().return_(&[v]);
                        } else {
                            let v = builder.use_var(int_vars[*reg]);
                            builder.ins().return_(&[v]);
                        }
                    } else if func.name == "main" {
                        let zero = builder.ins().iconst(types::I64, 0);
                        builder.ins().return_(&[zero]);
                    } else {
                        builder.ins().return_(&[]);
                    }
                    terminated = true;
                }
                EirInst::Print { reg, ty } | EirInst::Println { reg, ty } => {
                    if *ty == EirType::F64 {
                        if let (Some(printf_f64_fid), Some(fmt_f64_id)) = (self.printf_f64_fid, self.fmt_f64_id) {
                            let dprintf_ref = self.module.declare_func_in_func(printf_f64_fid, builder.func);
                            let fd = builder.ins().iconst(types::I32, 1);
                            let fval = builder.use_var(float_vars[*reg]);
                            let fmt_ref = self.module.declare_data_in_func(fmt_f64_id, builder.func);
                            let fmt_ptr = builder.ins().symbol_value(types::I64, fmt_ref);
                            builder.ins().call(dprintf_ref, &[fd, fmt_ptr, fval]);
                        }
                    } else {
                        if let (Some(printf_fid), Some(fmt_i64_id)) = (self.printf_fid, self.fmt_i64_id) {
                            let printf_ref = self.module.declare_func_in_func(printf_fid, builder.func);
                            let val = builder.use_var(int_vars[*reg]);
                            let fmt_ref = self.module.declare_data_in_func(fmt_i64_id, builder.func);
                            let fmt_ptr = builder.ins().symbol_value(types::I64, fmt_ref);
                            builder.ins().call(printf_ref, &[fmt_ptr, val]);
                        }
                    }
                    if let Some(fflush_fid) = self.fflush_fid {
                        let fflush_ref = self.module.declare_func_in_func(fflush_fid, builder.func);
                        let zero_ptr = builder.ins().iconst(types::I64, 0);
                        builder.ins().call(fflush_ref, &[zero_ptr]);
                    }
                }
                EirInst::ConstString { dest, val } => {
                    if let Some(&str_id) = self.string_literals.get(val) {
                        let data_ref = self.module.declare_data_in_func(str_id, builder.func);
                        let ptr = builder.ins().symbol_value(types::I64, data_ref);
                        let tag = builder.ins().iconst(types::I64, 0xfff4_0000_0000_0000_u64 as i64);
                        let tagged = builder.ins().bor(ptr, tag);
                        builder.def_var(int_vars[*dest], tagged);
                    }
                }
                EirInst::ConstFuncPtr { dest, symbol } | EirInst::MakeClosure { dest, symbol, .. } => {
                    if let Some(&(fid, _)) = self.declared_funcs.get(symbol) {
                        let fn_ref = self.module.declare_func_in_func(fid, builder.func);
                        let fn_addr = builder.ins().func_addr(types::I64, fn_ref);
                        builder.def_var(int_vars[*dest], fn_addr);
                    }
                }
                EirInst::DefineGlobal { name, src } => {
                    if let Some(&data_id) = self.global_data_ids.get(name) {
                        let data_ref = self.module.declare_data_in_func(data_id, builder.func);
                        let ptr = builder.ins().symbol_value(types::I64, data_ref);
                        let val = builder.use_var(int_vars[*src]);
                        builder.ins().store(cranelift_codegen::ir::MemFlagsData::new(), val, ptr, 0);
                    }
                }
                EirInst::GetGlobal { dest, name } => {
                    if name == "print" || name == "println" {
                        let print_tag = builder.ins().iconst(types::I64, 0xfff8_0000_0000_0001_u64 as i64);
                        builder.def_var(int_vars[*dest], print_tag);
                    } else if let Some(&data_id) = self.global_data_ids.get(name) {
                        let data_ref = self.module.declare_data_in_func(data_id, builder.func);
                        let ptr = builder.ins().symbol_value(types::I64, data_ref);
                        let val = builder.ins().load(types::I64, cranelift_codegen::ir::MemFlagsData::new(), ptr, 0);
                        builder.def_var(int_vars[*dest], val);
                    } else {
                        let zero = builder.ins().iconst(types::I64, 0);
                        builder.def_var(int_vars[*dest], zero);
                    }
                }
                EirInst::CallDynamic { dest, callee, args } => {
                    let callee_val = builder.use_var(int_vars[*callee]);
                    let print_tag = builder.ins().iconst(types::I64, 0xfff8_0000_0000_0001_u64 as i64);
                    let is_print = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, callee_val, print_tag);

                    let print_block = builder.create_block();
                    let indirect_block = builder.create_block();
                    let merge_block = builder.create_block();

                    builder.ins().brif(is_print, print_block, &[], indirect_block, &[]);

                    // print_block: print all arguments
                    builder.switch_to_block(print_block);
                    for &arg_reg in args {
                        let arg_val = builder.use_var(int_vars[arg_reg]);
                        let mask = builder.ins().iconst(types::I64, 0xffff_0000_0000_0000_u64 as i64);
                        let masked = builder.ins().band(arg_val, mask);
                        let str_tag = builder.ins().iconst(types::I64, 0xfff4_0000_0000_0000_u64 as i64);
                        let is_str = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, masked, str_tag);

                        let str_block = builder.create_block();
                        let int_block = builder.create_block();
                        let next_arg_block = builder.create_block();

                        builder.ins().brif(is_str, str_block, &[], int_block, &[]);

                        builder.switch_to_block(str_block);
                        if let (Some(printf_fid), Some(fmt_str_id)) = (self.printf_fid, self.fmt_str_id) {
                            let printf_ref = self.module.declare_func_in_func(printf_fid, builder.func);
                            let ptr_mask = builder.ins().iconst(types::I64, 0x0000_ffff_ffff_ffff_u64 as i64);
                            let raw_ptr = builder.ins().band(arg_val, ptr_mask);
                            let fmt_ref = self.module.declare_data_in_func(fmt_str_id, builder.func);
                            let fmt_ptr = builder.ins().symbol_value(types::I64, fmt_ref);
                            builder.ins().call(printf_ref, &[fmt_ptr, raw_ptr]);
                        }
                        builder.ins().jump(next_arg_block, &[]);

                        builder.switch_to_block(int_block);
                        if let (Some(printf_fid), Some(fmt_i64_id)) = (self.printf_fid, self.fmt_i64_id) {
                            let printf_ref = self.module.declare_func_in_func(printf_fid, builder.func);
                            let fmt_ref = self.module.declare_data_in_func(fmt_i64_id, builder.func);
                            let fmt_ptr = builder.ins().symbol_value(types::I64, fmt_ref);
                            builder.ins().call(printf_ref, &[fmt_ptr, arg_val]);
                        }
                        builder.ins().jump(next_arg_block, &[]);

                        builder.switch_to_block(next_arg_block);
                    }
                    if let Some(fflush_fid) = self.fflush_fid {
                        let fflush_ref = self.module.declare_func_in_func(fflush_fid, builder.func);
                        let zero_ptr = builder.ins().iconst(types::I64, 0);
                        builder.ins().call(fflush_ref, &[zero_ptr]);
                    }
                    let zero_ret = builder.ins().iconst(types::I64, 0);
                    builder.def_var(int_vars[*dest], zero_ret);
                    builder.ins().jump(merge_block, &[]);

                    // indirect_block: dynamic closure/function call
                    builder.switch_to_block(indirect_block);
                    let mut dyn_sig = self.module.make_signature();
                    for _ in args {
                        dyn_sig.params.push(AbiParam::new(types::I64));
                    }
                    dyn_sig.returns.push(AbiParam::new(types::I64));
                    let sig_ref = builder.import_signature(dyn_sig);
                    let mut arg_vals = Vec::new();
                    for &arg_reg in args {
                        arg_vals.push(builder.use_var(int_vars[arg_reg]));
                    }
                    let call_inst = builder.ins().call_indirect(sig_ref, callee_val, &arg_vals);
                    let results = builder.inst_results(call_inst);
                    let indirect_res = if !results.is_empty() {
                        results[0]
                    } else {
                        builder.ins().iconst(types::I64, 0)
                    };
                    builder.def_var(int_vars[*dest], indirect_res);
                    builder.ins().jump(merge_block, &[]);

                    // merge_block
                    builder.switch_to_block(merge_block);
                }
                _ => {}
            }
        }

        if !terminated {
            if func.name == "main" {
                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().return_(&[zero]);
            } else {
                builder.ins().return_(&[]);
            }
        }

        builder.seal_all_blocks();
        Ok(())
    }
}

/// Compiles an Eronom source script ahead-of-time directly using Cranelift.
pub fn build_cranelift_binary(
    script_path: &Path,
    output_path: &Path,
    target: Option<&str>,
) -> anyhow::Result<()> {
    let stmts = parse_and_resolve_imports(script_path)
        .map_err(|e| anyhow::anyhow!("Compile error in {}: {}", script_path.display(), e))?;
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts)
        .map_err(|e| anyhow::anyhow!("Bytecode compilation error: {}", e))?;

    let module_name = script_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "er_module".to_string());

    let eir_module = super::lower::lower_program(&function, &module_name);

    let backend = CraneliftBackend::new(&module_name, target)?;
    let obj_bytes = backend.compile_module(&eir_module)?;

    let temp_dir = std::env::temp_dir();
    let temp_obj = temp_dir.join(format!("er_clif_{}_{}.o", module_name, std::process::id()));
    std::fs::write(&temp_obj, &obj_bytes)?;

    let status = Command::new("cc")
        .arg("-O3")
        .arg("-fomit-frame-pointer")
        .arg(&temp_obj)
        .arg("-o")
        .arg(output_path)
        .status();

    let _ = std::fs::remove_file(&temp_obj);

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => anyhow::bail!("Linker failed with exit code: {:?}", s.code()),
        Err(e) => anyhow::bail!("Failed to execute host linker (cc): {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cranelift_build_and_run_fib() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_fib_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_clif_fib_bin_{}", std::process::id()));

        let script = r#"
fn fib(n) {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
let res = fib(15);
print(res);
"#;
        std::fs::write(&script_path, script).unwrap();
        let res = build_cranelift_binary(&script_path, &bin_path, None);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "build_cranelift_binary failed: {:?}", res.err());
        assert!(bin_path.exists());

        let output = Command::new(&bin_path).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("610"), "Expected fib(15)=610, got: {}", stdout);
    }

    #[test]
    fn test_cranelift_build_and_run_loop_and_string() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_loop_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_clif_loop_bin_{}", std::process::id()));

        let script = r#"
let sum = 0;
let i = 1;
while (i <= 100) {
    sum = sum + i;
    i = i + 1;
}
print(sum);
print("Done Cranelift!");
"#;
        std::fs::write(&script_path, script).unwrap();
        let res = build_cranelift_binary(&script_path, &bin_path, None);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "build_cranelift_binary failed: {:?}", res.err());
        assert!(bin_path.exists());

        let output = Command::new(&bin_path).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("5050"), "Expected sum=5050, got: {}", stdout);
        assert!(stdout.contains("Done Cranelift!"), "Expected 'Done Cranelift!', got: {}", stdout);
    }
}
