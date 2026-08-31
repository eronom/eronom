use crate::vm::bytecode::{Function, Instruction, OpCode};
use super::type_flow::RegType;

pub fn emit_obj(
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
            let offset = (ra % 24) * 8;
            mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset, ra));
            mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", ra, offset));
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
                mir.push_str("          mov tmp1, i64:16(desc_ptr)\n");
                mir.push_str(&format!("          bne check_ff1_{}, tmp1, name_ptr\n", idx));
                mir.push_str("          mov tmp2, i64:24(desc_ptr)\n");
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
                mir.push_str("          mov tmp1, i64:32(desc_ptr)\n");
                mir.push_str(&format!("          bne check_ff2_{}, tmp1, name_ptr\n", idx));
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

                // Check fast_fields[2]
                mir.push_str(&format!("check_ff2_{}:\n", idx));
                mir.push_str("          mov tmp1, i64:48(desc_ptr)\n");
                mir.push_str(&format!("          bne check_ff3_{}, tmp1, name_ptr\n", idx));
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

                // Check fast_fields[3]
                mir.push_str(&format!("check_ff3_{}:\n", idx));
                mir.push_str("          mov tmp1, i64:64(desc_ptr)\n");
                mir.push_str(&format!("          bne fallback_get_prop_{}, tmp1, name_ptr\n", idx));
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
            mir.push_str("          mov tmp1, i64:16(desc_ptr)\n");
            mir.push_str(&format!("          bne check_set_ff1_{}, tmp1, name_ptr\n", idx));
            mir.push_str("          mov tmp2, i64:24(desc_ptr)\n");
            mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
            mir.push_str("          mul tmp3, tmp2, 8\n");
            mir.push_str("          add start_ptr, start_ptr, tmp3\n");
            mir.push_str(&format!("          mov i64:0(start_ptr), r{}\n", rb));
            mir.push_str(&format!("          jmp check_wb_set_prop_{}\n", idx));

            // Check fast_fields[1]
            mir.push_str(&format!("check_set_ff1_{}:\n", idx));
            mir.push_str("          mov tmp1, i64:32(desc_ptr)\n");
            mir.push_str(&format!("          bne check_set_ff2_{}, tmp1, name_ptr\n", idx));
            mir.push_str("          mov tmp2, i64:40(desc_ptr)\n");
            mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
            mir.push_str("          mul tmp3, tmp2, 8\n");
            mir.push_str("          add start_ptr, start_ptr, tmp3\n");
            mir.push_str(&format!("          mov i64:0(start_ptr), r{}\n", rb));
            mir.push_str(&format!("          jmp check_wb_set_prop_{}\n", idx));

            // Check fast_fields[2]
            mir.push_str(&format!("check_set_ff2_{}:\n", idx));
            mir.push_str("          mov tmp1, i64:48(desc_ptr)\n");
            mir.push_str(&format!("          bne check_set_ff3_{}, tmp1, name_ptr\n", idx));
            mir.push_str("          mov tmp2, i64:56(desc_ptr)\n");
            mir.push_str("          mov start_ptr, i64:40(obj_ptr)\n");
            mir.push_str("          mul tmp3, tmp2, 8\n");
            mir.push_str("          add start_ptr, start_ptr, tmp3\n");
            mir.push_str(&format!("          mov i64:0(start_ptr), r{}\n", rb));
            mir.push_str(&format!("          jmp check_wb_set_prop_{}\n", idx));

            // Check fast_fields[3]
            mir.push_str(&format!("check_set_ff3_{}:\n", idx));
            mir.push_str("          mov tmp1, i64:64(desc_ptr)\n");
            mir.push_str(&format!("          bne fallback_set_prop_{}, tmp1, name_ptr\n", idx));
            mir.push_str("          mov tmp2, i64:72(desc_ptr)\n");
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
        _ => {}
    }
}
