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

const X86_64_SYS_EXIT_BYTES: &[u8] = &[
    0x48, 0xc7, 0xc0, 0x3c, 0x00, 0x00, 0x00, // mov $60, %rax
    0x0f, 0x05,                               // syscall
    0xc3,                                     // ret
];

const X86_64_PRINT_I64_BYTES: &[u8] = &[
    0x53, 0x41, 0x54, 0x41, 0x55, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8d, 0x74, 0x24, 0x1f, 0xc6, 0x06,
    0x0a, 0x49, 0xc7, 0xc5, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89, 0xf8, 0x48, 0x85, 0xc0, 0x79, 0x0c,
    0x48, 0xf7, 0xd8, 0x49, 0xc7, 0xc4, 0x01, 0x00, 0x00, 0x00, 0xeb, 0x03, 0x4d, 0x31, 0xe4, 0x48,
    0x85, 0xc0, 0x75, 0x0b, 0x48, 0xff, 0xce, 0xc6, 0x06, 0x30, 0x49, 0xff, 0xc5, 0xeb, 0x1f, 0x48,
    0x85, 0xc0, 0x74, 0x1a, 0x48, 0x31, 0xd2, 0x48, 0xc7, 0xc3, 0x0a, 0x00, 0x00, 0x00, 0x48, 0xf7,
    0xf3, 0x80, 0xc2, 0x30, 0x48, 0xff, 0xce, 0x88, 0x16, 0x49, 0xff, 0xc5, 0xeb, 0xe1, 0x4d, 0x85,
    0xe4, 0x74, 0x09, 0x48, 0xff, 0xce, 0xc6, 0x06, 0x2d, 0x49, 0xff, 0xc5, 0x48, 0xc7, 0xc0, 0x01,
    0x00, 0x00, 0x00, 0x48, 0xc7, 0xc7, 0x01, 0x00, 0x00, 0x00, 0x4c, 0x89, 0xea, 0x0f, 0x05, 0x48,
    0x83, 0xc4, 0x20, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0xc3,
];

const X86_64_PRINT_STR_BYTES: &[u8] = &[
    0x53, 0x41, 0x54, 0x48, 0x89, 0xfe, 0x48, 0x31, 0xd2, 0x80, 0x3c, 0x16, 0x00, 0x74, 0x05, 0x48,
    0xff, 0xc2, 0xeb, 0xf5, 0x48, 0x85, 0xd2, 0x74, 0x10, 0x48, 0xc7, 0xc0, 0x01, 0x00, 0x00, 0x00,
    0x48, 0xc7, 0xc7, 0x01, 0x00, 0x00, 0x00, 0x0f, 0x05, 0x48, 0x83, 0xec, 0x10, 0xc6, 0x04, 0x24,
    0x0a, 0x48, 0x89, 0xe6, 0x48, 0xc7, 0xc0, 0x01, 0x00, 0x00, 0x00, 0x48, 0xc7, 0xc7, 0x01, 0x00,
    0x00, 0x00, 0x48, 0xc7, 0xc2, 0x01, 0x00, 0x00, 0x00, 0x0f, 0x05, 0x48, 0x83, 0xc4, 0x10, 0x41,
    0x5c, 0x5b, 0xc3,
];

const X86_64_WIN_SYS_EXIT_BYTES: &[u8] = &[
    0x55,                         // push rbp
    0x48, 0x89, 0xe5,             // mov rbp, rsp
    0x48, 0x83, 0xec, 0x20,       // sub rsp, 32
    0xe8, 0x00, 0x00, 0x00, 0x00, // call thunk_ExitProcess (patched by pe_writer)
];

const X86_64_WIN_PRINT_I64_BYTES: &[u8] = &[
    0x55, 0x48, 0x89, 0xe5, 0x53, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x48, 0x83, 0xec, 0x50, 0x48,
    0x8d, 0x74, 0x24, 0x4f, 0xc6, 0x06, 0x0a, 0x49, 0xc7, 0xc5, 0x01, 0x00, 0x00, 0x00, 0x48, 0x89,
    0xc8, 0x48, 0x85, 0xc0, 0x79, 0x0c, 0x48, 0xf7, 0xd8, 0x49, 0xc7, 0xc4, 0x01, 0x00, 0x00, 0x00,
    0xeb, 0x03, 0x4d, 0x31, 0xe4, 0x48, 0x85, 0xc0, 0x75, 0x0b, 0x48, 0xff, 0xce, 0xc6, 0x06, 0x30,
    0x49, 0xff, 0xc5, 0xeb, 0x1f, 0x48, 0x85, 0xc0, 0x74, 0x1a, 0x48, 0x31, 0xd2, 0x48, 0xc7, 0xc3,
    0x0a, 0x00, 0x00, 0x00, 0x48, 0xf7, 0xf3, 0x80, 0xc2, 0x30, 0x48, 0xff, 0xce, 0x88, 0x16, 0x49,
    0xff, 0xc5, 0xeb, 0xe1, 0x4d, 0x85, 0xe4, 0x74, 0x09, 0x48, 0xff, 0xce, 0xc6, 0x06, 0x2d, 0x49,
    0xff, 0xc5, 0x48, 0xc7, 0xc1, 0xf5, 0xff, 0xff, 0xff, 0xe8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89,
    0xc1, 0x48, 0x89, 0xf2, 0x4d, 0x89, 0xe8, 0x4c, 0x8d, 0x4c, 0x24, 0x28, 0x48, 0xc7, 0x44, 0x24,
    0x20, 0x00, 0x00, 0x00, 0x00, 0xe8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x83, 0xc4, 0x50, 0x41, 0x5e,
    0x41, 0x5d, 0x41, 0x5c, 0x5b, 0x5d, 0xc3,
];

const X86_64_WIN_PRINT_STR_BYTES: &[u8] = &[
    0x55, 0x48, 0x89, 0xe5, 0x53, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x48, 0x83, 0xec, 0x50, 0x48,
    0x89, 0xce, 0x48, 0x85, 0xf6, 0x74, 0x7c, 0x48, 0x31, 0xd2, 0x80, 0x3c, 0x16, 0x00, 0x74, 0x05,
    0x48, 0xff, 0xc2, 0xeb, 0xf5, 0x48, 0x85, 0xd2, 0x74, 0x2e, 0x49, 0x89, 0xd5, 0x48, 0xc7, 0xc1,
    0xf5, 0xff, 0xff, 0xff, 0xe8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xc3, 0x48, 0x89, 0xd9, 0x48,
    0x89, 0xf2, 0x4d, 0x89, 0xe8, 0x4c, 0x8d, 0x4c, 0x24, 0x28, 0x48, 0xc7, 0x44, 0x24, 0x20, 0x00,
    0x00, 0x00, 0x00, 0xe8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x85, 0xdb, 0x75, 0x0f, 0x48, 0xc7, 0xc1,
    0xf5, 0xff, 0xff, 0xff, 0xe8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xc3, 0xc6, 0x44, 0x24, 0x4f,
    0x0a, 0x48, 0x89, 0xd9, 0x48, 0x8d, 0x54, 0x24, 0x4f, 0x49, 0xc7, 0xc0, 0x01, 0x00, 0x00, 0x00,
    0x4c, 0x8d, 0x4c, 0x24, 0x28, 0x48, 0xc7, 0x44, 0x24, 0x20, 0x00, 0x00, 0x00, 0x00, 0xe8, 0x00,
    0x00, 0x00, 0x00, 0x48, 0x83, 0xc4, 0x50, 0x41, 0x5e, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0x5d, 0xc3,
];

pub struct CraneliftBackend {
    pub module: ObjectModule,
    pub target_triple: Triple,
    pub is_server: bool,
    declared_funcs: HashMap<String, (FuncId, Signature)>,
    #[allow(dead_code)]
    runtime_funcs: HashMap<String, FuncId>,
    sys_exit_fid: Option<FuncId>,
    print_i64_fid: Option<FuncId>,
    print_str_fid: Option<FuncId>,
    serve_config_fid: Option<FuncId>,
    alloc_object_fid: Option<FuncId>,
    object_set_fid: Option<FuncId>,
    object_get_fid: Option<FuncId>,
    str_eq_fid: Option<FuncId>,
    json_stringify_fid: Option<FuncId>,
    str_concat_fid: Option<FuncId>,
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
            match t_str.to_lowercase().as_str() {
                "windows" | "win" | "win64" => Triple::from_str("x86_64-pc-windows-msvc").unwrap(),
                "macos" | "mac" | "darwin" => {
                    #[cfg(target_arch = "aarch64")]
                    { Triple::from_str("aarch64-apple-darwin").unwrap() }
                    #[cfg(not(target_arch = "aarch64"))]
                    { Triple::from_str("x86_64-apple-darwin").unwrap() }
                }
                "linux" | "linux64" => Triple::from_str("x86_64-unknown-linux-gnu").unwrap(),
                "linux-arm" | "linux-aarch64" | "arm64" => Triple::from_str("aarch64-unknown-linux-gnu").unwrap(),
                "riscv" | "riscv64" => Triple::from_str("riscv64gc-unknown-linux-gnu").unwrap(),
                other => Triple::from_str(other)
                    .map_err(|e| anyhow::anyhow!("Invalid target triple '{}': {}", other, e))?,
            }
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
            is_server: false,
            declared_funcs: HashMap::new(),
            runtime_funcs: HashMap::new(),
            sys_exit_fid: None,
            print_i64_fid: None,
            print_str_fid: None,
            serve_config_fid: None,
            alloc_object_fid: None,
            object_set_fid: None,
            object_get_fid: None,
            str_eq_fid: None,
            json_stringify_fid: None,
            str_concat_fid: None,
            global_data_ids: HashMap::new(),
            string_literals: HashMap::new(),
        })
    }

    #[allow(dead_code)]
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
        // 1. Define direct zero-libc routines (Linux syscalls or Windows Win32 IAT routines)
        let is_windows = self.target_triple.operating_system == target_lexicon::OperatingSystem::Windows;
        if self.target_triple.architecture == target_lexicon::Architecture::X86_64 {
            let (exit_bytes, print_i64_bytes, print_str_bytes) = if is_windows {
                (
                    X86_64_WIN_SYS_EXIT_BYTES,
                    X86_64_WIN_PRINT_I64_BYTES,
                    X86_64_WIN_PRINT_STR_BYTES,
                )
            } else {
                (
                    X86_64_SYS_EXIT_BYTES,
                    X86_64_PRINT_I64_BYTES,
                    X86_64_PRINT_STR_BYTES,
                )
            };

            let mut exit_sig = self.module.make_signature();
            exit_sig.params.push(AbiParam::new(types::I64));
            let exit_fid = self.module.declare_function("er_sys_exit", Linkage::Export, &exit_sig)?;
            self.module.define_function_bytes(exit_fid, 16, exit_bytes, &[])?;
            self.sys_exit_fid = Some(exit_fid);

            let mut print_i64_sig = self.module.make_signature();
            print_i64_sig.params.push(AbiParam::new(types::I64));
            let print_i64_fid = self.module.declare_function("er_print_i64", Linkage::Export, &print_i64_sig)?;
            self.module.define_function_bytes(print_i64_fid, 16, print_i64_bytes, &[])?;
            self.print_i64_fid = Some(print_i64_fid);

            let mut print_str_sig = self.module.make_signature();
            print_str_sig.params.push(AbiParam::new(types::I64));
            let print_str_fid = self.module.declare_function("er_print_str", Linkage::Export, &print_str_sig)?;
            self.module.define_function_bytes(print_str_fid, 16, print_str_bytes, &[])?;
            self.print_str_fid = Some(print_str_fid);
        }

        // Declare native runtime imports for HTTP and object operations
        if self.is_server {
            let serve_config_fid = self.declare_runtime_fn("er_clif_serve_config", &[types::I64], None)?;
            self.serve_config_fid = Some(serve_config_fid);

            let alloc_object_fid = self.declare_runtime_fn("er_clif_alloc_object", &[], Some(types::I64))?;
            self.alloc_object_fid = Some(alloc_object_fid);

            let object_set_fid = self.declare_runtime_fn("er_clif_object_set", &[types::I64, types::I64, types::I64], None)?;
            self.object_set_fid = Some(object_set_fid);

            let object_get_fid = self.declare_runtime_fn("er_clif_object_get", &[types::I64, types::I64], Some(types::I64))?;
            self.object_get_fid = Some(object_get_fid);

            let str_eq_fid = self.declare_runtime_fn("er_clif_str_eq", &[types::I64, types::I64], Some(types::I64))?;
            self.str_eq_fid = Some(str_eq_fid);

            let json_stringify_fid = self.declare_runtime_fn("er_clif_json_stringify", &[types::I64], Some(types::I64))?;
            self.json_stringify_fid = Some(json_stringify_fid);

            let str_concat_fid = self.declare_runtime_fn("er_clif_str_concat", &[types::I64, types::I64], Some(types::I64))?;
            self.str_concat_fid = Some(str_concat_fid);
        }

        // 2. Pre-scan and declare global variables and string literals
        for func in &eir.functions {
            for inst in &func.instructions {
                match inst {
                    EirInst::DefineGlobal { name, .. } | EirInst::GetGlobal { name, .. } => {
                        if name != "print" && name != "println" && name != "serve" && name != "JSON" && !self.global_data_ids.contains_key(name) {
                            let data_id = self.module.declare_data(&format!("__er_g_{}", name), Linkage::Local, true, false)?;
                            let mut desc = DataDescription::new();
                            desc.define_zeroinit(8);
                            self.module.define_data(data_id, &desc)?;
                            self.global_data_ids.insert(name.clone(), data_id);
                        }
                    }
                    EirInst::ConstString { val, .. } | EirInst::GetProperty { key: val, .. } | EirInst::SetProperty { key: val, .. } => {
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

        // 5. Emit standard standalone _start entrypoint (invokes main and er_sys_exit)
        // If compiling as an HTTP server binary, CRT handles _start when linking with cc
        if !self.is_server {
            let start_sig = self.module.make_signature();
            let start_fid = self.module.declare_function("_start", Linkage::Export, &start_sig)?;
            ctx.func.signature = start_sig;
            ctx.func.name = UserFuncName::User(UserExternalName::new(start_fid.as_u32(), 0));

            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fn_builder_ctx);
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            if let Some(&(main_fid, _)) = self.declared_funcs.get("main") {
                let main_ref = self.module.declare_func_in_func(main_fid, builder.func);
                let call_inst = builder.ins().call(main_ref, &[]);
                let ret_val = builder.inst_results(call_inst)[0];

                if let Some(exit_fid) = self.sys_exit_fid {
                    let exit_ref = self.module.declare_func_in_func(exit_fid, builder.func);
                    builder.ins().call(exit_ref, &[ret_val]);
                }
            } else if let Some(exit_fid) = self.sys_exit_fid {
                let exit_ref = self.module.declare_func_in_func(exit_fid, builder.func);
                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().call(exit_ref, &[zero]);
            }
            builder.ins().return_(&[]);
            builder.finalize(self.module.isa().frontend_config());
            self.module.define_function(start_fid, &mut ctx)?;
            self.module.clear_context(&mut ctx);
        }

        // 6. Emit finalized native object code
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
                            EirBinaryOp::Add => {
                                if let Some(concat_fid) = self.str_concat_fid {
                                    let str_tag = builder.ins().iconst(types::I64, 0xfff4_0000_0000_0000_u64 as i64);
                                    let mask = builder.ins().iconst(types::I64, 0xffff_0000_0000_0000_u64 as i64);
                                    let l_tag = builder.ins().band(l, mask);
                                    let r_tag = builder.ins().band(r, mask);
                                    let l_is_str = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, l_tag, str_tag);
                                    let r_is_str = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, r_tag, str_tag);
                                    let either_str = builder.ins().bor(l_is_str, r_is_str);

                                    let concat_block = builder.create_block();
                                    let add_block = builder.create_block();
                                    let merge_add_block = builder.create_block();

                                    builder.ins().brif(either_str, concat_block, &[], add_block, &[]);

                                    builder.switch_to_block(concat_block);
                                    let concat_ref = self.module.declare_func_in_func(concat_fid, builder.func);
                                    let call_inst = builder.ins().call(concat_ref, &[l, r]);
                                    let c_res = builder.inst_results(call_inst)[0];
                                    builder.def_var(int_vars[*dest], c_res);
                                    builder.ins().jump(merge_add_block, &[]);

                                    builder.switch_to_block(add_block);
                                    let i_res = builder.ins().iadd(l, r);
                                    builder.def_var(int_vars[*dest], i_res);
                                    builder.ins().jump(merge_add_block, &[]);

                                    builder.switch_to_block(merge_add_block);
                                    builder.use_var(int_vars[*dest])
                                } else {
                                    builder.ins().iadd(l, r)
                                }
                            }
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
                                if let Some(streq_fid) = self.str_eq_fid {
                                    let streq_ref = self.module.declare_func_in_func(streq_fid, builder.func);
                                    let call_inst = builder.ins().call(streq_ref, &[l, r]);
                                    builder.inst_results(call_inst)[0]
                                } else {
                                    let c = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, l, r);
                                    builder.ins().uextend(types::I64, c)
                                }
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
                EirInst::Print { reg, .. } | EirInst::Println { reg, .. } => {
                    if let Some(print_i64_fid) = self.print_i64_fid {
                        let print_ref = self.module.declare_func_in_func(print_i64_fid, builder.func);
                        let val = builder.use_var(int_vars[*reg]);
                        builder.ins().call(print_ref, &[val]);
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
                    } else if name == "serve" {
                        let serve_tag = builder.ins().iconst(types::I64, 0xfff8_0000_0000_0002_u64 as i64);
                        builder.def_var(int_vars[*dest], serve_tag);
                    } else if name == "JSON" {
                        let json_tag = builder.ins().iconst(types::I64, 0xfff8_0000_0000_0003_u64 as i64);
                        builder.def_var(int_vars[*dest], json_tag);
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

                    let serve_tag = builder.ins().iconst(types::I64, 0xfff8_0000_0000_0002_u64 as i64);
                    let is_serve = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, callee_val, serve_tag);

                    let stringify_tag = builder.ins().iconst(types::I64, 0xfff8_0000_0000_0004_u64 as i64);
                    let is_stringify = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, callee_val, stringify_tag);

                    let print_block = builder.create_block();
                    let check_serve_block = builder.create_block();
                    let serve_block = builder.create_block();
                    let check_stringify_block = builder.create_block();
                    let stringify_block = builder.create_block();
                    let indirect_block = builder.create_block();
                    let merge_block = builder.create_block();

                    builder.ins().brif(is_print, print_block, &[], check_serve_block, &[]);

                    builder.switch_to_block(check_serve_block);
                    builder.ins().brif(is_serve, serve_block, &[], check_stringify_block, &[]);

                    builder.switch_to_block(check_stringify_block);
                    builder.ins().brif(is_stringify, stringify_block, &[], indirect_block, &[]);

                    // serve_block
                    builder.switch_to_block(serve_block);
                    if let Some(serve_fid) = self.serve_config_fid {
                        let serve_ref = self.module.declare_func_in_func(serve_fid, builder.func);
                        let cfg_val = if !args.is_empty() {
                            builder.use_var(int_vars[args[0]])
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        };
                        builder.ins().call(serve_ref, &[cfg_val]);
                    }
                    let zero_ret = builder.ins().iconst(types::I64, 0);
                    builder.def_var(int_vars[*dest], zero_ret);
                    builder.ins().jump(merge_block, &[]);

                    // stringify_block
                    builder.switch_to_block(stringify_block);
                    if let Some(stringify_fid) = self.json_stringify_fid {
                        let stringify_ref = self.module.declare_func_in_func(stringify_fid, builder.func);
                        let arg_val = if !args.is_empty() {
                            builder.use_var(int_vars[args[0]])
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        };
                        let call_inst = builder.ins().call(stringify_ref, &[arg_val]);
                        let res = builder.inst_results(call_inst)[0];
                        builder.def_var(int_vars[*dest], res);
                    } else {
                        let zero = builder.ins().iconst(types::I64, 0);
                        builder.def_var(int_vars[*dest], zero);
                    }
                    builder.ins().jump(merge_block, &[]);

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
                        if let Some(print_str_fid) = self.print_str_fid {
                            let print_ref = self.module.declare_func_in_func(print_str_fid, builder.func);
                            let ptr_mask = builder.ins().iconst(types::I64, 0x0000_ffff_ffff_ffff_u64 as i64);
                            let raw_ptr = builder.ins().band(arg_val, ptr_mask);
                            builder.ins().call(print_ref, &[raw_ptr]);
                        }
                        builder.ins().jump(next_arg_block, &[]);

                        builder.switch_to_block(int_block);
                        if let Some(print_i64_fid) = self.print_i64_fid {
                            let print_ref = self.module.declare_func_in_func(print_i64_fid, builder.func);
                            builder.ins().call(print_ref, &[arg_val]);
                        }
                        builder.ins().jump(next_arg_block, &[]);

                        builder.switch_to_block(next_arg_block);
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
                EirInst::MakeObject { dest, start_reg, pair_count } => {
                    if let (Some(alloc_fid), Some(set_fid)) = (self.alloc_object_fid, self.object_set_fid) {
                        let alloc_ref = self.module.declare_func_in_func(alloc_fid, builder.func);
                        let set_ref = self.module.declare_func_in_func(set_fid, builder.func);
                        let call_alloc = builder.ins().call(alloc_ref, &[]);
                        let obj_val = builder.inst_results(call_alloc)[0];
                        for i in 0..*pair_count {
                            let k_reg = start_reg + i * 2;
                            let v_reg = start_reg + i * 2 + 1;
                            let k_val = builder.use_var(int_vars[k_reg]);
                            let v_val = builder.use_var(int_vars[v_reg]);
                            builder.ins().call(set_ref, &[obj_val, k_val, v_val]);
                        }
                        builder.def_var(int_vars[*dest], obj_val);
                    }
                }
                EirInst::GetProperty { dest, obj, key } => {
                    let obj_val = builder.use_var(int_vars[*obj]);
                    if key == "stringify" {
                        let json_tag = builder.ins().iconst(types::I64, 0xfff8_0000_0000_0003_u64 as i64);
                        let is_json = builder.ins().icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, obj_val, json_tag);
                        let normal_prop_block = builder.create_block();
                        let stringify_prop_block = builder.create_block();
                        let merge_prop_block = builder.create_block();

                        builder.ins().brif(is_json, stringify_prop_block, &[], normal_prop_block, &[]);

                        builder.switch_to_block(stringify_prop_block);
                        let stringify_tag = builder.ins().iconst(types::I64, 0xfff8_0000_0000_0004_u64 as i64);
                        builder.def_var(int_vars[*dest], stringify_tag);
                        builder.ins().jump(merge_prop_block, &[]);

                        builder.switch_to_block(normal_prop_block);
                        if let Some(get_fid) = self.object_get_fid {
                            let get_ref = self.module.declare_func_in_func(get_fid, builder.func);
                            let key_ptr = if let Some(&id) = self.string_literals.get(key) {
                                let data_ref = self.module.declare_data_in_func(id, builder.func);
                                builder.ins().symbol_value(types::I64, data_ref)
                            } else {
                                builder.ins().iconst(types::I64, 0)
                            };
                            let call_inst = builder.ins().call(get_ref, &[obj_val, key_ptr]);
                            let res = builder.inst_results(call_inst)[0];
                            builder.def_var(int_vars[*dest], res);
                        }
                        builder.ins().jump(merge_prop_block, &[]);

                        builder.switch_to_block(merge_prop_block);
                    } else if let Some(get_fid) = self.object_get_fid {
                        let get_ref = self.module.declare_func_in_func(get_fid, builder.func);
                        let key_ptr = if let Some(&id) = self.string_literals.get(key) {
                            let data_ref = self.module.declare_data_in_func(id, builder.func);
                            builder.ins().symbol_value(types::I64, data_ref)
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        };
                        let call_inst = builder.ins().call(get_ref, &[obj_val, key_ptr]);
                        let res = builder.inst_results(call_inst)[0];
                        builder.def_var(int_vars[*dest], res);
                    }
                }
                EirInst::SetProperty { obj, key, val } => {
                    if let Some(set_fid) = self.object_set_fid {
                        let set_ref = self.module.declare_func_in_func(set_fid, builder.func);
                        let obj_val = builder.use_var(int_vars[*obj]);
                        let val_val = builder.use_var(int_vars[*val]);
                        let key_ptr = if let Some(&id) = self.string_literals.get(key) {
                            let data_ref = self.module.declare_data_in_func(id, builder.func);
                            builder.ins().symbol_value(types::I64, data_ref)
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        };
                        builder.ins().call(set_ref, &[obj_val, key_ptr, val_val]);
                    }
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

fn find_static_lib(name: &str) -> Option<std::path::PathBuf> {
    let lib_names = [
        format!("lib{}.a", name),
        format!("{}.lib", name),
        format!("lib{}.lib", name),
    ];

    // 1. Environment variable override
    if let Ok(dir) = std::env::var("ERONOM_LIB_DIR") {
        for lib_name in &lib_names {
            let p = Path::new(&dir).join(lib_name);
            if p.exists() {
                return Some(p);
            }
        }
    }

    // 2. Current executable directory (e.g. ~/.eronom/lib or next to binary)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for lib_name in &lib_names {
                let candidate1 = parent.join(lib_name);
                if candidate1.exists() {
                    return Some(candidate1);
                }
                let candidate2 = parent.join("lib").join(lib_name);
                if candidate2.exists() {
                    return Some(candidate2);
                }
            }
        }
    }

    // 3. target/er_native/ directory in CARGO_MANIFEST_DIR
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for lib_name in &lib_names {
        let native_dir = manifest_dir.join("target").join("er_native").join(lib_name);
        if native_dir.exists() {
            return Some(native_dir);
        }
    }

    // 4. Scan all build directories under target/ (including target/<triple>/<profile>/build)
    let target_dir = manifest_dir.join("target");
    let mut search_dirs = Vec::new();
    for profile in &["debug", "release"] {
        search_dirs.push(target_dir.join(profile).join("build"));
    }
    if let Ok(entries) = std::fs::read_dir(&target_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                for profile in &["debug", "release"] {
                    let candidate = p.join(profile).join("build");
                    if candidate.is_dir() {
                        search_dirs.push(candidate);
                    }
                }
            }
        }
    }

    for build_dir in search_dirs {
        if let Ok(entries) = std::fs::read_dir(&build_dir) {
            for entry in entries.flatten() {
                let out_dir = entry.path().join("out");
                if out_dir.is_dir() {
                    for lib_name in &lib_names {
                        let out_lib = out_dir.join(lib_name);
                        if out_lib.exists() {
                            return Some(out_lib);
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn find_msvc_linker() -> std::path::PathBuf {
    // 1. Direct invocation from PATH
    if Command::new("link.exe").arg("/?").output().is_ok() {
        return std::path::PathBuf::from("link.exe");
    }

    // 2. Environment variables from VS developer prompt
    if let Ok(vc_dir) = std::env::var("VCINSTALLDIR") {
        let msvc_tools = Path::new(&vc_dir).join("Tools").join("MSVC");
        if let Ok(entries) = std::fs::read_dir(&msvc_tools) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("bin").join("Hostx64").join("x64").join("link.exe");
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    // 3. Fallback search in Visual Studio directories
    for vs_base in &[
        r"C:\Program Files\Microsoft Visual Studio",
        r"C:\Program Files (x86)\Microsoft Visual Studio",
    ] {
        let base = Path::new(vs_base);
        if let Ok(years) = std::fs::read_dir(base) {
            for year in years.flatten() {
                if let Ok(editions) = std::fs::read_dir(year.path()) {
                    for edition in editions.flatten() {
                        let msvc_dir = edition.path().join("VC").join("Tools").join("MSVC");
                        if let Ok(versions) = std::fs::read_dir(&msvc_dir) {
                            for version in versions.flatten() {
                                let candidate = version.path().join("bin").join("Hostx64").join("x64").join("link.exe");
                                if candidate.exists() {
                                    return candidate;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    std::path::PathBuf::from("link.exe")
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

    let is_server = eir_module.functions.iter().any(|f| {
        f.instructions.iter().any(|i| matches!(i, EirInst::GetGlobal { name, .. } if name == "serve" || name == "er_http_create_server"))
    });

    let mut backend = CraneliftBackend::new(&module_name, target)?;
    backend.is_server = is_server;
    let target_os = backend.target_triple.operating_system;
    let target_arch = backend.target_triple.architecture;
    let obj_bytes = backend.compile_module(&eir_module)?;

    let temp_dir = std::env::temp_dir();
    let temp_obj = temp_dir.join(format!("er_clif_{}_{}.o", module_name, std::process::id()));
    std::fs::write(&temp_obj, &obj_bytes)?;

    // Dual-Mode Linker Step:
    let linked = if is_server {
        match target_os {
            target_lexicon::OperatingSystem::Windows => {
                #[cfg(target_os = "windows")]
                {
                    let liber_http = find_static_lib("er_http").unwrap_or_else(|| std::path::PathBuf::from("er_http.lib"));
                    let libusockets = find_static_lib("usockets").unwrap_or_else(|| std::path::PathBuf::from("usockets.lib"));
                    let libuv = find_static_lib("uv").unwrap_or_else(|| std::path::PathBuf::from("uv.lib"));
                    let linker_bin = find_msvc_linker();
                    let mut cmd = Command::new(&linker_bin);
                    cmd.arg(&format!("/OUT:{}", output_path.display()))
                        .arg(&temp_obj)
                        .arg(&liber_http)
                        .arg(&libusockets)
                        .arg(&libuv)
                        .arg("ws2_32.lib")
                        .arg("psapi.lib")
                        .arg("user32.lib")
                        .arg("advapi32.lib")
                        .arg("iphlpapi.lib")
                        .arg("userenv.lib")
                        .arg("dbghelp.lib")
                        .arg("ole32.lib")
                        .arg("shell32.lib")
                        .arg("kernel32.lib")
                        .arg("msvcrt.lib")
                        .arg("msvcprt.lib")
                        .arg("/SUBSYSTEM:CONSOLE")
                        .arg("/OPT:REF")
                        .arg("/OPT:ICF");

                    if let Some(parent) = liber_http.parent() {
                        cmd.arg(format!("/LIBPATH:{}", parent.display()));
                    }
                    if let Some(parent) = libusockets.parent() {
                        cmd.arg(format!("/LIBPATH:{}", parent.display()));
                    }
                    if let Some(parent) = libuv.parent() {
                        cmd.arg(format!("/LIBPATH:{}", parent.display()));
                    }

                    let output = cmd.output();
                    match output {
                        Ok(ref out) if out.status.success() => true,
                        Ok(ref out) => {
                            eprintln!("link.exe failed with exit code {:?}:\nSTDOUT:\n{}\nSTDERR:\n{}",
                                out.status.code(),
                                String::from_utf8_lossy(&out.stdout),
                                String::from_utf8_lossy(&out.stderr)
                            );
                            false
                        }
                        Err(ref e) => {
                            eprintln!("Failed to invoke link.exe ({:?}): {:?}", linker_bin, e);
                            false
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    if let Some(stub_path) = crate::jit::aot::resolve_target_stub(None, Some("windows")) {
                        crate::cli::bundle::create_standalone_binary(&stub_path, script_path, None, output_path).is_ok()
                    } else {
                        false
                    }
                }
            }
            target_lexicon::OperatingSystem::Darwin(_) | target_lexicon::OperatingSystem::MacOSX { .. } => {
                let liber_http = find_static_lib("er_http")
                    .ok_or_else(|| anyhow::anyhow!("Could not find liber_http.a for Cranelift server linking"))?;
                let libusockets = find_static_lib("usockets")
                    .ok_or_else(|| anyhow::anyhow!("Could not find libusockets.a for Cranelift server linking"))?;
                let status = Command::new("clang")
                    .arg("-Wl,-dead_strip")
                    .arg(&temp_obj)
                    .arg(&liber_http)
                    .arg(&libusockets)
                    .arg("-lc++")
                    .arg("-lpthread")
                    .arg("-o")
                    .arg(output_path)
                    .status();
                matches!(status, Ok(s) if s.success())
            }
            _ => {
                // Linux / System V ELF: Link with liber_http.a, libusockets.a, -lstdc++, -lpthread, -lm
                let liber_http = find_static_lib("er_http")
                    .ok_or_else(|| anyhow::anyhow!("Could not find liber_http.a for Cranelift server linking"))?;
                let libusockets = find_static_lib("usockets")
                    .ok_or_else(|| anyhow::anyhow!("Could not find libusockets.a for Cranelift server linking"))?;
                let cc_bin = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
                let status = Command::new(cc_bin)
                    .arg("-s")
                    .arg(&temp_obj)
                    .arg(&liber_http)
                    .arg(&libusockets)
                    .arg("-lstdc++")
                    .arg("-lpthread")
                    .arg("-lm")
                    .arg("-o")
                    .arg(output_path)
                    .status();
                matches!(status, Ok(s) if s.success())
            }
        }
    } else {
        // Zero-Toolchain Cross-Platform Linker Step for compute scripts:
        match target_os {
            target_lexicon::OperatingSystem::Windows => {
                // Pure-Rust PE32+ Synthesizer: directly emits standalone Windows PE32+ (.exe) with Win32 IAT thunks
                if super::pe_writer::link_object_to_static_pe64(&obj_bytes, output_path).is_ok() {
                    true
                } else if let Some(stub_path) = crate::jit::aot::resolve_target_stub(None, Some("windows")) {
                    crate::cli::bundle::create_standalone_binary(&stub_path, script_path, None, output_path).is_ok()
                } else {
                    false
                }
            }
            target_lexicon::OperatingSystem::Darwin(_) | target_lexicon::OperatingSystem::MacOSX { .. } => {
                let is_arm64 = matches!(target_arch, target_lexicon::Architecture::Aarch64(_));
                #[cfg(target_os = "macos")]
                {
                    let status = Command::new("ld")
                        .arg(&temp_obj)
                        .arg("-o")
                        .arg(output_path)
                        .status();
                    if matches!(status, Ok(s) if s.success()) {
                        true
                    } else {
                        super::macho_writer::link_object_to_static_macho64(&obj_bytes, output_path, is_arm64).is_ok()
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    // Pure-Rust Mach-O 64-bit Synthesizer: emits standalone macOS binary without ld64 or runner stub
                    if super::macho_writer::link_object_to_static_macho64(&obj_bytes, output_path, is_arm64).is_ok() {
                        true
                    } else if let Some(stub_path) = crate::jit::aot::resolve_target_stub(None, Some("macos")) {
                        crate::cli::bundle::create_standalone_binary(&stub_path, script_path, None, output_path).is_ok()
                    } else {
                        false
                    }
                }
            }
            _ => {
                // Linux / System V ELF
                let system_linker = if Command::new("ld").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
                    Some("ld".to_string())
                } else {
                    ["/usr/bin/ld", "/bin/ld", "/usr/local/bin/ld", "/usr/bin/mold", "/usr/bin/gold", "/usr/bin/lld"]
                        .iter()
                        .find(|&&p| Path::new(p).exists())
                        .map(|&s| s.to_string())
                };

                if let Some(linker_bin) = system_linker {
                    let status = Command::new(linker_bin)
                        .arg("-s")
                        .arg(&temp_obj)
                        .arg("-o")
                        .arg(output_path)
                        .status();
                    matches!(status, Ok(s) if s.success())
                } else {
                    super::elf_writer::link_object_to_static_elf64(&obj_bytes, output_path).is_ok()
                }
            }
        }
    };

    let _ = std::fs::remove_file(&temp_obj);

    if !linked {
        anyhow::bail!("Failed to link binary");
    }

    // On Windows, ensure both output_path and output_path.exe exist
    let exe_path = if output_path.extension().map(|e| e == "exe").unwrap_or(false) {
        output_path.to_path_buf()
    } else {
        output_path.with_extension("exe")
    };
    if exe_path != output_path {
        if exe_path.exists() && !output_path.exists() {
            let _ = std::fs::copy(&exe_path, output_path);
        } else if output_path.exists() && !exe_path.exists() {
            let _ = std::fs::copy(output_path, &exe_path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::aot::lower;
    use object::{Object, ObjectSection, ObjectSymbol};

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
        let actual_bin = if bin_path.with_extension("exe").exists() {
            bin_path.with_extension("exe")
        } else {
            bin_path.clone()
        };
        assert!(actual_bin.exists());

        let output = Command::new(&actual_bin).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(bin_path.with_extension("exe"));

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
        let actual_bin = if bin_path.with_extension("exe").exists() {
            bin_path.with_extension("exe")
        } else {
            bin_path.clone()
        };
        assert!(actual_bin.exists());

        let output = Command::new(&actual_bin).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(bin_path.with_extension("exe"));

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("5050"), "Expected sum=5050, got: {}", stdout);
        assert!(stdout.contains("Done Cranelift!"), "Expected 'Done Cranelift!', got: {}", stdout);
    }

    #[test]
    fn test_cranelift_zero_toolchain_direct_execution() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_zero_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_clif_zero_bin_{}", std::process::id()));

        let script = r#"
fn fib(n) {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
let res = fib(12);
print(res);
print("Zero CC Verified!");
"#;
        std::fs::write(&script_path, script).unwrap();
        let res = build_cranelift_binary(&script_path, &bin_path, None);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "build_cranelift_binary failed: {:?}", res.err());
        let actual_bin = if bin_path.with_extension("exe").exists() {
            bin_path.with_extension("exe")
        } else {
            bin_path.clone()
        };
        assert!(actual_bin.exists());

        let output = Command::new(&actual_bin).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(bin_path.with_extension("exe"));

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("144"), "Expected fib(12)=144, got: {}", stdout);
        assert!(stdout.contains("Zero CC Verified!"), "Expected 'Zero CC Verified!', got: {}", stdout);
    }

    #[test]
    fn test_cranelift_build_with_empty_path() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_empty_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_clif_empty_bin_{}", std::process::id()));

        let script = r#"
fn fib(n) {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
let res = fib(10);
print(res);
"#;
        std::fs::write(&script_path, script).unwrap();
        let res = build_cranelift_binary(&script_path, &bin_path, None);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "build_cranelift_binary failed: {:?}", res.err());
        let actual_bin = if bin_path.with_extension("exe").exists() {
            bin_path.with_extension("exe")
        } else {
            bin_path.clone()
        };
        assert!(actual_bin.exists());

        let output = Command::new(&actual_bin).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(bin_path.with_extension("exe"));
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("55"), "Expected 55, got: {}", stdout);
    }

    #[test]
    fn test_cranelift_cross_windows_obj() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_win_probe_{}.er", std::process::id()));
        std::fs::write(&script_path, "let x = 42;\nprint(x);\n").unwrap();
        let stmts = parse_and_resolve_imports(&script_path).unwrap();
        let compiler = Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let eir_module = lower::lower_program(&function, "test_win_probe");
        let backend = CraneliftBackend::new("test_win_probe", Some("windows")).unwrap();
        let obj_bytes = backend.compile_module(&eir_module).unwrap();
        let _ = std::fs::remove_file(&script_path);

        assert!(!obj_bytes.is_empty(), "Object bytes should not be empty");
        let parsed = object::File::parse(&obj_bytes[..]).unwrap();
        assert_eq!(parsed.format(), object::BinaryFormat::Coff);
    }

    #[test]
    fn test_cranelift_cross_macos_obj() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_mac_probe_{}.er", std::process::id()));
        std::fs::write(&script_path, "let x = 42;\nprint(x);\n").unwrap();
        let stmts = parse_and_resolve_imports(&script_path).unwrap();
        let compiler = Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let eir_module = lower::lower_program(&function, "test_mac_probe");
        let backend = CraneliftBackend::new("test_mac_probe", Some("macos")).unwrap();
        let obj_bytes = backend.compile_module(&eir_module).unwrap();
        let _ = std::fs::remove_file(&script_path);

        assert!(!obj_bytes.is_empty(), "Object bytes should not be empty");
        let parsed = object::File::parse(&obj_bytes[..]).unwrap();
        println!("macOS object format: {:?}", parsed.format());
        assert_eq!(parsed.format(), object::BinaryFormat::MachO);
        for sec in parsed.sections() {
            println!("  Mach-O Section: {} (size: {})", sec.name().unwrap_or("?"), sec.data().unwrap().len());
        }
        for sym in parsed.symbols() {
            if let Ok(name) = sym.name() {
                println!("  Mach-O Symbol: {} at 0x{:x}", name, sym.address());
            }
        }
        if let Some(text) = parsed.section_by_name("__text") {
            println!("  Mach-O __text relocations count: {}", text.relocations().count());
            for (offset, reloc) in text.relocations() {
                println!("    reloc at 0x{:x}: {:?}", offset, reloc.target());
            }
        }
    }

    #[test]
    fn test_cranelift_cross_windows_micro_binary() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_win_bin_{}.er", std::process::id()));
        let out_path = temp_dir.join(format!("test_clif_win_bin_{}.exe", std::process::id()));
        std::fs::write(&script_path, "let x = 100;\nprint(x);\n").unwrap();

        let res = build_cranelift_binary(&script_path, &out_path, Some("windows"));
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "Windows micro binary build failed: {:?}", res.err());
        assert!(out_path.exists());
        let meta = std::fs::metadata(&out_path).unwrap();
        println!("Generated Windows .exe size: {} bytes", meta.len());
        // Verify micro-binary size is < 10 KB (no 2.5MB runner stub)
        assert!(meta.len() < 10_000, "Windows executable should be < 10KB, got {}", meta.len());

        let bytes = std::fs::read(&out_path).unwrap();
        let parsed = object::File::parse(&bytes[..]).unwrap();
        assert_eq!(parsed.format(), object::BinaryFormat::Pe);

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            let (code, output) = crate::jit::aot::pe_writer::run_pe64_standalone(&bytes).unwrap();
            assert_eq!(code, 0, "Windows executable exited with non-zero code");
            assert_eq!(output.trim(), "100", "Expected output '100', got '{}'", output);
        }

        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn test_cranelift_cross_windows_fib_execution() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_win_fib_{}.er", std::process::id()));
        let out_path = temp_dir.join(format!("test_clif_win_fib_{}.exe", std::process::id()));
        std::fs::write(&script_path, "fn fib(n) {\n    if (n <= 1) {\n        return n;\n    }\n    return fib(n - 1) + fib(n - 2);\n}\nlet res = fib(10);\nprint(res);\n").unwrap();

        let res = build_cranelift_binary(&script_path, &out_path, Some("windows"));
        let _ = std::fs::remove_file(&script_path);
        assert!(res.is_ok(), "Build failed: {:?}", res.err());

        let _bytes = std::fs::read(&out_path).unwrap();
        let _ = std::fs::remove_file(&out_path);

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            let (code, output) = crate::jit::aot::pe_writer::run_pe64_standalone(&_bytes).unwrap();
            assert_eq!(code, 0, "Windows executable exited with non-zero code");
            assert_eq!(output.trim(), "55", "Expected output '55', got '{}'", output);
        }
    }

    #[test]
    fn test_cranelift_cross_macos_micro_binary() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_mac_bin_{}.er", std::process::id()));
        let out_path = temp_dir.join(format!("test_clif_mac_bin_{}", std::process::id()));
        std::fs::write(&script_path, "let x = 100;\nprint(x);\n").unwrap();

        let res = build_cranelift_binary(&script_path, &out_path, Some("macos"));
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "macOS micro binary build failed: {:?}", res.err());
        assert!(out_path.exists());
        let meta = std::fs::metadata(&out_path).unwrap();
        println!("Generated macOS binary size: {} bytes", meta.len());
        // Verify micro-binary size is < 10 KB (no runner stub)
        assert!(meta.len() < 10_000, "macOS binary should be < 10KB, got {}", meta.len());

        let bytes = std::fs::read(&out_path).unwrap();
        let parsed = object::File::parse(&bytes[..]).unwrap();
        assert_eq!(parsed.format(), object::BinaryFormat::MachO);
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn test_cranelift_build_server_binary() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_clif_srv_{}.er", std::process::id()));
        let out_path = temp_dir.join(format!("test_clif_srv_bin_{}", std::process::id()));
        let script = r#"
import { serve, Response } from "std/http"

serve({
    port: 3999,
    fetch: (req) => {
        if (req.path == "/health") {
            return Response.text("OK", { status: 200 })
        }
        return Response.json({ status: "running", path: req.path })
    }
})
"#;
        std::fs::write(&script_path, script).unwrap();
        let res = build_cranelift_binary(&script_path, &out_path, None);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "Server build failed: {:?}", res.err());
        let actual_bin = if out_path.exists() {
            out_path.clone()
        } else {
            out_path.with_extension("exe")
        };
        assert!(actual_bin.exists(), "Server binary should exist on disk");
        let _ = std::fs::remove_file(&out_path);
        let _ = std::fs::remove_file(out_path.with_extension("exe"));
    }
}
