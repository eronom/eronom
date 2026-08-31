use crate::vm::bytecode::{Function, Instruction, OpCode};
use crate::vm::value::{Value, TAG_FALSE, TAG_NULL};
use crate::vm::gc::GcObject;
use super::type_flow::{resolve_branch_target, RegType};

pub fn emit_call_and_control(
    mir: &mut String,
    idx: usize,
    instruction: &Instruction,
    func: &Function,
    func_obj: *mut GcObject,
    func_name: &str,
    ra: usize,
    rb: usize,
    _rc: usize,
    num_regs: usize,
    types_at_inst: &[Vec<RegType>],
    next_types: &[RegType],
    is_init: &[Vec<bool>],
    live_in: &[Vec<bool>],
    save_all_registers: &impl Fn(&mut String, usize),
    sync_edge: &impl Fn(&mut String, usize, usize),
) {
    match instruction.op {
        OpCode::Jump => {
            let target = (idx as i32 + 1 + instruction.operand as i32) as usize;
            sync_edge(mir, idx, target);
            mir.push_str(&format!("          jmp inst_{}\n", target));
        }
        OpCode::Loop => {
            let target = (idx as i32 + 1 - instruction.operand as i32) as usize;
            mir.push_str("          add loop_counter, loop_counter, 1\n");
            mir.push_str("          and tmp, loop_counter, 127\n");
            mir.push_str(&format!("          bne no_yield_gc_{}, tmp, 0\n", idx));
            mir.push_str("          mov tmp1, er_gc_needs_step\n");
            mir.push_str("          mov status, u8:0(tmp1)\n");
            mir.push_str(&format!("          beq no_yield_gc_{}, status, 0\n", idx));
            save_all_registers(mir, idx);
            mir.push_str(&format!("          mov i64:(ip_out), {}\n", target));
            mir.push_str("          ret 2\n");
            mir.push_str(&format!("no_yield_gc_{}:\n", idx));
            sync_edge(mir, idx, target);
            mir.push_str(&format!("          jmp inst_{}\n", target));
        }
        OpCode::JumpIfFalse => {
            let prev_was_optimized_cmp = if idx > 0 {
                let prev_inst = &func.chunk.code[idx - 1];
                let prev_was_2inst_cmp = matches!(prev_inst.op, OpCode::Less | OpCode::Greater | OpCode::Equal) && instruction.ra == prev_inst.ra;
                let prev_was_3inst_cmp = if idx > 1 {
                    let pprev_inst = &func.chunk.code[idx - 2];
                    prev_inst.op == OpCode::Not && prev_inst.ra == instruction.ra && prev_inst.rb == instruction.ra
                        && matches!(pprev_inst.op, OpCode::Less | OpCode::Greater | OpCode::Equal) && pprev_inst.ra == instruction.ra
                } else {
                    false
                };
                prev_was_2inst_cmp || prev_was_3inst_cmp
            } else {
                false
            };
            if !prev_was_optimized_cmp {
                let raw_target = (idx as i32 + 1 + instruction.operand as i32) as usize;
                let target = resolve_branch_target(&func.chunk.code, raw_target);
                if types_at_inst[idx][ra] != RegType::Double {
                    mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_FALSE));
                    mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_NULL));
                    // Fall-through path
                    sync_edge(mir, idx, idx + 1);
                    mir.push_str(&format!("          jmp inst_{}\n", idx + 1));
                    // Branch taken path
                    mir.push_str(&format!("take_branch_{}:\n", idx));
                    sync_edge(mir, idx, target);
                    mir.push_str(&format!("          jmp inst_{}\n", target));
                } else {
                    // Always fall through, but we still need to sync registers for the fall-through path!
                    sync_edge(mir, idx, idx + 1);
                    mir.push_str(&format!("          jmp inst_{}\n", idx + 1));
                }
            }
        }
        OpCode::Call => {
            let arg_count = instruction.operand as usize;
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
                save_all_registers(mir, idx);
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
                mir.push_str("          call p_array_pop, er_jit_array_pop, tmp2, tmp\n");
                if next_types[ra] == RegType::Double {
                    mir.push_str("          mov i64:0(cast_ptr), tmp2\n");
                    mir.push_str(&format!("          dmov d{}, d:0(cast_ptr)\n", ra));
                } else {
                    mir.push_str(&format!("          mov r{}, tmp2\n", ra));
                }
                mir.push_str(&format!("          jmp done_call_{}\n", idx));

                mir.push_str(&format!("normal_call_{}:\n", idx));
                save_all_registers(mir, idx);
            } else {
                save_all_registers(mir, idx);
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
            mir.push_str("          blt err_label, status, -1\n");
            mir.push_str(&format!("          bne not_fast_call_{}, status, 0\n", idx));
            mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
            mir.push_str(&format!("          dmov d{}, d:{}(frame_slots)\n", ra, ra * 8));
            mir.push_str(&format!("          jmp done_call_{}\n", idx));
            mir.push_str(&format!("not_fast_call_{}:\n", idx));

            mir.push_str(&format!("          call p_call_non_vm, er_jit_call_non_vm, status, vm, dest_ptr, r{}, {}, {}, frame_slots, {}, {}\n", rb, rb, arg_count, idx, ra));
            mir.push_str(&format!("          beq call_vm_label_{}, status, -1\n", idx));
            mir.push_str(&format!("          beq suspend_label_{}, status, -3\n", idx));
            mir.push_str("          blt err_label, status, 0\n");
            mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
            mir.push_str(&format!("          dmov d{}, d:{}(frame_slots)\n", ra, ra * 8));
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
        OpCode::Return => {
            let has_closures = func.chunk.code.iter().any(|inst| inst.op == OpCode::Closure);
            if has_closures {
                save_all_registers(mir, idx);
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
            let offset = (ra % 24) * 8;
            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset, ra));
            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset));
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
            let has_closures = true;
            save_all_registers(mir, idx);
            mir.push_str(&format!("          mov tmp1, i64:{}(constants_ptr)\n", const_idx * 8));
            mir.push_str(&format!("          call p_make_closure, er_jit_make_closure, r{}, vm, tmp1\n", ra));
            for r in 0..num_regs {
                if r != ra && is_init[idx][r] && (has_closures || live_in[idx][r]) {
                    if types_at_inst[idx][r] == RegType::Double {
                        mir.push_str(&format!("          dmov d{}, d:{}(frame_slots)\n", r, r * 8));
                    } else {
                        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", r, r * 8));
                    }
                }
            }
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
            save_all_registers(mir, idx);
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
        _ => {}
    }
}
