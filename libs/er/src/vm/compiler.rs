use crate::frontend::{Expr, LiteralValue, Stmt, TokenType};
use super::value::Value;
use super::bytecode::{Function, Chunk, OpCode};
use super::gc::{gc_allocate, GcData};

pub struct Compiler {
    function: Function,
    locals: Vec<Local>,
    scope_depth: usize,
    next_reg: usize,
}

struct Local {
    name: String,
    depth: usize,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            function: Function {
                name: None,
                chunk: Chunk::default(),
                arity: 0,
                jit_ptr: std::cell::Cell::new(None),
            },
            locals: Vec::new(),
            scope_depth: 0,
            next_reg: 0,
        }
    }

    fn current_chunk(&mut self) -> &mut Chunk {
        &mut self.function.chunk
    }

    pub fn compile(mut self, stmts: &[Stmt]) -> Result<Function, String> {
        for stmt in stmts {
            self.compile_stmt(stmt)?;
        }
        self.current_chunk().write_instruction(OpCode::LoadNull, 0, 0, 0, 0);
        self.current_chunk().write_instruction(OpCode::Return, 0, 0, 0, 0);
        Ok(self.function)
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        self.next_reg = self.locals.len();
        match stmt {
            Stmt::Expr(expr) => {
                self.compile_expr(expr, self.next_reg)?;
            }
            Stmt::Print(expr) => {
                let callee_reg = self.next_reg;
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::string(gc_allocate(GcData::String(std::rc::Rc::from("print")))));
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
            Stmt::VarDecl(name, _, expr) => {
                if self.scope_depth > 0 {
                    let local_reg = self.locals.len();
                    self.compile_expr(expr, local_reg)?;
                    self.locals.push(Local {
                        name: name.clone(),
                        depth: self.scope_depth,
                    });
                } else {
                    let temp_reg = self.next_reg;
                    self.compile_expr(expr, temp_reg)?;
                    let name_idx = self
                        .current_chunk()
                        .add_constant(Value::string(gc_allocate(GcData::String(std::rc::Rc::from(name.as_str())))));
                    self.current_chunk().write_instruction(
                        OpCode::DefineGlobal,
                        temp_reg as u8,
                        0,
                        0,
                        name_idx as u32,
                    );
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
            Stmt::For(var_name, start, end, body) => {
                self.begin_scope();
                let var_reg = self.locals.len();
                self.compile_expr(start, var_reg)?;
                self.locals.push(Local {
                    name: var_name.clone(),
                    depth: self.scope_depth,
                });

                let limit_reg = self.locals.len();
                self.compile_expr(end, limit_reg)?;
                let temp_name = format!("*loop_limit_{}", self.locals.len());
                self.locals.push(Local {
                    name: temp_name.clone(),
                    depth: self.scope_depth,
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
                    name: temp_one_name.clone(),
                    depth: self.scope_depth,
                });

                let loop_start = self.current_chunk().code.len();

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

                self.current_chunk().write_instruction(
                    OpCode::Add,
                    var_reg as u8,
                    var_reg as u8,
                    one_reg as u8,
                    0,
                );

                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);
                self.end_scope();
            }
            Stmt::Return(expr) => {
                let reg = self.next_reg;
                if let Some(e) = expr {
                    self.compile_expr(e, reg)?;
                } else {
                    self.current_chunk().write_instruction(OpCode::LoadNull, reg as u8, 0, 0, 0);
                }
                self.current_chunk().write_instruction(OpCode::Return, reg as u8, 0, 0, 0);
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr, dest: usize) -> Result<(), String> {
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
                            .add_constant(Value::string(gc_allocate(GcData::String(std::rc::Rc::from(s.as_str())))));
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
            Expr::Variable(name) => {
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
                } else {
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::string(gc_allocate(GcData::String(std::rc::Rc::from(name.as_str())))));
                    self.current_chunk().write_instruction(
                        OpCode::GetGlobal,
                        dest as u8,
                        0,
                        0,
                        idx as u32,
                    );
                }
            }
            Expr::Assign(name, val) => {
                self.compile_expr(val, dest)?;
                if let Some(idx) = self.resolve_local(name) {
                    if dest != idx {
                        self.current_chunk().write_instruction(
                            OpCode::Move,
                            idx as u8,
                            dest as u8,
                            0,
                            0,
                        );
                    }
                } else {
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::string(gc_allocate(GcData::String(std::rc::Rc::from(name.as_str())))));
                    self.current_chunk().write_instruction(
                        OpCode::SetGlobal,
                        dest as u8,
                        0,
                        0,
                        idx as u32,
                    );
                }
            }
            Expr::Binary(left, op, right) => {
                self.compile_expr(left, dest)?;
                let temp = std::cmp::max(self.next_reg, dest + 1);
                self.compile_expr(right, temp)?;
                let code = match op {
                    TokenType::Plus => OpCode::Add,
                    TokenType::Minus => OpCode::Sub,
                    TokenType::Star => OpCode::Mul,
                    TokenType::Slash => OpCode::Div,
                    TokenType::EqualEqual => OpCode::Equal,
                    TokenType::Greater => OpCode::Greater,
                    TokenType::Less => OpCode::Less,
                    _ => return Err("Invalid binary operator".into()),
                };
                self.current_chunk().write_instruction(
                    code,
                    dest as u8,
                    dest as u8,
                    temp as u8,
                    0,
                );
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
            Expr::Get(obj, name) => {
                self.compile_expr(obj, dest)?;
                let name_idx = self
                    .current_chunk()
                    .add_constant(Value::string(gc_allocate(GcData::String(std::rc::Rc::from(name.as_str())))));
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
                    .add_constant(Value::string(gc_allocate(GcData::String(std::rc::Rc::from(name.as_str())))));
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
            Expr::Object(pairs) => {
                let start_reg = std::cmp::max(self.next_reg, dest);
                for (i, (key, val)) in pairs.iter().enumerate() {
                    let k_idx = self
                        .current_chunk()
                        .add_constant(Value::string(gc_allocate(GcData::String(std::rc::Rc::from(key.as_str())))));
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
            Expr::Function(params, body) => {
                let mut compiler = Compiler::new();
                compiler.function.arity = params.len();
                compiler.next_reg = params.len();
                compiler.begin_scope();
                for param in params {
                    compiler.locals.push(Local {
                        name: param.clone(),
                        depth: compiler.scope_depth,
                    });
                }
                compiler.compile_stmt(body)?;
                compiler.current_chunk().write_instruction(OpCode::LoadNull, 0, 0, 0, 0);
                compiler.current_chunk().write_instruction(OpCode::Return, 0, 0, 0, 0);

                let func = compiler.function;
                let func_ptr = gc_allocate(GcData::Function(func));
                let idx = self
                    .current_chunk()
                    .add_constant(Value::function(func_ptr));
                self.current_chunk().write_instruction(
                    OpCode::LoadConst,
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

    fn emit_jump(&mut self, op: OpCode, cond_reg: usize) -> usize {
        self.current_chunk().write_instruction(op, cond_reg as u8, 0, 0, 0);
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
        self.current_chunk().write_instruction(OpCode::Loop, 0, 0, 0, offset as u32);
    }
}
