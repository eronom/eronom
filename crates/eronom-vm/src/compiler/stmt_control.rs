use super::{Compiler, Local, LoopContext};
use crate::frontend::ast::{Expr, Stmt, SwitchCase};
use crate::vm::bytecode::OpCode;
use crate::vm::value::Value;

impl Compiler {
    pub(crate) fn compile_while(&mut self, cond: &Expr, body: &Stmt) -> Result<(), String> {
        let loop_start = self.current_chunk().code.len();
        self.loop_stack.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            is_switch: false,
        });

        let cond_reg = self.next_reg;
        self.compile_expr(cond, cond_reg)?;
        let exit_jump = self.emit_jump(OpCode::JumpIfFalse, cond_reg);

        self.compile_stmt(body)?;

        let continue_target = self.current_chunk().code.len();
        let loop_ctx = self.loop_stack.pop().unwrap();
        for c_jump in loop_ctx.continue_jumps {
            self.patch_jump_to(c_jump, continue_target);
        }

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);

        let end_ip = self.current_chunk().code.len();
        for b_jump in loop_ctx.break_jumps {
            self.patch_jump_to(b_jump, end_ip);
        }
        Ok(())
    }

    pub(crate) fn compile_for(&mut self, var_name: &str, start: &Expr, end: &Expr, body: &Stmt) -> Result<(), String> {
        self.begin_scope();
        let var_reg = self.locals.len();
        self.compile_expr(start, var_reg)?;
        self.locals.push(Local {
            name: var_name.to_string(),
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let limit_reg = self.locals.len();
        self.compile_expr(end, limit_reg)?;
        let temp_name = format!("*loop_limit_{}", self.locals.len());
        self.locals.push(Local {
            name: temp_name,
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let one_reg = self.locals.len();
        let one_idx = self.current_chunk().add_constant(Value::number(1.0));
        self.current_chunk().write_instruction(
            OpCode::LoadConst,
            one_reg as u8,
            0,
            0,
            one_idx as u32,
        );
        let temp_one_name = format!("*loop_one_{}", self.locals.len());
        self.locals.push(Local {
            name: temp_one_name,
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let loop_start = self.current_chunk().code.len();
        self.loop_stack.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            is_switch: false,
        });

        let cond_reg = self.locals.len();
        self.current_chunk().write_instruction(
            OpCode::Less,
            cond_reg as u8,
            var_reg as u8,
            limit_reg as u8,
            0,
        );

        let exit_jump = self.emit_jump(OpCode::JumpIfFalse, cond_reg);

        self.compile_stmt(body)?;

        let continue_target = self.current_chunk().code.len();
        let loop_ctx = self.loop_stack.pop().unwrap();
        for c_jump in loop_ctx.continue_jumps {
            self.patch_jump_to(c_jump, continue_target);
        }

        self.current_chunk().write_instruction(
            OpCode::Add,
            var_reg as u8,
            var_reg as u8,
            one_reg as u8,
            0,
        );

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);

        let end_ip = self.current_chunk().code.len();
        for b_jump in loop_ctx.break_jumps {
            self.patch_jump_to(b_jump, end_ip);
        }
        self.end_scope();
        Ok(())
    }

    pub(crate) fn compile_for_in(&mut self, var_name: &str, iterable: &Expr, body: &Stmt) -> Result<(), String> {
        self.begin_scope();
        let iter_reg = self.locals.len();
        self.compile_expr(iterable, iter_reg)?;
        self.current_chunk().write_instruction(
            OpCode::ToIter,
            iter_reg as u8,
            iter_reg as u8,
            0,
            0,
        );
        let temp_iter_name = format!("*loop_iter_{}", self.locals.len());
        self.locals.push(Local {
            name: temp_iter_name,
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let len_reg = self.locals.len();
        self.current_chunk().write_instruction(
            OpCode::ArrayLen,
            len_reg as u8,
            iter_reg as u8,
            0,
            0,
        );
        let temp_len_name = format!("*loop_len_{}", self.locals.len());
        self.locals.push(Local {
            name: temp_len_name,
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let idx_reg = self.locals.len();
        let zero_idx = self.current_chunk().add_constant(Value::number(0.0));
        self.current_chunk().write_instruction(
            OpCode::LoadConst,
            idx_reg as u8,
            0,
            0,
            zero_idx as u32,
        );
        let temp_idx_name = format!("*loop_idx_{}", self.locals.len());
        self.locals.push(Local {
            name: temp_idx_name,
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let one_reg = self.locals.len();
        let one_idx = self.current_chunk().add_constant(Value::number(1.0));
        self.current_chunk().write_instruction(
            OpCode::LoadConst,
            one_reg as u8,
            0,
            0,
            one_idx as u32,
        );
        let temp_one_name = format!("*loop_one_{}", self.locals.len());
        self.locals.push(Local {
            name: temp_one_name,
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let var_reg = self.locals.len();
        self.current_chunk().write_instruction(
            OpCode::LoadNull,
            var_reg as u8,
            0,
            0,
            0,
        );
        self.locals.push(Local {
            name: var_name.to_string(),
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let loop_start = self.current_chunk().code.len();
        self.loop_stack.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            is_switch: false,
        });

        let cond_reg = self.locals.len();
        self.current_chunk().write_instruction(
            OpCode::Less,
            cond_reg as u8,
            idx_reg as u8,
            len_reg as u8,
            0,
        );

        let exit_jump = self.emit_jump(OpCode::JumpIfFalse, cond_reg);

        self.current_chunk().write_instruction(
            OpCode::GetIndex,
            var_reg as u8,
            iter_reg as u8,
            idx_reg as u8,
            0,
        );

        self.compile_stmt(body)?;

        let continue_target = self.current_chunk().code.len();
        let loop_ctx = self.loop_stack.pop().unwrap();
        for c_jump in loop_ctx.continue_jumps {
            self.patch_jump_to(c_jump, continue_target);
        }

        self.current_chunk().write_instruction(
            OpCode::Add,
            idx_reg as u8,
            idx_reg as u8,
            one_reg as u8,
            0,
        );

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);

        let end_ip = self.current_chunk().code.len();
        for b_jump in loop_ctx.break_jumps {
            self.patch_jump_to(b_jump, end_ip);
        }
        self.end_scope();
        Ok(())
    }

    pub(crate) fn compile_try(
        &mut self,
        try_body: &Stmt,
        catch_clause: &Option<(String, Box<Stmt>)>,
        finally_body: &Option<Box<Stmt>>,
    ) -> Result<(), String> {
        if catch_clause.is_none() && finally_body.is_none() {
            return Err("try statement requires a 'catch' or 'finally' block".to_string());
        }

        let try_start = self.current_chunk().code.len();
        self.compile_stmt(try_body)?;
        let try_exit_jump = self.emit_jump(OpCode::Jump, 0);
        let try_end = self.current_chunk().code.len();

        let mut catch_exit_jump = None;

        if let Some((err_var_name, catch_body)) = catch_clause {
            let catch_start = self.current_chunk().code.len();
            self.begin_scope();
            let err_reg = self.locals.len();
            self.locals.push(Local {
                name: err_var_name.clone(),
                depth: self.scope_depth,
                is_const: false,
                loc: crate::frontend::SourceLocation::default(),
                ty: None,
            });

            self.current_chunk().handlers.push(crate::vm::bytecode::ExceptionHandler {
                try_start,
                try_end,
                catch_ip: catch_start,
                err_reg: err_reg as u8,
                finally_ip: None,
            });

            self.compile_stmt(catch_body)?;
            self.end_scope();

            catch_exit_jump = Some(self.emit_jump(OpCode::Jump, 0));
        } else if let Some(finally_b) = finally_body {
            let rethrow_handler_start = self.current_chunk().code.len();
            self.begin_scope();
            let err_reg = self.locals.len();
            self.locals.push(Local {
                name: "*finally_err".to_string(),
                depth: self.scope_depth,
                is_const: false,
                loc: crate::frontend::SourceLocation::default(),
                ty: None,
            });

            self.current_chunk().handlers.push(crate::vm::bytecode::ExceptionHandler {
                try_start,
                try_end,
                catch_ip: rethrow_handler_start,
                err_reg: err_reg as u8,
                finally_ip: Some(rethrow_handler_start),
            });

            self.compile_stmt(finally_b)?;
            self.current_chunk().write_instruction(OpCode::Throw, err_reg as u8, 0, 0, 0);
            self.end_scope();
        }

        let normal_finally_start = self.current_chunk().code.len();
        self.patch_jump_to(try_exit_jump, normal_finally_start);
        if let Some(c_jump) = catch_exit_jump {
            self.patch_jump_to(c_jump, normal_finally_start);
        }

        if let Some(finally_b) = finally_body {
            self.compile_stmt(finally_b)?;
        }
        Ok(())
    }

    pub(crate) fn compile_switch(
        &mut self,
        target_expr: &Expr,
        cases: &[SwitchCase],
        default_body: &Option<Box<Stmt>>,
    ) -> Result<(), String> {
        self.begin_scope();
        let target_reg = self.locals.len();
        self.compile_expr(target_expr, target_reg)?;
        self.locals.push(Local {
            name: format!("*switch_target_{}", self.locals.len()),
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        self.loop_stack.push(LoopContext {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            is_switch: true,
        });

        let mut end_jumps = Vec::new();

        for case in cases {
            let mut match_jumps = Vec::new();
            let mut next_case_jumps = Vec::new();

            let val_count = case.values.len();
            for (i, val_expr) in case.values.iter().enumerate() {
                let val_reg = self.locals.len();
                self.compile_expr(val_expr, val_reg)?;
                let cond_reg = val_reg + 1;
                self.current_chunk().write_instruction(
                    OpCode::Equal,
                    cond_reg as u8,
                    target_reg as u8,
                    val_reg as u8,
                    0,
                );

                if i + 1 < val_count {
                    let skip_test = self.emit_jump(OpCode::JumpIfFalse, cond_reg);
                    let to_body = self.emit_jump(OpCode::Jump, 0);
                    match_jumps.push(to_body);
                    self.patch_jump(skip_test);
                } else {
                    let to_next_case = self.emit_jump(OpCode::JumpIfFalse, cond_reg);
                    next_case_jumps.push(to_next_case);
                }
            }

            let case_body_start = self.current_chunk().code.len();
            for mj in match_jumps {
                self.patch_jump_to(mj, case_body_start);
            }

            self.compile_stmt(&case.body)?;
            let to_end = self.emit_jump(OpCode::Jump, 0);
            end_jumps.push(to_end);

            let next_case_start = self.current_chunk().code.len();
            for ncj in next_case_jumps {
                self.patch_jump_to(ncj, next_case_start);
            }
        }

        if let Some(def_b) = default_body {
            self.compile_stmt(def_b)?;
        }

        let end_ip = self.current_chunk().code.len();
        for ej in end_jumps {
            self.patch_jump_to(ej, end_ip);
        }

        let loop_ctx = self.loop_stack.pop().unwrap();
        for bj in loop_ctx.break_jumps {
            self.patch_jump_to(bj, end_ip);
        }

        self.end_scope();
        Ok(())
    }
}
