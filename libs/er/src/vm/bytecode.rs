use super::value::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrayMethodType {
    Push,
    Pop,
}

#[derive(Clone, Default)]
pub struct Function {
    pub name: Option<String>,
    pub chunk: Chunk,
    pub arity: usize,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpCode {
    Constant,
    Return,
    Negate,
    Add,
    Sub,
    Mul,
    Div,
    Not,
    Equal,
    Greater,
    Less,
    Pop,
    DefineGlobal,
    GetGlobal,
    SetGlobal,
    GetLocal,
    SetLocal,
    JumpIfFalse,
    Jump,
    Loop,
    Call,
    MakeArray,
    MakeObject,
    GetProperty,
    SetProperty,
    GetIndex,
    SetIndex,
}

#[derive(Clone, Copy, Debug)]
pub struct Instruction {
    pub op: OpCode,
    pub operand: u32,
}

#[derive(Clone, Default)]
pub struct Chunk {
    pub code: Vec<Instruction>,
    pub constants: Vec<Value>,
}

impl Chunk {
    pub fn add_constant(&mut self, value: Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    pub fn write(&mut self, op: OpCode) {
        self.code.push(Instruction { op, operand: 0 });
    }

    pub fn write_operand(&mut self, op: OpCode, operand: usize) {
        self.code.push(Instruction {
            op,
            operand: operand as u32,
        });
    }
}

