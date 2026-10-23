use super::{Compiler, Local};
use crate::vm::bytecode::OpCode;
use crate::vm::value::Value;

impl Compiler {
    pub(crate) fn compile_await_handles(&mut self, handles_reg: usize) -> Result<(), String> {
        let len_reg = self.locals.len();
        self.locals.push(Local {
            name: format!("*handles_len_{}", self.locals.len()),
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let array_len_idx = self
            .current_chunk()
            .add_constant(Value::string_from_str("arrayLen"));
        self.current_chunk().write_instruction(
            OpCode::GetGlobal,
            len_reg as u8,
            0,
            0,
            array_len_idx as u32,
        );
        self.current_chunk().write_instruction(
            OpCode::Move,
            (len_reg + 1) as u8,
            handles_reg as u8,
            0,
            0,
        );
        self.current_chunk().write_instruction(
            OpCode::Call,
            len_reg as u8,
            len_reg as u8,
            0,
            1,
        );

        let var_reg = self.locals.len();
        let zero_idx = self.current_chunk().add_constant(Value::number(0.0));
        self.current_chunk().write_instruction(
            OpCode::LoadConst,
            var_reg as u8,
            0,
            0,
            zero_idx as u32,
        );
        self.locals.push(Local {
            name: format!("*await_i_{}", self.locals.len()),
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
        self.locals.push(Local {
            name: format!("*await_one_{}", self.locals.len()),
            depth: self.scope_depth,
            is_const: false,
            loc: crate::frontend::SourceLocation::default(),
            ty: None,
        });

        let loop_start = self.current_chunk().code.len();
        let cond_reg = self.locals.len();
        self.current_chunk().write_instruction(
            OpCode::Less,
            cond_reg as u8,
            var_reg as u8,
            len_reg as u8,
            0,
        );

        let exit_jump = self.emit_jump(OpCode::JumpIfFalse, cond_reg);

        let h_reg = self.locals.len();
        self.current_chunk().write_instruction(
            OpCode::GetIndex,
            h_reg as u8,
            handles_reg as u8,
            var_reg as u8,
            0,
        );

        let await_fn_reg = h_reg + 1;
        let await_name_idx = self
            .current_chunk()
            .add_constant(Value::string_from_str("futureAwait"));
        self.current_chunk().write_instruction(
            OpCode::GetGlobal,
            await_fn_reg as u8,
            0,
            0,
            await_name_idx as u32,
        );
        self.current_chunk().write_instruction(
            OpCode::Move,
            (await_fn_reg + 1) as u8,
            h_reg as u8,
            0,
            0,
        );
        self.current_chunk().write_instruction(
            OpCode::Call,
            await_fn_reg as u8,
            await_fn_reg as u8,
            0,
            1,
        );

        self.current_chunk().write_instruction(
            OpCode::Add,
            var_reg as u8,
            var_reg as u8,
            one_reg as u8,
            0,
        );

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);

        Ok(())
    }
}
