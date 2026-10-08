pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod typecheck;
pub mod transpile;
pub mod effects;
pub mod sync;

pub use token::{Token, TokenType};
pub use lexer::{Lexer, lex};
pub use ast::{Expr, LiteralValue, Stmt, SourceLocation, TypeNode, PrimitiveType, PropertySignature};
pub use parser::{Parser, parse_and_resolve_imports};
pub use typecheck::{check_program, TypeChecker, TypeError};
pub use transpile::{transpile_to_js, emit_expr, transpile_expr_reactivity, transform_script_reactivity};
pub use effects::{EffectRung, FunctionEffects, infer_effects, infer_expr_effect, infer_stmt_effect};
pub use sync::{check_sync_constraints, EffectError, SyncChecker};
