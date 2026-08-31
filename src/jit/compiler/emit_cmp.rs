use crate::vm::bytecode::{Function, Instruction, OpCode};
use super::type_flow::RegType;
use super::emit_cmp_equal::emit_equal;
use super::emit_cmp_rel::{emit_greater, emit_less};

pub fn emit_comparisons(
    mir: &mut String,
    idx: usize,
    instruction: &Instruction,
    func: &Function,
    ra: usize,
    rb: usize,
    rc: usize,
    num_regs: usize,
    types_at_inst: &[Vec<RegType>],
    sync_edge: &impl Fn(&mut String, usize, usize),
) {
    match instruction.op {
        OpCode::Equal => {
            emit_equal(mir, idx, func, ra, rb, rc, num_regs, types_at_inst, sync_edge);
        }
        OpCode::Greater => {
            emit_greater(mir, idx, func, ra, rb, rc, num_regs, types_at_inst, sync_edge);
        }
        OpCode::Less => {
            emit_less(mir, idx, func, ra, rb, rc, num_regs, types_at_inst, sync_edge);
        }
        _ => {}
    }
}
