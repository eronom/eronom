use crate::vm::bytecode::{Function, OpCode};

pub fn emit_prologue(
    mir: &mut String,
    module_name: &str,
    func_name: &str,
    func: &Function,
    num_regs: usize,
    param_is_double: &[bool],
    is_resume_target: &[bool],
) {
    mir.push_str(&format!("{}: module\n", module_name));
    mir.push_str(&format!("          export {}\n", func_name));
    mir.push_str("          import er_jit_negate, er_jit_not, er_jit_add, er_jit_sub, er_jit_mul, er_jit_div, er_jit_mod, er_jit_bit_and, er_jit_bit_or, er_jit_bit_xor, er_jit_bit_not, er_jit_shift_left, er_jit_shift_right, er_jit_typeof, er_jit_to_iter, er_jit_array_len_op, er_jit_equal, er_jit_greater, er_jit_less, er_jit_define_global, er_jit_get_global, er_jit_set_global, er_jit_make_array, er_jit_make_object, er_jit_get_property, er_jit_set_property, er_jit_get_index, er_jit_set_index, er_jit_call_fast, er_jit_call_non_vm, er_jit_array_push, er_jit_array_pop, er_jit_has_error, er_jit_needs_gc, er_gc_needs_step, er_jit_define_struct, er_jit_get_upvalue, er_jit_set_upvalue, er_jit_make_closure, er_jit_close_upvalues, er_jit_await, er_jit_write_barrier\n");

    mir.push_str(&format!(
        "{}: func i64, p:vm, p:frame_slots, p:constants_ptr, i64:start_ip, p:ip_out, p:dest_reg_out, p:func_reg_out, p:arg_count_out, p:ret_val_out\n",
        func_name
    ));

    mir.push_str("p_needs_gc: proto i64\n");
    mir.push_str("p_negate: proto i64, p:vm, i64:src\n");
    mir.push_str("p_not: proto i64, i64:src\n");
    mir.push_str("p_add: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_sub: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_mul: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_div: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_mod: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_bit_and: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_bit_or: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_bit_xor: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_bit_not: proto i64, p:vm, i64:src\n");
    mir.push_str("p_shift_left: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_shift_right: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_typeof: proto i64, p:vm, i64:src\n");
    mir.push_str("p_to_iter: proto i64, p:vm, i64:src\n");
    mir.push_str("p_array_len_op: proto i64, p:vm, i64:src\n");
    mir.push_str("p_equal: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_greater: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_less: proto i64, p:vm, i64:b, i64:c\n");
    mir.push_str("p_def_global: proto i64, p:vm, i64:name, i64:val\n");
    mir.push_str("p_get_global: proto i64, p:vm, i64:name\n");
    mir.push_str("p_set_global: proto i64, p:vm, i64:val, i64:name\n");
    mir.push_str("p_make_array: proto i64, p:vm, p:start, i64:count\n");
    mir.push_str("p_make_object: proto i64, p:vm, p:start, i64:count\n");
    mir.push_str("p_get_property: proto i64, p:vm, i64:obj, i64:name\n");
    mir.push_str("p_set_property: proto i64, p:vm, i64:obj, i64:val, i64:name\n");
    mir.push_str("p_get_index: proto i64, p:vm, i64:obj, i64:idx\n");
    mir.push_str("p_set_index: proto i64, p:vm, i64:obj, i64:idx, i64:val\n");
    mir.push_str("p_call_fast: proto i64, p:vm, i64:callee, p:callee_slots, p:dest, i64:inst_idx, i64:dest_reg\n");
    mir.push_str("p_call_non_vm: proto i64, p:vm, p:dest, i64:callee, i64:func_reg, i64:arg_count, p:frame_slots, i64:inst_idx, i64:dest_reg\n");
    mir.push_str("p_array_push: proto i64, i64:arr, i64:arg\n");
    mir.push_str("p_array_pop: proto i64, i64:arr\n");
    mir.push_str("p_has_error: proto i64, p:vm\n");
    mir.push_str("p_def_struct: proto i64, p:vm, i64:name, i64:fields, i64:methods\n");
    mir.push_str("p_get_upvalue: proto i64, p:vm, i64:idx\n");
    mir.push_str("p_set_upvalue: proto i64, p:vm, i64:idx, i64:val\n");
    mir.push_str("p_make_closure: proto i64, p:vm, i64:raw_fn\n");
    mir.push_str("p_close_upvalues: proto i64, p:vm, i64:slot\n");
    mir.push_str("p_await: proto i64, p:vm, i64:await_val, p:dest\n");
    mir.push_str("p_write_barrier: proto i64, p:parent, i64:child\n");
    mir.push_str("p_jit_fn: proto i64, p:vm, p:frame_slots, p:constants_ptr, i64:start_ip, p:ip_out, p:dest_reg_out, p:func_reg_out, p:arg_count_out, p:ret_val_out\n");

    mir.push_str("          local i64:tmp, i64:tmp1, i64:tmp2, i64:tmp3, i64:status, i64:res_bool, i64:res_val, i64:cast_ptr, i64:loop_counter\n");
    mir.push_str("          local i64:ra_ptr, i64:rb_ptr, i64:rc_ptr, i64:name_ptr, i64:val_ptr, i64:start_ptr, i64:dest_ptr, i64:idx_ptr, i64:obj_ptr, i64:desc_ptr\n");
    mir.push_str("          local d:da, d:db, d:dres\n");

    if num_regs > 0 {
        let mut local_regs = String::new();
        for i in 0..num_regs {
            if i > 0 {
                local_regs.push_str(", ");
            }
            local_regs.push_str(&format!("i64:r{}, d:d{}", i, i));
        }
        mir.push_str(&format!("          local {}\n", local_regs));
    }

    mir.push_str("          alloca cast_ptr, 192\n");
    mir.push_str("          mov loop_counter, 0\n");
    mir.push_str("          bne resume_dispatch, start_ip, 0\n");
    for i in 0..func.arity.min(num_regs) {
        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", i, i * 8));
        if param_is_double[i] {
            let offset = (i % 24) * 8;
            mir.push_str(&format!("          ubge deopt_entry, r{}, 0xffe8000000000000\n", i));
            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset, i));
            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", i, offset));
        }
    }
    mir.push_str("          jmp entry_0\n");
    mir.push_str("deopt_entry:\n");
    mir.push_str("          mov i64:(ip_out), 0\n");
    mir.push_str("          ret 4\n");

    mir.push_str("resume_dispatch:\n");
    for i in 0..num_regs {
        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", i, i * 8));
    }
    for ip_target in 1..func.chunk.code.len() {
        if is_resume_target[ip_target] {
            mir.push_str(&format!("          beq entry_{}, start_ip, {}\n", ip_target, ip_target));
        }
    }
    mir.push_str("          jmp err_label\n");
}

pub fn calculate_max_regs(func: &Function) -> usize {
    let mut num_regs = func.arity;
    for inst in &func.chunk.code {
        let max_accessed = match inst.op {
            OpCode::MakeArray => inst.rb as usize + inst.operand as usize,
            OpCode::MakeObject => inst.rb as usize + 2 * (inst.operand as usize),
            OpCode::Call => inst.rb as usize + 1 + inst.operand as usize,
            OpCode::DefineStruct | OpCode::CloseUpvalue => 0,
            OpCode::GetUpvalue | OpCode::Closure | OpCode::SetUpvalue => inst.ra as usize + 1,
            OpCode::Await => {
                let m = (inst.ra as usize).max(inst.rb as usize);
                m + 1
            }
            _ => {
                let mut m = inst.ra as usize;
                if inst.rb as usize > m { m = inst.rb as usize; }
                if inst.rc as usize > m { m = inst.rc as usize; }
                m + 1
            }
        };
        if max_accessed > num_regs {
            num_regs = max_accessed;
        }
    }
    num_regs
}

pub fn calculate_param_doubles(func: &Function) -> Vec<bool> {
    let mut param_is_double = vec![false; func.arity];
    for p in 0..func.arity {
        let mut used_as_non_double = false;
        for inst in &func.chunk.code {
            let ra = inst.ra as usize;
            let rb = inst.rb as usize;
            match inst.op {
                OpCode::Call if rb == p => { used_as_non_double = true; }
                OpCode::GetProperty if rb == p => { used_as_non_double = true; }
                OpCode::SetProperty if ra == p => { used_as_non_double = true; }
                OpCode::GetIndex if rb == p => { used_as_non_double = true; }
                OpCode::SetIndex if ra == p => { used_as_non_double = true; }
                OpCode::ToIter if rb == p => { used_as_non_double = true; }
                OpCode::ArrayLen if rb == p => { used_as_non_double = true; }
                _ => {}
            }
        }
        if !used_as_non_double {
            param_is_double[p] = true;
        }
    }
    param_is_double
}

pub fn calculate_resume_targets(func: &Function) -> Vec<bool> {
    let mut is_resume_target = vec![false; func.chunk.code.len()];
    is_resume_target[0] = true;
    for (i, inst) in func.chunk.code.iter().enumerate() {
        if inst.op == OpCode::Loop {
            let target = (i as i32 + 1 - inst.operand as i32) as usize;
            if target < is_resume_target.len() {
                is_resume_target[target] = true;
            }
        } else if inst.op == OpCode::Call || inst.op == OpCode::Await {
            if i + 1 < is_resume_target.len() {
                is_resume_target[i + 1] = true;
            }
        }
    }
    for handler in &func.chunk.handlers {
        if handler.catch_ip < is_resume_target.len() {
            is_resume_target[handler.catch_ip] = true;
        }
        if let Some(finally_ip) = handler.finally_ip {
            if finally_ip < is_resume_target.len() {
                is_resume_target[finally_ip] = true;
            }
        }
    }
    is_resume_target
}
