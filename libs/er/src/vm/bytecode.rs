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

#[derive(Debug, Clone, Copy)]
pub enum OpCode {
    Constant(usize),
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
    DefineGlobal(usize),
    GetGlobal(usize),
    SetGlobal(usize),
    GetLocal(usize),
    SetLocal(usize),
    JumpIfFalse(usize),
    Jump(usize),
    Loop(usize),
    Call(usize),       // arity
    MakeArray(usize),  // initial elements
    MakeObject(usize), // key-value pairs (so 2x elements on stack)
    GetProperty(usize),
    SetProperty(usize),
    GetIndex,
    SetIndex,
}

#[derive(Clone, Default)]
pub struct Chunk {
    pub code: Vec<OpCode>,
    pub constants: Vec<Value>,
}

impl Chunk {
    pub fn add_constant(&mut self, value: Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    pub fn write(&mut self, op: OpCode) {
        self.code.push(op);
    }
}
