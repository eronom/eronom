use crate::vm::bytecode::{Function, OpCode};
use crate::jit::compiler::type_flow::{analyze_types, RegType};
use crate::jit::compiler::emit_header::calculate_max_regs;
use super::ir::{EirBinaryOp, EirFunction, EirInst, EirModule, EirType, EirUnaryOp};

/// Lower a bytecode function into a typed E-IR function with unboxed numeric and control-flow operations.
pub fn lower_function(func: &Function, name_override: Option<&str>) -> EirFunction {
    let name = name_override
        .map(|s| s.to_string())
        .or_else(|| func.name.clone())
        .unwrap_or_else(|| "er_main".to_string());

    let num_regs = calculate_max_regs(func).max(32);
    let param_is_double = vec![false; func.arity];
    let (types_at_inst, _) = analyze_types(func, num_regs, &param_is_double);

    let mut instructions = Vec::with_capacity(func.chunk.code.len());
    let mut param_types = Vec::with_capacity(func.arity);
    for i in 0..func.arity {
        if i < param_is_double.len() && param_is_double[i] {
            param_types.push(EirType::F64);
        } else {
            param_types.push(EirType::Dynamic);
        }
    }

    for (idx, inst) in func.chunk.code.iter().enumerate() {
        let ra = inst.ra as usize;
        let rb = inst.rb as usize;
        let rc = inst.rc as usize;
        let operand = inst.operand as usize;
        let reg_type = if idx < types_at_inst.len() && ra < types_at_inst[idx].len() {
            match types_at_inst[idx][ra] {
                RegType::Double => EirType::F64,
                RegType::Unknown => EirType::Dynamic,
            }
        } else {
            EirType::Dynamic
        };

        match inst.op {
            OpCode::LoadConst => {
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
                instructions.push(EirInst::ConstNull { dest: ra });
            }
            OpCode::LoadBool => {
                instructions.push(EirInst::ConstBool { dest: ra, val: operand != 0 });
            }
            OpCode::Move => {
                instructions.push(EirInst::Move { dest: ra, src: rb, ty: reg_type });
            }
            OpCode::Negate => {
                instructions.push(EirInst::Unary {
                    dest: ra,
                    op: EirUnaryOp::Neg,
                    ty: reg_type,
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
                    ty: reg_type,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Sub => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Sub,
                    ty: reg_type,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Mul => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Mul,
                    ty: reg_type,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Div => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Div,
                    ty: reg_type,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::Mod => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Mod,
                    ty: reg_type,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::BitAnd => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::BitAnd,
                    ty: reg_type,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::BitOr => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::BitOr,
                    ty: reg_type,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::BitXor => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::BitXor,
                    ty: reg_type,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::ShiftLeft => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Shl,
                    ty: reg_type,
                    left: rb,
                    right: rc,
                });
            }
            OpCode::ShiftRight => {
                instructions.push(EirInst::Binary {
                    dest: ra,
                    op: EirBinaryOp::Shr,
                    ty: reg_type,
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
                instructions.push(EirInst::GetGlobal { dest: ra, name });
            }
            OpCode::Call => {
                let mut args = Vec::with_capacity(operand);
                for i in 1..=operand {
                    args.push(rb + i);
                }
                instructions.push(EirInst::CallDynamic {
                    dest: ra,
                    callee: rb,
                    args,
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
            OpCode::MakeArray => {
                instructions.push(EirInst::AllocArray { dest: ra, cap: operand });
            }
            OpCode::GetIndex => {
                instructions.push(EirInst::ArrayGet { dest: ra, arr: rb, index: rc });
            }
            OpCode::SetIndex => {
                instructions.push(EirInst::ArraySet { arr: ra, index: rb, val: rc });
            }
            OpCode::Return => {
                instructions.push(EirInst::Return { val: Some((ra, reg_type)) });
            }
            _ => {
                // For other opcodes, use direct native helper call dispatch
                instructions.push(EirInst::CallNative {
                    dest: ra,
                    symbol: format!("er_jit_op_{:?}", inst.op),
                    args: vec![ra, rb, rc],
                    ret_ty: reg_type,
                });
            }
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

/// Lower a compiled program into an E-IR module ready for ahead-of-time code generation.
pub fn lower_program(main_func: &Function, module_name: &str) -> EirModule {
    let mut module = EirModule::new(module_name);
    let main_eir = lower_function(main_func, Some("main"));
    module.functions.push(main_eir);
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
}

