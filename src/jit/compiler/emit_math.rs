use crate::vm::bytecode::{Function, Instruction, OpCode};
use crate::vm::value::{Value, TAG_FALSE, TAG_NULL, TAG_TRUE};
use super::type_flow::RegType;

pub fn emit_math_and_unary(
    mir: &mut String,
    idx: usize,
    instruction: &Instruction,
    func: &Function,
    ra: usize,
    rb: usize,
    rc: usize,
    num_regs: usize,
    types_at_inst: &[Vec<RegType>],
    next_types: &[RegType],
) {
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
            let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
            let next_ra_is_double = next_types[ra] == RegType::Double;
            if rb_is_double {
                mir.push_str(&format!("          dmov d{}, d{}\n", ra, rb));
                if !next_ra_is_double {
                    let offset = (ra % 24) * 8;
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset, ra));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset));
                }
            } else {
                mir.push_str(&format!("          mov r{}, r{}\n", ra, rb));
                if next_ra_is_double {
                    let offset = (ra % 24) * 8;
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset, ra));
                    mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset));
                }
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
            let prev_was_fused_cmp_not = if idx > 0 {
                let prev_inst = &func.chunk.code[idx - 1];
                let next_is_jmp = if idx + 1 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[idx + 1];
                    next_inst.op == OpCode::JumpIfFalse && next_inst.ra == instruction.ra
                } else {
                    false
                };
                matches!(prev_inst.op, OpCode::Less | OpCode::Greater | OpCode::Equal) && instruction.ra == prev_inst.ra && instruction.rb == prev_inst.ra && next_is_jmp
            } else {
                false
            };

            if !prev_was_fused_cmp_not {
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
        OpCode::TypeOf => {
            let offset1 = ((idx * 3) % 24) * 8;
            let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
            if rb_is_double {
                mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
            }
            mir.push_str(&format!("          call p_typeof, er_jit_typeof, r{}, vm, r{}\n", ra, rb));
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
        _ => {}
    }
}
