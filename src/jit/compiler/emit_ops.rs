use crate::vm::bytecode::{Function, Instruction, OpCode};
use super::type_flow::RegType;
use super::emit_math::emit_math_and_unary;
use super::emit_cmp::emit_comparisons;

pub fn emit_op(
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
    sync_edge: &impl Fn(&mut String, usize, usize),
) {
    match instruction.op {
        OpCode::Equal | OpCode::Greater | OpCode::Less => {
            emit_comparisons(
                mir,
                idx,
                instruction,
                func,
                ra,
                rb,
                rc,
                num_regs,
                types_at_inst,
                sync_edge,
            );
        }
        _ => {
            emit_math_and_unary(
                mir,
                idx,
                instruction,
                func,
                ra,
                rb,
                rc,
                num_regs,
                types_at_inst,
                next_types,
            );
        }
    }
}
