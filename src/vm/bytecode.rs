use super::value::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrayMethodType {
    Push,
    Pop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UpvalueDescriptor {
    pub is_local: bool,
    pub index: u8,
}

#[derive(Clone, Default)]
pub struct Function {
    pub name: Option<String>,
    pub chunk: Chunk,
    pub arity: usize,
    pub jit_ptr: std::cell::Cell<Option<*const std::ffi::c_void>>,
    pub invocation_count: std::cell::Cell<usize>,
    pub is_async: bool,
    pub has_loop: bool,
    pub upvalues: Vec<UpvalueDescriptor>,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    ShiftLeft,
    ShiftRight,
    TypeOf,
    ToIter,
    ArrayLen,
    Equal,
    Greater,
    Less,
    DefineGlobal,
    GetGlobal,
    SetGlobal,
    GetUpvalue,
    SetUpvalue,
    Closure,
    CloseUpvalue,
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
    Await,
    Return,
    DefineStruct,
    Throw,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExceptionHandler {
    pub try_start: usize,
    pub try_end: usize,
    pub catch_ip: usize,
    pub err_reg: u8,
    pub finally_ip: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    pub handlers: Vec<ExceptionHandler>,
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

    pub fn find_handler(&self, ip: usize) -> Option<&ExceptionHandler> {
        self.handlers
            .iter()
            .filter(|h| ip >= h.try_start && ip < h.try_end)
            .min_by_key(|h| h.try_end - h.try_start)
    }
}
