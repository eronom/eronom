use std::collections::HashMap;
use crate::frontend::{Expr, LiteralValue, SourceLocation, TokenType};
use super::structs::{FlattenedStructInfo, InterfaceInfo};
use super::Local;

pub fn get_expr_type(
    expr: &Expr,
    locals: &[Local],
    global_types: &HashMap<String, String>,
    structs: &HashMap<String, FlattenedStructInfo>,
    interfaces: &HashMap<String, InterfaceInfo>,
) -> Option<String> {
    match expr {
        Expr::Literal(LiteralValue::Number(_)) => Some("int".to_string()),
        Expr::Literal(LiteralValue::String(_)) => Some("string".to_string()),
        Expr::Literal(LiteralValue::Boolean(_)) => Some("boolean".to_string()),
        Expr::Literal(LiteralValue::Null) => Some("null".to_string()),
        Expr::Array(_) => Some("array".to_string()),
        Expr::Object(_) => Some("object".to_string()),
        Expr::Function(_, ret_type, _) => {
            if let Some(r) = ret_type {
                Some(format!("function:{}", r))
            } else {
                Some("function".to_string())
            }
        }
        Expr::Variable(name, _) => {
            if let Some(local) = locals.iter().rev().find(|l| &l.name == name) {
                return local.ty.clone();
            }
            if let Some(ty) = global_types.get(name) {
                return Some(ty.clone());
            }
            None
        }
        Expr::StructInst(struct_name, _, _) => Some(struct_name.clone()),
        Expr::Call(callee, _) => {
            if let Expr::Variable(name, _) = &**callee {
                if structs.contains_key(name) {
                    return Some(name.clone());
                }
                if let Some(ty) = global_types.get(name) {
                    if ty.starts_with("function:") {
                        return Some(ty["function:".len()..].to_string());
                    }
                }
            }
            None
        }
        Expr::Binary(left, op, right) => {
            match op {
                TokenType::Plus => {
                    let left_ty = get_expr_type(left, locals, global_types, structs, interfaces);
                    let right_ty = get_expr_type(right, locals, global_types, structs, interfaces);
                    if left_ty.as_deref() == Some("string") || right_ty.as_deref() == Some("string") {
                        Some("string".to_string())
                    } else {
                        Some("int".to_string())
                    }
                }
                TokenType::Minus | TokenType::Star | TokenType::Slash | TokenType::Percent |
                TokenType::Ampersand | TokenType::Pipe | TokenType::Caret |
                TokenType::LessLess | TokenType::GreaterGreater => Some("int".to_string()),
                TokenType::EqualEqual | TokenType::BangEqual | TokenType::Less | TokenType::LessEqual |
                TokenType::Greater | TokenType::GreaterEqual | TokenType::And | TokenType::Or => Some("boolean".to_string()),
                _ => None,
            }
        }
        Expr::Unary(op, _) => {
            match op {
                TokenType::Bang => Some("boolean".to_string()),
                TokenType::Minus | TokenType::Tilde => Some("int".to_string()),
                TokenType::Typeof => Some("string".to_string()),
                _ => None,
            }
        }
        Expr::Prefix(_, _) | Expr::Postfix(_, _) => Some("int".to_string()),
        Expr::Ternary(_, then_branch, else_branch) => {
            let then_ty = get_expr_type(then_branch, locals, global_types, structs, interfaces);
            let else_ty = get_expr_type(else_branch, locals, global_types, structs, interfaces);
            if then_ty == else_ty {
                then_ty
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn is_type_compatible(
    expected: &str,
    actual: &str,
    structs: &HashMap<String, FlattenedStructInfo>,
    interfaces: &HashMap<String, InterfaceInfo>,
) -> bool {
    let exp_lower = expected.to_lowercase();
    let act_lower = actual.to_lowercase();

    if exp_lower == act_lower {
        return true;
    }

    if act_lower == "null" {
        return true;
    }

    // Number types
    let is_exp_num = matches!(exp_lower.as_str(), "int" | "number" | "float" | "i32" | "i64" | "f32" | "f64");
    let is_act_num = matches!(act_lower.as_str(), "int" | "number" | "float" | "i32" | "i64" | "f32" | "f64");
    if is_exp_num && is_act_num {
        return true;
    }

    // Boolean types
    let is_exp_bool = matches!(exp_lower.as_str(), "bool" | "boolean");
    let is_act_bool = matches!(act_lower.as_str(), "bool" | "boolean");
    if is_exp_bool && is_act_bool {
        return true;
    }

    // String types
    let is_exp_str = matches!(exp_lower.as_str(), "str" | "string");
    let is_act_str = matches!(act_lower.as_str(), "str" | "string");
    if is_exp_str && is_act_str {
        return true;
    }

    // Function types
    if (exp_lower == "function" || exp_lower == "fn") && (act_lower == "function" || act_lower == "fn" || act_lower.starts_with("function:")) {
        return true;
    }

    // Array types
    if exp_lower == "array" && act_lower == "array" {
        return true;
    }

    // Object types
    if exp_lower == "object" && act_lower == "object" {
        return true;
    }

    // Struct embedding / inheritance polymorphism
    if let Some(act_struct) = structs.get(actual) {
        if act_struct.composed.contains(&expected.to_string()) {
            return true;
        }
    }

    // Interface satisfaction
    if interfaces.contains_key(expected) {
        if let Some(struct_info) = structs.get(actual) {
            let iface = interfaces.get(expected).unwrap();
            let struct_fields: HashMap<String, String> = struct_info.fields.iter().cloned().collect();
            for (f_name, f_ty) in &iface.fields {
                if let Some(sf_ty) = struct_fields.get(f_name) {
                    if !is_type_compatible(f_ty, sf_ty, structs, interfaces) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            let struct_methods: HashMap<String, usize> = struct_info.methods.iter().map(|(m, p, _)| (m.clone(), p.len())).collect();
            for (m_name, m_params) in &iface.methods {
                if let Some(&param_count) = struct_methods.get(m_name) {
                    if param_count != m_params.len() {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            return true;
        }
    }

    false
}

pub fn check_type(
    expr: &Expr,
    expected_type: &str,
    structs: &HashMap<String, FlattenedStructInfo>,
    interfaces: &HashMap<String, InterfaceInfo>,
    locals: &[Local],
    global_types: &HashMap<String, String>,
    loc: &SourceLocation,
) -> Result<(), String> {
    // 1. If it's a struct instantiation or object literal checked against struct
    if let Expr::StructInst(struct_name, pairs, s_loc) = expr {
        if struct_name != expected_type && !is_type_compatible(expected_type, struct_name, structs, interfaces) {
            if interfaces.contains_key(expected_type) {
                return Err(format!(
                    "error: Struct \"{}\" does not implement interface \"{}\"\n    at {}:{}:{}",
                    struct_name, expected_type, s_loc.file_path, s_loc.line, s_loc.col
                ));
            }
            return Err(format!(
                "error: Expected type \"{}\" but got struct \"{}\"\n    at {}:{}:{}",
                expected_type, struct_name, s_loc.file_path, s_loc.line, s_loc.col
            ));
        }
        if let Some(s_info) = structs.get(struct_name) {
            let mut object_fields = HashMap::new();
            for (k, v) in pairs {
                object_fields.insert(k.clone(), v);
            }
            for (field_name, field_type) in &s_info.fields {
                if let Some(field_val_expr) = object_fields.remove(field_name) {
                    check_type(field_val_expr, field_type, structs, interfaces, locals, global_types, s_loc)?;
                } else {
                    return Err(format!(
                        "error: Missing field \"{}\" of type \"{}\" in struct \"{}\"\n    at {}:{}:{}",
                        field_name, field_type, struct_name, s_loc.file_path, s_loc.line, s_loc.col
                    ));
                }
            }
            if !object_fields.is_empty() {
                let extra_fields: Vec<String> = object_fields.keys().cloned().collect();
                return Err(format!(
                    "error: Extra fields {:?} not declared in struct \"{}\"\n    at {}:{}:{}",
                    extra_fields, struct_name, s_loc.file_path, s_loc.line, s_loc.col
                ));
            }
        }
        return Ok(());
    }

    // 2. Struct expected with object literal
    if let Some(struct_info) = structs.get(expected_type) {
        match expr {
            Expr::Object(pairs) => {
                let mut object_fields = HashMap::new();
                for (k, v) in pairs {
                    object_fields.insert(k.clone(), v);
                }
                for (field_name, field_type) in &struct_info.fields {
                    if let Some(field_val_expr) = object_fields.remove(field_name) {
                        check_type(field_val_expr, field_type, structs, interfaces, locals, global_types, loc)?;
                    } else {
                        return Err(format!(
                            "error: Missing field \"{}\" of type \"{}\" in struct \"{}\"\n    at {}:{}:{}",
                            field_name, field_type, expected_type, loc.file_path, loc.line, loc.col
                        ));
                    }
                }
                if !object_fields.is_empty() {
                    let extra_fields: Vec<String> = object_fields.keys().cloned().collect();
                    return Err(format!(
                        "error: Extra fields {:?} not declared in struct \"{}\"\n    at {}:{}:{}",
                        extra_fields, expected_type, loc.file_path, loc.line, loc.col
                    ));
                }
                return Ok(());
            }
            Expr::Array(_) => return Ok(()),
            Expr::Literal(LiteralValue::Null) => return Ok(()),
            _ => {}
        }
    }

    // 3. Interface expected
    if let Some(interface_info) = interfaces.get(expected_type) {
        if let Expr::Object(pairs) = expr {
            if !interface_info.methods.is_empty() {
                return Err(format!(
                    "error: Object literal cannot satisfy interface \"{}\" because the interface requires methods: {:?}\n    at {}:{}:{}",
                    expected_type,
                    interface_info.methods.iter().map(|(m, _)| m).collect::<Vec<_>>(),
                    loc.file_path, loc.line, loc.col
                ));
            }
            let mut object_fields = HashMap::new();
            for (k, v) in pairs {
                object_fields.insert(k.clone(), v);
            }
            for (field_name, field_type) in &interface_info.fields {
                if let Some(field_val_expr) = object_fields.remove(field_name) {
                    check_type(field_val_expr, field_type, structs, interfaces, locals, global_types, loc)?;
                } else {
                    return Err(format!(
                        "error: Missing field \"{}\" of type \"{}\" required by interface \"{}\"\n    at {}:{}:{}",
                        field_name, field_type, expected_type, loc.file_path, loc.line, loc.col
                    ));
                }
            }
            return Ok(());
        }
    }

    // 4. Inferred expression type check
    if let Some(actual_type) = get_expr_type(expr, locals, global_types, structs, interfaces) {
        if is_type_compatible(expected_type, &actual_type, structs, interfaces) {
            return Ok(());
        } else {
            if interfaces.contains_key(expected_type) && structs.contains_key(&actual_type) {
                return Err(format!(
                    "error: Struct \"{}\" does not implement interface \"{}\"\n    at {}:{}:{}",
                    actual_type, expected_type, loc.file_path, loc.line, loc.col
                ));
            }
            let got_str = match expr {
                Expr::Literal(LiteralValue::Number(n)) => n.to_string(),
                Expr::Literal(LiteralValue::String(s)) => format!("\"{}\"", s),
                Expr::Literal(LiteralValue::Boolean(b)) => b.to_string(),
                Expr::Literal(LiteralValue::Null) => "null".to_string(),
                _ => format!("type \"{}\"", actual_type),
            };
            return Err(format!(
                "error: Expected type \"{}\" but got {}\n    at {}:{}:{}",
                expected_type, got_str, loc.file_path, loc.line, loc.col
            ));
        }
    }

    // 5. Literal check fallback
    if let Expr::Literal(val) = expr {
        let (actual_name, got_str) = match val {
            LiteralValue::Number(n) => ("number", n.to_string()),
            LiteralValue::String(s) => ("string", format!("\"{}\"", s)),
            LiteralValue::Boolean(b) => ("boolean", b.to_string()),
            LiteralValue::Null => ("null", "null".to_string()),
        };
        if !is_type_compatible(expected_type, actual_name, structs, interfaces) {
            return Err(format!(
                "error: Expected type \"{}\" but got {}\n    at {}:{}:{}",
                expected_type, got_str, loc.file_path, loc.line, loc.col
            ));
        }
    }

    Ok(())
}
