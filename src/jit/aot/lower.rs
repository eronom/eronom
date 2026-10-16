use fnv::FnvHashMap;
use crate::vm::bytecode::{Function, OpCode};
use crate::jit::compiler::emit_header::calculate_max_regs;
use super::ir::{EirBinaryOp, EirFunction, EirInst, EirModule, EirType, EirUnaryOp};

fn aot_analyze_types(func: &Function, num_regs: usize) -> Vec<Vec<EirType>> {
    let code = &func.chunk.code;
    let n = code.len();
    if n == 0 {
        return Vec::new();
    }
    let mut in_types = vec![vec![EirType::Dynamic; num_regs]; n];
    let mut worklist = vec![0];
    let mut in_worklist = vec![false; n];
    in_worklist[0] = true;

    while let Some(pc) = worklist.pop() {
        in_worklist[pc] = false;
        let inst = &code[pc];
        let mut out = in_types[pc].clone();

        let ra = inst.ra as usize;
        let rb = inst.rb as usize;
        let rc = inst.rc as usize;

        match inst.op {
            OpCode::LoadConst => {
                if let Some(&val) = func.chunk.constants.get(inst.operand as usize) {
                    if ra < num_regs {
                        if val.is_number() {
                            let f = val.as_number();
                            if f.fract() != 0.0 || f < i64::MIN as f64 || f > i64::MAX as f64 {
                                out[ra] = EirType::F64;
                            } else {
                                out[ra] = EirType::I64;
                            }
                        } else if val.is_boolean() {
                            out[ra] = EirType::Bool;
                        } else {
                            out[ra] = EirType::Dynamic;
                        }
                    }
                }
            }
            OpCode::Move => {
                if ra < num_regs && rb < num_regs {
                    out[ra] = in_types[pc][rb];
                }
            }
            OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div => {
                if ra < num_regs {
                    let rb_is_f64 = rb < num_regs && in_types[pc][rb] == EirType::F64;
                    let rc_is_f64 = rc < num_regs && in_types[pc][rc] == EirType::F64;
                    let rb_is_i64 = rb < num_regs && in_types[pc][rb] == EirType::I64;
                    let rc_is_i64 = rc < num_regs && in_types[pc][rc] == EirType::I64;
                    if rb_is_f64 || rc_is_f64 {
                        out[ra] = EirType::F64;
                    } else if rb_is_i64 && rc_is_i64 {
                        out[ra] = EirType::I64;
                    } else {
                        out[ra] = EirType::Dynamic;
                    }
                }
            }
            OpCode::Negate => {
                if ra < num_regs && rb < num_regs {
                    out[ra] = in_types[pc][rb];
                }
            }
            OpCode::Not | OpCode::Equal | OpCode::Greater | OpCode::Less => {
                if ra < num_regs {
                    out[ra] = EirType::Bool;
                }
            }
            _ => {
                if ra < num_regs {
                    out[ra] = EirType::Dynamic;
                }
            }
        }

        let mut successors = Vec::new();
        match inst.op {
            OpCode::Jump => {
                let target = pc + 1 + inst.operand as usize;
                if target < n {
                    successors.push(target);
                }
            }
            OpCode::JumpIfFalse => {
                if pc + 1 < n {
                    successors.push(pc + 1);
                }
                let target = pc + 1 + inst.operand as usize;
                if target < n {
                    successors.push(target);
                }
            }
            OpCode::Loop => {
                let opnd = inst.operand as usize;
                let target = if pc + 1 >= opnd { pc + 1 - opnd } else { 0 };
                if target < n {
                    successors.push(target);
                }
            }
            OpCode::Return => {}
            _ => {
                if pc + 1 < n {
                    successors.push(pc + 1);
                }
            }
        }

        for succ in successors {
            let mut changed = false;
            for r in 0..num_regs {
                let old_ty = in_types[succ][r];
                let new_ty = out[r];
                let merged = if old_ty == new_ty {
                    old_ty
                } else {
                    EirType::Dynamic
                };
                if merged != old_ty {
                    in_types[succ][r] = merged;
                    changed = true;
                }
            }
            if changed && !in_worklist[succ] {
                worklist.push(succ);
                in_worklist[succ] = true;
            }
        }
    }

    in_types
}

/// Lower a bytecode function into a typed E-IR function with unboxed numeric and control-flow operations.
pub fn lower_function_with_symbols(
    func: &Function,
    name_override: Option<&str>,
    child_syms: &[(usize, String)],
    global_funcs: &FnvHashMap<String, String>,
) -> EirFunction {
    let name = name_override
        .map(|s| s.to_string())
        .or_else(|| func.name.clone())
        .unwrap_or_else(|| "er_main".to_string());

    let num_regs = calculate_max_regs(func).max(32);
    let in_types = aot_analyze_types(func, num_regs);

    let mut instructions = Vec::with_capacity(func.chunk.code.len());
    let mut param_types = Vec::with_capacity(func.arity);
    for _ in 0..func.arity {
        param_types.push(EirType::Dynamic);
    }

    let mut reg_fns: FnvHashMap<usize, String> = FnvHashMap::default();
    let mut local_globals = global_funcs.clone();
    if let Some(ref self_name) = func.name {
        local_globals.insert(self_name.clone(), name.clone());
    }

    for (idx, inst) in func.chunk.code.iter().enumerate() {
        let ra = inst.ra as usize;
        let rb = inst.rb as usize;
        let rc = inst.rc as usize;
        let operand = inst.operand as usize;

        let rb_ty = in_types.get(idx).and_then(|row| row.get(rb)).copied().unwrap_or(EirType::Dynamic);
        let rc_ty = in_types.get(idx).and_then(|row| row.get(rc)).copied().unwrap_or(EirType::Dynamic);
        let bin_ty = if rb_ty == EirType::F64 || rc_ty == EirType::F64 {
            EirType::F64
        } else if rb_ty == EirType::I64 && rc_ty == EirType::I64 {
            EirType::I64
        } else {
            EirType::Dynamic
        };
        match inst.op {
            OpCode::LoadConst => {
                reg_fns.remove(&ra);
                if let Some(&val) = func.chunk.constants.get(operand) {
                    if val.is_number() {
                        let n = val.as_number();
                        if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                            instructions.push(EirInst::ConstI64 { dest: ra, val: n as i64 });
                        } else {
                            instructions.push(EirInst::ConstF64 { dest: ra, val: n });
                        }
                    } else if val.is_boolean() {
                        instructions.push(EirInst::ConstBool { dest: ra, val: val.as_boolean() });
                    } else if let Some(s) = val.as_str() {
                        instructions.push(EirInst::ConstString { dest: ra, val: s.to_string() });
                    } else {
                        instructions.push(EirInst::ConstI64 { dest: ra, val: 0 });
                    }
                }
            }
            OpCode::LoadNull => {
                reg_fns.remove(&ra);
                instructions.push(EirInst::ConstNull { dest: ra });
            }
            OpCode::LoadBool => {
                reg_fns.remove(&ra);
                instructions.push(EirInst::ConstBool { dest: ra, val: rb != 0 || operand != 0 });
            }
            OpCode::Move => {
                if let Some(sym) = reg_fns.get(&rb).cloned() {
                    reg_fns.insert(ra, sym);
                } else {
                    reg_fns.remove(&ra);
                }
                instructions.push(EirInst::Move { dest: ra, src: rb, ty: rb_ty });
            }
            OpCode::Negate => {
                instructions.push(EirInst::Unary {
                    dest: ra,
                    op: EirUnaryOp::Neg,
                    ty: rb_ty,
                    src: rb,
                });
            }
            OpCode::Not => {
                instructions.push(EirInst::Unary {
                    dest: ra,
                    op: EirUnaryOp::Not,
                    ty: EirType::Bool,
                    src: rb,
                });
            }
            OpCode::Add => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Add,
                    ty: bin_ty,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Sub => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Sub,
                    ty: bin_ty,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Mul => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Mul,
                    ty: bin_ty,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Div => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Div,
                    ty: bin_ty,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Mod => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Mod,
                    ty: EirType::Dynamic,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::BitAnd => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::BitAnd,
                    ty: EirType::Dynamic,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::BitOr => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::BitOr,
                    ty: EirType::Dynamic,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::BitXor => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::BitXor,
                    ty: EirType::Dynamic,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::ShiftLeft => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Shl,
                    ty: EirType::Dynamic,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::ShiftRight => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Shr,
                    ty: EirType::Dynamic,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Equal => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Eq,
                    ty: EirType::Bool,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Greater => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Gt,
                    ty: EirType::Bool,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Less => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Lt,
                    ty: EirType::Bool,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::DefineGlobal | OpCode::SetGlobal => {
                let name = func
                    .chunk
                    .constants
                    .get(operand)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Some(sym) = reg_fns.get(&ra) {
                    local_globals.insert(name.clone(), sym.clone());
                }
                instructions.push(EirInst::DefineGlobal { name, src: ra });
            }
            OpCode::GetGlobal => {
                let name = func
                    .chunk
                    .constants
                    .get(operand)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Some(sym) = local_globals.get(&name) {
                    reg_fns.insert(ra, sym.clone());
                    instructions.push(EirInst::ConstFuncPtr { dest: ra, symbol: sym.clone() });
                } else {
                    reg_fns.remove(&ra);
                    instructions.push(EirInst::GetGlobal { dest: ra, name });
                }
            }
            OpCode::Call => {
                let mut args = Vec::with_capacity(operand);
                for i in 1..=operand {
                    args.push(rb + i);
                }
                if let Some(target_fn) = reg_fns.get(&rb) {
                    instructions.push(EirInst::Call {
                        dest: ra,
                        func_name: target_fn.clone(),
                        args,
                        ret_ty: EirType::Dynamic,
                    });
                } else {
                    instructions.push(EirInst::CallDynamic {
                        dest: ra,
                        callee: rb,
                        args,
                    });
                }
            }
            OpCode::Closure => {
                if let Some((_, sym)) = child_syms.iter().find(|(idx, _)| *idx == operand) {
                    let upvalues = if let Some(child_fn) = func.chunk.constants.get(operand).and_then(|v| extract_function(*v)) {
                        child_fn.upvalues.iter().map(|uv| (uv.is_local, uv.index as usize)).collect()
                    } else {
                        Vec::new()
                    };
                    instructions.push(EirInst::MakeClosure {
                        dest: ra,
                        symbol: sym.clone(),
                        upvalues,
                    });
                    reg_fns.insert(ra, sym.clone());
                } else {
                    instructions.push(EirInst::ConstNull { dest: ra });
                    reg_fns.remove(&ra);
                }
            }
            OpCode::MakeObject => {
                instructions.push(EirInst::MakeObject {
                    dest: ra,
                    start_reg: rb,
                    pair_count: operand,
                });
            }
            OpCode::GetProperty => {
                let key = func
                    .chunk
                    .constants
                    .get(operand)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                instructions.push(EirInst::GetProperty {
                    dest: ra,
                    obj: rb,
                    key,
                });
            }
            OpCode::SetProperty => {
                let key = func
                    .chunk
                    .constants
                    .get(operand)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                instructions.push(EirInst::SetProperty {
                    obj: ra,
                    key,
                    val: rb,
                });
            }
            OpCode::MakeArray => {
                instructions.push(EirInst::MakeArray {
                    dest: ra,
                    start_reg: rb,
                    count: operand,
                });
            }
            OpCode::GetIndex => {
                instructions.push(EirInst::ArrayGet {
                    dest: ra,
                    arr: rb,
                    index: rc,
                });
            }
            OpCode::SetIndex => {
                instructions.push(EirInst::ArraySet {
                    arr: ra,
                    index: rb,
                    val: rc,
                });
            }
            OpCode::GetUpvalue => {
                instructions.push(EirInst::GetUpvalue {
                    dest: ra,
                    upval_idx: operand,
                });
            }
            OpCode::SetUpvalue => {
                instructions.push(EirInst::SetUpvalue {
                    src: ra,
                    upval_idx: operand,
                });
            }
            OpCode::Jump => {
                let target = idx + 1 + operand;
                instructions.push(EirInst::Jump { target });
            }
            OpCode::JumpIfFalse => {
                let target = idx + 1 + operand;
                instructions.push(EirInst::BranchIfFalse { cond: ra, target });
            }
            OpCode::Loop => {
                let target = if idx + 1 >= operand { idx + 1 - operand } else { 0 };
                instructions.push(EirInst::Jump { target });
            }
            OpCode::Return => {
                let ra_ty = in_types.get(idx).and_then(|row| row.get(ra)).copied().unwrap_or(EirType::Dynamic);
                instructions.push(EirInst::Return { val: Some((ra, ra_ty)) });
            }
            _ => {
                let ra_ty = in_types.get(idx).and_then(|row| row.get(ra)).copied().unwrap_or(EirType::Dynamic);
                // For other opcodes, use direct native helper call dispatch
                instructions.push(EirInst::CallNative {
                    dest: ra,
                    symbol: format!("er_jit_op_{:?}", inst.op),
                    args: vec![ra, rb, rc],
                    ret_ty: ra_ty,
                });
            }
        }

        if inst.op != OpCode::Closure
            && inst.op != OpCode::GetGlobal
            && inst.op != OpCode::Move
            && inst.op != OpCode::DefineGlobal
            && inst.op != OpCode::SetGlobal
        {
            reg_fns.remove(&ra);
        }
    }

    EirFunction {
        name,
        arity: func.arity,
        param_types,
        return_type: EirType::Dynamic,
        reg_count: num_regs,
        instructions,
    }
}

fn extract_function_info(val: crate::vm::Value) -> Option<(usize, Function)> {
    if !val.is_function() {
        return None;
    }
    unsafe {
        let ptr = val.as_gc_ptr();
        if ptr.is_null() {
            return None;
        }
        match &(*ptr).data {
            crate::vm::gc::GcData::Function(f) => {
                let fn_ptr = (&**f) as *const Function as usize;
                Some((fn_ptr, *f.clone()))
            }
            crate::vm::gc::GcData::Closure(c) => {
                if !c.function.is_null() {
                    if let crate::vm::gc::GcData::Function(f) = &(*c.function).data {
                        let fn_ptr = (&**f) as *const Function as usize;
                        return Some((fn_ptr, *f.clone()));
                    }
                }
                None
            }
            _ => None,
        }
    }
}

fn extract_function(val: crate::vm::Value) -> Option<Function> {
    extract_function_info(val).map(|(_, f)| f)
}

fn infer_closure_names(func: &Function) -> FnvHashMap<usize, String> {
    let mut const_to_name: FnvHashMap<usize, String> = FnvHashMap::default();
    let code = &func.chunk.code;
    for (i, inst) in code.iter().enumerate() {
        if inst.op == OpCode::Closure {
            let closure_reg = inst.ra;
            let const_idx = inst.operand as usize;
            for next_inst in code.iter().skip(i + 1) {
                if (next_inst.op == OpCode::DefineGlobal || next_inst.op == OpCode::SetGlobal) && next_inst.ra == closure_reg {
                    if let Some(name_str) = func.chunk.constants.get(next_inst.operand as usize).and_then(|v| v.as_str()) {
                        const_to_name.insert(const_idx, name_str.to_string());
                    }
                    break;
                }
                if next_inst.ra == closure_reg {
                    break;
                }
            }
        }
    }
    const_to_name
}

/// Lower a bytecode function into a typed E-IR function.
pub fn lower_function(func: &Function, name_override: Option<&str>) -> EirFunction {
    let dummy = FnvHashMap::default();
    lower_function_with_symbols(func, name_override, &[], &dummy)
}

/// Lower a compiled program into an E-IR module ready for ahead-of-time code generation.
pub fn lower_program(main_func: &Function, module_name: &str) -> EirModule {
    let mut module = EirModule::new(module_name);
    let mut func_symbols: FnvHashMap<usize, String> = FnvHashMap::default();
    let mut used_symbols: fnv::FnvHashSet<String> = fnv::FnvHashSet::default();
    let mut global_funcs = FnvHashMap::default();
    used_symbols.insert("main".to_string());

    let mut fn_counter = 0usize;
    let mut scan_queue = vec![main_func.clone()];

    // Pass 1: Discover all functions and register unique global symbols
    while let Some(f) = scan_queue.pop() {
        let inferred_names = infer_closure_names(&f);
        for (idx, &val) in f.chunk.constants.iter().enumerate() {
            if let Some((fn_ptr_key, mut child_fn)) = extract_function_info(val) {
                if !func_symbols.contains_key(&fn_ptr_key) {
                    if child_fn.name.is_none() {
                        if let Some(inferred) = inferred_names.get(&idx) {
                            child_fn.name = Some(inferred.clone());
                        }
                    }
                    let base_name = child_fn.name.as_deref().unwrap_or("");
                    let sym = if !base_name.is_empty() && !used_symbols.contains(&format!("er_fn_{}", base_name)) {
                        format!("er_fn_{}", base_name)
                    } else {
                        fn_counter += 1;
                        if base_name.is_empty() {
                            format!("er_fn_{}", fn_counter)
                        } else {
                            format!("er_fn_{}_{}", base_name, fn_counter)
                        }
                    };
                    used_symbols.insert(sym.clone());
                    func_symbols.insert(fn_ptr_key, sym.clone());
                    if let Some(ref n) = child_fn.name {
                        if !global_funcs.contains_key(n) {
                            global_funcs.insert(n.clone(), sym.clone());
                        }
                    }
                    global_funcs.insert(sym.clone(), sym);
                    scan_queue.push(child_fn);
                }
            }
        }
    }

    // Pass 2: Lower each function with global function symbols
    let mut lowered = Vec::new();
    let mut pending = vec![(main_func.clone(), "main".to_string())];
    let mut visited_funcs: fnv::FnvHashSet<usize> = fnv::FnvHashSet::default();

    while let Some((func, name)) = pending.pop() {
        let mut child_syms = Vec::new();
        for (const_idx, &val) in func.chunk.constants.iter().enumerate() {
            if let Some((fn_ptr_key, child_fn)) = extract_function_info(val) {
                let sym = func_symbols.get(&fn_ptr_key).cloned().unwrap_or_else(|| {
                    fn_counter += 1;
                    format!("er_fn_{}", fn_counter)
                });
                child_syms.push((const_idx, sym.clone()));
                if visited_funcs.insert(fn_ptr_key) {
                    pending.push((child_fn, sym));
                }
            }
        }
        let eir_fn = lower_function_with_symbols(&func, Some(&name), &child_syms, &global_funcs);
        lowered.push(eir_fn);
    }

    // Order child functions first so that forward declarations and caller functions work cleanly
    lowered.reverse();
    module.functions = lowered;
    module
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::bytecode::{Chunk, Instruction, OpCode};
    use crate::vm::value::Value;

    #[test]
    fn test_lower_arithmetic_function() {
        let mut chunk = Chunk::default();
        chunk.constants.push(Value::number(10.0));
        chunk.constants.push(Value::number(20.0));
        chunk.code.push(Instruction { op: OpCode::LoadConst, ra: 0, rb: 0, rc: 0, operand: 0 });
        chunk.code.push(Instruction { op: OpCode::LoadConst, ra: 1, rb: 0, rc: 0, operand: 1 });
        chunk.code.push(Instruction { op: OpCode::Add, ra: 2, rb: 0, rc: 1, operand: 0 });
        chunk.code.push(Instruction { op: OpCode::Return, ra: 2, rb: 0, rc: 0, operand: 0 });

        let func = Function {
            name: Some("test_add".to_string()),
            chunk,
            arity: 0,
            ..Default::default()
        };

        let eir = lower_function(&func, None);
        assert_eq!(eir.name, "test_add");
        assert_eq!(eir.instructions.len(), 4);
        assert!(matches!(eir.instructions[0], EirInst::ConstI64 { dest: 0, val: 10 }));
        assert!(matches!(eir.instructions[1], EirInst::ConstI64 { dest: 1, val: 20 }));
        assert!(matches!(eir.instructions[2], EirInst::Binary { dest: 2, op: EirBinaryOp::Add, left: 0, right: 1, .. }));
    }

    #[test]
    fn test_lower_fib_recursive_call() {
        let stmts = crate::frontend::parse_and_resolve_imports(std::path::Path::new("fib.er")).unwrap();
        let compiler = crate::vm::compiler::Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let eir_module = lower_program(&function, "fib");
        let fib_fn = eir_module.functions.iter().find(|f| f.name == "er_fn_fib").expect("fib function should exist");
        let direct_calls: Vec<_> = fib_fn.instructions.iter().filter(|i| matches!(i, EirInst::Call { func_name, .. } if func_name == "er_fn_fib")).collect();
        assert_eq!(direct_calls.len(), 2, "fib should have 2 direct recursive calls");
    }
}

