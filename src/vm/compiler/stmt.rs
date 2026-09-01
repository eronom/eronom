use super::{Compiler, Local};
use super::types::check_type;
use crate::frontend::{Expr, Stmt};
use crate::vm::bytecode::OpCode;
use crate::vm::value::Value;

impl Compiler {
    pub(crate) fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        self.next_reg = self.locals.len();
        match stmt {
            Stmt::Expr(expr) => {
                if self.concurrent_scopes.last().is_some() {
                    match expr {
                        Expr::Call(_, _) => {
                            let spawn_expr = Expr::Spawn(Box::new(expr.clone()));
                            self.compile_expr(&spawn_expr, self.next_reg)?;
                        }
                        _ => {
                            self.compile_expr(expr, self.next_reg)?;
                        }
                    }
                } else {
                    self.compile_expr(expr, self.next_reg)?;
                }
            }
            Stmt::Print(expr) => {
                let callee_reg = self.next_reg;
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::string_from_str("print"));
                self.current_chunk().write_instruction(
                    OpCode::GetGlobal,
                    callee_reg as u8,
                    0,
                    0,
                    name_idx as u32,
                );
                self.compile_expr(expr, callee_reg + 1)?;
                self.current_chunk().write_instruction(
                    OpCode::Call,
                    callee_reg as u8,
                    callee_reg as u8,
                    0,
                    1,
                );
            }
            Stmt::VarDecl(name, type_annotation, is_const, expr, loc) => {
                let mut expr = expr.clone();
                if let Some(t_name) = type_annotation {
                    if self.structs.contains_key(t_name) {
                        match &expr {
                            Expr::Array(items) => {
                                if items.is_empty() {
                                    expr = Expr::Call(
                                        Box::new(Expr::Variable(t_name.clone(), loc.clone())),
                                        Vec::new(),
                                    );
                                } else {
                                    let mut new_items = Vec::new();
                                    for item in items {
                                        let call_expr = Expr::Call(
                                            Box::new(Expr::Variable(t_name.clone(), loc.clone())),
                                            vec![item.clone()],
                                        );
                                        new_items.push(call_expr);
                                    }
                                    expr = Expr::Array(new_items);
                                }
                            }
                            _ => {}
                        }
                    }
                    check_type(&expr, t_name, &self.structs, &self.interfaces, &self.locals, &self.global_types, loc)?;
                }
                if self.scope_depth > 0 {
                    let local_reg = self.locals.len();
                    self.compile_expr(&expr, local_reg)?;
                    self.locals.push(Local {
                        name: name.clone(),
                        depth: self.scope_depth,
                        is_const: *is_const,
                        loc: loc.clone(),
                        ty: type_annotation.clone(),
                    });
                } else {
                    let temp_reg = self.next_reg;
                    self.compile_expr(&expr, temp_reg)?;
                    let name_idx = self
                        .current_chunk()
                        .add_constant(Value::string_from_str(name.as_str()));
                    self.current_chunk().write_instruction(
                        OpCode::DefineGlobal,
                        temp_reg as u8,
                        0,
                        0,
                        name_idx as u32,
                    );
                    if let Some(t_name) = type_annotation {
                        self.global_types.insert(name.clone(), t_name.clone());
                    }
                    if *is_const {
                        self.const_globals.borrow_mut().insert(name.clone(), loc.clone());
                    }
                }
            }
            Stmt::Block(stmts) => {
                self.begin_scope();
                for s in stmts {
                    self.compile_stmt(s)?;
                }
                self.end_scope();
            }
            Stmt::If(cond, then_b, else_b) => {
                let cond_reg = self.next_reg;
                self.compile_expr(cond, cond_reg)?;
                let then_jump = self.emit_jump(OpCode::JumpIfFalse, cond_reg);
                self.compile_stmt(then_b)?;

                if let Some(else_b) = else_b {
                    let else_jump = self.emit_jump(OpCode::Jump, 0);
                    self.patch_jump(then_jump);
                    self.compile_stmt(else_b)?;
                    self.patch_jump(else_jump);
                } else {
                    self.patch_jump(then_jump);
                }
            }
            Stmt::While(cond, body) => {
                self.compile_while(cond, body)?;
            }
            Stmt::For(var_name, start, end, body) => {
                self.compile_for(var_name, start, end, body)?;
            }
            Stmt::ForIn(var_name, iterable, body) => {
                self.compile_for_in(var_name, iterable, body)?;
            }
            Stmt::Break => {
                if self.loop_stack.is_empty() {
                    return Err("Cannot use 'break' outside of a loop or switch statement".to_string());
                }
                let jmp = self.emit_jump(OpCode::Jump, 0);
                self.loop_stack.last_mut().unwrap().break_jumps.push(jmp);
            }
            Stmt::Continue => {
                let target_idx = self.loop_stack.iter().rposition(|c| !c.is_switch);
                if let Some(idx) = target_idx {
                    let jmp = self.emit_jump(OpCode::Jump, 0);
                    self.loop_stack[idx].continue_jumps.push(jmp);
                } else {
                    return Err("Cannot use 'continue' outside of a loop".to_string());
                }
            }
            Stmt::Throw(expr) => {
                let reg = self.next_reg;
                self.compile_expr(expr, reg)?;
                self.current_chunk().write_instruction(OpCode::Throw, reg as u8, 0, 0, 0);
            }
            Stmt::Try(try_body, catch_clause, finally_body) => {
                self.compile_try(try_body, catch_clause, finally_body)?;
            }
            Stmt::Switch(target_expr, cases, default_body) => {
                self.compile_switch(target_expr, cases, default_body)?;
            }
            Stmt::Return(expr, loc) => {
                let reg = self.next_reg;
                if let Some(e) = expr {
                    if let Some(ref ret_ty) = self.current_return_type {
                        check_type(e, ret_ty, &self.structs, &self.interfaces, &self.locals, &self.global_types, loc)?;
                    }
                    self.compile_expr(e, reg)?;
                } else {
                    if let Some(ref ret_ty) = self.current_return_type {
                        if ret_ty != "void" && ret_ty != "null" {
                            return Err(format!("error: Expected return type \"{}\" but got void/null\n    at {}:{}:{}", ret_ty, loc.file_path, loc.line, loc.col));
                        }
                    }
                    self.current_chunk().write_instruction(OpCode::LoadNull, reg as u8, 0, 0, 0);
                }
                self.current_chunk().write_instruction(OpCode::Return, reg as u8, 0, 0, 0);
            }
            Stmt::Import(_, _) => {
                return Err("Import statement should be resolved before compilation".to_string());
            }
            Stmt::Export(inner) => {
                self.compile_stmt(inner)?;
            }
            Stmt::Struct(name, composed, fields, methods, _) => {
                self.compile_struct_decl(name, composed, fields, methods)?;
            }
            Stmt::Interface(_, _, _, _) => {}
            Stmt::Concurrent(body) => {
                self.begin_scope();
                let handles_reg = self.locals.len();
                self.locals.push(Local {
                    name: format!("*concurrent_handles_{}", self.locals.len()),
                    depth: self.scope_depth,
                    is_const: false,
                    loc: crate::frontend::SourceLocation::default(),
                    ty: None,
                });
                self.current_chunk().write_instruction(
                    OpCode::MakeArray,
                    handles_reg as u8,
                    handles_reg as u8,
                    0,
                    0,
                );
                self.concurrent_scopes.push(handles_reg);
                self.compile_stmt(body)?;
                self.concurrent_scopes.pop();

                self.compile_await_handles(handles_reg)?;
                self.end_scope();
            }
        }
        Ok(())
    }
}
