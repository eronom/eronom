use crate::vm::bytecode::Function;
use crate::vm::value::{TAG_FALSE, TAG_NULL, TAG_TRUE};
use super::type_flow::{resolve_branch_target, RegType};

pub fn emit_equal(
    mir: &mut String,
    idx: usize,
    func: &Function,
    ra: usize,
    rb: usize,
    rc: usize,
    num_regs: usize,
    types_at_inst: &[Vec<RegType>],
    sync_edge: &impl Fn(&mut String, usize, usize),
) {
    let offset1 = ((idx * 3) % 24) * 8;
    let offset2 = ((idx * 3 + 1) % 24) * 8;
    let is_fused_3 = if idx + 2 < func.chunk.code.len() {
        let next_inst = &func.chunk.code[idx + 1];
        let jmp_inst = &func.chunk.code[idx + 2];
        next_inst.op == crate::vm::bytecode::OpCode::Not && next_inst.ra == ra as u8 && next_inst.rb == ra as u8
            && jmp_inst.op == crate::vm::bytecode::OpCode::JumpIfFalse && jmp_inst.ra == ra as u8
    } else {
        false
    };
    let is_fused_2 = if !is_fused_3 && idx + 1 < func.chunk.code.len() {
        let next_inst = &func.chunk.code[idx + 1];
        next_inst.op == crate::vm::bytecode::OpCode::JumpIfFalse && next_inst.ra == ra as u8
    } else {
        false
    };

    let rb_is_double = rb < num_regs && types_at_inst[idx][rb] == RegType::Double;
    let rc_is_double = rc < num_regs && types_at_inst[idx][rc] == RegType::Double;

    if is_fused_3 {
        let jmp_inst = &func.chunk.code[idx + 2];
        let raw_target = (idx + 3 + jmp_inst.operand as usize) as usize;
        let target = resolve_branch_target(&func.chunk.code, raw_target);

        if rb_is_double && rc_is_double {
            mir.push_str(&format!("          dbeq take_branch_{}, d{}, d{}\n", idx, rb, rc));
            sync_edge(mir, idx, idx + 3);
            mir.push_str(&format!("          jmp inst_{}\n", idx + 3));
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
            mir.push_str(&format!("          dbeq take_branch_{}, d{}, d{}\n", idx, rb, rc));
            sync_edge(mir, idx, idx + 3);
            mir.push_str(&format!("          jmp inst_{}\n", idx + 3));

            mir.push_str(&format!("fallback_eq_{}:\n", idx));
            if rb_is_double {
                mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset1, rb));
                mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rb, offset1));
            }
            if rc_is_double {
                mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset2, rc));
                mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", rc, offset2));
            }
            mir.push_str(&format!("          call p_equal, er_jit_equal, r{}, vm, r{}, r{}\n", ra, rb, rc));
            mir.push_str("          mov status, u8:0(vm)\n");
            mir.push_str("          bne err_label, status, 0\n");
            mir.push_str(&format!("          beq take_branch_{}, r{}, {}\n", idx, ra, TAG_TRUE));
            sync_edge(mir, idx, idx + 3);
            mir.push_str(&format!("          jmp inst_{}\n", idx + 3));
        }
        mir.push_str(&format!("take_branch_{}:\n", idx));
        sync_edge(mir, idx, target);
        mir.push_str(&format!("          jmp inst_{}\n", target));
    } else if is_fused_2 {
        let next_inst = &func.chunk.code[idx + 1];
        let raw_target = (idx + 2 + next_inst.operand as usize) as usize;
        let target = resolve_branch_target(&func.chunk.code, raw_target);

        if rb_is_double && rc_is_double {
            mir.push_str(&format!("          dbne take_branch_{}, d{}, d{}\n", idx, rb, rc));
            sync_edge(mir, idx, idx + 2);
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
            sync_edge(mir, idx, idx + 2);
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
            sync_edge(mir, idx, idx + 2);
            mir.push_str(&format!("          jmp inst_{}\n", idx + 2));
        }
        mir.push_str(&format!("take_branch_{}:\n", idx));
        sync_edge(mir, idx, target);
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
