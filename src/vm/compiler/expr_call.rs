use super::{Compiler, Local};
use crate::frontend::Expr;
use crate::vm::bytecode::OpCode;
use crate::vm::gc::{gc_allocate, GcData};
use crate::vm::value::Value;

impl Compiler {
    pub(crate) fn compile_call(&mut self, callee: &Expr, args: &[Expr], dest: usize) -> Result<(), String> {
        let mut compiled_super = false;
        if let Expr::Get(obj, name) = callee {
            if let Expr::Variable(var_name, _) = &**obj {
                if var_name == "super" {
                    if let Some(ref struct_name) = self.current_struct {
                        if let Some(struct_info) = self.structs.get(struct_name) {
                            let mut parent_match = None;
                            for parent in &struct_info.composed {
                                if let Some(parent_info) = self.structs.get(parent) {
                                    if let Some(m) = parent_info.methods.iter().find(|(m_name, _, _)| m_name == name) {
                                        parent_match = Some((parent.clone(), m.clone()));
                                        break;
                                    }
                                }
                            }
                            if let Some((parent_name, (_m_name, m_params, m_body))) = parent_match {
                                let temp_start = std::cmp::max(self.next_reg, dest);
                                
                                let mut method_params = vec!["this".to_string()];
                                method_params.extend(m_params.clone());

                                let mut compiler = Compiler::new();
                                compiler.const_globals = self.const_globals.clone();
                                compiler.structs = self.structs.clone();
                                compiler.interfaces = self.interfaces.clone();
                                compiler.global_types = self.global_types.clone();
                                compiler.current_struct = Some(parent_name);
                                compiler.function.arity = method_params.len();
                                compiler.function.is_async = false;
                                compiler.next_reg = method_params.len();
                                compiler.begin_scope();
                                for param in &method_params {
                                    compiler.locals.push(Local {
                                        name: param.clone(),
                                        depth: compiler.scope_depth,
                                        is_const: false,
                                        loc: crate::frontend::SourceLocation::default(),
                                        ty: None,
                                    });
                                }
                                compiler.compile_stmt(&m_body)?;
                                compiler.current_chunk().write_instruction(OpCode::LoadNull, 0, 0, 0, 0);
                                compiler.current_chunk().write_instruction(OpCode::Return, 0, 0, 0, 0);

                                let func = compiler.function;
                                let func_ptr = gc_allocate(GcData::Function(Box::new(func)));
                                let func_val = Value::function(func_ptr);

                                let func_idx = self.current_chunk().add_constant(func_val);
                                
                                self.current_chunk().write_instruction(
                                    OpCode::LoadConst,
                                    temp_start as u8,
                                    0,
                                    0,
                                    func_idx as u32,
                                );

                                let this_reg = self.locals.iter().enumerate()
                                    .find(|(_, l)| l.name == "this")
                                    .map(|(idx, _)| idx)
                                    .unwrap_or(0);
                                
                                self.current_chunk().write_instruction(
                                    OpCode::Move,
                                    (temp_start + 1) as u8,
                                    this_reg as u8,
                                    0,
                                    0,
                                );

                                for (i, arg) in args.iter().enumerate() {
                                    self.compile_expr(arg, temp_start + 2 + i)?;
                                }

                                self.current_chunk().write_instruction(
                                    OpCode::Call,
                                    dest as u8,
                                    temp_start as u8,
                                    0,
                                    (args.len() + 1) as u32,
                                );
                                compiled_super = true;
                            }
                        }
                    }
                }
            }
        }

        if !compiled_super {
            let temp_start = std::cmp::max(self.next_reg, dest);
            self.compile_expr(callee, temp_start)?;
            for (i, arg) in args.iter().enumerate() {
                self.compile_expr(arg, temp_start + 1 + i)?;
            }
            self.current_chunk().write_instruction(
                OpCode::Call,
                dest as u8,
                temp_start as u8,
                0,
                args.len() as u32,
            );
        }
        Ok(())
    }
}
