use crate::vm::bytecode::{Function, Instruction, OpCode};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegType {
    Unknown,
    Double,
}

pub fn resolve_branch_target(code: &[Instruction], mut target: usize) -> usize {
    let mut depth = 0;
    while target < code.len() && depth < 16 {
        let inst = &code[target];
        if inst.op == OpCode::Jump {
            target = (target as i32 + 1 + inst.operand as i32) as usize;
            depth += 1;
        } else if inst.op == OpCode::JumpIfFalse {
            target = (target as i32 + 1 + inst.operand as i32) as usize;
            depth += 1;
        } else {
            break;
        }
    }
    target
}

pub fn analyze_types(
    func: &Function,
    num_regs: usize,
    param_is_double: &[bool],
) -> (Vec<Vec<RegType>>, Vec<Vec<bool>>) {
    let debug_jit = std::env::var("ER_DEBUG_JIT").is_ok();
    let mut types_at_inst = vec![vec![RegType::Unknown; num_regs]; func.chunk.code.len()];
    let mut is_init = vec![vec![false; num_regs]; func.chunk.code.len()];
    if num_regs > 0 {
        for r in 0..num_regs {
            if r < func.arity {
                is_init[0][r] = true;
                if param_is_double[r] {
                    types_at_inst[0][r] = RegType::Double;
                } else {
                    types_at_inst[0][r] = RegType::Unknown;
                }
            } else {
                types_at_inst[0][r] = RegType::Unknown;
            }
        }
    }

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
        let mut next_init = is_init[pc].clone();
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
                    next_init[ra] = true;
                    next_types[ra] = if val.is_number() { RegType::Double } else { RegType::Unknown };
                }
            }
            OpCode::Move => {
                if ra < num_regs {
                    next_init[ra] = true;
                    if rb < num_regs {
                        next_types[ra] = current_types[rb];
                    }
                }
            }
            OpCode::Add => {
                if ra < num_regs {
                    next_init[ra] = true;
                    if rb < num_regs && rc < num_regs {
                        next_types[ra] = if current_types[rb] == RegType::Double && current_types[rc] == RegType::Double {
                            RegType::Double
                        } else {
                            RegType::Unknown
                        };
                    }
                }
            }
            OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Negate |
            OpCode::Mod | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::BitNot | OpCode::ShiftLeft | OpCode::ShiftRight | OpCode::ArrayLen => {
                if ra < num_regs {
                    next_init[ra] = true;
                    next_types[ra] = RegType::Double;
                }
            }
            OpCode::Equal | OpCode::Greater | OpCode::Less => {
                let is_fused_3 = if pc + 2 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[pc + 1];
                    let jmp_inst = &func.chunk.code[pc + 2];
                    next_inst.op == OpCode::Not && next_inst.ra == inst.ra && next_inst.rb == inst.ra
                        && jmp_inst.op == OpCode::JumpIfFalse && jmp_inst.ra == inst.ra
                } else {
                    false
                };
                let is_fused_2 = if !is_fused_3 && pc + 1 < func.chunk.code.len() {
                    let jmp_inst = &func.chunk.code[pc + 1];
                    jmp_inst.op == OpCode::JumpIfFalse && jmp_inst.ra == inst.ra
                } else {
                    false
                };
                if !is_fused_3 && !is_fused_2 && ra < num_regs {
                    next_init[ra] = true;
                    next_types[ra] = RegType::Unknown;
                }
            }
            OpCode::Not |
            OpCode::LoadNull | OpCode::LoadBool |
            OpCode::GetGlobal | OpCode::GetProperty | OpCode::GetIndex |
            OpCode::MakeArray | OpCode::MakeObject | OpCode::TypeOf | OpCode::ToIter |
            OpCode::GetUpvalue | OpCode::Closure | OpCode::Await | OpCode::Call => {
                if ra < num_regs {
                    next_init[ra] = true;
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
                let raw_target = (pc as i32 + 1 + inst.operand as i32) as usize;
                let target = resolve_branch_target(&func.chunk.code, raw_target);
                successors.push(pc + 1);
                successors.push(target);
            }
            OpCode::Less | OpCode::Greater | OpCode::Equal => {
                let is_fused_3 = if pc + 2 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[pc + 1];
                    let jmp_inst = &func.chunk.code[pc + 2];
                    next_inst.op == OpCode::Not && next_inst.ra == inst.ra && next_inst.rb == inst.ra
                        && jmp_inst.op == OpCode::JumpIfFalse && jmp_inst.ra == inst.ra
                } else {
                    false
                };
                let is_fused_2 = if !is_fused_3 && pc + 1 < func.chunk.code.len() {
                    let jmp_inst = &func.chunk.code[pc + 1];
                    jmp_inst.op == OpCode::JumpIfFalse && jmp_inst.ra == inst.ra
                } else {
                    false
                };

                if is_fused_3 {
                    let raw_target = (pc + 3 + func.chunk.code[pc + 2].operand as usize) as usize;
                    let target = resolve_branch_target(&func.chunk.code, raw_target);
                    successors.push(pc + 3);
                    successors.push(target);
                } else if is_fused_2 {
                    let raw_target = (pc + 2 + func.chunk.code[pc + 1].operand as usize) as usize;
                    let target = resolve_branch_target(&func.chunk.code, raw_target);
                    successors.push(pc + 2);
                    successors.push(target);
                } else {
                    successors.push(pc + 1);
                }
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
                if next_init[r] && !is_init[succ][r] {
                    is_init[succ][r] = true;
                    changed = true;
                }
                let old = types_at_inst[succ][r];
                let new_t = if !visited[succ] {
                    next_types[r]
                } else if old == RegType::Double && next_types[r] == RegType::Double {
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

    (types_at_inst, is_init)
}
