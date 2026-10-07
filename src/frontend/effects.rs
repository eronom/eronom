use std::collections::HashMap;
use crate::frontend::ast::{Expr, Stmt};

/// The 3-point lattice of effect rungs: pure ⊏ impure ⊏ suspends
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EffectRung {
    Pure = 0,      // Deterministic, no side effects, never suspends
    Impure = 1,    // May mutate state or emit I/O, but never suspends/parks
    Suspends = 2,  // May park/suspend execution (network, timers, cross-fiber joins)
}

impl EffectRung {
    pub fn combine(self, other: Self) -> Self {
        std::cmp::max(self, other)
    }
}

impl std::fmt::Display for EffectRung {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EffectRung::Pure => write!(f, "pure"),
            EffectRung::Impure => write!(f, "impure"),
            EffectRung::Suspends => write!(f, "suspends"),
        }
    }
}

/// Stores inferred effect rungs for functions, methods, and closures
#[derive(Debug, Clone)]
pub struct FunctionEffects {
    pub functions: HashMap<String, EffectRung>,
    pub methods: HashMap<String, EffectRung>,
}

impl Default for FunctionEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionEffects {
    pub fn new() -> Self {
        let mut fe = Self {
            functions: HashMap::new(),
            methods: HashMap::new(),
        };

        // Seed known suspending primitives
        fe.functions.insert("fetch".to_string(), EffectRung::Suspends);
        fe.functions.insert("fetchEvented".to_string(), EffectRung::Suspends);
        fe.functions.insert("fetchSync".to_string(), EffectRung::Impure);
        fe.functions.insert("futureAwait".to_string(), EffectRung::Suspends);
        fe.functions.insert("sleep".to_string(), EffectRung::Suspends);
        fe.functions.insert("spawnTask".to_string(), EffectRung::Suspends);
        fe.functions.insert("wait".to_string(), EffectRung::Suspends);
        fe.functions.insert("await".to_string(), EffectRung::Suspends);

        // Seed known impure functions (side-effects without pausing)
        fe.functions.insert("print".to_string(), EffectRung::Impure);
        fe.functions.insert("println".to_string(), EffectRung::Impure);
        fe.functions.insert("write".to_string(), EffectRung::Impure);
        fe.functions.insert("setIoMode".to_string(), EffectRung::Impure);
        fe.functions.insert("setTimeout".to_string(), EffectRung::Impure);
        fe.functions.insert("clearTimeout".to_string(), EffectRung::Impure);
        fe.functions.insert("arrayPush".to_string(), EffectRung::Impure);
        fe.functions.insert("arrayPop".to_string(), EffectRung::Impure);
        fe.functions.insert("arrayShift".to_string(), EffectRung::Impure);

        // Seed known pure functions
        fe.functions.insert("arrayLen".to_string(), EffectRung::Pure);
        fe.functions.insert("stringIndexOf".to_string(), EffectRung::Pure);
        fe.functions.insert("now".to_string(), EffectRung::Pure);
        fe.functions.insert("localTimeString".to_string(), EffectRung::Pure);

        fe
    }

    pub fn get_function_rung(&self, name: &str) -> EffectRung {
        self.functions.get(name).copied().unwrap_or(EffectRung::Pure)
    }

    pub fn get_method_rung(&self, struct_name: &str, method_name: &str) -> EffectRung {
        let key = format!("{}.{}", struct_name, method_name);
        self.methods.get(&key).copied().unwrap_or(EffectRung::Pure)
    }
}

/// Infer the effect rung of an expression
pub fn infer_expr_effect(expr: &Expr, fe: &FunctionEffects) -> EffectRung {
    match expr {
        Expr::Literal(_) => EffectRung::Pure,
        Expr::Variable(name, _) => fe.get_function_rung(name),
        Expr::Assign(_, val, _) => EffectRung::Impure.combine(infer_expr_effect(val, fe)),
        Expr::Binary(left, _, right) | Expr::Logical(left, _, right) => {
            infer_expr_effect(left, fe).combine(infer_expr_effect(right, fe))
        }
        Expr::Unary(_, inner) | Expr::TypeCast(inner, _, _) | Expr::New(inner) => {
            infer_expr_effect(inner, fe)
        }
        Expr::Prefix(_, inner) | Expr::Postfix(_, inner) => {
            EffectRung::Impure.combine(infer_expr_effect(inner, fe))
        }
        Expr::Ternary(cond, then_b, else_b) => {
            infer_expr_effect(cond, fe)
                .combine(infer_expr_effect(then_b, fe))
                .combine(infer_expr_effect(else_b, fe))
        }
        Expr::Array(elements) => {
            elements.iter().fold(EffectRung::Pure, |acc, e| acc.combine(infer_expr_effect(e, fe)))
        }
        Expr::Object(pairs) => {
            pairs.iter().fold(EffectRung::Pure, |acc, (_, e)| acc.combine(infer_expr_effect(e, fe)))
        }
        Expr::Get(target, prop) => {
            if let Expr::Variable(target_name, _) = target.as_ref() {
                if target_name == "Io" && prop == "wait" {
                    return EffectRung::Suspends;
                }
                if target_name == "Task" && (prop == "race" || prop == "timeout" || prop == "forEach" || prop == "scope") {
                    return EffectRung::Suspends;
                }
            }
            infer_expr_effect(target, fe)
        }
        Expr::Set(target, _, val) => {
            EffectRung::Impure
                .combine(infer_expr_effect(target, fe))
                .combine(infer_expr_effect(val, fe))
        }
        Expr::GetIndex(target, idx) => {
            infer_expr_effect(target, fe).combine(infer_expr_effect(idx, fe))
        }
        Expr::SetIndex(target, idx, val) => {
            EffectRung::Impure
                .combine(infer_expr_effect(target, fe))
                .combine(infer_expr_effect(idx, fe))
                .combine(infer_expr_effect(val, fe))
        }
        Expr::StructInst(_, fields, _) => {
            fields.iter().fold(EffectRung::Pure, |acc, (_, e)| acc.combine(infer_expr_effect(e, fe)))
        }
        Expr::Spawn(inner) => EffectRung::Impure.combine(infer_expr_effect(inner, fe)),
        Expr::Await(inner) => EffectRung::Suspends.combine(infer_expr_effect(inner, fe)),
        Expr::Function(_, _, _body, _is_async, _is_sync) => {
            // Function value definition itself does not execute until called
            EffectRung::Pure
        }
        Expr::Call(callee, args) => {
            let mut callee_rung = infer_expr_effect(callee, fe);
            match callee.as_ref() {
                Expr::Variable(name, _) => {
                    callee_rung = callee_rung.combine(fe.get_function_rung(name));
                }
                Expr::Get(target, method) => {
                    if method == "join" || method == "wait" {
                        callee_rung = callee_rung.combine(EffectRung::Suspends);
                    }
                    if let Expr::Variable(target_name, _) = target.as_ref() {
                        if target_name == "Io" && method == "wait" {
                            callee_rung = callee_rung.combine(EffectRung::Suspends);
                        }
                        if target_name == "Task" && (method == "race" || method == "timeout" || method == "forEach" || method == "scope") {
                            callee_rung = callee_rung.combine(EffectRung::Suspends);
                        }
                    }
                }
                Expr::Function(_, _, body, _, _) => {
                    callee_rung = callee_rung.combine(infer_stmt_effect(body, fe));
                }
                _ => {}
            }
            args.iter().fold(callee_rung, |acc, a| acc.combine(infer_expr_effect(a, fe)))
        }
    }
}

/// Infer the effect rung of a statement
pub fn infer_stmt_effect(stmt: &Stmt, fe: &FunctionEffects) -> EffectRung {
    match stmt {
        Stmt::Expr(e) => infer_expr_effect(e, fe),
        Stmt::Print(e) => EffectRung::Impure.combine(infer_expr_effect(e, fe)),
        Stmt::VarDecl(_, _, _, init, _) => infer_expr_effect(init, fe),
        Stmt::Block(stmts) => {
            stmts.iter().fold(EffectRung::Pure, |acc, s| acc.combine(infer_stmt_effect(s, fe)))
        }
        Stmt::If(cond, then_b, else_b) => {
            let cond_rung = infer_expr_effect(cond, fe);
            let then_rung = infer_stmt_effect(then_b, fe);
            let else_rung = else_b.as_ref().map_or(EffectRung::Pure, |eb| infer_stmt_effect(eb, fe));
            cond_rung.combine(then_rung).combine(else_rung)
        }
        Stmt::While(cond, body) => {
            infer_expr_effect(cond, fe).combine(infer_stmt_effect(body, fe))
        }
        Stmt::For(_, start, end, body) => {
            infer_expr_effect(start, fe)
                .combine(infer_expr_effect(end, fe))
                .combine(infer_stmt_effect(body, fe))
        }
        Stmt::ForIn(_, iter, body) => {
            infer_expr_effect(iter, fe).combine(infer_stmt_effect(body, fe))
        }
        Stmt::Break | Stmt::Continue => EffectRung::Pure,
        Stmt::Throw(e) => EffectRung::Impure.combine(infer_expr_effect(e, fe)),
        Stmt::Try(try_b, catch_c, finally_b) => {
            let mut rung = infer_stmt_effect(try_b, fe);
            if let Some((_, cb)) = catch_c {
                rung = rung.combine(infer_stmt_effect(cb, fe));
            }
            if let Some(fb) = finally_b {
                rung = rung.combine(infer_stmt_effect(fb, fe));
            }
            rung
        }
        Stmt::Switch(target, cases, def_b) => {
            let mut rung = infer_expr_effect(target, fe);
            for c in cases {
                for v in &c.values {
                    rung = rung.combine(infer_expr_effect(v, fe));
                }
                rung = rung.combine(infer_stmt_effect(&c.body, fe));
            }
            if let Some(db) = def_b {
                rung = rung.combine(infer_stmt_effect(db, fe));
            }
            rung
        }
        Stmt::Return(opt_e, _) => {
            opt_e.as_ref().map_or(EffectRung::Pure, |e| infer_expr_effect(e, fe))
        }
        Stmt::Import(_, _) => EffectRung::Pure,
        Stmt::Export(inner) => infer_stmt_effect(inner, fe),
        Stmt::Struct(name, _, _, methods, _) => {
            for (m_name, _, body) in methods {
                let m_rung = infer_stmt_effect(body, fe);
                let key = format!("{}.{}", name, m_name);
                // Recorded in fe during whole-program inference
                let _ = m_rung;
                let _ = key;
            }
            EffectRung::Pure
        }
        Stmt::Interface(_, _, _, _) | Stmt::TypeAlias(_, _, _, _) | Stmt::Enum(_, _, _) => {
            EffectRung::Pure
        }
        Stmt::Concurrent(body) => EffectRung::Suspends.combine(infer_stmt_effect(body, fe)),
        Stmt::Sync(body) => infer_stmt_effect(body, fe),
    }
}

/// Perform monotonic fixed-point effect inference over the entire program AST
pub fn infer_effects(stmts: &[Stmt]) -> FunctionEffects {
    let mut fe = FunctionEffects::new();

    // 1. Collect all declared functions and methods
    let mut fn_bodies: Vec<(String, &Stmt)> = Vec::new();
    let mut method_bodies: Vec<(String, String, &Stmt)> = Vec::new();

    fn collect_functions<'a>(
        s: &'a Stmt,
        fn_bodies: &mut Vec<(String, &'a Stmt)>,
        method_bodies: &mut Vec<(String, String, &'a Stmt)>,
    ) {
        match s {
            Stmt::VarDecl(name, _, _, Expr::Function(_, _, body, _, _), _) => {
                fn_bodies.push((name.clone(), body));
            }
            Stmt::Struct(name, _, _, methods, _) => {
                for (m_name, _, body) in methods {
                    method_bodies.push((name.clone(), m_name.clone(), body));
                }
            }
            Stmt::Export(inner) => collect_functions(inner, fn_bodies, method_bodies),
            Stmt::Block(stmts) => {
                for inner in stmts {
                    collect_functions(inner, fn_bodies, method_bodies);
                }
            }
            Stmt::Sync(inner) | Stmt::Concurrent(inner) => {
                collect_functions(inner, fn_bodies, method_bodies);
            }
            _ => {}
        }
    }

    for s in stmts {
        collect_functions(s, &mut fn_bodies, &mut method_bodies);
    }

    // Initialize collected functions to Pure if not already set
    for (name, _) in &fn_bodies {
        fe.functions.entry(name.clone()).or_insert(EffectRung::Pure);
    }
    for (s_name, m_name, _) in &method_bodies {
        let key = format!("{}.{}", s_name, m_name);
        fe.methods.entry(key).or_insert(EffectRung::Pure);
    }

    // 2. Fixed-point iteration on the 3-point lattice (Pure < Impure < Suspends)
    let max_iterations = 10;
    for _ in 0..max_iterations {
        let mut changed = false;

        for (name, body) in &fn_bodies {
            let current = fe.get_function_rung(name);
            let inferred = infer_stmt_effect(body, &fe);
            if inferred > current {
                fe.functions.insert(name.clone(), inferred);
                changed = true;
            }
        }

        for (s_name, m_name, body) in &method_bodies {
            let key = format!("{}.{}", s_name, m_name);
            let current = fe.methods.get(&key).copied().unwrap_or(EffectRung::Pure);
            let inferred = infer_stmt_effect(body, &fe);
            if inferred > current {
                fe.methods.insert(key, inferred);
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    fe
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{Lexer, Parser};

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
    fn test_pure_function_inferred_pure() {
        let code = "fn add(a, b) { return a + b }";
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        assert_eq!(fe.get_function_rung("add"), EffectRung::Pure);
    }

    #[test]
    fn test_impure_function_inferred_impure() {
        let code = "fn log(msg) { print(msg) }";
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        assert_eq!(fe.get_function_rung("log"), EffectRung::Impure);
    }

    #[test]
    fn test_suspending_function_inferred_suspends() {
        let code = "fn fetchUser(id) { return fetch(\"https://api.com/\" + id) }";
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        assert_eq!(fe.get_function_rung("fetchUser"), EffectRung::Suspends);
    }

    #[test]
    fn test_propagated_suspension() {
        let code = r#"
            fn lowLevelFetch(url) {
                return fetch(url)
            }
            fn midLevelWorker(url) {
                return lowLevelFetch(url)
            }
            fn topLevelApp() {
                return midLevelWorker("https://data.com")
            }
        "#;
        let stmts = parse_code(code);
        let fe = infer_effects(&stmts);
        assert_eq!(fe.get_function_rung("lowLevelFetch"), EffectRung::Suspends);
        assert_eq!(fe.get_function_rung("midLevelWorker"), EffectRung::Suspends);
        assert_eq!(fe.get_function_rung("topLevelApp"), EffectRung::Suspends);
    }
}

