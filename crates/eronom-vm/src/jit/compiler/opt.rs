use crate::vm::bytecode::{Function, OpCode};

pub fn compute_liveness_with_dead(
    func: &Function,
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

pub fn get_aliased_registers(func: &Function, root_reg: usize, is_dead: &[bool]) -> Vec<usize> {
    let mut regs = vec![root_reg];
    for pc in 0..func.chunk.code.len() {
        if is_dead[pc] {
            continue;
        }
        let inst = &func.chunk.code[pc];
        if inst.op == OpCode::Move && regs.contains(&(inst.rb as usize)) {
            let ra = inst.ra as usize;
            if !regs.contains(&ra) {
                regs.push(ra);
            }
        }
    }
    regs
}

pub fn array_escapes(
    func: &Function,
    arr_reg: usize,
    num_regs: usize,
    is_dead: &[bool],
    live_out: &[Vec<bool>],
) -> bool {
    let code = &func.chunk.code;
    let n = code.len();
    let arr_aliases = get_aliased_registers(func, arr_reg, is_dead);

    for pc in 0..n {
        if is_dead[pc] {
            continue;
        }
        let inst = &code[pc];
        let ra = inst.ra as usize;
        let rb = inst.rb as usize;

        match inst.op {
            OpCode::SetGlobal | OpCode::DefineGlobal | OpCode::Return | OpCode::Throw |
            OpCode::SetUpvalue | OpCode::Closure => {
                if arr_aliases.contains(&ra) {
                    return true;
                }
            }
            OpCode::SetProperty => {
                if arr_aliases.contains(&ra) || arr_aliases.contains(&rb) {
                    return true;
                }
            }
            OpCode::MakeArray => {
                for i in 0..inst.operand as usize {
                    if arr_aliases.contains(&(rb + i)) {
                        return true;
                    }
                }
            }
            OpCode::MakeObject => {
                for i in 0..(inst.operand as usize * 2) {
                    if arr_aliases.contains(&(rb + i)) {
                        return true;
                    }
                }
            }
            OpCode::Call => {
                if arr_aliases.contains(&rb) {
                    return true;
                }
                let mut is_array_method_call = false;
                for prev in (0..pc).rev() {
                    if !is_dead[prev] && code[prev].ra as usize == rb {
                        if code[prev].op == OpCode::GetProperty && arr_aliases.contains(&(code[prev].rb as usize)) {
                            let c_idx = code[prev].operand as usize;
                            if c_idx < func.chunk.constants.len() {
                                let name = func.chunk.constants[c_idx].as_str().unwrap_or("");
                                if name == "push" || name == "pop" || name == "length" {
                                    is_array_method_call = true;
                                }
                            }
                        }
                        break;
                    }
                }

                if !is_array_method_call {
                    for i in 0..inst.operand as usize {
                        if arr_aliases.contains(&(rb + 1 + i)) {
                            return true;
                        }
                    }
                }
            }
            OpCode::GetProperty => {
                if arr_aliases.contains(&rb) {
                    let c_idx = inst.operand as usize;
                    if c_idx < func.chunk.constants.len() {
                        let name = func.chunk.constants[c_idx].as_str().unwrap_or("");
                        if name == "push" || name == "pop" || name == "length" {
                            let method_reg = ra;
                            if method_reg < num_regs && live_out[pc][method_reg] {
                                for call_pc in (pc + 1)..n {
                                    if !is_dead[call_pc] && code[call_pc].op == OpCode::Call && code[call_pc].rb as usize == method_reg {
                                        let dest_reg = code[call_pc].ra as usize;
                                        if dest_reg < num_regs && live_out[call_pc][dest_reg] {
                                            return true;
                                        }
                                        break;
                                    }
                                }
                            }
                        } else {
                            return true;
                        }
                    } else {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

pub fn eliminate_dead_instructions(
    func: &Function,
    num_regs: usize,
    is_resume_target: &[bool],
) -> (Vec<bool>, Vec<Vec<bool>>, Vec<Vec<bool>>) {
    let n = func.chunk.code.len();
    let has_closures = func.chunk.code.iter().any(|inst| inst.op == OpCode::Closure);
    if has_closures {
        let (in_set, out_set) = compute_liveness_with_dead(func, num_regs, &vec![false; n]);
        return (vec![false; n], in_set, out_set);
    }
    let mut is_dead = vec![false; n];
    let mut live_in = vec![vec![false; num_regs]; n];
    let mut live_out = vec![vec![false; num_regs]; n];

    for _ in 0..8 {
        let (in_set, out_set) = compute_liveness_with_dead(func, num_regs, &is_dead);
        live_in = in_set;
        live_out = out_set;
        let mut changed = false;

        for pc in 0..n {
            if is_dead[pc] || (pc < is_resume_target.len() && is_resume_target[pc]) {
                continue;
            }
            let inst = &func.chunk.code[pc];
            let ra = inst.ra as usize;

            if inst.op == OpCode::MakeArray {
                if ra < num_regs && !array_escapes(func, ra, num_regs, &is_dead, &live_out) {
                    is_dead[pc] = true;
                    changed = true;
                    let arr_aliases = get_aliased_registers(func, ra, &is_dead);
                    for user_pc in (pc + 1)..n {
                        if !is_dead[user_pc] {
                            let u_inst = &func.chunk.code[user_pc];
                            if u_inst.op == OpCode::Move && arr_aliases.contains(&(u_inst.rb as usize)) {
                                is_dead[user_pc] = true;
                            } else if u_inst.op == OpCode::GetProperty && arr_aliases.contains(&(u_inst.rb as usize)) {
                                let method_reg = u_inst.ra as usize;
                                is_dead[user_pc] = true;
                                for call_pc in (user_pc + 1)..n {
                                    if !is_dead[call_pc] && func.chunk.code[call_pc].op == OpCode::Call && func.chunk.code[call_pc].rb as usize == method_reg {
                                        is_dead[call_pc] = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            } else if ra < num_regs && !live_out[pc][ra] {
                match inst.op {
                    OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod |
                    OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::BitNot |
                    OpCode::ShiftLeft | OpCode::ShiftRight | OpCode::Negate | OpCode::Not |
                    OpCode::Equal | OpCode::Greater | OpCode::Less |
                    OpCode::GetProperty | OpCode::GetIndex | OpCode::TypeOf | OpCode::ArrayLen |
                    OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadBool | OpCode::Move |
                    OpCode::MakeObject => {
                        is_dead[pc] = true;
                        changed = true;
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
