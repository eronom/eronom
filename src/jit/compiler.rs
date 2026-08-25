use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicUsize, Ordering};
use fnv::FnvHashMap;
use crate::vm::value::{Value, TAG_FALSE, TAG_NULL, TAG_TRUE};
use crate::vm::bytecode::{OpCode, Instruction};
use crate::vm::execute::VM;
use crate::vm::gc::{GcData, GcObject};
use super::bindings::{
    _MIR_init, MIR_finish, MIR_scan_string, MIR_load_module, MIR_load_external,
    MIR_link, MIR_gen_init, MIR_gen, MIR_gen_finish, MIR_get_module_list,
    MIR_set_gen_interface, MIR_gen_set_optimize_level, MirDlist, MirModule,
};
use super::helpers;

static JIT_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn next_id() -> usize {
    JIT_COUNTER.fetch_add(1, Ordering::Relaxed)
}

struct ThreadJitState {
    ctx: *mut c_void,
    cache: FnvHashMap<Vec<Instruction>, *const c_void>,
}

impl Drop for ThreadJitState {
    fn drop(&mut self) {
        unsafe {
            MIR_gen_finish(self.ctx);
            MIR_finish(self.ctx);
        }
    }
}

thread_local! {
    static JIT_STATE: std::cell::RefCell<Option<ThreadJitState>> = std::cell::RefCell::new(None);
}

// Ensure the JIT is initialized for the VM context
pub fn get_or_init_jit_ctx(_vm: &mut VM) -> *mut c_void {
    JIT_STATE.with(|state| {
        let mut borrow = state.borrow_mut();
        if borrow.is_none() {
            unsafe {
                let ctx = _MIR_init(std::ptr::null_mut(), std::ptr::null_mut());
                MIR_gen_init(ctx);
                MIR_gen_set_optimize_level(ctx, 3);
                register_helpers(ctx);
                *borrow = Some(ThreadJitState {
                    ctx,
                    cache: FnvHashMap::default(),
                });
            }
        }
        borrow.as_ref().unwrap().ctx
    })
}

// Safely free all MIR native code buffers and clear the JIT cache on hot-reloads
pub fn reset_jit_state() {
    JIT_STATE.with(|state| {
        *state.borrow_mut() = None;
    });
    crate::jit::helpers::reset_global_ic();
}


pub fn compile_function(vm: &mut VM, func_obj: *mut GcObject) -> *const c_void {
    let func = match unsafe { &(*func_obj).data } {
        GcData::Function(f) => f,
        _ => panic!("Expected function object"),
    };

    if let Some(ptr) = func.jit_ptr.get() {
        return ptr;
    }

    let instructions = &func.chunk.code;
    let cached_ptr = JIT_STATE.with(|state| {
        let borrow = state.borrow();
        if let Some(s) = borrow.as_ref() {
            s.cache.get(instructions).copied()
        } else {
            None
        }
    });

    if let Some(ptr) = cached_ptr {
        func.jit_ptr.set(Some(ptr));
        return ptr;
    }

    let ctx = get_or_init_jit_ctx(vm);
    let id = next_id();
    let module_name = format!("m_{}", id);
    let func_name = format!("jit_fn_{}", id);

    let mut mir = String::new();
    mir.push_str(&format!("{}: module\n", module_name));
    mir.push_str(&format!("          export {}\n", func_name));
    mir.push_str("          import er_jit_negate, er_jit_not, er_jit_add, er_jit_sub, er_jit_mul, er_jit_div, er_jit_mod, er_jit_bit_and, er_jit_bit_or, er_jit_bit_xor, er_jit_bit_not, er_jit_shift_left, er_jit_shift_right, er_jit_typeof, er_jit_to_iter, er_jit_array_len_op, er_jit_equal, er_jit_greater, er_jit_less, er_jit_define_global, er_jit_get_global, er_jit_set_global, er_jit_make_array, er_jit_make_object, er_jit_get_property, er_jit_set_property, er_jit_get_index, er_jit_set_index, er_jit_call_fast, er_jit_call_non_vm, er_jit_array_push, er_jit_array_pop, er_jit_has_error, er_jit_needs_gc, er_gc_needs_step, er_jit_define_struct, er_jit_get_upvalue, er_jit_set_upvalue, er_jit_make_closure, er_jit_close_upvalues, er_jit_await, er_jit_write_barrier\n");

    // Signature: returns status code (i64), arguments are pointers to vm, frame_slots, constants, etc.
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
    }
    mir.push_str("          jmp entry_0\n");
    mir.push_str("resume_dispatch:\n");
    for i in 0..num_regs {
        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", i, i * 8));
    }
    for ip_target in 1..func.chunk.code.len() {
        mir.push_str(&format!("          beq entry_{}, start_ip, {}\n", ip_target, ip_target));
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum RegType {
        Unknown,
        Double,
    }

    let debug_jit = std::env::var("ER_DEBUG_JIT").is_ok();
    let mut types_at_inst = vec![vec![RegType::Double; num_regs]; func.chunk.code.len()];
    if num_regs > 0 {
        for r in 0..num_regs {
            types_at_inst[0][r] = RegType::Unknown;
        }
    }

    let (is_dead, live_in, _live_out) = eliminate_dead_instructions(func, num_regs);

    let mut worklist = vec![0];
    let mut in_worklist = vec![false; func.chunk.code.len()];
    let mut visited = vec![false; func.chunk.code.len()];
    if !func.chunk.code.is_empty() {
        in_worklist[0] = true;
    }

    while let Some(pc) = worklist.pop() {
        in_worklist[pc] = false;
        visited[pc] = true;
        let current_types = &types_at_inst[pc];
        let inst = &func.chunk.code[pc];

        let mut next_types = current_types.clone();
        let ra = inst.ra as usize;
        let rb = inst.rb as usize;
        let rc = inst.rc as usize;

        if debug_jit {
            println!("  [PROP START] pc={} op={:?} ra={} rb={} rc={} current_types={:?}", pc, inst.op, ra, rb, rc, current_types);
        }

        match inst.op {
            OpCode::LoadConst => {
                let val = func.chunk.constants[inst.operand as usize];
                if ra < num_regs {
                    next_types[ra] = if val.is_number() { RegType::Double } else { RegType::Unknown };
                }
            }
            OpCode::Move => {
                if ra < num_regs && rb < num_regs {
                    next_types[ra] = current_types[rb];
                }
            }
            OpCode::Add => {
                if ra < num_regs && rb < num_regs && rc < num_regs {
                    next_types[ra] = if current_types[rb] == RegType::Double && current_types[rc] == RegType::Double {
                        RegType::Double
                    } else {
                        RegType::Unknown
                    };
                }
            }
            OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Negate |
            OpCode::Mod | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::BitNot | OpCode::ShiftLeft | OpCode::ShiftRight | OpCode::ArrayLen => {
                if ra < num_regs {
                    next_types[ra] = RegType::Double;
                }
            }
            OpCode::Not | OpCode::Equal | OpCode::Greater | OpCode::Less |
            OpCode::LoadNull | OpCode::LoadBool |
            OpCode::GetGlobal | OpCode::GetProperty | OpCode::GetIndex |
            OpCode::MakeArray | OpCode::MakeObject | OpCode::TypeOf | OpCode::ToIter |
            OpCode::GetUpvalue | OpCode::Closure | OpCode::Await | OpCode::Call => {
                if ra < num_regs {
                    next_types[ra] = RegType::Unknown;
                }
            }
            _ => {}
        }

        let mut successors = Vec::new();
        match inst.op {
            OpCode::Return | OpCode::Throw => {}
            OpCode::Jump => {
                let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                successors.push(target);
            }
            OpCode::Loop => {
                let target = (pc as i32 + 1 - inst.operand as i32) as usize;
                successors.push(target);
            }
            OpCode::JumpIfFalse => {
                let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                successors.push(pc + 1);
                successors.push(target);
            }
            _ => {
                successors.push(pc + 1);
            }
        }

        for succ in successors {
            if succ >= func.chunk.code.len() {
                continue;
            }
            let mut changed = false;
            for r in 0..num_regs {
                let old = types_at_inst[succ][r];
                let new_t = if old == RegType::Double && next_types[r] == RegType::Double {
                    RegType::Double
                } else {
                    RegType::Unknown
                };
                if new_t != old {
                    if debug_jit {
                        println!("    [PROP UPDATE] succ={} reg={} old={:?} new={:?}", succ, r, old, new_t);
                    }
                    types_at_inst[succ][r] = new_t;
                    changed = true;
                }
            }
            if (changed || !visited[succ]) && !in_worklist[succ] {
                worklist.push(succ);
                in_worklist[succ] = true;
            }
        }
        if debug_jit {
            println!("  [PROP END] pc={} next_types={:?}", pc, next_types);
        }
    }

    if debug_jit {
        println!("DEBUG types_at_inst for function {:?}:", func.name);
        for (i, types) in types_at_inst.iter().enumerate() {
            println!("  inst_{}: {:?} (op={:?})", i, types, func.chunk.code[i].op);
        }
    }


    let sync_edge = |mir: &mut String, pc: usize, target: usize| {
        if target < types_at_inst.len() {
            for r in 0..num_regs {
                if live_in[target][r] {
                    if types_at_inst[pc][r] == RegType::Double && types_at_inst[target][r] == RegType::Unknown {
                        let offset = (r % 24) * 8;
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset, r));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", r, offset));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", r * 8, r));
                    }
                }
            }
        }
    };

    // Entry points conversion: from r{} to d{}
    for ip_target in 0..func.chunk.code.len() {
        mir.push_str(&format!("entry_{}:\n", ip_target));
        for r in 0..num_regs {
            if live_in[ip_target][r] && types_at_inst[ip_target][r] == RegType::Double {
                let offset = (r % 24) * 8;
                mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset, r));
                mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", r, offset));
            }
        }
        mir.push_str(&format!("          jmp inst_{}\n", ip_target));
    }


    let save_all_registers = |mir: &mut String, idx: usize| {
        for r in 0..num_regs {
            if types_at_inst[idx][r] == RegType::Double {
                mir.push_str(&format!("          dmov d:{}(frame_slots), d{}\n", r * 8, r));
            } else {
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", r * 8, r));
            }
        }
    };



    for (idx, instruction) in func.chunk.code.iter().enumerate() {
        mir.push_str(&format!("inst_{}:\n", idx));

        // Get types before this instruction and next instruction
        let mut next_types = types_at_inst[idx].clone();
        let ra = instruction.ra as usize;
        let rb = instruction.rb as usize;
        let rc = instruction.rc as usize;
        match instruction.op {
            OpCode::LoadConst => {
                let val = func.chunk.constants[instruction.operand as usize];
                if ra < num_regs {
                    next_types[ra] = if val.is_number() { RegType::Double } else { RegType::Unknown };
                }
            }
            OpCode::Move => {
                if ra < num_regs && rb < num_regs {
                    next_types[ra] = types_at_inst[idx][rb];
                }
            }
            OpCode::Add => {
                if ra < num_regs && rb < num_regs && rc < num_regs {
                    next_types[ra] = if types_at_inst[idx][rb] == RegType::Double && types_at_inst[idx][rc] == RegType::Double {
                        RegType::Double
                    } else {
                        RegType::Unknown
                    };
                }
            }
            OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Negate |
            OpCode::Mod | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::BitNot | OpCode::ShiftLeft | OpCode::ShiftRight | OpCode::ArrayLen => {
                if ra < num_regs {
                    next_types[ra] = RegType::Double;
                }
            }
            OpCode::Not | OpCode::Equal | OpCode::Greater | OpCode::Less |
            OpCode::LoadNull | OpCode::LoadBool |
            OpCode::GetGlobal | OpCode::GetProperty | OpCode::GetIndex |
            OpCode::MakeArray | OpCode::MakeObject | OpCode::TypeOf | OpCode::ToIter |
            OpCode::GetUpvalue | OpCode::Closure | OpCode::Await | OpCode::Call => {
                if ra < num_regs {
                    next_types[ra] = RegType::Unknown;
                }
            }
            _ => {}
        }

        if is_dead[idx] {
            continue;
        }

        match instruction.op {
            OpCode::LoadConst => {
                let val = func.chunk.constants[instruction.operand as usize];
                if val.is_number() {
                    mir.push_str(&format!("          dmov d{}, d:{}(constants_ptr)\n", ra, instruction.operand * 8));
                } else {
                    mir.push_str(&format!("          mov tmp, i64:{}(constants_ptr)\n", instruction.operand * 8));
                    mir.push_str(&format!("          mov r{}, tmp\n", ra));
                }
            }
            OpCode::LoadNull => {
                mir.push_str(&format!("          mov r{}, {}\n", ra, TAG_NULL));
            }
            OpCode::LoadBool => {
                let tag = if instruction.rb != 0 { Value::boolean(true).0 } else { Value::boolean(false).0 };
                mir.push_str(&format!("          mov r{}, {}\n", ra, tag));
            }
            OpCode::Move => {
                if types_at_inst[idx][rb] == RegType::Double {
                    mir.push_str(&format!("          dmov d{}, d{}\n", ra, rb));
                } else {
                    mir.push_str(&format!("          mov r{}, r{}\n", ra, rb));
                }
            }
            OpCode::Negate => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;

                if rb_is_double {
                    mir.push_str(&format!("          dneg d{}, d{}\n", ra, rb));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset1));
                    }
                } else {
                    mir.push_str(&format!("          ubge fallback_neg_{}, r{}, 0xffe8000000000000\n", idx, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                    mir.push_str(&format!("          dneg d{}, d{}\n", ra, rb));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset1));
                    }
                    mir.push_str(&format!("          jmp done_neg_{}\n", idx));

                    // Fallback
                    mir.push_str(&format!("fallback_neg_{}:\n", idx));
                    mir.push_str(&format!("          call p_negate, er_jit_negate, r{}, vm, r{}\n", ra, rb));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset2));
                    }
                    mir.push_str(&format!("done_neg_{}:\n", idx));
                }
            }
            OpCode::Not => {
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                if rb_is_double {
                    mir.push_str(&format!("          mov r{}, {}\n", ra, TAG_FALSE));
                } else {
                    mir.push_str(&format!("          mov tmp1, {}\n", TAG_FALSE));
                    mir.push_str(&format!("          beq done_not_{}, r{}, {}\n", idx, rb, TAG_TRUE));
                    mir.push_str(&format!("          beq set_true_{}, r{}, {}\n", idx, rb, TAG_FALSE));
                    mir.push_str(&format!("          beq set_true_{}, r{}, {}\n", idx, rb, TAG_NULL));
                    mir.push_str(&format!("          jmp done_not_{}\n", idx));
                    mir.push_str(&format!("set_true_{}:\n", idx));
                    mir.push_str(&format!("          mov tmp1, {}\n", TAG_TRUE));
                    mir.push_str(&format!("done_not_{}:\n", idx));
                    mir.push_str(&format!("          mov r{}, tmp1\n", ra));
                }
            }
            OpCode::Add => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;

                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;

                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          dadd d{}, d{}, d{}\n", ra, rb, rc));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                } else {
                    if !rb_is_double {
                        mir.push_str(&format!("          ubge fallback_add_{}, r{}, 0xffe8000000000000\n", idx, rb));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          ubge fallback_add_{}, r{}, 0xffe8000000000000\n", idx, rc));
                    }
                    if !rb_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          dadd d{}, d{}, d{}\n", ra, rb, rc));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                    mir.push_str(&format!("          jmp done_add_{}\n", idx));

                    // Fallback
                    mir.push_str(&format!("fallback_add_{}:\n", idx));
                    if rb_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                    }
                    if rc_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          call p_add, er_jit_add, r{}, vm, r{}, r{}\n", ra, rb, rc));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                    }
                    mir.push_str(&format!("done_add_{}:\n", idx));
                }
            }
            OpCode::Sub => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;

                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;

                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          dsub d{}, d{}, d{}\n", ra, rb, rc));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                } else {
                    if !rb_is_double {
                        mir.push_str(&format!("          ubge fallback_sub_{}, r{}, 0xffe8000000000000\n", idx, rb));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          ubge fallback_sub_{}, r{}, 0xffe8000000000000\n", idx, rc));
                    }
                    if !rb_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          dsub d{}, d{}, d{}\n", ra, rb, rc));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                    mir.push_str(&format!("          jmp done_sub_{}\n", idx));

                    // Fallback
                    mir.push_str(&format!("fallback_sub_{}:\n", idx));
                    if rb_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                    }
                    if rc_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          call p_sub, er_jit_sub, r{}, vm, r{}, r{}\n", ra, rb, rc));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                    }
                    mir.push_str(&format!("done_sub_{}:\n", idx));
                }
            }
            OpCode::Mul => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;

                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;

                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          dmul d{}, d{}, d{}\n", ra, rb, rc));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                } else {
                    if !rb_is_double {
                        mir.push_str(&format!("          ubge fallback_mul_{}, r{}, 0xffe8000000000000\n", idx, rb));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          ubge fallback_mul_{}, r{}, 0xffe8000000000000\n", idx, rc));
                    }
                    if !rb_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          dmul d{}, d{}, d{}\n", ra, rb, rc));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                    mir.push_str(&format!("          jmp done_mul_{}\n", idx));

                    // Fallback
                    mir.push_str(&format!("fallback_mul_{}:\n", idx));
                    if rb_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                    }
                    if rc_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          call p_mul, er_jit_mul, r{}, vm, r{}, r{}\n", ra, rb, rc));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                    }
                    mir.push_str(&format!("done_mul_{}:\n", idx));
                }
            }
            OpCode::Div => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;

                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;

                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          ddiv d{}, d{}, d{}\n", ra, rb, rc));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                } else {
                    if !rb_is_double {
                        mir.push_str(&format!("          ubge fallback_div_{}, r{}, 0xffe8000000000000\n", idx, rb));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          ubge fallback_div_{}, r{}, 0xffe8000000000000\n", idx, rc));
                    }
                    if !rb_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          ddiv d{}, d{}, d{}\n", ra, rb, rc));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                    mir.push_str(&format!("          jmp done_div_{}\n", idx));

                    // Fallback
                    mir.push_str(&format!("fallback_div_{}:\n", idx));
                    if rb_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                    }
                    if rc_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          call p_div, er_jit_div, r{}, vm, r{}, r{}\n", ra, rb, rc));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                    }
                    mir.push_str(&format!("done_div_{}:\n", idx));
                }
            }
            OpCode::Mod => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;
                if rb_is_double {
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                }
                if rc_is_double {
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                }
                mir.push_str(&format!("          call p_mod, er_jit_mod, r{}, vm, r{}, r{}\n", ra, rb, rc));
                mir.push_str("          mov status, u8:0(vm)\n");
                mir.push_str("          bne err_label, status, 0\n");
                if next_ra_is_double {
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                    mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                }
            }
            OpCode::BitAnd => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;
                if rb_is_double && rc_is_double {
                    // Fast path: both numeric — inline integer op
                    mir.push_str(&format!("          d2i tmp1, d{}\n", rb));
                    mir.push_str(&format!("          d2i tmp2, d{}\n", rc));
                    mir.push_str("          and tmp3, tmp1, tmp2\n");
                    mir.push_str(&format!("          i2d d{}, tmp3\n", ra));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                } else {
                    if rb_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                    }
                    if rc_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          call p_bit_and, er_jit_bit_and, r{}, vm, r{}, r{}\n", ra, rb, rc));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                    }
                }
            }
            OpCode::BitOr => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;
                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          d2i tmp1, d{}\n", rb));
                    mir.push_str(&format!("          d2i tmp2, d{}\n", rc));
                    mir.push_str("          or tmp3, tmp1, tmp2\n");
                    mir.push_str(&format!("          i2d d{}, tmp3\n", ra));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                } else {
                    if rb_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                    }
                    if rc_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          call p_bit_or, er_jit_bit_or, r{}, vm, r{}, r{}\n", ra, rb, rc));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                    }
                }
            }
            OpCode::BitXor => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;
                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          d2i tmp1, d{}\n", rb));
                    mir.push_str(&format!("          d2i tmp2, d{}\n", rc));
                    mir.push_str("          xor tmp3, tmp1, tmp2\n");
                    mir.push_str(&format!("          i2d d{}, tmp3\n", ra));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                } else {
                    if rb_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                    }
                    if rc_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          call p_bit_xor, er_jit_bit_xor, r{}, vm, r{}, r{}\n", ra, rb, rc));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                    }
                }
            }
            OpCode::BitNot => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;
                if rb_is_double {
                    // Fast path: numeric — inline bit-not
                    mir.push_str(&format!("          d2i tmp1, d{}\n", rb));
                    mir.push_str("          not tmp3, tmp1\n");
                    mir.push_str(&format!("          i2d d{}, tmp3\n", ra));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset2));
                    }
                } else {
                    mir.push_str(&format!("          call p_bit_not, er_jit_bit_not, r{}, vm, r{}\n", ra, rb));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset2));
                    }
                }
                let _ = offset1; // suppress unused warning
            }
            OpCode::ShiftLeft => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;
                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          d2i tmp1, d{}\n", rb));
                    mir.push_str(&format!("          d2i tmp2, d{}\n", rc));
                    mir.push_str("          lsl tmp3, tmp1, tmp2\n");
                    mir.push_str(&format!("          i2d d{}, tmp3\n", ra));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                } else {
                    if rb_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                    }
                    if rc_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          call p_shift_left, er_jit_shift_left, r{}, vm, r{}, r{}\n", ra, rb, rc));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                    }
                }
            }
            OpCode::ShiftRight => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;
                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          d2i tmp1, d{}\n", rb));
                    mir.push_str(&format!("          d2i tmp2, d{}\n", rc));
                    mir.push_str("          asr tmp3, tmp1, tmp2\n");
                    mir.push_str(&format!("          i2d d{}, tmp3\n", ra));
                    if !next_ra_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset3, ra));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    }
                } else {
                    if rb_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                    }
                    if rc_is_double {
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                    }
                    mir.push_str(&format!("          call p_shift_right, er_jit_shift_right, r{}, vm, r{}, r{}\n", ra, rb, rc));
                    mir.push_str("          mov status, u8:0(vm)\n");
                    mir.push_str("          bne err_label, status, 0\n");
                    if next_ra_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset3, ra));
                        mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset3));
                    }
                }
            }
            OpCode::TypeOf => {
                let offset1 = ((idx * 3) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                if rb_is_double {
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                }
                mir.push_str(&format!("          call p_typeof, er_jit_typeof, r{}, vm, r{}\n", ra, rb));
            }
            OpCode::ToIter => {
                let offset1 = ((idx * 3) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                if rb_is_double {
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                }
                mir.push_str(&format!("          call p_to_iter, er_jit_to_iter, r{}, vm, r{}\n", ra, rb));
                mir.push_str("          mov status, u8:0(vm)\n");
                mir.push_str("          bne err_label, status, 0\n");
            }
            OpCode::ArrayLen => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let next_ra_is_double = next_types[ra] == RegType::Double;
                if rb_is_double {
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                }
                mir.push_str(&format!("          call p_array_len_op, er_jit_array_len_op, r{}, vm, r{}\n", ra, rb));
                mir.push_str("          mov status, u8:0(vm)\n");
                mir.push_str("          bne err_label, status, 0\n");
                if next_ra_is_double {
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, ra));
                    mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset2));
                }
            }
            OpCode::Equal => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let next_is_jmp_if_false = if idx + 1 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[idx + 1];
                    next_inst.op == OpCode::JumpIfFalse && next_inst.ra == ra as u8
                } else {
                    false
                };

                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;

                if next_is_jmp_if_false {
                    let next_inst = &func.chunk.code[idx + 1];
                    let target = (idx + 2 + next_inst.operand as usize) as usize;

                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          dbne take_branch_{}, d{}, d{}\n", idx, rb, rc));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 2);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 2));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_eq_{}, r{}, 0xffe8000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_eq_{}, r{}, 0xffe8000000000000\n", idx, rc));
                        }
                        if !rb_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          dbne take_branch_{}, d{}, d{}\n", idx, rb, rc));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 2);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 2));

                        mir.push_str(&format!("fallback_eq_{}:\n", idx));
                        if rb_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                        }
                        if rc_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                        }
                        // Pure native MIR pointer/tag equality fast paths
                        mir.push_str(&format!("          beq eq_fallthrough_{}, r{}, r{}\n", idx, rb, rc));
                        mir.push_str(&format!("          mov tmp, r{}\n", rb));
                        mir.push_str(&format!("          xor tmp, tmp, r{}\n", rc));
                        mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                        mir.push_str(&format!("          bne take_branch_{}, tmp, 0\n", idx));
                        mir.push_str(&format!("          mov tmp, r{}\n", rb));
                        mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                        mir.push_str(&format!("          bne take_branch_{}, tmp, 0xfff4000000000000\n", idx));
                        mir.push_str(&format!("          call p_equal, er_jit_equal, r{}, vm, r{}, r{}\n", ra, rb, rc));
                        mir.push_str("          mov status, u8:0(vm)\n");
                        mir.push_str("          bne err_label, status, 0\n");
                        mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_FALSE));
                        mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_NULL));
                        mir.push_str(&format!("eq_fallthrough_{}:\n", idx));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 2);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 2));
                    }
                    // Common Trampoline Block
                    mir.push_str(&format!("take_branch_{}:\n", idx));
                    sync_edge(&mut mir, idx, target);
                    mir.push_str(&format!("          jmp inst_{}\n", target));
                } else {
                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          deq res_bool, d{}, d{}\n", rb, rc));
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_eq_{}, r{}, 0xffe8000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_eq_{}, r{}, 0xffe8000000000000\n", idx, rc));
                        }
                        if !rb_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          deq res_bool, d{}, d{}\n", rb, rc));
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                        mir.push_str(&format!("          jmp done_eq_{}\n", idx));

                        mir.push_str(&format!("fallback_eq_{}:\n", idx));
                        if rb_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                        }
                        if rc_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                        }
                        // Pure native MIR pointer/tag equality fast paths
                        mir.push_str(&format!("          bne not_exact_eq_{}, r{}, r{}\n", idx, rb, rc));
                        mir.push_str(&format!("          mov r{}, {}\n", ra, TAG_TRUE));
                        mir.push_str(&format!("          jmp done_eq_{}\n", idx));
                        mir.push_str(&format!("not_exact_eq_{}:\n", idx));
                        mir.push_str(&format!("          mov tmp, r{}\n", rb));
                        mir.push_str(&format!("          xor tmp, tmp, r{}\n", rc));
                        mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                        mir.push_str(&format!("          beq same_tag_{}, tmp, 0\n", idx));
                        mir.push_str(&format!("          mov r{}, {}\n", ra, TAG_FALSE));
                        mir.push_str(&format!("          jmp done_eq_{}\n", idx));
                        mir.push_str(&format!("same_tag_{}:\n", idx));
                        mir.push_str(&format!("          mov tmp, r{}\n", rb));
                        mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                        mir.push_str(&format!("          beq string_eq_{}, tmp, 0xfff4000000000000\n", idx));
                        mir.push_str(&format!("          mov r{}, {}\n", ra, TAG_FALSE));
                        mir.push_str(&format!("          jmp done_eq_{}\n", idx));
                        mir.push_str(&format!("string_eq_{}:\n", idx));
                        mir.push_str(&format!("          call p_equal, er_jit_equal, r{}, vm, r{}, r{}\n", ra, rb, rc));
                        mir.push_str("          mov status, u8:0(vm)\n");
                        mir.push_str("          bne err_label, status, 0\n");
                        mir.push_str(&format!("done_eq_{}:\n", idx));
                    }
                }
            }
            OpCode::Greater => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let next_is_jmp_if_false = if idx + 1 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[idx + 1];
                    next_inst.op == OpCode::JumpIfFalse && next_inst.ra == ra as u8
                } else {
                    false
                };

                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;

                if next_is_jmp_if_false {
                    let next_inst = &func.chunk.code[idx + 1];
                    let target = (idx + 2 + next_inst.operand as usize) as usize;

                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          dble take_branch_{}, d{}, d{}\n", idx, rb, rc));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 2);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 2));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_gt_{}, r{}, 0xffe8000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_gt_{}, r{}, 0xffe8000000000000\n", idx, rc));
                        }
                        if !rb_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          dble take_branch_{}, d{}, d{}\n", idx, rb, rc));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 2);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 2));

                        mir.push_str(&format!("fallback_gt_{}:\n", idx));
                        if rb_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                        }
                        if rc_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          call p_greater, er_jit_greater, r{}, vm, r{}, r{}\n", ra, rb, rc));
                        mir.push_str("          mov status, u8:0(vm)\n");
                        mir.push_str("          bne err_label, status, 0\n");
                        mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_FALSE));
                        mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_NULL));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 2);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 2));
                    }
                    // Common Trampoline Block
                    mir.push_str(&format!("take_branch_{}:\n", idx));
                    sync_edge(&mut mir, idx, target);
                    mir.push_str(&format!("          jmp inst_{}\n", target));
                } else {
                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          dgt res_bool, d{}, d{}\n", rb, rc));
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_gt_{}, r{}, 0xffe8000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_gt_{}, r{}, 0xffe8000000000000\n", idx, rc));
                        }
                        if !rb_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          dgt res_bool, d{}, d{}\n", rb, rc));
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                        mir.push_str(&format!("          jmp done_gt_{}\n", idx));

                        mir.push_str(&format!("fallback_gt_{}:\n", idx));
                        if rb_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                        }
                        if rc_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          call p_greater, er_jit_greater, r{}, vm, r{}, r{}\n", ra, rb, rc));
                        mir.push_str("          mov status, u8:0(vm)\n");
                        mir.push_str("          bne err_label, status, 0\n");
                        mir.push_str(&format!("done_gt_{}:\n", idx));
                    }
                }
            }
            OpCode::Less => {
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let next_is_jmp_if_false = if idx + 1 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[idx + 1];
                    next_inst.op == OpCode::JumpIfFalse && next_inst.ra == ra as u8
                } else {
                    false
                };

                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;

                if next_is_jmp_if_false {
                    let next_inst = &func.chunk.code[idx + 1];
                    let target = (idx + 2 + next_inst.operand as usize) as usize;

                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          dbge take_branch_{}, d{}, d{}\n", idx, rb, rc));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 2);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 2));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_lt_{}, r{}, 0xffe8000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_lt_{}, r{}, 0xffe8000000000000\n", idx, rc));
                        }
                        if !rb_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          dbge take_branch_{}, d{}, d{}\n", idx, rb, rc));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 2);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 2));

                        mir.push_str(&format!("fallback_lt_{}:\n", idx));
                        if rb_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                        }
                        if rc_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          call p_less, er_jit_less, r{}, vm, r{}, r{}\n", ra, rb, rc));
                        mir.push_str("          mov status, u8:0(vm)\n");
                        mir.push_str("          bne err_label, status, 0\n");
                        mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_FALSE));
                        mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_NULL));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 2);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 2));
                    }
                    // Common Trampoline Block
                    mir.push_str(&format!("take_branch_{}:\n", idx));
                    sync_edge(&mut mir, idx, target);
                    mir.push_str(&format!("          jmp inst_{}\n", target));
                } else {
                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          dlt res_bool, d{}, d{}\n", rb, rc));
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_lt_{}, r{}, 0xffe8000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_lt_{}, r{}, 0xffe8000000000000\n", idx, rc));
                        }
                        if !rb_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rb, offset1));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          dlt res_bool, d{}, d{}\n", rb, rc));
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                        mir.push_str(&format!("          jmp done_lt_{}\n", idx));

                        mir.push_str(&format!("fallback_lt_{}:\n", idx));
                        if rb_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
                        }
                        if rc_is_double {
                            mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                            mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
                        }
                        mir.push_str(&format!("          call p_less, er_jit_less, r{}, vm, r{}, r{}\n", ra, rb, rc));
                        mir.push_str("          mov status, u8:0(vm)\n");
                        mir.push_str("          bne err_label, status, 0\n");
                        mir.push_str(&format!("done_lt_{}:\n", idx));
                    }
                }
            }
            OpCode::DefineGlobal => {
                let c_idx = instruction.operand;
                if ra < num_regs && types_at_inst[idx][ra] == RegType::Double {
                    let offset = (ra % 24) * 8;
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset, ra));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset));
                }
                mir.push_str(&format!("          mov tmp1, i64:{}(constants_ptr)\n", c_idx * 8));
                mir.push_str(&format!("          call p_def_global, er_jit_define_global, status, vm, tmp1, r{}\n", ra));
            }
            OpCode::DefineStruct => {
                let name_idx = instruction.operand;
                let fields_idx = instruction.ra as u32;
                let methods_idx = instruction.rb as u32;
                mir.push_str(&format!("          mov tmp1, i64:{}(constants_ptr)\n", name_idx * 8));
                mir.push_str(&format!("          mov tmp2, i64:{}(constants_ptr)\n", fields_idx * 8));
                mir.push_str(&format!("          mov tmp3, i64:{}(constants_ptr)\n", methods_idx * 8));
                mir.push_str("          call p_def_struct, er_jit_define_struct, status, vm, tmp1, tmp2, tmp3\n");
            }
            OpCode::GetGlobal => {
                let c_idx = instruction.operand;
                mir.push_str(&format!("          mov tmp1, i64:{}(constants_ptr)\n", c_idx * 8));
                mir.push_str(&format!("          call p_get_global, er_jit_get_global, r{}, vm, tmp1\n", ra));
                mir.push_str("          mov status, u8:0(vm)\n");
                mir.push_str("          bne err_label, status, 0\n");
                if next_types[ra] == RegType::Double {
                    let offset = (ra % 24) * 8;
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset, ra));
                    mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset));
                }
            }
            OpCode::SetGlobal => {
                let c_idx = instruction.operand;
                if ra < num_regs && types_at_inst[idx][ra] == RegType::Double {
                    let offset = (ra % 24) * 8;
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset, ra));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset));
                }
                mir.push_str(&format!("          mov tmp1, i64:{}(constants_ptr)\n", c_idx * 8));
                mir.push_str(&format!("          call p_set_global, er_jit_set_global, status, vm, r{}, tmp1\n", ra));
                mir.push_str("          mov status, u8:0(vm)\n");
                mir.push_str("          bne err_label, status, 0\n");
            }
            OpCode::Jump => {
                let target = (idx as i32 + 1 + instruction.operand as i32) as usize;
                sync_edge(&mut mir, idx, target);
                mir.push_str(&format!("          jmp inst_{}\n", target));
            }
            OpCode::Loop => {
                let target = (idx as i32 + 1 - instruction.operand as i32) as usize;
                mir.push_str("          add loop_counter, loop_counter, 1\n");
                mir.push_str("          and tmp, loop_counter, 1023\n");
                mir.push_str(&format!("          bne no_yield_gc_{}, tmp, 0\n", idx));
                mir.push_str("          mov tmp1, er_gc_needs_step\n");
                mir.push_str("          mov status, u8:0(tmp1)\n");
                mir.push_str(&format!("          beq no_yield_gc_{}, status, 0\n", idx));
                save_all_registers(&mut mir, idx);
                mir.push_str(&format!("          mov i64:(ip_out), {}\n", target));
                mir.push_str("          ret 2\n");
                mir.push_str(&format!("no_yield_gc_{}:\n", idx));
                sync_edge(&mut mir, idx, target);
                mir.push_str(&format!("          jmp inst_{}\n", target));
            }
            OpCode::JumpIfFalse => {
                let prev_was_optimized_cmp = if idx > 0 {
                    let prev_inst = &func.chunk.code[idx - 1];
                    matches!(prev_inst.op, OpCode::Less | OpCode::Greater | OpCode::Equal) && instruction.ra == prev_inst.ra
                } else {
                    false
                };
                if !prev_was_optimized_cmp {
                    let target = (idx as i32 + 1 + instruction.operand as i32) as usize;
                    if types_at_inst[idx][ra] != RegType::Double {
                        mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_FALSE));
                        mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_NULL));
                        // Fall-through path
                        sync_edge(&mut mir, idx, idx + 1);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 1));
                        // Branch taken path
                        mir.push_str(&format!("take_branch_{}:\n", idx));
                        sync_edge(&mut mir, idx, target);
                        mir.push_str(&format!("          jmp inst_{}\n", target));
                    } else {
                        // Always fall through, but we still need to sync registers for the fall-through path!
                        sync_edge(&mut mir, idx, idx + 1);
                        mir.push_str(&format!("          jmp inst_{}\n", idx + 1));
                    }
                }
            }
            OpCode::Call => {
                let arg_count = instruction.operand as usize;
                let mut extra = vec![rb];
                for i in 0..arg_count {
                    extra.push(rb + 1 + i);
                }

                if arg_count == 1 {
                    // Check if callee is array push method (TAG_METHOD_PUSH = 0xfff9000000000000)
                    mir.push_str(&format!("          mov tmp, r{}\n", rb));
                    mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                    mir.push_str(&format!("          bne normal_call_{}, tmp, 0xfff9000000000000\n", idx));

                    let arg_reg = rb + 1;
                    if arg_reg < num_regs && types_at_inst[idx][arg_reg] == RegType::Double {
                        mir.push_str(&format!("          dmov d:0(cast_ptr), d{}\n", arg_reg));
                        mir.push_str(&format!("          mov r{}, i64:0(cast_ptr)\n", arg_reg));
                    }

                    // Native inlined array push fast path
                    mir.push_str(&format!("          mov obj_ptr, r{}\n", rb));
                    mir.push_str("          and obj_ptr, obj_ptr, 0x0000ffffffffffff\n");
                    mir.push_str("          mov tmp1, i64:40(obj_ptr)\n"); // len
                    mir.push_str("          mov tmp2, i64:24(obj_ptr)\n"); // cap
                    mir.push_str(&format!("          bge fallback_push_{}, tmp1, tmp2\n", idx));
                    mir.push_str("          mov start_ptr, i64:32(obj_ptr)\n"); // buf_ptr
                    mir.push_str("          mul tmp3, tmp1, 8\n");
                    mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                    mir.push_str(&format!("          mov i64:0(start_ptr), r{}\n", arg_reg));
                    mir.push_str("          add tmp1, tmp1, 1\n");
                    mir.push_str("          mov i64:40(obj_ptr), tmp1\n");
                    mir.push_str("          mov tmp3, u8:0(obj_ptr)\n");
                    mir.push_str(&format!("          bne done_wb_push_{}, tmp3, 2\n", idx));
                    mir.push_str(&format!("          ublt done_wb_push_{}, r{}, 0xfff4000000000000\n", idx, arg_reg));
                    mir.push_str(&format!("          call p_write_barrier, er_jit_write_barrier, status, obj_ptr, r{}\n", arg_reg));
                    mir.push_str(&format!("done_wb_push_{}:\n", idx));
                    mir.push_str("          i2d da, tmp1\n");
                    if next_types[ra] == RegType::Double {
                        mir.push_str(&format!("          dmov d{}, da\n", ra));
                    } else {
                        mir.push_str("          dmov d:0(cast_ptr), da\n");
                        mir.push_str(&format!("          mov r{}, i64:0(cast_ptr)\n", ra));
                    }
                    mir.push_str(&format!("          jmp done_call_{}\n", idx));

                    mir.push_str(&format!("fallback_push_{}:\n", idx));
                    mir.push_str(&format!("          mov tmp, r{}\n", rb));
                    mir.push_str("          and tmp, tmp, 0x0000ffffffffffff\n");
                    mir.push_str("          or tmp, tmp, 0xfff5000000000000\n"); // Convert TAG_METHOD_PUSH to TAG_ARRAY
                    mir.push_str(&format!("          call p_array_push, er_jit_array_push, tmp2, tmp, r{}\n", rb + 1));
                    if next_types[ra] == RegType::Double {
                        mir.push_str("          mov i64:0(cast_ptr), tmp2\n");
                        mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                    } else {
                        mir.push_str(&format!("          mov r{}, tmp2\n", ra));
                    }
                    mir.push_str(&format!("          jmp done_call_{}\n", idx));

                    mir.push_str(&format!("normal_call_{}:\n", idx));
                    save_all_registers(&mut mir, idx);
                } else if arg_count == 0 {
                    // Check if callee is array pop method (TAG_METHOD_POP = 0xfffa000000000000)
                    mir.push_str(&format!("          mov tmp, r{}\n", rb));
                    mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                    mir.push_str(&format!("          bne normal_call_{}, tmp, 0xfffa000000000000\n", idx));

                    // Native inlined array pop fast path
                    mir.push_str(&format!("          mov obj_ptr, r{}\n", rb));
                    mir.push_str("          and obj_ptr, obj_ptr, 0x0000ffffffffffff\n");
                    mir.push_str("          mov tmp1, i64:40(obj_ptr)\n"); // len
                    mir.push_str(&format!("          ble fallback_pop_{}, tmp1, 0\n", idx));
                    mir.push_str("          sub tmp1, tmp1, 1\n");
                    mir.push_str("          mov i64:40(obj_ptr), tmp1\n");
                    mir.push_str("          mov start_ptr, i64:32(obj_ptr)\n"); // buf_ptr
                    mir.push_str("          mul tmp3, tmp1, 8\n");
                    mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                    mir.push_str(&format!("          mov r{}, i64:0(start_ptr)\n", ra));
                    if next_types[ra] == RegType::Double {
                        mir.push_str(&format!("          mov i64:0(cast_ptr), r{}\n", ra));
                        mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                    }
                    mir.push_str(&format!("          jmp done_call_{}\n", idx));

                    mir.push_str(&format!("fallback_pop_{}:\n", idx));
                    mir.push_str(&format!("          mov tmp, r{}\n", rb));
                    mir.push_str("          and tmp, tmp, 0x0000ffffffffffff\n");
                    mir.push_str("          or tmp, tmp, 0xfff5000000000000\n"); // Convert TAG_METHOD_POP to TAG_ARRAY
                    mir.push_str(&format!("          call p_array_pop, er_jit_array_pop, tmp2, tmp\n"));
                    if next_types[ra] == RegType::Double {
                        mir.push_str("          mov i64:0(cast_ptr), tmp2\n");
                        mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                    } else {
                        mir.push_str(&format!("          mov r{}, tmp2\n", ra));
                    }
                    mir.push_str(&format!("          jmp done_call_{}\n", idx));

                    mir.push_str(&format!("normal_call_{}:\n", idx));
                    save_all_registers(&mut mir, idx);
                } else {
                    save_all_registers(&mut mir, idx);
                }

                if arg_count == func.arity && !func.is_async {
                    let func_val_u64 = Value::function(func_obj).0;
                    mir.push_str(&format!("          bne not_self_call_{}, r{}, {}\n", idx, rb, func_val_u64));
                    // Direct recursive call fast-path!
                    mir.push_str(&format!("          add start_ptr, frame_slots, {}\n", (rb + 1) * 8));
                    mir.push_str(&format!("          call p_jit_fn, {}, status, vm, start_ptr, constants_ptr, 0, ip_out, dest_reg_out, func_reg_out, arg_count_out, ret_val_out\n", func_name));
                    mir.push_str(&format!("          bne not_normal_ret_{}, status, 1\n", idx));
                    mir.push_str(&format!("          mov r{}, i64:0(ret_val_out)\n", ra));
                    if next_types[ra] == RegType::Double {
                        mir.push_str(&format!("          dmov d{}, d:0(ret_val_out)\n", ra));
                    }
                    mir.push_str(&format!("          jmp done_call_{}\n", idx));
                    mir.push_str(&format!("not_normal_ret_{}:\n", idx));
                    mir.push_str(&format!("          beq suspend_label_{}, status, 3\n", idx));
                    mir.push_str(&format!("          beq suspend_label_{}, status, -3\n", idx));
                    mir.push_str("          blt err_label, status, 0\n");
                    mir.push_str(&format!("          jmp done_call_{}\n", idx));
                    mir.push_str(&format!("not_self_call_{}:\n", idx));
                }

                mir.push_str(&format!("          add dest_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          add start_ptr, frame_slots, {}\n", (rb + 1) * 8));
                mir.push_str(&format!("          call p_call_fast, er_jit_call_fast, status, vm, r{}, start_ptr, dest_ptr, {}, {}\n", rb, idx, ra));
                mir.push_str(&format!("          beq suspend_label_{}, status, -3\n", idx));
                mir.push_str(&format!("          blt err_label, status, -1\n"));
                mir.push_str(&format!("          bne not_fast_call_{}, status, 0\n", idx));
                mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                if next_types[ra] == RegType::Double {
                    mir.push_str(&format!("          dmov d{}, d:{}(frame_slots)\n", ra, ra * 8));
                }
                mir.push_str(&format!("          jmp done_call_{}\n", idx));
                mir.push_str(&format!("not_fast_call_{}:\n", idx));

                mir.push_str(&format!("          call p_call_non_vm, er_jit_call_non_vm, status, vm, dest_ptr, r{}, {}, {}, frame_slots, {}, {}\n", rb, rb, arg_count, idx, ra));
                mir.push_str(&format!("          beq call_vm_label_{}, status, -1\n", idx));
                mir.push_str(&format!("          beq suspend_label_{}, status, -3\n", idx));
                mir.push_str(&format!("          blt err_label, status, 0\n"));
                mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                if next_types[ra] == RegType::Double {
                    mir.push_str(&format!("          dmov d{}, d:{}(frame_slots)\n", ra, ra * 8));
                }
                mir.push_str(&format!("          jmp done_call_{}\n", idx));
                mir.push_str(&format!("suspend_label_{}:\n", idx));
                mir.push_str(&format!("          mov i64:(ip_out), {}\n", idx));
                mir.push_str(&format!("          mov i64:(dest_reg_out), {}\n", ra));
                mir.push_str(&format!("          mov i64:(func_reg_out), {}\n", rb));
                mir.push_str(&format!("          mov i64:(arg_count_out), {}\n", arg_count));
                mir.push_str("          ret 3\n");
                mir.push_str(&format!("call_vm_label_{}:\n", idx));
                mir.push_str(&format!("          mov i64:(ip_out), {}\n", idx));
                mir.push_str(&format!("          mov i64:(dest_reg_out), {}\n", ra));
                mir.push_str(&format!("          mov i64:(func_reg_out), {}\n", rb));
                mir.push_str(&format!("          mov i64:(arg_count_out), {}\n", arg_count));
                mir.push_str("          ret 0\n");
                mir.push_str(&format!("done_call_{}:\n", idx));
            }
            OpCode::MakeArray => {
                let count = instruction.operand as usize;
                let mut extra = Vec::new();
                for i in 0..count {
                    extra.push(rb + i);
                }
                for &r in &extra {
                    if r < num_regs {
                        if types_at_inst[idx][r] == RegType::Double {
                            mir.push_str(&format!("          dmov d:{}(frame_slots), d{}\n", r * 8, r));
                        } else {
                            mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", r * 8, r));
                        }
                    }
                }
                mir.push_str(&format!("          add start_ptr, frame_slots, {}\n", rb * 8));
                mir.push_str(&format!("          call p_make_array, er_jit_make_array, r{}, vm, start_ptr, {}\n", ra, count));
            }
            OpCode::MakeObject => {
                let count = instruction.operand as usize;
                let mut extra = Vec::new();
                for i in 0..(count * 2) {
                    extra.push(rb + i);
                }
                for &r in &extra {
                    if r < num_regs {
                        if types_at_inst[idx][r] == RegType::Double {
                            mir.push_str(&format!("          dmov d:{}(frame_slots), d{}\n", r * 8, r));
                        } else {
                            mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", r * 8, r));
                        }
                    }
                }
                mir.push_str(&format!("          add start_ptr, frame_slots, {}\n", rb * 8));
                mir.push_str(&format!("          call p_make_object, er_jit_make_object, r{}, vm, start_ptr, {}\n", ra, count));
            }
            OpCode::GetProperty => {
                let c_idx = instruction.operand;
                let name_val = func.chunk.constants[c_idx as usize];
                let mut is_push = false;
                let mut is_pop = false;
                if name_val.is_string() {
                    let s = name_val.as_str().unwrap_or("");
                    if s == "push" {
                        is_push = true;
                    } else if s == "pop" {
                        is_pop = true;
                    }
                }

                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                if rb_is_double {
                    mir.push_str(&format!("          dmov d:0(cast_ptr), d{}\n", rb));
                    mir.push_str(&format!("          mov r{}, i64:0(cast_ptr)\n", rb));
                }

                if is_push || is_pop {
                    let method_tag = if is_push { "0xfff9000000000000" } else { "0xfffa000000000000" };
                    mir.push_str(&format!("          mov tmp, r{}\n", rb));
                    mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                    mir.push_str(&format!("          bne fallback_get_prop_{}, tmp, 0xfff5000000000000\n", idx));
                    mir.push_str(&format!("          mov tmp, r{}\n", rb));
                    mir.push_str("          and tmp, tmp, 0x0000ffffffffffff\n");
                    mir.push_str(&format!("          or r{}, tmp, {}\n", ra, method_tag));
                    mir.push_str(&format!("          jmp done_get_prop_{}\n", idx));
                } else {
                    // Inlined Monomorphic / 4-Way Shape Inline Cache
                    mir.push_str(&format!("          mov name_ptr, i64:{}(constants_ptr)\n", c_idx * 8));
                    mir.push_str(&format!("          mov tmp, r{}\n", rb));
                    mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                    mir.push_str(&format!("          bne fallback_get_prop_{}, tmp, 0xfff6000000000000\n", idx));
                    mir.push_str(&format!("          mov obj_ptr, r{}\n", rb));
                    mir.push_str("          and obj_ptr, obj_ptr, 0x0000ffffffffffff\n");
                    mir.push_str("          mov desc_ptr, i64:24(obj_ptr)\n");

                    // Check fast_fields[0]
                    mir.push_str("          mov tmp1, i64:32(desc_ptr)\n");
                    mir.push_str(&format!("          bne check_ff1_{}, tmp1, name_ptr\n", idx));
                    mir.push_str("          mov tmp2, i64:40(desc_ptr)\n");
                    mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
                    mir.push_str("          mul tmp3, tmp2, 8\n");
                    mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                    mir.push_str(&format!("          mov r{}, i64:0(start_ptr)\n", ra));
                    if next_types[ra] == RegType::Double {
                        mir.push_str(&format!("          mov i64:0(cast_ptr), r{}\n", ra));
                        mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                    }
                    mir.push_str(&format!("          jmp done_get_prop_{}\n", idx));

                    // Check fast_fields[1]
                    mir.push_str(&format!("check_ff1_{}:\n", idx));
                    mir.push_str("          mov tmp1, i64:48(desc_ptr)\n");
                    mir.push_str(&format!("          bne check_ff2_{}, tmp1, name_ptr\n", idx));
                    mir.push_str("          mov tmp2, i64:56(desc_ptr)\n");
                    mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
                    mir.push_str("          mul tmp3, tmp2, 8\n");
                    mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                    mir.push_str(&format!("          mov r{}, i64:0(start_ptr)\n", ra));
                    if next_types[ra] == RegType::Double {
                        mir.push_str(&format!("          mov i64:0(cast_ptr), r{}\n", ra));
                        mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                    }
                    mir.push_str(&format!("          jmp done_get_prop_{}\n", idx));

                    // Check fast_fields[2]
                    mir.push_str(&format!("check_ff2_{}:\n", idx));
                    mir.push_str("          mov tmp1, i64:64(desc_ptr)\n");
                    mir.push_str(&format!("          bne check_ff3_{}, tmp1, name_ptr\n", idx));
                    mir.push_str("          mov tmp2, i64:72(desc_ptr)\n");
                    mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
                    mir.push_str("          mul tmp3, tmp2, 8\n");
                    mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                    mir.push_str(&format!("          mov r{}, i64:0(start_ptr)\n", ra));
                    if next_types[ra] == RegType::Double {
                        mir.push_str(&format!("          mov i64:0(cast_ptr), r{}\n", ra));
                        mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                    }
                    mir.push_str(&format!("          jmp done_get_prop_{}\n", idx));

                    // Check fast_fields[3]
                    mir.push_str(&format!("check_ff3_{}:\n", idx));
                    mir.push_str("          mov tmp1, i64:80(desc_ptr)\n");
                    mir.push_str(&format!("          bne fallback_get_prop_{}, tmp1, name_ptr\n", idx));
                    mir.push_str("          mov tmp2, i64:88(desc_ptr)\n");
                    mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
                    mir.push_str("          mul tmp3, tmp2, 8\n");
                    mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                    mir.push_str(&format!("          mov r{}, i64:0(start_ptr)\n", ra));
                    if next_types[ra] == RegType::Double {
                        mir.push_str(&format!("          mov i64:0(cast_ptr), r{}\n", ra));
                        mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                    }
                    mir.push_str(&format!("          jmp done_get_prop_{}\n", idx));
                }

                mir.push_str(&format!("fallback_get_prop_{}:\n", idx));
                mir.push_str(&format!("          mov tmp1, i64:{}(constants_ptr)\n", c_idx * 8));
                mir.push_str(&format!("          call p_get_property, er_jit_get_property, r{}, vm, r{}, tmp1\n", ra, rb));
                mir.push_str("          mov status, u8:0(vm)\n");
                mir.push_str("          bne err_label, status, 0\n");
                if next_types[ra] == RegType::Double {
                    mir.push_str(&format!("          mov i64:0(cast_ptr), r{}\n", ra));
                    mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                }
                mir.push_str(&format!("done_get_prop_{}:\n", idx));
            }
            OpCode::SetProperty => {
                let c_idx = instruction.operand;

                if ra < num_regs && types_at_inst[idx][ra] == RegType::Double {
                    mir.push_str(&format!("          dmov d:0(cast_ptr), d{}\n", ra));
                    mir.push_str(&format!("          mov r{}, i64:0(cast_ptr)\n", ra));
                }
                if rb < num_regs && types_at_inst[idx][rb] == RegType::Double {
                    mir.push_str(&format!("          dmov d:8(cast_ptr), d{}\n", rb));
                    mir.push_str(&format!("          mov r{}, i64:8(cast_ptr)\n", rb));
                }

                // Inline Shape IC for SetProperty
                mir.push_str(&format!("          mov name_ptr, i64:{}(constants_ptr)\n", c_idx * 8));
                mir.push_str(&format!("          mov tmp, r{}\n", ra));
                mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                mir.push_str(&format!("          bne fallback_set_prop_{}, tmp, 0xfff6000000000000\n", idx));
                mir.push_str(&format!("          mov obj_ptr, r{}\n", ra));
                mir.push_str("          and obj_ptr, obj_ptr, 0x0000ffffffffffff\n");
                mir.push_str("          mov desc_ptr, i64:24(obj_ptr)\n");

                // Check fast_fields[0]
                mir.push_str("          mov tmp1, i64:32(desc_ptr)\n");
                mir.push_str(&format!("          bne check_set_ff1_{}, tmp1, name_ptr\n", idx));
                mir.push_str("          mov tmp2, i64:40(desc_ptr)\n");
                mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
                mir.push_str("          mul tmp3, tmp2, 8\n");
                mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                mir.push_str(&format!("          mov i64:0(start_ptr), r{}\n", rb));
                mir.push_str(&format!("          jmp check_wb_set_prop_{}\n", idx));

                // Check fast_fields[1]
                mir.push_str(&format!("check_set_ff1_{}:\n", idx));
                mir.push_str("          mov tmp1, i64:48(desc_ptr)\n");
                mir.push_str(&format!("          bne check_set_ff2_{}, tmp1, name_ptr\n", idx));
                mir.push_str("          mov tmp2, i64:56(desc_ptr)\n");
                mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
                mir.push_str("          mul tmp3, tmp2, 8\n");
                mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                mir.push_str(&format!("          mov i64:0(start_ptr), r{}\n", rb));
                mir.push_str(&format!("          jmp check_wb_set_prop_{}\n", idx));

                // Check fast_fields[2]
                mir.push_str(&format!("check_set_ff2_{}:\n", idx));
                mir.push_str("          mov tmp1, i64:64(desc_ptr)\n");
                mir.push_str(&format!("          bne check_set_ff3_{}, tmp1, name_ptr\n", idx));
                mir.push_str("          mov tmp2, i64:72(desc_ptr)\n");
                mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
                mir.push_str("          mul tmp3, tmp2, 8\n");
                mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                mir.push_str(&format!("          mov i64:0(start_ptr), r{}\n", rb));
                mir.push_str(&format!("          jmp check_wb_set_prop_{}\n", idx));

                // Check fast_fields[3]
                mir.push_str(&format!("check_set_ff3_{}:\n", idx));
                mir.push_str("          mov tmp1, i64:80(desc_ptr)\n");
                mir.push_str(&format!("          bne fallback_set_prop_{}, tmp1, name_ptr\n", idx));
                mir.push_str("          mov tmp2, i64:88(desc_ptr)\n");
                mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
                mir.push_str("          mul tmp3, tmp2, 8\n");
                mir.push_str("          add start_ptr, start_ptr, tmp3\n");
                mir.push_str(&format!("          mov i64:0(start_ptr), r{}\n", rb));

                mir.push_str(&format!("check_wb_set_prop_{}:\n", idx));
                mir.push_str("          mov tmp3, u8:0(obj_ptr)\n");
                mir.push_str(&format!("          bne done_set_prop_{}, tmp3, 2\n", idx));
                mir.push_str(&format!("          ublt done_set_prop_{}, r{}, 0xfff4000000000000\n", idx, rb));
                mir.push_str(&format!("          call p_write_barrier, er_jit_write_barrier, status, obj_ptr, r{}\n", rb));
                mir.push_str(&format!("          jmp done_set_prop_{}\n", idx));

                mir.push_str(&format!("fallback_set_prop_{}:\n", idx));
                mir.push_str(&format!("          mov tmp1, i64:{}(constants_ptr)\n", c_idx * 8));
                mir.push_str(&format!("          call p_set_property, er_jit_set_property, status, vm, r{}, r{}, tmp1\n", ra, rb));
                mir.push_str("          mov status, u8:0(vm)\n");
                mir.push_str("          bne err_label, status, 0\n");
                mir.push_str(&format!("done_set_prop_{}:\n", idx));
            }
            OpCode::GetIndex => {
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                if rb_is_double {
                    mir.push_str(&format!("          dmov d:0(cast_ptr), d{}\n", rb));
                    mir.push_str(&format!("          mov r{}, i64:0(cast_ptr)\n", rb));
                }

                // Inlined native array indexing fast path
                mir.push_str(&format!("          mov tmp, r{}\n", rb));
                mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                mir.push_str(&format!("          bne fallback_get_idx_{}, tmp, 0xfff5000000000000\n", idx));

                if rc < num_regs && types_at_inst[idx][rc] == RegType::Double {
                    mir.push_str(&format!("          d2i idx_ptr, d{}\n", rc));
                } else {
                    mir.push_str(&format!("          ubge fallback_get_idx_{}, r{}, 0xffe8000000000000\n", idx, rc));
                    mir.push_str(&format!("          mov i64:8(cast_ptr), r{}\n", rc));
                    mir.push_str("          dmov da, d:8(cast_ptr)\n");
                    mir.push_str("          d2i idx_ptr, da\n");
                }
                mir.push_str(&format!("          blt fallback_get_idx_{}, idx_ptr, 0\n", idx));
                mir.push_str(&format!("          mov obj_ptr, r{}\n", rb));
                mir.push_str("          and obj_ptr, obj_ptr, 0x0000ffffffffffff\n");
                mir.push_str("          mov tmp1, i64:40(obj_ptr)\n"); // len
                mir.push_str(&format!("          bge fallback_get_idx_{}, idx_ptr, tmp1\n", idx));
                mir.push_str("          mov start_ptr, i64:32(obj_ptr)\n"); // buf_ptr
                mir.push_str("          mul tmp2, idx_ptr, 8\n");
                mir.push_str("          add start_ptr, start_ptr, tmp2\n");
                mir.push_str(&format!("          mov r{}, i64:0(start_ptr)\n", ra));
                if next_types[ra] == RegType::Double {
                    mir.push_str(&format!("          mov i64:0(cast_ptr), r{}\n", ra));
                    mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                }
                mir.push_str(&format!("          jmp done_get_idx_{}\n", idx));

                mir.push_str(&format!("fallback_get_idx_{}:\n", idx));
                if rc < num_regs && types_at_inst[idx][rc] == RegType::Double {
                    mir.push_str(&format!("          dmov d:8(cast_ptr), d{}\n", rc));
                    mir.push_str(&format!("          mov r{}, i64:8(cast_ptr)\n", rc));
                }
                mir.push_str(&format!("          call p_get_index, er_jit_get_index, r{}, vm, r{}, r{}\n", ra, rb, rc));
                mir.push_str("          mov status, u8:0(vm)\n");
                mir.push_str("          bne err_label, status, 0\n");
                if next_types[ra] == RegType::Double {
                    mir.push_str(&format!("          mov i64:0(cast_ptr), r{}\n", ra));
                    mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                }
                mir.push_str(&format!("done_get_idx_{}:\n", idx));
            }
            OpCode::SetIndex => {
                if ra < num_regs && types_at_inst[idx][ra] == RegType::Double {
                    mir.push_str(&format!("          dmov d:0(cast_ptr), d{}\n", ra));
                    mir.push_str(&format!("          mov r{}, i64:0(cast_ptr)\n", ra));
                }
                if rc < num_regs && types_at_inst[idx][rc] == RegType::Double {
                    mir.push_str(&format!("          dmov d:16(cast_ptr), d{}\n", rc));
                    mir.push_str(&format!("          mov r{}, i64:16(cast_ptr)\n", rc));
                }

                // Inlined native array SetIndex fast path
                mir.push_str(&format!("          mov tmp, r{}\n", ra));
                mir.push_str("          and tmp, tmp, 0xffff000000000000\n");
                mir.push_str(&format!("          bne fallback_set_idx_{}, tmp, 0xfff5000000000000\n", idx));

                if rb < num_regs && types_at_inst[idx][rb] == RegType::Double {
                    mir.push_str(&format!("          d2i idx_ptr, d{}\n", rb));
                } else {
                    mir.push_str(&format!("          ubge fallback_set_idx_{}, r{}, 0xffe8000000000000\n", idx, rb));
                    mir.push_str(&format!("          mov i64:8(cast_ptr), r{}\n", rb));
                    mir.push_str("          dmov da, d:8(cast_ptr)\n");
                    mir.push_str("          d2i idx_ptr, da\n");
                }
                mir.push_str(&format!("          blt fallback_set_idx_{}, idx_ptr, 0\n", idx));
                mir.push_str(&format!("          mov obj_ptr, r{}\n", ra));
                mir.push_str("          and obj_ptr, obj_ptr, 0x0000ffffffffffff\n");
                mir.push_str("          mov tmp1, i64:40(obj_ptr)\n"); // len
                mir.push_str(&format!("          bge fallback_set_idx_{}, idx_ptr, tmp1\n", idx));
                mir.push_str("          mov start_ptr, i64:32(obj_ptr)\n"); // buf_ptr
                mir.push_str("          mul tmp2, idx_ptr, 8\n");
                mir.push_str("          add start_ptr, start_ptr, tmp2\n");
                mir.push_str(&format!("          mov i64:0(start_ptr), r{}\n", rc));
                mir.push_str("          mov tmp3, u8:0(obj_ptr)\n");
                mir.push_str(&format!("          bne done_set_idx_{}, tmp3, 2\n", idx));
                mir.push_str(&format!("          ublt done_set_idx_{}, r{}, 0xfff4000000000000\n", idx, rc));
                mir.push_str(&format!("          call p_write_barrier, er_jit_write_barrier, status, obj_ptr, r{}\n", rc));
                mir.push_str(&format!("          jmp done_set_idx_{}\n", idx));

                mir.push_str(&format!("fallback_set_idx_{}:\n", idx));
                if rb < num_regs && types_at_inst[idx][rb] == RegType::Double {
                    mir.push_str(&format!("          dmov d:8(cast_ptr), d{}\n", rb));
                    mir.push_str(&format!("          mov r{}, i64:8(cast_ptr)\n", rb));
                }
                mir.push_str(&format!("          call p_set_index, er_jit_set_index, status, vm, r{}, r{}, r{}\n", ra, rb, rc));
                mir.push_str("          mov status, u8:0(vm)\n");
                mir.push_str("          bne err_label, status, 0\n");
                mir.push_str(&format!("done_set_idx_{}:\n", idx));
            }
            OpCode::Return => {
                let has_closures = func.chunk.code.iter().any(|inst| inst.op == OpCode::Closure);
                if has_closures {
                    mir.push_str("          call p_close_upvalues, er_jit_close_upvalues, status, vm, 0\n");
                }
                if types_at_inst[idx][ra] == RegType::Double {
                    let offset = (ra % 24) * 8;
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset, ra));
                    mir.push_str(&format!("          mov tmp, i64:{}(cast_ptr)\n", offset));
                    mir.push_str("          mov i64:(ret_val_out), tmp\n");
                } else {
                    mir.push_str(&format!("          mov i64:(ret_val_out), r{}\n", ra));
                }
                mir.push_str("          ret 1\n");
            }
            OpCode::Throw => {
                mir.push_str(&format!("          mov i64:(ip_out), {}\n", idx));
                mir.push_str(&format!("          mov i64:(ret_val_out), r{}\n", ra));
                mir.push_str("          ret -1\n");
            }
            OpCode::GetUpvalue => {
                let upval_idx = instruction.operand;
                mir.push_str(&format!("          call p_get_upvalue, er_jit_get_upvalue, r{}, vm, {}\n", ra, upval_idx));
                if next_types[ra] == RegType::Double {
                    let offset = (ra % 24) * 8;
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset, ra));
                    mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset));
                }
            }
            OpCode::SetUpvalue => {
                let upval_idx = instruction.operand;
                if ra < num_regs && types_at_inst[idx][ra] == RegType::Double {
                    let offset = (ra % 24) * 8;
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset, ra));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset));
                }
                mir.push_str(&format!("          call p_set_upvalue, er_jit_set_upvalue, status, vm, {}, r{}\n", upval_idx, ra));
            }
            OpCode::Closure => {
                let const_idx = instruction.operand;
                save_all_registers(&mut mir, idx);
                mir.push_str(&format!("          mov tmp1, i64:{}(constants_ptr)\n", const_idx * 8));
                mir.push_str(&format!("          call p_make_closure, er_jit_make_closure, r{}, vm, tmp1\n", ra));
            }

            OpCode::CloseUpvalue => {
                let rel_slot = instruction.operand;
                mir.push_str(&format!("          call p_close_upvalues, er_jit_close_upvalues, status, vm, {}\n", rel_slot));
            }
            OpCode::Await => {
                let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
                if rb_is_double {
                    let offset = (rb % 24) * 8;
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset, rb));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset));
                }
                save_all_registers(&mut mir, idx);
                mir.push_str(&format!("          add dest_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          call p_await, er_jit_await, status, vm, r{}, dest_ptr\n", rb));
                mir.push_str(&format!("          bne not_suspend_await_{}, status, -3\n", idx));
                mir.push_str(&format!("          mov i64:(ip_out), {}\n", idx));
                mir.push_str(&format!("          mov i64:(dest_reg_out), {}\n", ra));
                mir.push_str(&format!("          mov i64:(func_reg_out), {}\n", rb));
                mir.push_str("          ret 3\n");
                mir.push_str(&format!("not_suspend_await_{}:\n", idx));
                mir.push_str("          blt err_label, status, 0\n");
                mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                if next_types[ra] == RegType::Double {
                    mir.push_str(&format!("          dmov d{}, d:{}(frame_slots)\n", ra, ra * 8));
                }
            }
        }
    }


    mir.push_str("          ret 1\n");
    mir.push_str("err_label:\n");
    mir.push_str("          ret -1\n");
    mir.push_str("          endfunc\n");
    mir.push_str("          endmodule\n");

    let _ = std::fs::write("/home/vishnus/Downloads/eronom/temp_compiled.mir", &mir);
    let c_mir = CString::new(mir).unwrap();

    unsafe {
        MIR_scan_string(ctx, c_mir.as_ptr());
        let list_ptr = MIR_get_module_list(ctx) as *mut MirDlist;
        let module = (*list_ptr).tail as *mut MirModule;
        MIR_load_module(ctx, module as *mut c_void);
        MIR_link(ctx, Some(MIR_set_gen_interface), None);
        
        // Find the function item (type 0) dynamically
        let mut func_item = std::ptr::null_mut();
        let mut curr = (*module).head_item as *mut *mut c_void;
        while !curr.is_null() {
            let item_type = *(curr.offset(4) as *const u32);
            if item_type == 0 {
                func_item = curr as *mut c_void;
                break;
            }
            let next = *(curr.offset(3) as *const *mut c_void);
            curr = next as *mut *mut c_void;
        }

        assert!(!func_item.is_null(), "Could not find MIR_func_item in JIT module");
        let native_ptr = MIR_gen(ctx, func_item);
        
        JIT_STATE.with(|state| {
            if let Some(s) = &mut *state.borrow_mut() {
                s.cache.insert(instructions.clone(), native_ptr);
            }
        });

        func.jit_ptr.set(Some(native_ptr));
        native_ptr
    }
}

fn compute_liveness_with_dead(
    func: &crate::vm::bytecode::Function,
    num_regs: usize,
    is_dead: &[bool],
) -> (Vec<Vec<bool>>, Vec<Vec<bool>>) {
    let code = &func.chunk.code;
    let n = code.len();
    let mut live_in = vec![vec![false; num_regs]; n];
    let mut live_out = vec![vec![false; num_regs]; n];

    let mut gen_set = vec![vec![false; num_regs]; n];
    let mut kill = vec![vec![false; num_regs]; n];

    for pc in 0..n {
        if is_dead[pc] {
            continue;
        }
        let inst = &code[pc];
        let ra = inst.ra as usize;
        let rb = inst.rb as usize;
        let rc = inst.rc as usize;

        match inst.op {
            OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadBool | OpCode::GetGlobal | OpCode::GetUpvalue | OpCode::Closure => {
                if ra < num_regs {
                    kill[pc][ra] = true;
                }
            }
            OpCode::Move | OpCode::Negate | OpCode::Not | OpCode::Await |
            OpCode::BitNot | OpCode::TypeOf | OpCode::ToIter | OpCode::ArrayLen => {
                if rb < num_regs {
                    gen_set[pc][rb] = true;
                }
                if ra < num_regs {
                    kill[pc][ra] = true;
                }
            }
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod |
            OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::ShiftLeft | OpCode::ShiftRight |
            OpCode::Equal | OpCode::Greater | OpCode::Less | OpCode::GetIndex => {
                if rb < num_regs {
                    gen_set[pc][rb] = true;
                }
                if rc < num_regs {
                    gen_set[pc][rc] = true;
                }
                if ra < num_regs {
                    kill[pc][ra] = true;
                }
            }
            OpCode::DefineGlobal | OpCode::SetGlobal | OpCode::JumpIfFalse | OpCode::SetUpvalue => {
                if ra < num_regs {
                    gen_set[pc][ra] = true;
                }
            }
            OpCode::Return | OpCode::Throw => {
                if ra < num_regs {
                    gen_set[pc][ra] = true;
                }
            }
            OpCode::Loop | OpCode::Jump | OpCode::DefineStruct | OpCode::CloseUpvalue => {}
            OpCode::Call => {
                if rb < num_regs {
                    gen_set[pc][rb] = true;
                }
                for i in 0..inst.operand as usize {
                    let r = rb + 1 + i;
                    if r < num_regs {
                        gen_set[pc][r] = true;
                    }
                }
                if ra < num_regs {
                    kill[pc][ra] = true;
                }
            }
            OpCode::MakeArray => {
                for i in 0..inst.operand as usize {
                    let r = rb + i;
                    if r < num_regs {
                        gen_set[pc][r] = true;
                    }
                }
                if ra < num_regs {
                    kill[pc][ra] = true;
                }
            }
            OpCode::MakeObject => {
                for i in 0..(inst.operand as usize * 2) {
                    let r = rb + i;
                    if r < num_regs {
                        gen_set[pc][r] = true;
                    }
                }
                if ra < num_regs {
                    kill[pc][ra] = true;
                }
            }
            OpCode::GetProperty => {
                if rb < num_regs {
                    gen_set[pc][rb] = true;
                }
                if ra < num_regs {
                    kill[pc][ra] = true;
                }
            }
            OpCode::SetProperty => {
                if ra < num_regs {
                    gen_set[pc][ra] = true;
                }
                if rb < num_regs {
                    gen_set[pc][rb] = true;
                }
            }
            OpCode::SetIndex => {
                if ra < num_regs {
                    gen_set[pc][ra] = true;
                }
                if rb < num_regs {
                    gen_set[pc][rb] = true;
                }
                if rc < num_regs {
                    gen_set[pc][rc] = true;
                }
            }
        }
    }

    let mut worklist: Vec<usize> = (0..n).collect();
    let mut in_worklist = vec![true; n];

    while let Some(pc) = worklist.pop() {
        in_worklist[pc] = false;

        let mut successors = Vec::new();
        let inst = &code[pc];
        match inst.op {
            OpCode::Return | OpCode::Throw => {}
            OpCode::Jump => {
                let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                successors.push(target);
            }
            OpCode::Loop => {
                let target = (pc as i32 + 1 - inst.operand as i32) as usize;
                successors.push(target);
            }
            OpCode::JumpIfFalse => {
                let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                successors.push(target);
                successors.push(pc + 1);
            }
            _ => {
                successors.push(pc + 1);
            }
        }

        let mut new_live_out = vec![false; num_regs];
        for succ in successors {
            if succ < n {
                for r in 0..num_regs {
                    if live_in[succ][r] {
                        new_live_out[r] = true;
                    }
                }
            }
        }
        live_out[pc] = new_live_out;

        let mut changed = false;
        for r in 0..num_regs {
            let val = gen_set[pc][r] || (live_out[pc][r] && !kill[pc][r]);
            if val != live_in[pc][r] {
                live_in[pc][r] = val;
                changed = true;
            }
        }

        if changed {
            for pred in 0..n {
                let p_inst = &code[pred];
                let mut is_pred = false;
                match p_inst.op {
                    OpCode::Return | OpCode::Throw => {}
                    OpCode::Jump => {
                        let target = (pred as i32 + 1 + p_inst.operand as i32) as usize;
                        if target == pc {
                            is_pred = true;
                        }
                    }
                    OpCode::Loop => {
                        let target = (pred as i32 + 1 - p_inst.operand as i32) as usize;
                        if target == pc {
                            is_pred = true;
                        }
                    }
                    OpCode::JumpIfFalse => {
                        let target = (pred as i32 + 1 + p_inst.operand as i32) as usize;
                        if target == pc || pred + 1 == pc {
                            is_pred = true;
                        }
                    }
                    _ => {
                        if pred + 1 == pc {
                            is_pred = true;
                        }
                    }
                }

                if is_pred && !in_worklist[pred] {
                    worklist.push(pred);
                    in_worklist[pred] = true;
                }
            }
        }
    }

    (live_in, live_out)
}

fn eliminate_dead_instructions(
    func: &crate::vm::bytecode::Function,
    num_regs: usize,
) -> (Vec<bool>, Vec<Vec<bool>>, Vec<Vec<bool>>) {
    let n = func.chunk.code.len();
    let mut is_dead = vec![false; n];
    let mut live_in = vec![vec![false; num_regs]; n];
    let mut live_out = vec![vec![false; num_regs]; n];

    for _ in 0..8 {
        let (in_set, out_set) = compute_liveness_with_dead(func, num_regs, &is_dead);
        live_in = in_set;
        live_out = out_set;
        let mut changed = false;

        for pc in 0..n {
            if is_dead[pc] {
                continue;
            }
            let inst = &func.chunk.code[pc];
            let ra = inst.ra as usize;
            if ra < num_regs && !live_out[pc][ra] {
                match inst.op {
                    OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod |
                    OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::BitNot |
                    OpCode::ShiftLeft | OpCode::ShiftRight | OpCode::Negate | OpCode::Not |
                    OpCode::Equal | OpCode::Greater | OpCode::Less |
                    OpCode::GetProperty | OpCode::GetIndex | OpCode::TypeOf | OpCode::ArrayLen |
                    OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadBool | OpCode::Move |
                    OpCode::MakeArray | OpCode::MakeObject => {
                        is_dead[pc] = true;
                        changed = true;
                    }
                    OpCode::Call => {
                        let callee_reg = inst.rb as usize;
                        if callee_reg < num_regs {
                            for prev in (0..pc).rev() {
                                if func.chunk.code[prev].ra as usize == callee_reg {
                                    if func.chunk.code[prev].op == OpCode::GetProperty {
                                        let c_idx = func.chunk.code[prev].operand as usize;
                                        if c_idx < func.chunk.constants.len() {
                                            let name = func.chunk.constants[c_idx].as_str().unwrap_or("");
                                            if name == "push" || name == "pop" {
                                                let arr_reg = func.chunk.code[prev].rb as usize;
                                                if arr_reg < num_regs && !live_out[pc][arr_reg] {
                                                    is_dead[pc] = true;
                                                    changed = true;
                                                }
                                            }
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if !changed {
            break;
        }
    }

    (is_dead, live_in, live_out)
}

// Register FFI Helper Functions in the MIR JIT compiler
unsafe fn register_helpers(ctx: *mut c_void) {
    let helpers: &[(&str, *mut c_void)] = &[
        ("er_jit_negate", helpers::er_jit_negate as *mut c_void),
        ("er_jit_not", helpers::er_jit_not as *mut c_void),
        ("er_jit_add", helpers::er_jit_add as *mut c_void),
        ("er_jit_sub", helpers::er_jit_sub as *mut c_void),
        ("er_jit_mul", helpers::er_jit_mul as *mut c_void),
        ("er_jit_div", helpers::er_jit_div as *mut c_void),
        ("er_jit_mod", helpers::er_jit_mod as *mut c_void),
        ("er_jit_bit_and", helpers::er_jit_bit_and as *mut c_void),
        ("er_jit_bit_or", helpers::er_jit_bit_or as *mut c_void),
        ("er_jit_bit_xor", helpers::er_jit_bit_xor as *mut c_void),
        ("er_jit_bit_not", helpers::er_jit_bit_not as *mut c_void),
        ("er_jit_shift_left", helpers::er_jit_shift_left as *mut c_void),
        ("er_jit_shift_right", helpers::er_jit_shift_right as *mut c_void),
        ("er_jit_typeof", helpers::er_jit_typeof as *mut c_void),
        ("er_jit_to_iter", helpers::er_jit_to_iter as *mut c_void),
        ("er_jit_array_len_op", helpers::er_jit_array_len_op as *mut c_void),
        ("er_jit_equal", helpers::er_jit_equal as *mut c_void),
        ("er_jit_greater", helpers::er_jit_greater as *mut c_void),
        ("er_jit_less", helpers::er_jit_less as *mut c_void),
        ("er_jit_define_global", helpers::er_jit_define_global as *mut c_void),
        ("er_jit_get_global", helpers::er_jit_get_global as *mut c_void),
        ("er_jit_set_global", helpers::er_jit_set_global as *mut c_void),
        ("er_jit_make_array", helpers::er_jit_make_array as *mut c_void),
        ("er_jit_make_object", helpers::er_jit_make_object as *mut c_void),
        ("er_jit_get_property", helpers::er_jit_get_property as *mut c_void),
        ("er_jit_set_property", helpers::er_jit_set_property as *mut c_void),
        ("er_jit_get_index", helpers::er_jit_get_index as *mut c_void),
        ("er_jit_set_index", helpers::er_jit_set_index as *mut c_void),
        ("er_jit_call_fast", helpers::er_jit_call_fast as *mut c_void),
        ("er_jit_call_non_vm", helpers::er_jit_call_non_vm as *mut c_void),
        ("er_jit_array_push", helpers::er_jit_array_push as *mut c_void),
        ("er_jit_array_pop", helpers::er_jit_array_pop as *mut c_void),
        ("er_jit_has_error", helpers::er_jit_has_error as *mut c_void),
        ("er_jit_needs_gc", helpers::er_jit_needs_gc as *mut c_void),
        ("er_gc_needs_step", &crate::vm::gc::GC_NEEDS_STEP as *const _ as *mut c_void),
        ("er_jit_define_struct", helpers::er_jit_define_struct as *mut c_void),
        ("er_jit_get_upvalue", helpers::er_jit_get_upvalue as *mut c_void),
        ("er_jit_set_upvalue", helpers::er_jit_set_upvalue as *mut c_void),
        ("er_jit_make_closure", helpers::er_jit_make_closure as *mut c_void),
        ("er_jit_close_upvalues", helpers::er_jit_close_upvalues as *mut c_void),
        ("er_jit_await", helpers::er_jit_await as *mut c_void),
        ("er_jit_write_barrier", helpers::er_jit_write_barrier as *mut c_void),
    ];

    for &(name, ptr) in helpers {
        let cname = CString::new(name).unwrap();
        unsafe {
            MIR_load_external(ctx, cname.as_ptr(), ptr);
        }
    }
}
