use crate::frontend::{Expr, LiteralValue, Stmt, TokenType};
use super::value::Value;
use super::bytecode::{Function, Chunk, OpCode};
use super::gc::{gc_allocate, GcData};

pub struct Compiler {
    function: Function,
    locals: Vec<Local>,
    scope_depth: usize,
}

struct Local {
    name: String,
    depth: usize,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            function: Function {
                name: None,
                chunk: Chunk::default(),
                arity: 0,
            },
            locals: Vec::new(),
            scope_depth: 0,
        }
    }

    fn current_chunk(&mut self) -> &mut Chunk {
        &mut self.function.chunk
    }

    pub fn compile(mut self, stmts: &[Stmt]) -> Result<Function, String> {
        for stmt in stmts {
            self.compile_stmt(stmt)?;
        }
        self.current_chunk().write(OpCode::Return);
        Ok(self.function)
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Expr(expr) => {
                self.compile_expr(expr)?;
                self.current_chunk().write(OpCode::Pop);
            }
            Stmt::Print(expr) => {
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::String(gc_allocate(GcData::String("print".to_string()))));
                self.current_chunk().write_operand(OpCode::GetGlobal, name_idx);
                self.compile_expr(expr)?;
                self.current_chunk().write_operand(OpCode::Call, 1);
                self.current_chunk().write(OpCode::Pop);
            }
            Stmt::VarDecl(name, _, expr) => {
                self.compile_expr(expr)?;
                if self.scope_depth > 0 {
                    self.locals.push(Local {
                        name: name.clone(),
                        depth: self.scope_depth,
                    });
                } else {
                    let name_idx = self
                        .current_chunk()
                        .add_constant(Value::String(gc_allocate(GcData::String(name.clone()))));
                    self.current_chunk().write_operand(OpCode::DefineGlobal, name_idx);
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
                self.compile_expr(cond)?;
                let then_jump = self.emit_jump(OpCode::JumpIfFalse);
                self.current_chunk().write(OpCode::Pop); // pop condition
                self.compile_stmt(then_b)?;

                let else_jump = self.emit_jump(OpCode::Jump);
                self.patch_jump(then_jump);
                self.current_chunk().write(OpCode::Pop);

                if let Some(else_b) = else_b {
                    self.compile_stmt(else_b)?;
                }
                self.patch_jump(else_jump);
            }
            Stmt::For(var_name, start, end, body) => {
                self.begin_scope();
                self.compile_expr(start)?;
                self.locals.push(Local {
                    name: var_name.clone(),
                    depth: self.scope_depth,
                });

                let loop_start = self.current_chunk().code.len();

                // Condition: i < end
                let local_idx = self.resolve_local(var_name).unwrap();
                self.current_chunk().write_operand(OpCode::GetLocal, local_idx);
                self.compile_expr(end)?;
                self.current_chunk().write(OpCode::Less);

                let exit_jump = self.emit_jump(OpCode::JumpIfFalse);
                self.current_chunk().write(OpCode::Pop);

                self.compile_stmt(body)?;

                // Increment
                self.current_chunk().write_operand(OpCode::GetLocal, local_idx);
                let one_idx = self.current_chunk().add_constant(Value::Number(1.0));
                self.current_chunk().write_operand(OpCode::Constant, one_idx);
                self.current_chunk().write(OpCode::Add);
                self.current_chunk().write_operand(OpCode::SetLocal, local_idx);
                self.current_chunk().write(OpCode::Pop);

                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);
                self.current_chunk().write(OpCode::Pop);
                self.end_scope();
            }
            Stmt::Return(expr) => {
                if let Some(e) = expr {
                    self.compile_expr(e)?;
                } else {
                    let null_idx = self.current_chunk().add_constant(Value::Null);
                    self.current_chunk().write_operand(OpCode::Constant, null_idx);
                }
                self.current_chunk().write(OpCode::Return);
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Literal(val) => {
                let v = match val {
                    LiteralValue::Null => Value::Null,
                    LiteralValue::Boolean(b) => Value::Boolean(*b),
                    LiteralValue::Number(n) => Value::Number(*n),
                    LiteralValue::String(s) => Value::String(gc_allocate(GcData::String(s.clone()))),
                };
                let idx = self.current_chunk().add_constant(v);
                self.current_chunk().write_operand(OpCode::Constant, idx);
            }
            Expr::Variable(name) => {
                if let Some(idx) = self.resolve_local(name) {
                    self.current_chunk().write_operand(OpCode::GetLocal, idx);
                } else {
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::String(gc_allocate(GcData::String(name.clone()))));
                    self.current_chunk().write_operand(OpCode::GetGlobal, idx);
                }
            }
            Expr::Assign(name, val) => {
                self.compile_expr(val)?;
                if let Some(idx) = self.resolve_local(name) {
                    self.current_chunk().write_operand(OpCode::SetLocal, idx);
                } else {
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::String(gc_allocate(GcData::String(name.clone()))));
                    self.current_chunk().write_operand(OpCode::SetGlobal, idx);
                }
            }
            Expr::Binary(left, op, right) => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                match op {
                    TokenType::Plus => self.current_chunk().write(OpCode::Add),
                    TokenType::Minus => self.current_chunk().write(OpCode::Sub),
                    TokenType::Star => self.current_chunk().write(OpCode::Mul),
                    TokenType::Slash => self.current_chunk().write(OpCode::Div),
                    TokenType::EqualEqual => self.current_chunk().write(OpCode::Equal),
                    TokenType::Greater => self.current_chunk().write(OpCode::Greater),
                    TokenType::Less => self.current_chunk().write(OpCode::Less),
                    _ => return Err("Invalid binary operator".into()),
                }
            }
            Expr::Logical(left, op, right) => {
                self.compile_expr(left)?;
                if op == &TokenType::Or {
                    let else_jump = self.emit_jump(OpCode::JumpIfFalse);
                    let end_jump = self.emit_jump(OpCode::Jump);
                    self.patch_jump(else_jump);
                    self.current_chunk().write(OpCode::Pop);
                    self.compile_expr(right)?;
                    self.patch_jump(end_jump);
                } else if op == &TokenType::And {
                    let end_jump = self.emit_jump(OpCode::JumpIfFalse);
                    self.current_chunk().write(OpCode::Pop);
                    self.compile_expr(right)?;
                    self.patch_jump(end_jump);
                }
            }
            Expr::Call(callee, args) => {
                self.compile_expr(callee)?;
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.current_chunk().write_operand(OpCode::Call, args.len());
            }
            Expr::Get(obj, name) => {
                self.compile_expr(obj)?;
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::String(gc_allocate(GcData::String(name.clone()))));
                self.current_chunk().write_operand(OpCode::GetProperty, name_idx);
            }
            Expr::Set(obj, name, val) => {
                self.compile_expr(obj)?;
                self.compile_expr(val)?;
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::String(gc_allocate(GcData::String(name.clone()))));
                self.current_chunk().write_operand(OpCode::SetProperty, name_idx);
            }
            Expr::Array(items) => {
                for item in items {
                    self.compile_expr(item)?;
                }
                self.current_chunk().write_operand(OpCode::MakeArray, items.len());
            }
            Expr::Object(pairs) => {
                for (key, val) in pairs {
                    let k_idx = self
                        .current_chunk()
                        .add_constant(Value::String(gc_allocate(GcData::String(key.clone()))));
                    self.current_chunk().write_operand(OpCode::Constant, k_idx);
                    self.compile_expr(val)?;
                }
                self.current_chunk().write_operand(OpCode::MakeObject, pairs.len());
            }
            Expr::Function(params, body) => {
                let mut compiler = Compiler::new();
                compiler.function.arity = params.len();
                compiler.begin_scope();
                for param in params {
                    compiler.locals.push(Local {
                        name: param.clone(),
                        depth: compiler.scope_depth,
                    });
                }
                compiler.compile_stmt(body)?;
                let func = compiler.function;

                let func_ptr = gc_allocate(GcData::Function(func));
                let idx = self
                    .current_chunk()
                    .add_constant(Value::Function(func_ptr));
                self.current_chunk().write_operand(OpCode::Constant, idx);
            }
            Expr::GetIndex(obj, index) => {
                self.compile_expr(obj)?;
                self.compile_expr(index)?;
                self.current_chunk().write(OpCode::GetIndex);
            }
            Expr::SetIndex(obj, index, val) => {
                self.compile_expr(obj)?;
                self.compile_expr(index)?;
                self.compile_expr(val)?;
                self.current_chunk().write(OpCode::SetIndex);
            }
        }
        Ok(())
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.scope_depth -= 1;
        while let Some(local) = self.locals.last() {
            if local.depth > self.scope_depth {
                self.current_chunk().write(OpCode::Pop);
                self.locals.pop();
            } else {
                break;
            }
        }
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        self.locals
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, local)| if local.name == name { Some(i) } else { None })
    }

    fn emit_jump(&mut self, op: OpCode) -> usize {
        self.current_chunk().write_operand(op, 0);
        self.current_chunk().code.len() - 1
    }

    fn patch_jump(&mut self, offset: usize) {
        let jump = self.current_chunk().code.len() - 1 - offset;
        let inst = &mut self.current_chunk().code[offset];
        match inst.op {
            OpCode::JumpIfFalse | OpCode::Jump => inst.operand = jump as u32,
            _ => unreachable!(),
        }
    }

    fn emit_loop(&mut self, loop_start: usize) {
        let offset = self.current_chunk().code.len() - loop_start + 1;
        self.current_chunk().write_operand(OpCode::Loop, offset);
    }
}
