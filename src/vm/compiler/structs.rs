use std::collections::{HashMap, HashSet};
use crate::frontend::{Expr, Stmt};

#[derive(Clone)]
pub struct FlattenedStructInfo {
    pub composed: Vec<String>,
    pub fields: Vec<(String, String)>,
    pub methods: Vec<(String, Vec<String>, Stmt)>,
}

#[derive(Clone)]
pub struct RawStructInfo {
    pub composed: Vec<String>,
    pub fields: Vec<(String, String)>,
    pub methods: Vec<(String, Vec<String>, Stmt)>,
}

#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub fields: Vec<(String, String)>,
    pub methods: Vec<(String, Vec<String>)>,
}

pub fn collect_structs_expr(expr: &Expr, map: &mut HashMap<String, RawStructInfo>) {
    match expr {
        Expr::Function(_, _, body) => {
            collect_structs(std::slice::from_ref(body), map);
        }
        Expr::Call(callee, args) => {
            collect_structs_expr(callee, map);
            for arg in args {
                collect_structs_expr(arg, map);
            }
        }
        Expr::Binary(left, _, right) | Expr::Logical(left, _, right) => {
            collect_structs_expr(left, map);
            collect_structs_expr(right, map);
        }
        Expr::Unary(_, inner) | Expr::Prefix(_, inner) | Expr::Postfix(_, inner) | Expr::Spawn(inner) => {
            collect_structs_expr(inner, map);
        }
        Expr::Ternary(cond, then_e, else_e) => {
            collect_structs_expr(cond, map);
            collect_structs_expr(then_e, map);
            collect_structs_expr(else_e, map);
        }
        Expr::Array(items) => {
            for it in items {
                collect_structs_expr(it, map);
            }
        }
        Expr::Object(pairs) | Expr::StructInst(_, pairs, _) => {
            for (_, val) in pairs {
                collect_structs_expr(val, map);
            }
        }
        _ => {}
    }
}

pub fn collect_structs(stmts: &[Stmt], map: &mut HashMap<String, RawStructInfo>) {
    for stmt in stmts {
        match stmt {
            Stmt::Struct(name, composed, fields, methods, _) => {
                map.insert(
                    name.clone(),
                    RawStructInfo {
                        composed: composed.clone(),
                        fields: fields.clone(),
                        methods: methods.clone(),
                    },
                );
            }
            Stmt::Block(inner_stmts) => {
                collect_structs(inner_stmts, map);
            }
            Stmt::If(_, then_branch, else_branch) => {
                collect_structs(std::slice::from_ref(then_branch), map);
                if let Some(eb) = else_branch {
                    collect_structs(std::slice::from_ref(eb), map);
                }
            }
            Stmt::While(_, body) | Stmt::For(_, _, _, body) | Stmt::ForIn(_, _, body) | Stmt::Concurrent(body) => {
                collect_structs(std::slice::from_ref(body), map);
            }
            Stmt::Try(try_body, catch_clause, finally_body) => {
                collect_structs(std::slice::from_ref(try_body), map);
                if let Some((_, cb)) = catch_clause {
                    collect_structs(std::slice::from_ref(cb), map);
                }
                if let Some(fb) = finally_body {
                    collect_structs(std::slice::from_ref(fb), map);
                }
            }
            Stmt::Switch(_, cases, default_body) => {
                for c in cases {
                    collect_structs(std::slice::from_ref(&c.body), map);
                }
                if let Some(db) = default_body {
                    collect_structs(std::slice::from_ref(db), map);
                }
            }
            Stmt::VarDecl(_, _, _, init, _) | Stmt::Expr(init) | Stmt::Print(init) | Stmt::Throw(init) => {
                collect_structs_expr(init, map);
            }
            Stmt::Return(Some(expr), _) => {
                collect_structs_expr(expr, map);
            }
            Stmt::Export(inner) => {
                collect_structs(std::slice::from_ref(inner), map);
            }
            _ => {}
        }
    }
}

pub fn collect_interfaces(stmts: &[Stmt], map: &mut HashMap<String, InterfaceInfo>) {
    for stmt in stmts {
        match stmt {
            Stmt::Interface(name, fields, methods, _) => {
                map.insert(
                    name.clone(),
                    InterfaceInfo {
                        fields: fields.clone(),
                        methods: methods.clone(),
                    },
                );
            }
            Stmt::Block(inner_stmts) => {
                collect_interfaces(inner_stmts, map);
            }
            Stmt::If(_, then_branch, else_branch) => {
                collect_interfaces(std::slice::from_ref(then_branch), map);
                if let Some(eb) = else_branch {
                    collect_interfaces(std::slice::from_ref(eb), map);
                }
            }
            Stmt::While(_, body) | Stmt::For(_, _, _, body) | Stmt::ForIn(_, _, body) | Stmt::Concurrent(body) => {
                collect_interfaces(std::slice::from_ref(body), map);
            }
            Stmt::Try(try_body, catch_clause, finally_body) => {
                collect_interfaces(std::slice::from_ref(try_body), map);
                if let Some((_, cb)) = catch_clause {
                    collect_interfaces(std::slice::from_ref(cb), map);
                }
                if let Some(fb) = finally_body {
                    collect_interfaces(std::slice::from_ref(fb), map);
                }
            }
            Stmt::Switch(_, cases, default_body) => {
                for c in cases {
                    collect_interfaces(std::slice::from_ref(&c.body), map);
                }
                if let Some(db) = default_body {
                    collect_interfaces(std::slice::from_ref(db), map);
                }
            }
            Stmt::Export(inner) => {
                collect_interfaces(std::slice::from_ref(inner), map);
            }
            _ => {}
        }
    }
}

pub fn flatten_struct(
    name: &str,
    raw_structs: &HashMap<String, RawStructInfo>,
    resolved: &mut HashMap<String, FlattenedStructInfo>,
    visiting: &mut HashSet<String>,
) -> Result<FlattenedStructInfo, String> {
    if let Some(info) = resolved.get(name) {
        return Ok(info.clone());
    }

    if visiting.contains(name) {
        return Err(format!("Cyclic struct embedding detected in struct '{}'", name));
    }

    let raw = raw_structs
        .get(name)
        .ok_or_else(|| format!("Struct '{}' not defined", name))?;

    visiting.insert(name.to_string());

    let mut all_composed = Vec::new();
    let mut all_fields = Vec::new();
    let mut all_methods = Vec::new();

    for parent_name in &raw.composed {
        all_composed.push(parent_name.clone());
        let parent_flattened = flatten_struct(parent_name, raw_structs, resolved, visiting)?;
        for ancestor in parent_flattened.composed {
            if !all_composed.contains(&ancestor) {
                all_composed.push(ancestor);
            }
        }
        for field in parent_flattened.fields {
            if !all_fields.iter().any(|(f_name, _)| f_name == &field.0) {
                all_fields.push(field);
            }
        }
        for method in parent_flattened.methods {
            if !all_methods.iter().any(|(m_name, _, _)| m_name == &method.0) {
                all_methods.push(method);
            }
        }
    }

    for field in &raw.fields {
        if let Some(idx) = all_fields.iter().position(|(f_name, _)| f_name == &field.0) {
            all_fields[idx] = field.clone();
        } else {
            all_fields.push(field.clone());
        }
    }

    for method in &raw.methods {
        if let Some(idx) = all_methods.iter().position(|(m_name, _, _)| m_name == &method.0) {
            all_methods[idx] = method.clone();
        } else {
            all_methods.push(method.clone());
        }
    }

    visiting.remove(name);

    let flattened = FlattenedStructInfo {
        composed: all_composed,
        fields: all_fields,
        methods: all_methods,
    };

    resolved.insert(name.to_string(), flattened.clone());
    Ok(flattened)
}
