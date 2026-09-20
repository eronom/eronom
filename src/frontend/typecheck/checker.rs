use crate::frontend::ast::{
    Expr, LiteralValue, PrimitiveType, PropertySignature, SourceLocation, Stmt, TypeNode,
};
use crate::frontend::token::TokenType;
use super::assignability::is_assignable;
use super::env::TypeEnv;
use super::TypeError;

pub struct TypeChecker {
    pub env: TypeEnv,
    pub errors: Vec<TypeError>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            env: TypeEnv::new(),
            errors: Vec::new(),
        }
    }

    pub fn check_program(&mut self, stmts: &[Stmt]) {
        // Pass 1: Hoist type aliases, structs, interfaces, and enums
        for stmt in stmts {
            match stmt {
                Stmt::TypeAlias(name, params, ty, _) => {
                    self.env.define_type_alias(name.clone(), params.clone(), ty.clone());
                }
                Stmt::Struct(name, _, fields, _, _) => {
                    let parsed_fields = fields
                        .iter()
                        .map(|(n, t_str)| (n.clone(), TypeNode::from_ident(t_str)))
                        .collect();
                    self.env.define_struct(name.clone(), parsed_fields);
                }
                Stmt::Interface(name, fields, _, _) => {
                    let parsed_fields = fields
                        .iter()
                        .map(|(n, t_str)| (n.clone(), TypeNode::from_ident(t_str)))
                        .collect();
                    self.env.define_interface(name.clone(), parsed_fields);
                }
                Stmt::Enum(name, variants, _) => {
                    let props = variants
                        .iter()
                        .map(|(v_name, _)| PropertySignature {
                            name: v_name.clone(),
                            ty: TypeNode::Primitive(PrimitiveType::Number),
                            optional: false,
                        })
                        .collect();
                    self.env.define_var(name.clone(), TypeNode::Object(props));
                }
                _ => {}
            }
        }

        // Pass 2: Check all statements sequentially
        for stmt in stmts {
            self.check_stmt(stmt);
        }
    }

    pub fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl(name, type_ann, _is_const, init, loc) => {
                let init_ty = self.infer_expr(init);

                if let Some(expected_ty) = type_ann {
                    if !is_assignable(expected_ty, &init_ty, &self.env) {
                        self.errors.push(TypeError {
                            message: format!(
                                "Type mismatch in variable declaration '{}': expected '{}', found '{}'",
                                name, expected_ty, init_ty
                            ),
                            loc: loc.clone(),
                        });
                    }
                    self.env.define_var(name.clone(), expected_ty.clone());
                } else {
                    self.env.define_var(name.clone(), init_ty);
                }
            }
            Stmt::Expr(expr) => {
                self.infer_expr(expr);
            }
            Stmt::Print(expr) => {
                self.infer_expr(expr);
            }
            Stmt::Block(stmts) => {
                self.env.push_scope();
                for s in stmts {
                    self.check_stmt(s);
                }
                self.env.pop_scope();
            }
            Stmt::If(cond, then_branch, else_branch) => {
                let _cond_ty = self.infer_expr(cond);

                // Type narrowing for `then` branch
                self.env.push_scope();
                self.apply_condition_narrowing(cond, true);
                self.check_stmt(then_branch);
                self.env.pop_scope();

                // Type narrowing for `else` branch
                if let Some(else_b) = else_branch {
                    self.env.push_scope();
                    self.apply_condition_narrowing(cond, false);
                    self.check_stmt(else_b);
                    self.env.pop_scope();
                }
            }
            Stmt::While(cond, body) => {
                let _cond_ty = self.infer_expr(cond);
                self.env.push_scope();
                self.apply_condition_narrowing(cond, true);
                self.check_stmt(body);
                self.env.pop_scope();
            }
            Stmt::For(var_name, start, end, body) => {
                let start_ty = self.infer_expr(start);
                let end_ty = self.infer_expr(end);

                let int_ty = TypeNode::Primitive(PrimitiveType::Int);
                if !is_assignable(&int_ty, &start_ty, &self.env) || !is_assignable(&int_ty, &end_ty, &self.env) {
                    self.errors.push(TypeError {
                        message: "Range loop boundaries must evaluate to integers".to_string(),
                        loc: SourceLocation::default(),
                    });
                }

                self.env.push_scope();
                self.env.define_var(var_name.clone(), int_ty);
                self.check_stmt(body);
                self.env.pop_scope();
            }
            Stmt::ForIn(var_name, iter_expr, body) => {
                let iter_ty = self.infer_expr(iter_expr);
                let item_ty = match self.env.resolve_type(&iter_ty) {
                    TypeNode::Array(elem) => *elem,
                    _ => TypeNode::Primitive(PrimitiveType::Any),
                };

                self.env.push_scope();
                self.env.define_var(var_name.clone(), item_ty);
                self.check_stmt(body);
                self.env.pop_scope();
            }
            Stmt::Return(expr_opt, loc) => {
                let ret_ty = if let Some(e) = expr_opt {
                    self.infer_expr(e)
                } else {
                    TypeNode::Primitive(PrimitiveType::Void)
                };

                if let Some(ref expected_ret) = self.env.current_return_type {
                    if !is_assignable(expected_ret, &ret_ty, &self.env) {
                        self.errors.push(TypeError {
                            message: format!(
                                "Return type mismatch: expected '{}', found '{}'",
                                expected_ret, ret_ty
                            ),
                            loc: loc.clone(),
                        });
                    }
                }
            }
            Stmt::Try(try_body, catch_clause, finally_body) => {
                self.check_stmt(try_body);
                if let Some((param, catch_body)) = catch_clause {
                    self.env.push_scope();
                    self.env.define_var(param.clone(), TypeNode::Primitive(PrimitiveType::Any));
                    self.check_stmt(catch_body);
                    self.env.pop_scope();
                }
                if let Some(finally_b) = finally_body {
                    self.check_stmt(finally_b);
                }
            }
            Stmt::Throw(expr) => {
                self.infer_expr(expr);
            }
            Stmt::Export(inner) => {
                self.check_stmt(inner);
            }
            Stmt::Concurrent(body) => {
                self.check_stmt(body);
            }
            _ => {}
        }
    }

    pub fn infer_expr(&mut self, expr: &Expr) -> TypeNode {
        match expr {
            Expr::Literal(val) => match val {
                LiteralValue::Number(n) => {
                    if n.fract() == 0.0 {
                        TypeNode::Primitive(PrimitiveType::Int)
                    } else {
                        TypeNode::Primitive(PrimitiveType::Float)
                    }
                }
                LiteralValue::String(_) => TypeNode::Primitive(PrimitiveType::String),
                LiteralValue::Boolean(_) => TypeNode::Primitive(PrimitiveType::Bool),
                LiteralValue::Null => TypeNode::Primitive(PrimitiveType::Null),
            },
            Expr::Variable(name, loc) => {
                if let Some(ty) = self.env.get_var(name) {
                    ty.clone()
                } else {
                    self.errors.push(TypeError {
                        message: format!("Cannot find name '{}' in current scope", name),
                        loc: loc.clone(),
                    });
                    TypeNode::Primitive(PrimitiveType::Any)
                }
            }
            Expr::Assign(name, val, loc) => {
                let val_ty = self.infer_expr(val);
                if let Some(var_ty) = self.env.get_var(name).cloned() {
                    if !is_assignable(&var_ty, &val_ty, &self.env) {
                        self.errors.push(TypeError {
                            message: format!(
                                "Cannot assign type '{}' to variable '{}' of type '{}'",
                                val_ty, name, var_ty
                            ),
                            loc: loc.clone(),
                        });
                    }
                } else {
                    self.errors.push(TypeError {
                        message: format!("Cannot assign to undeclared variable '{}'", name),
                        loc: loc.clone(),
                    });
                }
                val_ty
            }
            Expr::Binary(left, op, right) => {
                let left_ty = self.infer_expr(left);
                let right_ty = self.infer_expr(right);

                match op {
                    TokenType::Plus => {
                        if matches!(left_ty, TypeNode::Primitive(PrimitiveType::String))
                            || matches!(right_ty, TypeNode::Primitive(PrimitiveType::String))
                        {
                            TypeNode::Primitive(PrimitiveType::String)
                        } else if matches!(left_ty, TypeNode::Primitive(PrimitiveType::Float))
                            || matches!(right_ty, TypeNode::Primitive(PrimitiveType::Float))
                        {
                            TypeNode::Primitive(PrimitiveType::Float)
                        } else {
                            TypeNode::Primitive(PrimitiveType::Int)
                        }
                    }
                    TokenType::Minus | TokenType::Star | TokenType::Slash | TokenType::Percent => {
                        if matches!(left_ty, TypeNode::Primitive(PrimitiveType::Float))
                            || matches!(right_ty, TypeNode::Primitive(PrimitiveType::Float))
                        {
                            TypeNode::Primitive(PrimitiveType::Float)
                        } else {
                            TypeNode::Primitive(PrimitiveType::Int)
                        }
                    }
                    TokenType::EqualEqual
                    | TokenType::BangEqual
                    | TokenType::Less
                    | TokenType::LessEqual
                    | TokenType::Greater
                    | TokenType::GreaterEqual => TypeNode::Primitive(PrimitiveType::Bool),
                    TokenType::Ampersand
                    | TokenType::Pipe
                    | TokenType::Caret
                    | TokenType::LessLess
                    | TokenType::GreaterGreater => TypeNode::Primitive(PrimitiveType::Int),
                    _ => TypeNode::Primitive(PrimitiveType::Any),
                }
            }
            Expr::Logical(left, _, right) => {
                self.infer_expr(left);
                self.infer_expr(right);
                TypeNode::Primitive(PrimitiveType::Bool)
            }
            Expr::Unary(op, operand) => {
                let op_ty = self.infer_expr(operand);
                match op {
                    TokenType::Bang => TypeNode::Primitive(PrimitiveType::Bool),
                    TokenType::Minus => op_ty,
                    TokenType::Typeof => TypeNode::Primitive(PrimitiveType::String),
                    _ => op_ty,
                }
            }
            Expr::Prefix(_, operand) | Expr::Postfix(_, operand) => {
                self.infer_expr(operand);
                TypeNode::Primitive(PrimitiveType::Int)
            }
            Expr::Ternary(_, then_branch, else_branch) => {
                let then_ty = self.infer_expr(then_branch);
                let else_ty = self.infer_expr(else_branch);
                if then_ty == else_ty {
                    then_ty
                } else {
                    TypeNode::Union(vec![then_ty, else_ty])
                }
            }
            Expr::Array(items) => {
                if items.is_empty() {
                    TypeNode::Array(Box::new(TypeNode::Primitive(PrimitiveType::Any)))
                } else {
                    let first_ty = self.infer_expr(&items[0]);
                    for item in items.iter().skip(1) {
                        let item_ty = self.infer_expr(item);
                        if !is_assignable(&first_ty, &item_ty, &self.env) {
                            // Heterogeneous array becomes union
                            return TypeNode::Array(Box::new(TypeNode::Primitive(PrimitiveType::Any)));
                        }
                    }
                    TypeNode::Array(Box::new(first_ty))
                }
            }
            Expr::Object(pairs) => {
                let mut props = Vec::new();
                for (k, v) in pairs {
                    let v_ty = self.infer_expr(v);
                    props.push(PropertySignature {
                        name: k.clone(),
                        ty: v_ty,
                        optional: false,
                    });
                }
                TypeNode::Object(props)
            }
            Expr::Function(params, return_type, body) => {
                self.env.push_scope();
                let prev_ret = self.env.current_return_type.take();
                self.env.current_return_type = return_type.clone();

                let mut param_types = Vec::new();
                for p in params {
                    let ty = p.ty.clone().unwrap_or(TypeNode::Primitive(PrimitiveType::Any));
                    self.env.define_var(p.name.clone(), ty.clone());
                    param_types.push(ty);
                }

                self.check_stmt(body);

                self.env.current_return_type = prev_ret;
                self.env.pop_scope();

                TypeNode::Function {
                    params: param_types,
                    return_type: Box::new(
                        return_type
                            .clone()
                            .unwrap_or(TypeNode::Primitive(PrimitiveType::Any)),
                    ),
                }
            }
            Expr::Call(callee, args) => {
                let callee_ty = self.infer_expr(callee);
                let arg_types: Vec<TypeNode> = args.iter().map(|a| self.infer_expr(a)).collect();

                match self.env.resolve_type(&callee_ty) {
                    TypeNode::Function {
                        params,
                        return_type,
                    } => {
                        for (idx, (param_ty, arg_ty)) in params.iter().zip(arg_types.iter()).enumerate() {
                            if !is_assignable(param_ty, arg_ty, &self.env) {
                                self.errors.push(TypeError {
                                    message: format!(
                                        "Argument {} type mismatch: expected '{}', got '{}'",
                                        idx + 1,
                                        param_ty,
                                        arg_ty
                                    ),
                                    loc: SourceLocation::default(),
                                });
                            }
                        }
                        *return_type
                    }
                    _ => TypeNode::Primitive(PrimitiveType::Any),
                }
            }
            Expr::Get(obj, prop) => {
                let obj_ty = self.infer_expr(obj);
                match self.env.resolve_type(&obj_ty) {
                    TypeNode::Object(props) => {
                        for p in props {
                            if &p.name == prop {
                                return p.ty;
                            }
                        }
                        TypeNode::Primitive(PrimitiveType::Any)
                    }
                    TypeNode::Named(ref struct_name, _) => {
                        if let Some(fields) = self.env.get_struct(struct_name) {
                            for (f_name, f_ty) in fields {
                                if f_name == prop {
                                    return f_ty.clone();
                                }
                            }
                        }
                        TypeNode::Primitive(PrimitiveType::Any)
                    }
                    _ => TypeNode::Primitive(PrimitiveType::Any),
                }
            }
            Expr::Set(obj, prop, val) => {
                let obj_ty = self.infer_expr(obj);
                let val_ty = self.infer_expr(val);

                if let TypeNode::Object(props) = self.env.resolve_type(&obj_ty) {
                    for p in props {
                        if &p.name == prop && !is_assignable(&p.ty, &val_ty, &self.env) {
                            self.errors.push(TypeError {
                                message: format!(
                                    "Cannot assign type '{}' to property '{}' of type '{}'",
                                    val_ty, prop, p.ty
                                ),
                                loc: SourceLocation::default(),
                            });
                        }
                    }
                }
                val_ty
            }
            Expr::GetIndex(obj, _) => {
                let obj_ty = self.infer_expr(obj);
                match self.env.resolve_type(&obj_ty) {
                    TypeNode::Array(elem) => *elem,
                    _ => TypeNode::Primitive(PrimitiveType::Any),
                }
            }
            Expr::SetIndex(_, _, val) => self.infer_expr(val),
            Expr::TypeCast(_, target_type, _) => target_type.clone(),
            Expr::StructInst(name, pairs, loc) => {
                if let Some(fields) = self.env.get_struct(name).cloned() {
                    let field_map: std::collections::HashMap<&str, &TypeNode> =
                        fields.iter().map(|(n, t)| (n.as_str(), t)).collect();

                    for (k, v) in pairs {
                        let v_ty = self.infer_expr(v);
                        if let Some(expected_f_ty) = field_map.get(k.as_str()) {
                            if !is_assignable(expected_f_ty, &v_ty, &self.env) {
                                self.errors.push(TypeError {
                                    message: format!(
                                        "Field '{}' of struct '{}' expects '{}', got '{}'",
                                        k, name, expected_f_ty, v_ty
                                    ),
                                    loc: loc.clone(),
                                });
                            }
                        } else {
                            self.errors.push(TypeError {
                                message: format!("Unknown field '{}' for struct '{}'", k, name),
                                loc: loc.clone(),
                            });
                        }
                    }
                }
                TypeNode::Named(name.clone(), Vec::new())
            }
            Expr::Spawn(inner) => self.infer_expr(inner),
        }
    }

    fn apply_condition_narrowing(&mut self, cond: &Expr, is_truthy: bool) {
        match cond {
            // typeof x == "string"
            Expr::Binary(left, op, right) if *op == TokenType::EqualEqual => {
                if let (Expr::Unary(TokenType::Typeof, target), Expr::Literal(LiteralValue::String(type_str))) =
                    (&**left, &**right)
                {
                    if let Expr::Variable(name, _) = &**target {
                        if is_truthy {
                            let narrowed_ty = match type_str.as_str() {
                                "string" => TypeNode::Primitive(PrimitiveType::String),
                                "number" | "int" => TypeNode::Primitive(PrimitiveType::Number),
                                "boolean" => TypeNode::Primitive(PrimitiveType::Bool),
                                other => TypeNode::Named(other.to_string(), Vec::new()),
                            };
                            self.env.set_var(name, narrowed_ty);
                        }
                    }
                } else if let (Expr::Variable(name, _), Expr::Literal(LiteralValue::Null)) = (&**left, &**right) {
                    // x == null
                    if !is_truthy {
                        self.narrow_non_null(name);
                    }
                }
            }
            // x != null
            Expr::Binary(left, op, right) if *op == TokenType::BangEqual => {
                if let (Expr::Variable(name, _), Expr::Literal(LiteralValue::Null)) = (&**left, &**right) {
                    if is_truthy {
                        self.narrow_non_null(name);
                    }
                }
            }
            _ => {}
        }
    }

    fn narrow_non_null(&mut self, name: &str) {
        if let Some(current_ty) = self.env.get_var(name).cloned() {
            let resolved = self.env.resolve_type(&current_ty);
            if let TypeNode::Union(members) = resolved {
                let non_null_members: Vec<TypeNode> = members
                    .into_iter()
                    .filter(|m| !matches!(m, TypeNode::Primitive(PrimitiveType::Null)))
                    .collect();

                if non_null_members.len() == 1 {
                    self.env.set_var(name, non_null_members[0].clone());
                } else if !non_null_members.is_empty() {
                    self.env.set_var(name, TypeNode::Union(non_null_members));
                }
            }
        }
    }
}
