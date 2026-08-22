use super::token::TokenType;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceLocation {
    pub file_path: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(LiteralValue),
    Variable(String, SourceLocation),
    Assign(String, Box<Expr>, SourceLocation),
    Binary(Box<Expr>, TokenType, Box<Expr>),
    Logical(Box<Expr>, TokenType, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Get(Box<Expr>, String),
    Set(Box<Expr>, String, Box<Expr>),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Function(Vec<String>, Box<Stmt>), // params, body
    GetIndex(Box<Expr>, Box<Expr>),
    SetIndex(Box<Expr>, Box<Expr>, Box<Expr>),
    StructInst(String, Vec<(String, Expr)>, SourceLocation),
    Spawn(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum LiteralValue {
    Number(f64),
    String(String),
    Boolean(bool),
    Null,
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub values: Vec<Expr>,
    pub body: Box<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Print(Expr),
    VarDecl(String, Option<String>, bool, Expr, SourceLocation), // name, type_annotation, is_const, initializer, location
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    For(String, Expr, Expr, Box<Stmt>), // var, start, end, body
    Break,
    Continue,
    Throw(Expr),
    Try(Box<Stmt>, Option<(String, Box<Stmt>)>, Option<Box<Stmt>>), // try_body, catch_clause (param_name, catch_body), finally_body
    Switch(Expr, Vec<SwitchCase>, Option<Box<Stmt>>), // target_expr, cases, default_body
    Return(Option<Expr>),
    Import(Vec<String>, String), // imported names, source path
    Export(Box<Stmt>), // exported declaration statement
    Struct(String, Vec<String>, Vec<(String, String)>, Vec<(String, Vec<String>, Stmt)>, SourceLocation), // name, composed, fields (name, type), methods (name, params, body), location
    Interface(String, Vec<(String, String)>, Vec<(String, Vec<String>)>, SourceLocation), // name, fields (name, type), methods (name, params), location
    Concurrent(Box<Stmt>),
}
