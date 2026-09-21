use std::fmt;
use super::token::TokenType;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceLocation {
    pub file_path: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveType {
    Int,
    Float,
    Number,
    String,
    Bool,
    Void,
    Any,
    Unknown,
    Never,
    Null,
}

impl fmt::Display for PrimitiveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrimitiveType::Int => write!(f, "int"),
            PrimitiveType::Float => write!(f, "float"),
            PrimitiveType::Number => write!(f, "number"),
            PrimitiveType::String => write!(f, "string"),
            PrimitiveType::Bool => write!(f, "bool"),
            PrimitiveType::Void => write!(f, "void"),
            PrimitiveType::Any => write!(f, "any"),
            PrimitiveType::Unknown => write!(f, "unknown"),
            PrimitiveType::Never => write!(f, "never"),
            PrimitiveType::Null => write!(f, "null"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertySignature {
    pub name: String,
    pub ty: TypeNode,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeNode {
    Primitive(PrimitiveType),
    Array(Box<TypeNode>),
    Tuple(Vec<TypeNode>),
    Object(Vec<PropertySignature>),
    Function {
        params: Vec<TypeNode>,
        return_type: Box<TypeNode>,
    },
    Union(Vec<TypeNode>),
    Intersection(Vec<TypeNode>),
    Nullable(Box<TypeNode>),
    Named(String, Vec<TypeNode>),
    Literal(String),
}

impl TypeNode {
    pub fn from_ident(name: &str) -> Self {
        match name {
            "int" | "i32" | "i64" => TypeNode::Primitive(PrimitiveType::Int),
            "float" | "f32" | "f64" => TypeNode::Primitive(PrimitiveType::Float),
            "number" => TypeNode::Primitive(PrimitiveType::Number),
            "string" | "str" => TypeNode::Primitive(PrimitiveType::String),
            "bool" | "boolean" => TypeNode::Primitive(PrimitiveType::Bool),
            "void" => TypeNode::Primitive(PrimitiveType::Void),
            "any" => TypeNode::Primitive(PrimitiveType::Any),
            "unknown" => TypeNode::Primitive(PrimitiveType::Unknown),
            "never" => TypeNode::Primitive(PrimitiveType::Never),
            "null" => TypeNode::Primitive(PrimitiveType::Null),
            other => TypeNode::Named(other.to_string(), Vec::new()),
        }
    }
}

impl fmt::Display for TypeNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeNode::Primitive(p) => write!(f, "{}", p),
            TypeNode::Array(elem) => write!(f, "{}[]", elem),
            TypeNode::Tuple(elements) => {
                let inner = elements.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", ");
                write!(f, "[{}]", inner)
            }
            TypeNode::Object(props) => {
                let inner = props
                    .iter()
                    .map(|p| format!("{}{}: {}", p.name, if p.optional { "?" } else { "" }, p.ty))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{{ {} }}", inner)
            }
            TypeNode::Function { params, return_type } => {
                let inner = params.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
                write!(f, "({}) => {}", inner, return_type)
            }
            TypeNode::Union(members) => {
                let inner = members.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(" | ");
                write!(f, "{}", inner)
            }
            TypeNode::Intersection(members) => {
                let inner = members.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(" & ");
                write!(f, "{}", inner)
            }
            TypeNode::Nullable(inner) => write!(f, "{}?", inner),
            TypeNode::Named(name, args) => {
                if args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    let inner = args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ");
                    write!(f, "{}<{}>", name, inner)
                }
            }
            TypeNode::Literal(lit) => write!(f, "{}", lit),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnParam {
    pub name: String,
    pub ty: Option<TypeNode>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(LiteralValue),
    Variable(String, SourceLocation),
    Assign(String, Box<Expr>, SourceLocation),
    Binary(Box<Expr>, TokenType, Box<Expr>),
    Logical(Box<Expr>, TokenType, Box<Expr>),
    Unary(TokenType, Box<Expr>),
    Prefix(TokenType, Box<Expr>),
    Postfix(TokenType, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Get(Box<Expr>, String),
    Set(Box<Expr>, String, Box<Expr>),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Function(Vec<FnParam>, Option<TypeNode>, Box<Stmt>), // params, return_type, body
    GetIndex(Box<Expr>, Box<Expr>),
    SetIndex(Box<Expr>, Box<Expr>, Box<Expr>),
    StructInst(String, Vec<(String, Expr)>, SourceLocation),
    Spawn(Box<Expr>),
    TypeCast(Box<Expr>, TypeNode, SourceLocation),
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
    VarDecl(String, Option<TypeNode>, bool, Expr, SourceLocation), // name, type_annotation, is_const, initializer, location
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    For(String, Expr, Expr, Box<Stmt>), // var, start, end, body (range)
    ForIn(String, Expr, Box<Stmt>), // var, iterable, body (collection)
    Break,
    Continue,
    Throw(Expr),
    Try(Box<Stmt>, Option<(String, Box<Stmt>)>, Option<Box<Stmt>>), // try_body, catch_clause (param_name, catch_body), finally_body
    Switch(Expr, Vec<SwitchCase>, Option<Box<Stmt>>), // target_expr, cases, default_body
    Return(Option<Expr>, SourceLocation),
    Import(Vec<String>, String), // imported names, source path
    Export(Box<Stmt>), // exported declaration statement
    Struct(String, Vec<String>, Vec<(String, String)>, Vec<(String, Vec<String>, Stmt)>, SourceLocation), // name, composed, fields (name, type), methods (name, params, body), location
    Interface(String, Vec<(String, String)>, Vec<(String, Vec<String>)>, SourceLocation), // name, fields (name, type), methods (name, params), location
    Concurrent(Box<Stmt>),
    TypeAlias(String, Vec<String>, TypeNode, SourceLocation), // name, type_params, type_node, location
    Enum(String, Vec<(String, Option<LiteralValue>)>, SourceLocation), // name, variants (name, value), location
}

