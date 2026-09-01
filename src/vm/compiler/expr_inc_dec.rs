use super::Compiler;
use crate::frontend::{Expr, TokenType};
use crate::vm::bytecode::OpCode;
use crate::vm::value::Value;

impl Compiler {
    pub(crate) fn compile_prefix(&mut self, op: &TokenType, inner: &Expr, dest: usize) -> Result<(), String> {
        let is_plus = match op {
            TokenType::PlusPlus => true,
            TokenType::MinusMinus => false,
            _ => return Err("Invalid prefix operator".into()),
        };
        let update_op = if is_plus { OpCode::Add } else { OpCode::Sub };

        let one_reg = std::cmp::max(self.next_reg, dest + 1);
        let one_idx = self.current_chunk().add_constant(Value::number(1.0));
        self.current_chunk().write_instruction(
            OpCode::LoadConst,
            one_reg as u8,
            0,
            0,
            one_idx as u32,
        );

        match inner {
            Expr::Variable(name, loc) => {
                if let Some(idx) = self.resolve_local(name) {
                    if self.locals[idx].is_const {
                        return Err(self.format_const_assign_error(name, loc, &self.locals[idx].loc));
                    }
                    self.current_chunk().write_instruction(
                        update_op,
                        idx as u8,
                        idx as u8,
                        one_reg as u8,
                        0,
                    );
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
                    let (_, is_const, ref decl_loc, _) = self.upvalue_names[upval_idx];
                    if is_const {
                        return Err(self.format_const_assign_error(name, loc, decl_loc));
                    }
                    let temp = std::cmp::max(self.next_reg, dest + 1);
                    self.current_chunk().write_instruction(
                        OpCode::GetUpvalue,
                        temp as u8,
                        0,
                        0,
                        upval_idx as u32,
                    );
                    self.current_chunk().write_instruction(
                        update_op,
                        temp as u8,
                        temp as u8,
                        one_reg as u8,
                        0,
                    );
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
                        return Err(self.format_const_assign_error(name, loc, &decl_loc));
                    }
                    let name_idx = self
                        .current_chunk()
                        .add_constant(Value::string_from_str(name.as_str()));
                    self.current_chunk().write_instruction(
                        OpCode::GetGlobal,
                        dest as u8,
                        0,
                        0,
                        name_idx as u32,
                    );
                    self.current_chunk().write_instruction(
                        update_op,
                        dest as u8,
                        dest as u8,
                        one_reg as u8,
                        0,
                    );
                    self.current_chunk().write_instruction(
                        OpCode::SetGlobal,
                        dest as u8,
                        0,
                        0,
                        name_idx as u32,
                    );
                }
            }
            Expr::Get(obj, prop) => {
                let obj_reg = one_reg + 1;
                self.compile_expr(obj, obj_reg)?;
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::string_from_str(prop.as_str()));
                self.current_chunk().write_instruction(
                    OpCode::GetProperty,
                    dest as u8,
                    obj_reg as u8,
                    0,
                    name_idx as u32,
                );
                self.current_chunk().write_instruction(
                    update_op,
                    dest as u8,
                    dest as u8,
                    one_reg as u8,
                    0,
                );
                self.current_chunk().write_instruction(
                    OpCode::SetProperty,
                    obj_reg as u8,
                    dest as u8,
                    0,
                    name_idx as u32,
                );
            }
            Expr::GetIndex(obj, idx_expr) => {
                let obj_reg = one_reg + 1;
                self.compile_expr(obj, obj_reg)?;
                let idx_reg = obj_reg + 1;
                self.compile_expr(idx_expr, idx_reg)?;
                self.current_chunk().write_instruction(
                    OpCode::GetIndex,
                    dest as u8,
                    obj_reg as u8,
                    idx_reg as u8,
                    0,
                );
                self.current_chunk().write_instruction(
                    update_op,
                    dest as u8,
                    dest as u8,
                    one_reg as u8,
                    0,
                );
                self.current_chunk().write_instruction(
                    OpCode::SetIndex,
                    obj_reg as u8,
                    idx_reg as u8,
                    dest as u8,
                    0,
                );
            }
            _ => return Err("Invalid target for prefix increment/decrement".into()),
        }
        Ok(())
    }

    pub(crate) fn compile_postfix(&mut self, op: &TokenType, inner: &Expr, dest: usize) -> Result<(), String> {
        let is_plus = match op {
            TokenType::PlusPlus => true,
            TokenType::MinusMinus => false,
            _ => return Err("Invalid postfix operator".into()),
        };
        let update_op = if is_plus { OpCode::Add } else { OpCode::Sub };

        let one_reg = std::cmp::max(self.next_reg, dest + 1);
        let one_idx = self.current_chunk().add_constant(Value::number(1.0));
        self.current_chunk().write_instruction(
            OpCode::LoadConst,
            one_reg as u8,
            0,
            0,
            one_idx as u32,
        );

        match inner {
            Expr::Variable(name, loc) => {
                if let Some(idx) = self.resolve_local(name) {
                    if self.locals[idx].is_const {
                        return Err(self.format_const_assign_error(name, loc, &self.locals[idx].loc));
                    }
                    if dest != idx {
                        self.current_chunk().write_instruction(
                            OpCode::Move,
                            dest as u8,
                            idx as u8,
                            0,
                            0,
                        );
                    }
                    self.current_chunk().write_instruction(
                        update_op,
                        idx as u8,
                        idx as u8,
                        one_reg as u8,
                        0,
                    );
                } else if let Some(upval_idx) = self.resolve_upvalue(name) {
                    let (_, is_const, ref decl_loc, _) = self.upvalue_names[upval_idx];
                    if is_const {
                        return Err(self.format_const_assign_error(name, loc, decl_loc));
                    }
                    self.current_chunk().write_instruction(
                        OpCode::GetUpvalue,
                        dest as u8,
                        0,
                        0,
                        upval_idx as u32,
                    );
                    let temp = std::cmp::max(self.next_reg, dest + 1);
                    self.current_chunk().write_instruction(
                        update_op,
                        temp as u8,
                        dest as u8,
                        one_reg as u8,
                        0,
                    );
                    self.current_chunk().write_instruction(
                        OpCode::SetUpvalue,
                        temp as u8,
                        0,
                        0,
                        upval_idx as u32,
                    );
                } else {
                    if let Some(decl_loc) = self.const_globals.borrow().get(name).cloned() {
                        return Err(self.format_const_assign_error(name, loc, &decl_loc));
                    }
                    let name_idx = self
                        .current_chunk()
                        .add_constant(Value::string_from_str(name.as_str()));
                    self.current_chunk().write_instruction(
                        OpCode::GetGlobal,
                        dest as u8,
                        0,
                        0,
                        name_idx as u32,
                    );
                    let temp_reg = one_reg + 1;
                    self.current_chunk().write_instruction(
                        update_op,
                        temp_reg as u8,
                        dest as u8,
                        one_reg as u8,
                        0,
                    );
                    self.current_chunk().write_instruction(
                        OpCode::SetGlobal,
                        temp_reg as u8,
                        0,
                        0,
                        name_idx as u32,
                    );
                }
            }
            Expr::Get(obj, prop) => {
                let obj_reg = one_reg + 1;
                self.compile_expr(obj, obj_reg)?;
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::string_from_str(prop.as_str()));
                self.current_chunk().write_instruction(
                    OpCode::GetProperty,
                    dest as u8,
                    obj_reg as u8,
                    0,
                    name_idx as u32,
                );
                let temp_reg = obj_reg + 1;
                self.current_chunk().write_instruction(
                    update_op,
                    temp_reg as u8,
                    dest as u8,
                    one_reg as u8,
                    0,
                );
                self.current_chunk().write_instruction(
                    OpCode::SetProperty,
                    obj_reg as u8,
                    temp_reg as u8,
                    0,
                    name_idx as u32,
                );
            }
            Expr::GetIndex(obj, idx_expr) => {
                let obj_reg = one_reg + 1;
                self.compile_expr(obj, obj_reg)?;
                let idx_reg = obj_reg + 1;
                self.compile_expr(idx_expr, idx_reg)?;
                self.current_chunk().write_instruction(
                    OpCode::GetIndex,
                    dest as u8,
                    obj_reg as u8,
                    idx_reg as u8,
                    0,
                );
                let temp_reg = idx_reg + 1;
                self.current_chunk().write_instruction(
                    update_op,
                    temp_reg as u8,
                    dest as u8,
                    one_reg as u8,
                    0,
                );
                self.current_chunk().write_instruction(
                    OpCode::SetIndex,
                    obj_reg as u8,
                    idx_reg as u8,
                    temp_reg as u8,
                    0,
                );
            }
            _ => return Err("Invalid target for postfix increment/decrement".into()),
        }
        Ok(())
    }
}
