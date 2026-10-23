use crate::frontend::ast::{Expr, Stmt, SourceLocation};
use super::effects::{EffectRung, FunctionEffects, infer_expr_effect, infer_stmt_effect};

#[derive(Debug, Clone)]
pub struct EffectError {
    pub message: String,
    pub loc: SourceLocation,
}

impl std::fmt::Display for EffectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.loc.file_path.is_empty() {
            write!(f, "Effect error (line {}): {}", self.loc.line, self.message)
        } else {
            write!(
                f,
                "Effect error at {}:{}:{}: {}",
                self.loc.file_path, self.loc.line, self.loc.col, self.message
            )
        }
    }
}

impl std::error::Error for EffectError {}

pub struct SyncChecker<'a> {
    pub fe: &'a FunctionEffects,
    pub errors: Vec<EffectError>,
    pub in_sync_context: bool,
    pub sync_context_desc: String,
}

impl<'a> SyncChecker<'a> {
    pub fn new(fe: &'a FunctionEffects) -> Self {
        Self {
            fe,
            errors: Vec::new(),
            in_sync_context: false,
            sync_context_desc: String::new(),
        }
    }

    pub fn check_program(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.check_stmt(s);
        }
    }

    pub fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl(name, _, _, expr, loc) => {
                if let Expr::Function(_, _, body, _, is_sync) = expr {
                    if *is_sync {
                        let prev_sync = self.in_sync_context;
                        let prev_desc = self.sync_context_desc.clone();
                        self.in_sync_context = true;
                        self.sync_context_desc = format!("@sync function '{}'", name);

                        let body_rung = infer_stmt_effect(body, self.fe);
                        if body_rung == EffectRung::Suspends {
                            self.errors.push(EffectError {
                                message: format!(
                                    "Function '{}' is marked @sync, but performs suspending operation(s).",
                                    name
                                ),
                                loc: loc.clone(),
                            });
                        }

                        self.check_stmt(body);
                        self.in_sync_context = prev_sync;
                        self.sync_context_desc = prev_desc;
                        return;
                    }
                }
                self.check_expr(expr);
            }
            Stmt::Sync(body) => {
                let prev_sync = self.in_sync_context;
                let prev_desc = self.sync_context_desc.clone();
                self.in_sync_context = true;
                self.sync_context_desc = "sync block".to_string();

                let body_rung = infer_stmt_effect(body, self.fe);
                if body_rung == EffectRung::Suspends {
                    self.errors.push(EffectError {
                        message: "Code inside 'sync' block performs suspending operation(s).".to_string(),
                        loc: SourceLocation::default(),
                    });
                }

                self.check_stmt(body);
                self.in_sync_context = prev_sync;
                self.sync_context_desc = prev_desc;
            }
            Stmt::Expr(expr) => self.check_expr(expr),
            Stmt::Print(expr) => self.check_expr(expr),
            Stmt::Block(stmts) => {
                for s in stmts {
                    self.check_stmt(s);
                }
            }
            Stmt::If(cond, then_b, else_b) => {
                self.check_expr(cond);
                self.check_stmt(then_b);
                if let Some(eb) = else_b {
                    self.check_stmt(eb);
                }
            }
            Stmt::While(cond, body) => {
                self.check_expr(cond);
                self.check_stmt(body);
            }
            Stmt::For(_, start, end, body) => {
                self.check_expr(start);
                self.check_expr(end);
                self.check_stmt(body);
            }
            Stmt::ForIn(_, iter, body) => {
                self.check_expr(iter);
                self.check_stmt(body);
            }
            Stmt::Throw(expr) => self.check_expr(expr),
            Stmt::Try(try_b, catch_c, finally_b) => {
                self.check_stmt(try_b);
                if let Some((_, cb)) = catch_c {
                    self.check_stmt(cb);
                }
                if let Some(fb) = finally_b {
                    self.check_stmt(fb);
                }
            }
            Stmt::Switch(target, cases, def_b) => {
                self.check_expr(target);
                for c in cases {
                    for v in &c.values {
                        self.check_expr(v);
                    }
                    self.check_stmt(&c.body);
                }
                if let Some(db) = def_b {
                    self.check_stmt(db);
                }
            }
            Stmt::Return(opt_e, _) => {
                if let Some(e) = opt_e {
                    self.check_expr(e);
                }
            }
            Stmt::Export(inner) => self.check_stmt(inner),
            Stmt::Struct(name, _, _, methods, loc) => {
                for (m_name, _, body) in methods {
                    // Custom comparison methods must never suspend
                    if m_name == "compare" || m_name == "compareTo" || m_name == "equals" || m_name == "hashCode" {
                        let m_rung = infer_stmt_effect(body, self.fe);
                        if m_rung == EffectRung::Suspends {
                            self.errors.push(EffectError {
                                message: format!(
                                    "Struct comparison method '{}.{}' cannot suspend: comparisons must be @sync.",
                                    name, m_name
                                ),
                                loc: loc.clone(),
                            });
                        }
                    }
                    self.check_stmt(body);
                }
            }
            Stmt::Concurrent(body) => {
                if self.in_sync_context {
                    self.errors.push(EffectError {
                        message: format!(
                            "Cannot use 'concurrent' block inside {}: synchronous contexts must not suspend.",
                            self.sync_context_desc
                        ),
                        loc: SourceLocation::default(),
                    });
                }
                self.check_stmt(body);
            }
            _ => {}
        }
    }

    pub fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Call(callee, args) => {
                // Check if call suspends inside a @sync context
                if self.in_sync_context {
                    let call_rung = infer_expr_effect(expr, self.fe);
                    if call_rung == EffectRung::Suspends {
                        let callee_name = match callee.as_ref() {
                            Expr::Variable(n, _) => n.clone(),
                            Expr::Get(_, m) => m.clone(),
                            _ => "suspending expression".to_string(),
                        };
                        self.errors.push(EffectError {
                            message: format!(
                                "Cannot call '{}' inside {}: synchronous contexts must not suspend.",
                                callee_name, self.sync_context_desc
                            ),
                            loc: match callee.as_ref() {
                                Expr::Variable(_, loc) => loc.clone(),
                                _ => SourceLocation::default(),
                            },
                        });
                    }
                }

                // Check .sort(comparator) demands
                if let Expr::Get(_, method) = callee.as_ref() {
                    if method == "sort" && !args.is_empty() {
                        let arg = &args[0];
                        let arg_rung = match arg {
                            Expr::Function(_, _, body, _, _) => infer_stmt_effect(body, self.fe),
                            Expr::Variable(name, _) => self.fe.get_function_rung(name),
                            other => infer_expr_effect(other, self.fe),
                        };

                        if arg_rung == EffectRung::Suspends {
                            let loc = match arg {
                                Expr::Variable(_, l) => l.clone(),
                                _ => SourceLocation::default(),
                            };
                            self.errors.push(EffectError {
                                message: "Cannot pass suspending function to 'sort': sorting comparators must be @sync.".to_string(),
                                loc,
                            });
                        }
                    }
                }

                self.check_expr(callee);
                for a in args {
                    self.check_expr(a);
                }
            }
            Expr::Function(_, _, body, _, is_sync) => {
                let prev_sync = self.in_sync_context;
                let prev_desc = self.sync_context_desc.clone();
                if *is_sync {
                    self.in_sync_context = true;
                    self.sync_context_desc = "@sync closure".to_string();
                    let body_rung = infer_stmt_effect(body, self.fe);
                    if body_rung == EffectRung::Suspends {
                        self.errors.push(EffectError {
                            message: "Closure is marked @sync, but performs suspending operation(s).".to_string(),
                            loc: SourceLocation::default(),
                        });
                    }
                }
                self.check_stmt(body);
                self.in_sync_context = prev_sync;
                self.sync_context_desc = prev_desc;
            }
            Expr::Assign(_, val, _) => self.check_expr(val),
            Expr::Binary(left, _, right) | Expr::Logical(left, _, right) => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::Unary(_, inner) | Expr::Prefix(_, inner) | Expr::Postfix(_, inner) | Expr::TypeCast(inner, _, _) | Expr::New(inner) => {
                self.check_expr(inner);
            }
            Expr::Ternary(cond, then_b, else_b) => {
                self.check_expr(cond);
                self.check_expr(then_b);
                self.check_expr(else_b);
            }
            Expr::Array(elements) => {
                for el in elements {
                    self.check_expr(el);
                }
            }
            Expr::Object(pairs) => {
                for (_, val) in pairs {
                    self.check_expr(val);
                }
            }
            Expr::Get(target, _) => self.check_expr(target),
            Expr::Set(target, _, val) => {
                self.check_expr(target);
                self.check_expr(val);
            }
            Expr::GetIndex(target, idx) => {
                self.check_expr(target);
                self.check_expr(idx);
            }
            Expr::SetIndex(target, idx, val) => {
                self.check_expr(target);
                self.check_expr(idx);
                self.check_expr(val);
            }
            Expr::StructInst(_, fields, _) => {
                for (_, val) in fields {
                    self.check_expr(val);
                }
            }
            Expr::Spawn(inner) => self.check_expr(inner),
            Expr::Await(inner) => {
                if self.in_sync_context {
                    self.errors.push(EffectError {
                        message: format!(
                            "Cannot use 'await' inside {}: synchronous contexts must not suspend.",
                            self.sync_context_desc
                        ),
                        loc: SourceLocation::default(),
                    });
                }
                self.check_expr(inner);
            }
            _ => {}
        }
    }
}

/// Verify synchronous constraints across the program AST
pub fn check_sync_constraints(stmts: &[Stmt], fe: &FunctionEffects) -> Result<(), Vec<EffectError>> {
    let mut checker = SyncChecker::new(fe);
    checker.check_program(stmts);
    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{Lexer, Parser, effects::infer_effects};

    fn parse_code(code: &str) -> Vec<Stmt> {
        let mut lexer = Lexer::new(code);
        let mut tokens = Vec::new();
        loop {
            let t = lexer.next_token();
            let is_eof = t.ty == crate::frontend::TokenType::Eof;
            tokens.push(t);
            if is_eof {
                break;
            }
        }
        let mut parser = Parser::new(tokens);
        parser.parse().expect("Parse failed in test")
    }

    #[test]
    fn test_sync_function_rejects_suspension() {
        let code = r#"
            sync fn badCalc() {
                let data = fetch("https://api.com")
                return data
            }
        "#;
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        let result = check_sync_constraints(&stmts, &fe);
        assert!(result.is_err(), "Expected error for suspending sync function");
        let errs = result.unwrap_err();
        assert!(errs[0].message.contains("marked @sync"));
    }

    #[test]
    fn test_at_sync_function_rejects_suspension() {
        let code = r#"
            @sync fn badCalc() {
                return fetch("https://api.com")
            }
        "#;
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        let result = check_sync_constraints(&stmts, &fe);
        assert!(result.is_err(), "Expected error for @sync function calling fetch");
    }

    #[test]
    fn test_sync_block_rejects_suspension() {
        let code = r#"
            sync {
                fetch("https://api.com")
            }
        "#;
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        let result = check_sync_constraints(&stmts, &fe);
        assert!(result.is_err(), "Expected error for sync block calling fetch");
    }

    #[test]
    fn test_sort_comparator_rejects_suspending_closure() {
        let code = r#"
            let users = [1, 2, 3]
            users.sort((a, b) => {
                let diff = fetch("https://api.com/" + a)
                return diff
            })
        "#;
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        let result = check_sync_constraints(&stmts, &fe);
        assert!(result.is_err(), "Expected error for suspending comparator in sort");
        let errs = result.unwrap_err();
        assert!(errs[0].message.contains("Cannot pass suspending function to 'sort'"));
    }

    #[test]
    fn test_valid_sync_passes() {
        let code = r#"
            sync fn add(a, b) {
                return a + b
            }
            let users = [3, 1, 2]
            users.sort((a, b) => a - b)
        "#;
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        let result = check_sync_constraints(&stmts, &fe);
        assert!(result.is_ok(), "Expected valid synchronous code to pass");
    }

    #[test]
    fn test_struct_compare_method_rejects_suspension() {
        let code = r#"
            struct Player {
                id: int,
                fn compare(other) {
                    fetch("https://leaderboard.com")
                    return 0
                }
            }
        "#;
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        let result = check_sync_constraints(&stmts, &fe);
        assert!(result.is_err(), "Expected error for suspending struct compare method");
        let errs = result.unwrap_err();
        assert!(errs[0].message.contains("Struct comparison method 'Player.compare' cannot suspend"));
    }
}

