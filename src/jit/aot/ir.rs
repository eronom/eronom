use fnv::FnvHashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EirType {
    I64,
    F64,
    Bool,
    Ptr,
    Void,
    Dynamic,
}

impl EirType {
    pub fn mir_type_str(&self) -> &'static str {
        match self {
            EirType::I64 => "i64",
            EirType::F64 => "d",
            EirType::Bool => "i64",
            EirType::Ptr => "i64",
            EirType::Void => "",
            EirType::Dynamic => "i64",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EirBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EirUnaryOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Clone, Debug)]
pub enum EirInst {
    ConstI64 { dest: usize, val: i64 },
    ConstF64 { dest: usize, val: f64 },
    ConstBool { dest: usize, val: bool },
    ConstString { dest: usize, val: String },
    ConstNull { dest: usize },
    Move { dest: usize, src: usize, ty: EirType },
    Binary {
        dest: usize,
        op: EirBinaryOp,
        ty: EirType,
        left: usize,
        right: usize,
    },
    Unary {
        dest: usize,
        op: EirUnaryOp,
        ty: EirType,
        src: usize,
    },
    DefineGlobal {
        name: String,
        src: usize,
    },
    GetGlobal {
        dest: usize,
        name: String,
    },
    Call {
        dest: usize,
        func_name: String,
        args: Vec<usize>,
        ret_ty: EirType,
    },
    CallDynamic {
        dest: usize,
        callee: usize,
        args: Vec<usize>,
    },
    CallNative {
        dest: usize,
        symbol: String,
        args: Vec<usize>,
        ret_ty: EirType,
    },
    Print { reg: usize, ty: EirType },
    Println { reg: usize, ty: EirType },
    Jump { target: usize },
    BranchIfFalse { cond: usize, target: usize },
    Return { val: Option<(usize, EirType)> },
    AllocArray { dest: usize, cap: usize },
    ArrayPush { arr: usize, val: usize },
    ArrayGet { dest: usize, arr: usize, index: usize },
    ArraySet { arr: usize, index: usize, val: usize },
    AllocStruct { dest: usize, name: String, fields: usize },
    StructGetField { dest: usize, struct_reg: usize, offset: usize },
    StructSetField { struct_reg: usize, offset: usize, val: usize },
}

#[derive(Clone, Debug)]
pub struct StructLayout {
    pub name: String,
    pub field_offsets: FnvHashMap<String, usize>,
    pub size_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct EirFunction {
    pub name: String,
    pub arity: usize,
    pub param_types: Vec<EirType>,
    pub return_type: EirType,
    pub reg_count: usize,
    pub instructions: Vec<EirInst>,
}

#[derive(Clone, Debug)]
pub struct EirModule {
    pub name: String,
    pub functions: Vec<EirFunction>,
    pub struct_layouts: FnvHashMap<String, StructLayout>,
}

impl EirModule {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            functions: Vec::new(),
            struct_layouts: FnvHashMap::default(),
        }
    }
}
