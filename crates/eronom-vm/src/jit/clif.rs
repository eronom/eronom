use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{types, AbiParam, Block, InstBuilder};
use cranelift_codegen::settings::{self, Configurable, Flags};
use cranelift_codegen::{isa, Context};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};
use target_lexicon::Triple;

use crate::vm::bytecode::{Function, OpCode};
use crate::vm::execute::VM;
use crate::vm::gc::{GcData, GcObject};
use crate::vm::value::{TAG_FALSE, TAG_NULL, TAG_TRUE};
use crate::vm::value::Value;

pub type JitFn = unsafe extern "C" fn(
    vm: *mut VM,
    frame_slots: *mut Value,
    constants_ptr: *const Value,
    ip: usize,
    ip_out: *mut usize,
    dest_reg_out: *mut usize,
    func_reg_out: *mut usize,
    arg_count_out: *mut usize,
    ret_val_out: *mut Value,
) -> i64;

pub struct CraneliftJitEngine {
    pub module: JITModule,
    pub ctx: Context,
    pub fn_builder_ctx: FunctionBuilderContext,
    pub helper_funcs: HashMap<String, FuncId>,
    pub func_counter: usize,
}

impl CraneliftJitEngine {
    pub fn new() -> anyhow::Result<Self> {
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed")?;
        flag_builder.set("is_pic", "false")?;
        #[cfg(not(debug_assertions))]
        {
            let _ = flag_builder.set("enable_verifier", "false");
        }
        let flags = Flags::new(flag_builder);

        let target_triple = Triple::host();
        let isa = isa::lookup(target_triple.clone())
            .map_err(|e| anyhow::anyhow!("Unsupported target architecture '{}': {}", target_triple, e))?
            .finish(flags)?;

        let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
        register_vm_helpers(&mut builder);

        let mut module = JITModule::new(builder);
        let ctx = module.make_context();
        let fn_builder_ctx = FunctionBuilderContext::new();

        let helper_funcs = declare_vm_helpers(&mut module)?;

        Ok(Self {
            module,
            ctx,
            fn_builder_ctx,
            helper_funcs,
            func_counter: 0,
        })
    }

    pub fn compile_function(&mut self, func: &Function) -> anyhow::Result<*const c_void> {
        let id = self.func_counter;
        self.func_counter += 1;
        let func_name = format!("clif_jit_fn_{}", id);

        let mut sig = self.module.make_signature();
        // 9 pointer/integer arguments matching JitFn
        for _ in 0..9 {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));

        let func_id = self.module.declare_function(&func_name, Linkage::Export, &sig)?;
        self.ctx.func.signature = sig;

        let num_regs = calculate_num_regs(func);
        let is_resume_target = calculate_resume_targets(func);

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.fn_builder_ctx);
            let mut helper_refs = HashMap::new();
            for (name, &fid) in &self.helper_funcs {
                let f_ref = self.module.declare_func_in_func(fid, builder.func);
                helper_refs.insert(name.as_str(), f_ref);
            }

            let call_helper = |b: &mut FunctionBuilder, name: &str, args: &[cranelift_codegen::ir::Value]| {
                let f_ref = helper_refs[name];
                let call_inst = b.ins().call(f_ref, args);
                let results = b.inst_results(call_inst);
                if !results.is_empty() {
                    results[0]
                } else {
                    b.ins().iconst(types::I64, 0)
                }
            };

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);

            let params = builder.block_params(entry_block).to_vec();
            let vm_val = params[0];
            let frame_slots_val = params[1];
            let constants_ptr_val = params[2];
            let start_ip_val = params[3];
            let ip_out_val = params[4];
            let dest_reg_out_val = params[5];
            let func_reg_out_val = params[6];
            let arg_count_out_val = params[7];
            let ret_val_out_val = params[8];

            let mut reg_vars = Vec::with_capacity(num_regs);
            for _ in 0..num_regs {
                reg_vars.push(builder.declare_var(types::I64));
            }

            // Load arguments into initial registers from frame_slots
            let null_const = builder.ins().iconst(types::I64, TAG_NULL as i64);
            for i in 0..num_regs {
                if i < func.arity {
                    let offset = (i * 8) as i32;
                    let arg_val = builder.ins().load(types::I64, cranelift_codegen::ir::MemFlagsData::new(), frame_slots_val, offset);
                    builder.def_var(reg_vars[i], arg_val);
                } else {
                    builder.def_var(reg_vars[i], null_const);
                }
            }

            // Create basic blocks for branch and jump targets
            let mut block_map: HashMap<usize, Block> = HashMap::new();
            block_map.insert(0, builder.create_block());

            for (pc, inst) in func.chunk.code.iter().enumerate() {
                match inst.op {
                    OpCode::Jump => {
                        let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                        block_map.entry(target).or_insert_with(|| builder.create_block());
                    }
                    OpCode::JumpIfFalse => {
                        let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                        block_map.entry(target).or_insert_with(|| builder.create_block());
                        block_map.entry(pc + 1).or_insert_with(|| builder.create_block());
                    }
                    OpCode::Loop => {
                        let target = (pc as i32 + 1 - inst.operand as i32) as usize;
                        block_map.entry(target).or_insert_with(|| builder.create_block());
                        block_map.entry(pc + 1).or_insert_with(|| builder.create_block());
                    }
                    _ => {}
                }
                if is_resume_target[pc] {
                    block_map.entry(pc).or_insert_with(|| builder.create_block());
                }
            }

            // Check resume targets for Start IP dispatch
            let any_resume = is_resume_target.iter().any(|&b| b);
            if any_resume {
                let zero = builder.ins().iconst(types::I64, 0);
                let is_zero = builder.ins().icmp(IntCC::Equal, start_ip_val, zero);
                let resume_block = builder.create_block();
                builder.ins().brif(is_zero, block_map[&0], &[], resume_block, &[]);

                builder.switch_to_block(resume_block);
                // Reload registers from frame_slots when resuming
                for i in 0..num_regs {
                    let offset = (i * 8) as i32;
                    let val = builder.ins().load(types::I64, cranelift_codegen::ir::MemFlagsData::new(), frame_slots_val, offset);
                    builder.def_var(reg_vars[i], val);
                }
                for (&target, &blk) in &block_map {
                    if target > 0 && target < is_resume_target.len() && is_resume_target[target] {
                        let t_const = builder.ins().iconst(types::I64, target as i64);
                        let is_t = builder.ins().icmp(IntCC::Equal, start_ip_val, t_const);
                        let next_check = builder.create_block();
                        builder.ins().brif(is_t, blk, &[], next_check, &[]);
                        builder.switch_to_block(next_check);
                    }
                }
                builder.ins().jump(block_map[&0], &[]);
            } else {
                builder.ins().jump(block_map[&0], &[]);
            }

            let mut terminated = true;

            for (pc, inst) in func.chunk.code.iter().enumerate() {
                if let Some(&block) = block_map.get(&pc) {
                    if !terminated {
                        builder.ins().jump(block, &[]);
                    }
                    builder.switch_to_block(block);
                    terminated = false;
                }

                if terminated {
                    continue;
                }

                let ra = inst.ra as usize;
                let rb = inst.rb as usize;
                let rc = inst.rc as usize;
                let operand = inst.operand as usize;

                match inst.op {
                    OpCode::LoadConst => {
                        let offset = (operand * 8) as i32;
                        let v = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, offset);
                        builder.def_var(reg_vars[ra], v);
                    }
                    OpCode::LoadNull => {
                        let v = builder.ins().iconst(types::I64, TAG_NULL as i64);
                        builder.def_var(reg_vars[ra], v);
                    }
                    OpCode::LoadBool => {
                        let tag = if inst.rb != 0 { TAG_TRUE as i64 } else { TAG_FALSE as i64 };
                        let v = builder.ins().iconst(types::I64, tag);
                        builder.def_var(reg_vars[ra], v);
                    }
                    OpCode::Move => {
                        let v = builder.use_var(reg_vars[rb]);
                        builder.def_var(reg_vars[ra], v);
                    }
                    OpCode::Add => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_add", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::Sub => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_sub", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::Mul => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_mul", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::Div => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_div", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::Mod => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_mod", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::Negate => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let res = call_helper(&mut builder, "er_jit_negate", &[vm_val, vb]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::Not => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let res = call_helper(&mut builder, "er_jit_not", &[vm_val, vb]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::Equal => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_equal", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::Greater => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_greater", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::Less => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_less", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::BitAnd => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_bit_and", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::BitOr => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_bit_or", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::BitXor => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_bit_xor", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::BitNot => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let res = call_helper(&mut builder, "er_jit_bit_not", &[vm_val, vb]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::ShiftLeft => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_shift_left", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::ShiftRight => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let vc = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_shift_right", &[vm_val, vb, vc]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::TypeOf => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let res = call_helper(&mut builder, "er_jit_typeof", &[vm_val, vb]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::ToIter => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let res = call_helper(&mut builder, "er_jit_to_iter", &[vm_val, vb]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::ArrayLen => {
                        let vb = builder.use_var(reg_vars[rb]);
                        let res = call_helper(&mut builder, "er_jit_array_len_op", &[vm_val, vb]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::DefineGlobal => {
                        let offset = (operand * 8) as i32;
                        let name_val = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, offset);
                        let va = builder.use_var(reg_vars[ra]);
                        call_helper(&mut builder, "er_jit_define_global", &[vm_val, name_val, va]);
                    }
                    OpCode::GetGlobal => {
                        let offset = (operand * 8) as i32;
                        let name_val = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, offset);
                        let res = call_helper(&mut builder, "er_jit_get_global", &[vm_val, name_val]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::SetGlobal => {
                        let offset = (operand * 8) as i32;
                        let name_val = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, offset);
                        let va = builder.use_var(reg_vars[ra]);
                        call_helper(&mut builder, "er_jit_set_global", &[vm_val, va, name_val]);
                    }
                    OpCode::MakeArray => {
                        let count = inst.operand as usize;
                        for i in 0..count {
                            let v = builder.use_var(reg_vars[rb + i]);
                            builder.ins().store(MemFlags::new(), v, frame_slots_val, ((rb + i) * 8) as i32);
                        }
                        let start_offset = builder.ins().iconst(types::I64, (rb * 8) as i64);
                        let start_ptr = builder.ins().iadd(frame_slots_val, start_offset);
                        let count_const = builder.ins().iconst(types::I64, count as i64);
                        let res = call_helper(&mut builder, "er_jit_make_array", &[vm_val, start_ptr, count_const]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::MakeObject => {
                        let count = inst.operand as usize;
                        for i in 0..(2 * count) {
                            let v = builder.use_var(reg_vars[rb + i]);
                            builder.ins().store(MemFlags::new(), v, frame_slots_val, ((rb + i) * 8) as i32);
                        }
                        let start_offset = builder.ins().iconst(types::I64, (rb * 8) as i64);
                        let start_ptr = builder.ins().iadd(frame_slots_val, start_offset);
                        let count_const = builder.ins().iconst(types::I64, count as i64);
                        let res = call_helper(&mut builder, "er_jit_make_object", &[vm_val, start_ptr, count_const]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::GetProperty => {
                        let offset = (operand * 8) as i32;
                        let name_val = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, offset);
                        let obj = builder.use_var(reg_vars[rb]);
                        let res = call_helper(&mut builder, "er_jit_get_property", &[vm_val, obj, name_val]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::SetProperty => {
                        let offset = (operand * 8) as i32;
                        let name_val = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, offset);
                        let obj = builder.use_var(reg_vars[ra]);
                        let val = builder.use_var(reg_vars[rb]);
                        call_helper(&mut builder, "er_jit_set_property", &[vm_val, obj, val, name_val]);
                    }
                    OpCode::GetIndex => {
                        let obj = builder.use_var(reg_vars[rb]);
                        let idx = builder.use_var(reg_vars[rc]);
                        let res = call_helper(&mut builder, "er_jit_get_index", &[vm_val, obj, idx]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::SetIndex => {
                        let obj = builder.use_var(reg_vars[ra]);
                        let idx = builder.use_var(reg_vars[rb]);
                        let val = builder.use_var(reg_vars[rc]);
                        call_helper(&mut builder, "er_jit_set_index", &[vm_val, obj, idx, val]);
                    }
                    OpCode::DefineStruct => {
                        let name_val = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, (operand * 8) as i32);
                        let fields_val = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, (ra * 8) as i32);
                        let methods_val = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, (rb * 8) as i32);
                        call_helper(&mut builder, "er_jit_define_struct", &[vm_val, name_val, fields_val, methods_val]);
                    }
                    OpCode::GetUpvalue => {
                        let idx_val = builder.ins().iconst(types::I64, rb as i64);
                        let res = call_helper(&mut builder, "er_jit_get_upvalue", &[vm_val, idx_val]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::SetUpvalue => {
                        let idx_val = builder.ins().iconst(types::I64, ra as i64);
                        let val = builder.use_var(reg_vars[rb]);
                        call_helper(&mut builder, "er_jit_set_upvalue", &[vm_val, idx_val, val]);
                    }
                    OpCode::Closure => {
                        let offset = (operand * 8) as i32;
                        let raw_fn_val = builder.ins().load(types::I64, MemFlags::new(), constants_ptr_val, offset);
                        let res = call_helper(&mut builder, "er_jit_make_closure", &[vm_val, raw_fn_val]);
                        builder.def_var(reg_vars[ra], res);
                    }
                    OpCode::CloseUpvalue => {
                        let slot_val = builder.ins().iconst(types::I64, ra as i64);
                        call_helper(&mut builder, "er_jit_close_upvalues", &[vm_val, slot_val]);
                    }
                    OpCode::Jump => {
                        let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                        builder.ins().jump(block_map[&target], &[]);
                        terminated = true;
                    }
                    OpCode::JumpIfFalse => {
                        let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                        let cond = builder.use_var(reg_vars[ra]);
                        let false_tag = builder.ins().iconst(types::I64, TAG_FALSE as i64);
                        let null_tag = builder.ins().iconst(types::I64, TAG_NULL as i64);
                        let is_false = builder.ins().icmp(IntCC::Equal, cond, false_tag);
                        let is_null = builder.ins().icmp(IntCC::Equal, cond, null_tag);
                        let is_falsey = builder.ins().bor(is_false, is_null);
                        builder.ins().brif(is_falsey, block_map[&target], &[], block_map[&(pc + 1)], &[]);
                        terminated = true;
                    }
                    OpCode::Loop => {
                        let target = (pc as i32 + 1 - inst.operand as i32) as usize;
                        builder.ins().jump(block_map[&target], &[]);
                        terminated = true;
                    }
                    OpCode::Call => {
                        // 1. Flush registers back to frame_slots
                        for i in 0..num_regs {
                            let v = builder.use_var(reg_vars[i]);
                            builder.ins().store(MemFlags::new(), v, frame_slots_val, (i * 8) as i32);
                        }
                        // 2. Set yield metadata
                        let ip_c = builder.ins().iconst(types::I64, pc as i64);
                        builder.ins().store(MemFlags::new(), ip_c, ip_out_val, 0);
                        let dest_c = builder.ins().iconst(types::I64, ra as i64);
                        builder.ins().store(MemFlags::new(), dest_c, dest_reg_out_val, 0);
                        let func_c = builder.ins().iconst(types::I64, rb as i64);
                        builder.ins().store(MemFlags::new(), func_c, func_reg_out_val, 0);
                        let args_c = builder.ins().iconst(types::I64, inst.operand as i64);
                        builder.ins().store(MemFlags::new(), args_c, arg_count_out_val, 0);
                        // 3. Return status 0 (YieldCall)
                        let ret_status = builder.ins().iconst(types::I64, 0);
                        builder.ins().return_(&[ret_status]);
                        terminated = true;
                    }
                    OpCode::Return => {
                        let ret_val = builder.use_var(reg_vars[ra]);
                        builder.ins().store(MemFlags::new(), ret_val, ret_val_out_val, 0);
                        // Return status 1 (YieldReturn)
                        let ret_status = builder.ins().iconst(types::I64, 1);
                        builder.ins().return_(&[ret_status]);
                        terminated = true;
                    }
                    OpCode::Await => {
                        for i in 0..num_regs {
                            let v = builder.use_var(reg_vars[i]);
                            builder.ins().store(MemFlags::new(), v, frame_slots_val, (i * 8) as i32);
                        }
                        let ip_c = builder.ins().iconst(types::I64, pc as i64);
                        builder.ins().store(MemFlags::new(), ip_c, ip_out_val, 0);
                        let func_c = builder.ins().iconst(types::I64, ra as i64);
                        builder.ins().store(MemFlags::new(), func_c, func_reg_out_val, 0);
                        // Return status 3 (YieldSuspend)
                        let ret_status = builder.ins().iconst(types::I64, 3);
                        builder.ins().return_(&[ret_status]);
                        terminated = true;
                    }
                    OpCode::Throw => {
                        let ret_status = builder.ins().iconst(types::I64, -1);
                        builder.ins().return_(&[ret_status]);
                        terminated = true;
                    }
                }
            }

            if !terminated {
                let null_val = builder.ins().iconst(types::I64, TAG_NULL as i64);
                builder.ins().store(MemFlags::new(), null_val, ret_val_out_val, 0);
                let ret_status = builder.ins().iconst(types::I64, 1);
                builder.ins().return_(&[ret_status]);
            }

            builder.seal_all_blocks();
            builder.finalize(self.module.isa().frontend_config());
        }

        self.module.define_function(func_id, &mut self.ctx)?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions()?;

        let code_ptr = self.module.get_finalized_function(func_id);
        Ok(code_ptr as *const c_void)
    }
}

type MemFlags = cranelift_codegen::ir::MemFlagsData;

fn declare_vm_helpers(module: &mut JITModule) -> anyhow::Result<HashMap<String, FuncId>> {
    let mut map = HashMap::new();

    let helpers_3_args = [
        "er_jit_add", "er_jit_sub", "er_jit_mul", "er_jit_div", "er_jit_mod",
        "er_jit_bit_and", "er_jit_bit_or", "er_jit_bit_xor", "er_jit_shift_left", "er_jit_shift_right",
        "er_jit_equal", "er_jit_greater", "er_jit_less", "er_jit_define_global", "er_jit_set_global",
        "er_jit_make_array", "er_jit_make_object", "er_jit_get_property", "er_jit_get_index", "er_jit_set_upvalue",
    ];

    for name in helpers_3_args {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        let fid = module.declare_function(name, Linkage::Import, &sig)?;
        map.insert(name.to_string(), fid);
    }

    let helpers_2_args = [
        "er_jit_negate", "er_jit_not", "er_jit_bit_not", "er_jit_typeof", "er_jit_to_iter",
        "er_jit_array_len_op", "er_jit_get_global", "er_jit_get_upvalue",
        "er_jit_make_closure", "er_jit_close_upvalues",
    ];

    for name in helpers_2_args {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        let fid = module.declare_function(name, Linkage::Import, &sig)?;
        map.insert(name.to_string(), fid);
    }

    let helpers_4_args = [
        "er_jit_set_property", "er_jit_set_index", "er_jit_define_struct",
    ];

    for name in helpers_4_args {
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        let fid = module.declare_function(name, Linkage::Import, &sig)?;
        map.insert(name.to_string(), fid);
    }

    Ok(map)
}

fn register_vm_helpers(builder: &mut JITBuilder) {
    use crate::jit::helpers;

    let helpers: &[(&str, *const u8)] = &[
        ("er_jit_negate", helpers::er_jit_negate as *const u8),
        ("er_jit_not", helpers::er_jit_not as *const u8),
        ("er_jit_add", helpers::er_jit_add as *const u8),
        ("er_jit_sub", helpers::er_jit_sub as *const u8),
        ("er_jit_mul", helpers::er_jit_mul as *const u8),
        ("er_jit_div", helpers::er_jit_div as *const u8),
        ("er_jit_mod", helpers::er_jit_mod as *const u8),
        ("er_jit_bit_and", helpers::er_jit_bit_and as *const u8),
        ("er_jit_bit_or", helpers::er_jit_bit_or as *const u8),
        ("er_jit_bit_xor", helpers::er_jit_bit_xor as *const u8),
        ("er_jit_bit_not", helpers::er_jit_bit_not as *const u8),
        ("er_jit_shift_left", helpers::er_jit_shift_left as *const u8),
        ("er_jit_shift_right", helpers::er_jit_shift_right as *const u8),
        ("er_jit_typeof", helpers::er_jit_typeof as *const u8),
        ("er_jit_to_iter", helpers::er_jit_to_iter as *const u8),
        ("er_jit_array_len_op", helpers::er_jit_array_len_op as *const u8),
        ("er_jit_equal", helpers::er_jit_equal as *const u8),
        ("er_jit_greater", helpers::er_jit_greater as *const u8),
        ("er_jit_less", helpers::er_jit_less as *const u8),
        ("er_jit_define_global", helpers::er_jit_define_global as *const u8),
        ("er_jit_get_global", helpers::er_jit_get_global as *const u8),
        ("er_jit_set_global", helpers::er_jit_set_global as *const u8),
        ("er_jit_make_array", helpers::er_jit_make_array as *const u8),
        ("er_jit_make_object", helpers::er_jit_make_object as *const u8),
        ("er_jit_get_property", helpers::er_jit_get_property as *const u8),
        ("er_jit_set_property", helpers::er_jit_set_property as *const u8),
        ("er_jit_get_index", helpers::er_jit_get_index as *const u8),
        ("er_jit_set_index", helpers::er_jit_set_index as *const u8),
        ("er_jit_call_fast", helpers::er_jit_call_fast as *const u8),
        ("er_jit_call_non_vm", helpers::er_jit_call_non_vm as *const u8),
        ("er_jit_array_push", helpers::er_jit_array_push as *const u8),
        ("er_jit_array_pop", helpers::er_jit_array_pop as *const u8),
        ("er_jit_has_error", helpers::er_jit_has_error as *const u8),
        ("er_jit_needs_gc", helpers::er_jit_needs_gc as *const u8),
        ("er_jit_define_struct", helpers::er_jit_define_struct as *const u8),
        ("er_jit_get_upvalue", helpers::er_jit_get_upvalue as *const u8),
        ("er_jit_set_upvalue", helpers::er_jit_set_upvalue as *const u8),
        ("er_jit_make_closure", helpers::er_jit_make_closure as *const u8),
        ("er_jit_close_upvalues", helpers::er_jit_close_upvalues as *const u8),
        ("er_jit_await", helpers::er_jit_await as *const u8),
        ("er_jit_write_barrier", helpers::er_jit_write_barrier as *const u8),
    ];

    for &(name, ptr) in helpers {
        builder.symbol(name, ptr);
    }
}

fn calculate_num_regs(func: &Function) -> usize {
    let mut max_reg = 0;
    for inst in &func.chunk.code {
        max_reg = max_reg.max(inst.ra as usize).max(inst.rb as usize).max(inst.rc as usize);
        if inst.op == OpCode::Call || inst.op == OpCode::MakeArray {
            max_reg = max_reg.max(inst.rb as usize + inst.operand as usize);
        } else if inst.op == OpCode::MakeObject {
            max_reg = max_reg.max(inst.rb as usize + (inst.operand as usize * 2));
        }
    }
    (max_reg + 1).max(func.arity).max(1)
}

fn calculate_resume_targets(func: &Function) -> Vec<bool> {
    let mut targets = vec![false; func.chunk.code.len()];
    for (pc, inst) in func.chunk.code.iter().enumerate() {
        if matches!(inst.op, OpCode::Call | OpCode::Await) {
            if pc + 1 < targets.len() {
                targets[pc + 1] = true;
            }
        }
    }
    targets
}

thread_local! {
    static CLIF_JIT_ENGINE: RefCell<Option<CraneliftJitEngine>> = const { RefCell::new(None) };
    static CLIF_JIT_CACHE: RefCell<HashMap<Vec<crate::vm::bytecode::Instruction>, *const c_void>> = RefCell::new(HashMap::new());
}

pub fn get_or_init_clif_jit() -> *mut c_void {
    CLIF_JIT_ENGINE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            *borrow = Some(CraneliftJitEngine::new().expect("Failed to initialize Cranelift JIT engine"));
        }
        borrow.as_mut().unwrap() as *mut CraneliftJitEngine as *mut c_void
    })
}

pub fn clif_compile_function(_vm: &mut VM, func_obj: *mut GcObject) -> *const c_void {
    let func = match unsafe { &(*func_obj).data } {
        GcData::Function(f) => f,
        _ => panic!("Expected function object"),
    };

    if let Some(ptr) = func.jit_ptr.get() {
        return ptr;
    }

    let code = &func.chunk.code;
    let cached = CLIF_JIT_CACHE.with(|cache| cache.borrow().get(code).copied());
    if let Some(ptr) = cached {
        func.jit_ptr.set(Some(ptr));
        return ptr;
    }

    CLIF_JIT_ENGINE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            *borrow = Some(CraneliftJitEngine::new().expect("Failed to initialize Cranelift JIT engine"));
        }
        let engine = borrow.as_mut().unwrap();
        let code_ptr = engine.compile_function(func).expect("Failed to compile function with Cranelift JIT");
        func.jit_ptr.set(Some(code_ptr));
        CLIF_JIT_CACHE.with(|cache| cache.borrow_mut().insert(code.clone(), code_ptr));
        code_ptr
    })
}

pub fn clif_reset_jit_state() {
    CLIF_JIT_CACHE.with(|cache| cache.borrow_mut().clear());
    CLIF_JIT_ENGINE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

pub unsafe fn clif_cleanup_jit(_ptr: *mut c_void) {
    clif_reset_jit_state();
}
