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
    LoadConst,
    LoadNull,
    LoadBool,
    Move,
    Negate,
    Not,
    Add,
    Sub,
    Mul,
    Div,
    Equal,
    Greater,
    Less,
    DefineGlobal,
    GetGlobal,
    SetGlobal,
    Jump,
    JumpIfFalse,
    Loop,
    Call,
    MakeArray,
    MakeObject,
    GetProperty,
    SetProperty,
    GetIndex,
    SetIndex,
    Return,
}

#[derive(Clone, Copy, Debug)]
pub struct Instruction {
    pub op: OpCode,
    pub ra: u8,
    pub rb: u8,
    pub rc: u8,
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

    pub fn write_instruction(&mut self, op: OpCode, ra: u8, rb: u8, rc: u8, operand: u32) {
        self.code.push(Instruction {
            op,
            ra,
            rb,
            rc,
            operand,
        });
    }
}
