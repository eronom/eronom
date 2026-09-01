use super::{Compiler, Local};
use super::types::check_type;
use crate::frontend::{Expr, LiteralValue, TokenType};
use crate::vm::bytecode::OpCode;
use crate::vm::gc::{gc_allocate, GcData};
use crate::vm::value::Value;

impl Compiler {
    pub(crate) fn compile_expr(&mut self, expr: &Expr, dest: usize) -> Result<(), String> {
        match expr {
            Expr::Literal(val) => {
                match val {
                    LiteralValue::Null => {
                        self.current_chunk().write_instruction(OpCode::LoadNull, dest as u8, 0, 0, 0);
                    }
                    LiteralValue::Boolean(b) => {
                        self.current_chunk().write_instruction(
                            OpCode::LoadBool,
                            dest as u8,
                            *b as u8,
                            0,
                            0,
                        );
                    }
                    LiteralValue::Number(n) => {
                        let idx = self.current_chunk().add_constant(Value::number(*n));
                        self.current_chunk().write_instruction(
                            OpCode::LoadConst,
                            dest as u8,
                            0,
                            0,
                            idx as u32,
                        );
                    }
                    LiteralValue::String(s) => {
                        let idx = self
                            .current_chunk()
                            .add_constant(Value::string_from_str(s.as_str()));
                        self.current_chunk().write_instruction(
                            OpCode::LoadConst,
                            dest as u8,
                            0,
                            0,
                            idx as u32,
                        );
                    }
                }
            }
            Expr::Variable(name, _) => {
                if let Some(idx) = self.resolve_local(name) {
                    if dest != idx {
                        self.current_chunk().write_instruction(
                            OpCode::Move,
                            dest as u8,
                            idx as u8,
                            0,
                            0,
                        );
                    }
                } else if let Some(upval_idx) = self.resolve_upvalue(name) {
                    self.current_chunk().write_instruction(
                        OpCode::GetUpvalue,
                        dest as u8,
                        0,
                        0,
                        upval_idx as u32,
                    );
                } else {
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::string_from_str(name.as_str()));
                    self.current_chunk().write_instruction(
                        OpCode::GetGlobal,
                        dest as u8,
                        0,
                        0,
                        idx as u32,
                    );
                }
            }
            Expr::Assign(name, val, assign_loc) => {
                if let Some(idx) = self.resolve_local(name) {
                    if self.locals[idx].is_const {
                        return Err(self.format_const_assign_error(name, assign_loc, &self.locals[idx].loc));
                    }
                    if let Some(ref ty) = self.locals[idx].ty.clone() {
                        check_type(val, ty, &self.structs, &self.interfaces, &self.locals, &self.global_types, assign_loc)?;
                    }
                    self.compile_expr(val, dest)?;
                    if dest != idx {
                        self.current_chunk().write_instruction(
                            OpCode::Move,
                            idx as u8,
                            dest as u8,
                            0,
                            0,
                        );
                    }
                } else if let Some(upval_idx) = self.resolve_upvalue(name) {
                    let (_, is_const, ref decl_loc, ref ty_opt) = self.upvalue_names[upval_idx];
                    if is_const {
                        return Err(self.format_const_assign_error(name, assign_loc, decl_loc));
                    }
                    if let Some(ref ty) = ty_opt.clone() {
                        check_type(val, ty, &self.structs, &self.interfaces, &self.locals, &self.global_types, assign_loc)?;
                    }
                    let temp = std::cmp::max(self.next_reg, dest);
                    self.compile_expr(val, temp)?;
                    self.current_chunk().write_instruction(
                        OpCode::SetUpvalue,
                        temp as u8,
                        0,
                        0,
                        upval_idx as u32,
                    );
                    if dest != temp {
                        self.current_chunk().write_instruction(
                            OpCode::Move,
                            dest as u8,
                            temp as u8,
                            0,
                            0,
                        );
                    }
                } else {
                    if let Some(decl_loc) = self.const_globals.borrow().get(name).cloned() {
                        return Err(self.format_const_assign_error(name, assign_loc, &decl_loc));
                    }
                    if let Some(ty) = self.global_types.get(name).cloned() {
                        check_type(val, &ty, &self.structs, &self.interfaces, &self.locals, &self.global_types, assign_loc)?;
                    }
                    self.compile_expr(val, dest)?;
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::string_from_str(name.as_str()));
                    self.current_chunk().write_instruction(
                        OpCode::SetGlobal,
                        dest as u8,
                        0,
                        0,
                        idx as u32,
                    );
                }
            }
            Expr::Unary(op, inner) => {
                self.compile_expr(inner, dest)?;
                match op {
                    TokenType::Bang => {
                        self.current_chunk().write_instruction(
                            OpCode::Not,
                            dest as u8,
                            dest as u8,
                            0,
                            0,
                        );
                    }
                    TokenType::Minus => {
                        self.current_chunk().write_instruction(
                            OpCode::Negate,
                            dest as u8,
                            dest as u8,
                            0,
                            0,
                        );
                    }
                    TokenType::Tilde => {
                        self.current_chunk().write_instruction(
                            OpCode::BitNot,
                            dest as u8,
                            dest as u8,
                            0,
                            0,
                        );
                    }
                    TokenType::Typeof => {
                        self.current_chunk().write_instruction(
                            OpCode::TypeOf,
                            dest as u8,
                            dest as u8,
                            0,
                            0,
                        );
                    }
                    _ => return Err(format!("Unsupported unary operator: {:?}", op)),
                }
            }
            Expr::Prefix(op, inner) => {
                self.compile_prefix(op, inner, dest)?;
            }
            Expr::Postfix(op, inner) => {
                self.compile_postfix(op, inner, dest)?;
            }
            Expr::Ternary(cond, then_b, else_b) => {
                self.compile_expr(cond, dest)?;
                let else_jump = self.emit_jump(OpCode::JumpIfFalse, dest);
                self.compile_expr(then_b, dest)?;
                let end_jump = self.emit_jump(OpCode::Jump, 0);
                self.patch_jump(else_jump);
                self.compile_expr(else_b, dest)?;
                self.patch_jump(end_jump);
            }
            Expr::Binary(left, op, right) => {
                self.compile_expr(left, dest)?;
                let temp = std::cmp::max(self.next_reg, dest + 1);
                self.compile_expr(right, temp)?;
                match op {
                    TokenType::Plus => {
                        self.current_chunk().write_instruction(OpCode::Add, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::Minus => {
                        self.current_chunk().write_instruction(OpCode::Sub, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::Star => {
                        self.current_chunk().write_instruction(OpCode::Mul, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::Slash => {
                        self.current_chunk().write_instruction(OpCode::Div, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::Percent => {
                        self.current_chunk().write_instruction(OpCode::Mod, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::Ampersand => {
                        self.current_chunk().write_instruction(OpCode::BitAnd, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::Pipe => {
                        self.current_chunk().write_instruction(OpCode::BitOr, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::Caret => {
                        self.current_chunk().write_instruction(OpCode::BitXor, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::LessLess => {
                        self.current_chunk().write_instruction(OpCode::ShiftLeft, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::GreaterGreater => {
                        self.current_chunk().write_instruction(OpCode::ShiftRight, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::EqualEqual => {
                        self.current_chunk().write_instruction(OpCode::Equal, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::BangEqual => {
                        self.current_chunk().write_instruction(OpCode::Equal, dest as u8, dest as u8, temp as u8, 0);
                        self.current_chunk().write_instruction(OpCode::Not, dest as u8, dest as u8, 0, 0);
                    }
                    TokenType::Greater => {
                        self.current_chunk().write_instruction(OpCode::Greater, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::GreaterEqual => {
                        self.current_chunk().write_instruction(OpCode::Less, dest as u8, dest as u8, temp as u8, 0);
                        self.current_chunk().write_instruction(OpCode::Not, dest as u8, dest as u8, 0, 0);
                    }
                    TokenType::Less => {
                        self.current_chunk().write_instruction(OpCode::Less, dest as u8, dest as u8, temp as u8, 0);
                    }
                    TokenType::LessEqual => {
                        self.current_chunk().write_instruction(OpCode::Greater, dest as u8, dest as u8, temp as u8, 0);
                        self.current_chunk().write_instruction(OpCode::Not, dest as u8, dest as u8, 0, 0);
                    }
                    _ => return Err(format!("Invalid binary operator: {:?}", op)),
                }
            }
            Expr::Logical(left, op, right) => {
                if op == &TokenType::Or {
                    self.compile_expr(left, dest)?;
                    let jump_to_right = self.emit_jump(OpCode::JumpIfFalse, dest);
                    let jump_to_end = self.emit_jump(OpCode::Jump, 0);
                    self.patch_jump(jump_to_right);
                    self.compile_expr(right, dest)?;
                    self.patch_jump(jump_to_end);
                } else if op == &TokenType::And {
                    self.compile_expr(left, dest)?;
                    let jump_to_end = self.emit_jump(OpCode::JumpIfFalse, dest);
                    self.compile_expr(right, dest)?;
                    self.patch_jump(jump_to_end);
                }
            }
            Expr::Call(callee, args) => {
                self.compile_call(callee, args, dest)?;
            }
            Expr::Get(obj, name) => {
                self.compile_expr(obj, dest)?;
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::string_from_str(name.as_str()));
                self.current_chunk().write_instruction(
                    OpCode::GetProperty,
                    dest as u8,
                    dest as u8,
                    0,
                    name_idx as u32,
                );
            }
            Expr::Set(obj, name, val) => {
                self.compile_expr(obj, dest)?;
                let temp = std::cmp::max(self.next_reg, dest + 1);
                self.compile_expr(val, temp)?;
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::string_from_str(name.as_str()));
                self.current_chunk().write_instruction(
                    OpCode::SetProperty,
                    dest as u8,
                    temp as u8,
                    0,
                    name_idx as u32,
                );
                if dest != temp {
                    self.current_chunk().write_instruction(
                        OpCode::Move,
                        dest as u8,
                        temp as u8,
                        0,
                        0,
                    );
                }
            }
            Expr::Array(items) => {
                let start_reg = std::cmp::max(self.next_reg, dest);
                for (i, item) in items.iter().enumerate() {
                    self.compile_expr(item, start_reg + i)?;
                }
                self.current_chunk().write_instruction(
                    OpCode::MakeArray,
                    dest as u8,
                    start_reg as u8,
                    0,
                    items.len() as u32,
                );
            }
            Expr::Object(pairs) | Expr::StructInst(_, pairs, _) => {
                let start_reg = std::cmp::max(self.next_reg, dest);
                for (i, (key, val)) in pairs.iter().enumerate() {
                    let k_idx = self
                        .current_chunk()
                        .add_constant(Value::string_from_str(key.as_str()));
                    self.current_chunk().write_instruction(
                        OpCode::LoadConst,
                        (start_reg + i * 2) as u8,
                        0,
                        0,
                        k_idx as u32,
                    );
                    self.compile_expr(val, start_reg + i * 2 + 1)?;
                }
                self.current_chunk().write_instruction(
                    OpCode::MakeObject,
                    dest as u8,
                    start_reg as u8,
                    0,
                    pairs.len() as u32,
                );
            }
            Expr::Function(params, return_type, body) => {
                let mut compiler = Compiler::new();
                compiler.parent = Some(self as *mut Compiler);
                compiler.const_globals = self.const_globals.clone();
                compiler.structs = self.structs.clone();
                compiler.interfaces = self.interfaces.clone();
                compiler.global_types = self.global_types.clone();
                compiler.current_struct = self.current_struct.clone();
                compiler.current_return_type = return_type.clone();
                compiler.function.arity = params.len();
                compiler.function.is_async = false;
                compiler.next_reg = params.len();
                compiler.begin_scope();
                for param in params {
                    compiler.locals.push(Local {
                        name: param.name.clone(),
                        depth: compiler.scope_depth,
                        is_const: false,
                        loc: crate::frontend::SourceLocation::default(),
                        ty: param.ty.clone(),
                    });
                }
                compiler.compile_stmt(body)?;
                compiler.current_chunk().write_instruction(OpCode::LoadNull, 0, 0, 0, 0);
                compiler.current_chunk().write_instruction(OpCode::Return, 0, 0, 0, 0);

                let mut func = compiler.function;
                func.upvalues = compiler.upvalues;
                let func_ptr = gc_allocate(GcData::Function(Box::new(func)));
                let idx = self
                    .current_chunk()
                    .add_constant(Value::function(func_ptr));
                self.current_chunk().write_instruction(
                    OpCode::Closure,
                    dest as u8,
                    0,
                    0,
                    idx as u32,
                );
            }
            Expr::GetIndex(obj, index) => {
                self.compile_expr(obj, dest)?;
                let temp = std::cmp::max(self.next_reg, dest + 1);
                self.compile_expr(index, temp)?;
                self.current_chunk().write_instruction(
                    OpCode::GetIndex,
                    dest as u8,
                    dest as u8,
                    temp as u8,
                    0,
                );
            }
            Expr::SetIndex(obj, index, val) => {
                self.compile_expr(obj, dest)?;
                let temp1 = std::cmp::max(self.next_reg, dest + 1);
                self.compile_expr(index, temp1)?;
                let temp2 = std::cmp::max(self.next_reg, temp1 + 1);
                self.compile_expr(val, temp2)?;
                self.current_chunk().write_instruction(
                    OpCode::SetIndex,
                    dest as u8,
                    temp1 as u8,
                    temp2 as u8,
                    0,
                );
                if dest != temp2 {
                    self.current_chunk().write_instruction(
                        OpCode::Move,
                        dest as u8,
                        temp2 as u8,
                        0,
                        0,
                    );
                }
            }
            Expr::Spawn(inner) => {
                let (callee, args) = match &**inner {
                    Expr::Call(callee, args) => (callee.as_ref().clone(), args.clone()),
                    _ => return Err("spawn expects a function call expression".to_string()),
                };

                let spawn_call = Expr::Call(
                    Box::new(Expr::Variable("spawnTask".to_string(), crate::frontend::SourceLocation::default())),
                    vec![callee, Expr::Array(args)],
                );

                self.compile_expr(&spawn_call, dest)?;

                if let Some(&handles_reg) = self.concurrent_scopes.last() {
                    let push_fn_reg = std::cmp::max(self.next_reg, dest + 1);
                    let push_name_idx = self
                        .current_chunk()
                        .add_constant(Value::string_from_str("arrayPush"));
                    self.current_chunk().write_instruction(
                        OpCode::GetGlobal,
                        push_fn_reg as u8,
                        0,
                        0,
                        push_name_idx as u32,
                    );
                    self.current_chunk().write_instruction(
                        OpCode::Move,
                        (push_fn_reg + 1) as u8,
                        handles_reg as u8,
                        0,
                        0,
                    );
                    self.current_chunk().write_instruction(
                        OpCode::Move,
                        (push_fn_reg + 2) as u8,
                        dest as u8,
                        0,
                        0,
                    );
                    self.current_chunk().write_instruction(
                        OpCode::Call,
                        push_fn_reg as u8,
                        push_fn_reg as u8,
                        0,
                        2,
                    );
                }
            }
        }
        Ok(())
    }
}
