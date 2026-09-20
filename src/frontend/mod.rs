pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod typecheck;

pub use token::{Token, TokenType};
pub use lexer::{Lexer, lex};
pub use ast::{Expr, LiteralValue, Stmt, SourceLocation, TypeNode, PrimitiveType, PropertySignature};
pub use parser::{Parser, parse_and_resolve_imports};
pub use typecheck::{check_program, TypeChecker, TypeError};
