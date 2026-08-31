use eronom::frontend::{Expr, LiteralValue, Stmt};

pub fn has_http_import(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        if has_http_import_in_stmt(stmt) {
            return true;
        }
    }
    false
}

pub fn has_http_import_in_stmt(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::VarDecl(_, _, _, _, loc) => {
            if loc.file_path.ends_with("std/http.er") {
                return true;
            }
        }
        Stmt::Struct(_, _, _, _, loc) => {
            if loc.file_path.ends_with("std/http.er") {
                return true;
            }
        }
        Stmt::Interface(_, _, _, loc) => {
            if loc.file_path.ends_with("std/http.er") {
                return true;
            }
        }
        Stmt::Block(inner) => {
            if has_http_import(inner) {
                return true;
            }
        }
        Stmt::If(_, then_stmt, else_stmt) => {
            if has_http_import_in_stmt(then_stmt) {
                return true;
            }
            if let Some(e) = else_stmt {
                if has_http_import_in_stmt(e) {
                    return true;
                }
            }
        }
        Stmt::While(_, body) => {
            if has_http_import_in_stmt(body) {
                return true;
            }
        }
        Stmt::For(_, start, end, body) => {
            if has_http_import_in_stmt(body) {
                return true;
            }
        }
        Stmt::ForIn(_, _, body) => {
            if has_http_import_in_stmt(body) {
                return true;
            }
        }
        Stmt::Try(try_body, catch_clause, finally_body) => {
            if has_http_import_in_stmt(try_body) {
                return true;
            }
            if let Some((_, catch_b)) = catch_clause {
                if has_http_import_in_stmt(catch_b) {
                    return true;
                }
            }
            if let Some(finally_b) = finally_body {
                if has_http_import_in_stmt(finally_b) {
                    return true;
                }
            }
        }
        Stmt::Switch(_, cases, default_body) => {
            for c in cases {
                if has_http_import_in_stmt(&c.body) {
                    return true;
                }
            }
            if let Some(def_b) = default_body {
                if has_http_import_in_stmt(def_b) {
                    return true;
                }
            }
        }
        Stmt::Export(inner) => {
            if has_http_import_in_stmt(inner) {
                return true;
            }
        }
        _ => {}
    }
    false
}

pub fn find_listen_port_in_expr(expr: &Expr) -> Option<i32> {
    match expr {
        Expr::Call(callee, args) => {
            if let Expr::Get(_, method) = callee.as_ref() {
                if method == "listen" && !args.is_empty() {
                    if let Expr::Literal(LiteralValue::Number(n)) = &args[0] {
                        return Some(*n as i32);
                    }
                }
            }
            for arg in args {
                if let Some(port) = find_listen_port_in_expr(arg) {
                    return Some(port);
                }
            }
            None
        }
        Expr::Assign(_, val, _) => find_listen_port_in_expr(val),
        Expr::Binary(left, _, right) => {
            find_listen_port_in_expr(left).or_else(|| find_listen_port_in_expr(right))
        }
        Expr::Logical(left, _, right) => {
            find_listen_port_in_expr(left).or_else(|| find_listen_port_in_expr(right))
        }
        Expr::Unary(_, inner) => find_listen_port_in_expr(inner),
        Expr::Prefix(_, inner) => find_listen_port_in_expr(inner),
        Expr::Postfix(_, inner) => find_listen_port_in_expr(inner),
        Expr::Ternary(cond, then_b, else_b) => {
            find_listen_port_in_expr(cond)
                .or_else(|| find_listen_port_in_expr(then_b))
                .or_else(|| find_listen_port_in_expr(else_b))
        }
        Expr::Get(obj, _) => return find_listen_port_in_expr(obj),
        Expr::Set(target, _, val) => {
            find_listen_port_in_expr(target).or_else(|| find_listen_port_in_expr(val))
        }
        Expr::Array(elements) => {
            for el in elements {
                if let Some(port) = find_listen_port_in_expr(el) {
                    return Some(port);
                }
            }
            None
        }
        Expr::Object(entries) => {
            for (_, val) in entries {
                if let Some(port) = find_listen_port_in_expr(val) {
                    return Some(port);
                }
            }
            None
        }
        Expr::Function(_, _, body) => find_listen_port_in_stmt(body),
        Expr::GetIndex(target, index) => {
            find_listen_port_in_expr(target).or_else(|| find_listen_port_in_expr(index))
        }
        Expr::SetIndex(target, index, val) => find_listen_port_in_expr(target)
            .or_else(|| find_listen_port_in_expr(index))
            .or_else(|| find_listen_port_in_expr(val)),
        Expr::StructInst(_, fields, _) => {
            for (_, val) in fields {
                if let Some(port) = find_listen_port_in_expr(val) {
                    return Some(port);
                }
            }
            None
        }
        Expr::Spawn(inner) => find_listen_port_in_expr(inner),
        _ => None,
    }
}

pub fn find_listen_port_in_stmt(stmt: &Stmt) -> Option<i32> {
    match stmt {
        Stmt::Expr(expr) => find_listen_port_in_expr(expr),
        Stmt::Print(expr) => find_listen_port_in_expr(expr),
        Stmt::VarDecl(_, _, _, init, _) => find_listen_port_in_expr(init),
        Stmt::Block(stmts) => {
            for s in stmts {
                if let Some(port) = find_listen_port_in_stmt(s) {
                    return Some(port);
                }
            }
            None
        }
        Stmt::If(cond, then_stmt, else_stmt) => {
            if let Some(p) = find_listen_port_in_expr(cond) {
                return Some(p);
            }
            if let Some(p) = find_listen_port_in_stmt(then_stmt) {
                return Some(p);
            }
            if let Some(e) = else_stmt {
                if let Some(p) = find_listen_port_in_stmt(e) {
                    return Some(p);
                }
            }
            None
        }
        Stmt::While(cond, body) => {
            if let Some(p) = find_listen_port_in_expr(cond) {
                return Some(p);
            }
            if let Some(p) = find_listen_port_in_stmt(body) {
                return Some(p);
            }
            None
        }
        Stmt::For(_, start, end, body) => {
            if let Some(p) = find_listen_port_in_expr(start) {
                return Some(p);
            }
            if let Some(p) = find_listen_port_in_expr(end) {
                return Some(p);
            }
            if let Some(p) = find_listen_port_in_stmt(body) {
                return Some(p);
            }
            None
        }
        Stmt::ForIn(_, iterable, body) => {
            if let Some(p) = find_listen_port_in_expr(iterable) {
                return Some(p);
            }
            if let Some(p) = find_listen_port_in_stmt(body) {
                return Some(p);
            }
            None
        }
        Stmt::Throw(expr) => find_listen_port_in_expr(expr),
        Stmt::Try(try_body, catch_clause, finally_body) => {
            if let Some(p) = find_listen_port_in_stmt(try_body) {
                return Some(p);
            }
            if let Some((_, catch_b)) = catch_clause {
                if let Some(p) = find_listen_port_in_stmt(catch_b) {
                    return Some(p);
                }
            }
            if let Some(finally_b) = finally_body {
                if let Some(p) = find_listen_port_in_stmt(finally_b) {
                    return Some(p);
                }
            }
            None
        }
        Stmt::Switch(target, cases, default_body) => {
            if let Some(p) = find_listen_port_in_expr(target) {
                return Some(p);
            }
            for c in cases {
                for v in &c.values {
                    if let Some(p) = find_listen_port_in_expr(v) {
                        return Some(p);
                    }
                }
                if let Some(p) = find_listen_port_in_stmt(&c.body) {
                    return Some(p);
                }
            }
            if let Some(def_b) = default_body {
                if let Some(p) = find_listen_port_in_stmt(def_b) {
                    return Some(p);
                }
            }
            None
        }
        Stmt::Return(expr_opt, _) => {
            if let Some(expr) = expr_opt {
                find_listen_port_in_expr(expr)
            } else {
                None
            }
        }
        Stmt::Export(inner) => find_listen_port_in_stmt(inner),
        _ => None,
    }
}

pub fn find_listen_port(stmts: &[Stmt]) -> Option<i32> {
    for s in stmts {
        if let Some(port) = find_listen_port_in_stmt(s) {
            return Some(port);
        }
    }
    None
}
