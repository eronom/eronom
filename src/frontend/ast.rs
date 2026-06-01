use super::token::TokenType;

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(LiteralValue),
    Variable(String),
    Assign(String, Box<Expr>),
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
}

#[derive(Debug, Clone)]
pub enum LiteralValue {
    Number(f64),
    String(String),
    Boolean(bool),
    Null,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Print(Expr),
    VarDecl(String, bool, Expr), // name, is_const, initializer
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    For(String, Expr, Expr, Box<Stmt>), // var, start, end, body
    Return(Option<Expr>),
    Import(Vec<String>, String), // imported names, source path
    Export(Box<Stmt>), // exported declaration statement
}
