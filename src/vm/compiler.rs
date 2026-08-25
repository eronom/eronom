use crate::frontend::{Expr, LiteralValue, Stmt, TokenType};
use super::value::Value;
use super::bytecode::{Function, Chunk, OpCode};
use super::gc::{gc_allocate, GcData};

#[derive(Clone)]
pub struct FlattenedStructInfo {
    pub composed: Vec<String>,
    pub fields: Vec<(String, String)>,
    pub methods: Vec<(String, Vec<String>, Stmt)>,
}

#[derive(Clone)]
pub struct RawStructInfo {
    pub composed: Vec<String>,
    pub fields: Vec<(String, String)>,
    pub methods: Vec<(String, Vec<String>, Stmt)>,
}

#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub fields: Vec<(String, String)>,
    pub methods: Vec<(String, Vec<String>)>,
}

#[derive(Clone, Debug)]
struct LoopContext {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
    is_switch: bool,
}

pub struct Compiler {
    parent: Option<*mut Compiler>,
    function: Function,
    locals: Vec<Local>,
    upvalues: Vec<super::bytecode::UpvalueDescriptor>,
    upvalue_names: Vec<(String, bool, crate::frontend::SourceLocation, Option<String>)>,
    scope_depth: usize,
    next_reg: usize,
    const_globals: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, crate::frontend::SourceLocation>>>,
    structs: std::collections::HashMap<String, FlattenedStructInfo>,
    interfaces: std::collections::HashMap<String, InterfaceInfo>,
    global_types: std::collections::HashMap<String, String>,
    current_return_type: Option<String>,
    current_struct: Option<String>,
    concurrent_scopes: Vec<usize>,
    loop_stack: Vec<LoopContext>,
}

struct Local {
    name: String,
    depth: usize,
    is_const: bool,
    loc: crate::frontend::SourceLocation,
    ty: Option<String>,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            parent: None,
            function: Function {
                name: None,
                chunk: Chunk::default(),
                arity: 0,
                jit_ptr: std::cell::Cell::new(None),
                invocation_count: std::cell::Cell::new(0),
                is_async: false,
                upvalues: Vec::new(),
            },
            locals: Vec::new(),
            upvalues: Vec::new(),
            upvalue_names: Vec::new(),
            scope_depth: 0,
            next_reg: 0,
            const_globals: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
            structs: std::collections::HashMap::new(),
            interfaces: std::collections::HashMap::new(),
            global_types: std::collections::HashMap::new(),
            current_return_type: None,
            current_struct: None,
            concurrent_scopes: Vec::new(),
            loop_stack: Vec::new(),
        }
    }

    fn current_chunk(&mut self) -> &mut Chunk {
        &mut self.function.chunk
    }

    pub fn compile(mut self, stmts: &[Stmt]) -> Result<Function, String> {
        let mut raw_structs = std::collections::HashMap::new();
        collect_structs(stmts, &mut raw_structs);

        let mut resolved = std::collections::HashMap::new();
        for name in raw_structs.keys() {
            let mut visiting = std::collections::HashSet::new();
            flatten_struct(name, &raw_structs, &mut resolved, &mut visiting)?;
        }

        self.structs = resolved;
        collect_interfaces(stmts, &mut self.interfaces);

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
            }
            Stmt::For(var_name, start, end, body) => {
                self.begin_scope();
                let var_reg = self.locals.len();
                self.compile_expr(start, var_reg)?;
                self.locals.push(Local {
                    name: var_name.clone(),
                    depth: self.scope_depth,
                    is_const: false,
                    loc: crate::frontend::SourceLocation::default(),
                    ty: None,
                });

                let limit_reg = self.locals.len();
                self.compile_expr(end, limit_reg)?;
                let temp_name = format!("*loop_limit_{}", self.locals.len());
                self.locals.push(Local {
                    name: temp_name.clone(),
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
                    name: temp_one_name.clone(),
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
            }
            Stmt::ForIn(var_name, iterable, body) => {
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
                    name: var_name.clone(),
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
            }
            Stmt::Switch(target_expr, cases, default_body) => {
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
            Stmt::Struct(name, _, _, _, _) => {
                let flat = self.structs.get(name).cloned().unwrap_or(FlattenedStructInfo {
                    composed: Vec::new(),
                    fields: Vec::new(),
                    methods: Vec::new(),
                });
                let name_val = Value::string_from_str(name.as_str());
                let name_idx = self.current_chunk().add_constant(name_val);
                
                let mut field_names_vals = Vec::new();
                for (field_name, _) in &flat.fields {
                    field_names_vals.push(Value::string_from_str(field_name.as_str()));
                }
                let array_ptr = gc_allocate(GcData::Array(field_names_vals));
                let fields_val = Value::array(array_ptr);
                let fields_idx = self.current_chunk().add_constant(fields_val);

                let mut methods_map = crate::vm::gc::get_pooled_map(flat.methods.len());
                for (m_name, m_params, m_body) in &flat.methods {
                    let mut method_params = vec!["this".to_string()];
                    method_params.extend(m_params.clone());

                    let mut compiler = Compiler::new();
                    compiler.const_globals = self.const_globals.clone();
                    compiler.structs = self.structs.clone();
                    compiler.interfaces = self.interfaces.clone();
                    compiler.global_types = self.global_types.clone();
                    compiler.current_struct = Some(name.clone());
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
                    compiler.compile_stmt(m_body)?;
                    compiler.current_chunk().write_instruction(OpCode::LoadNull, 0, 0, 0, 0);
                    compiler.current_chunk().write_instruction(OpCode::Return, 0, 0, 0, 0);

                    let func = compiler.function;
                    let func_ptr = gc_allocate(GcData::Function(func));
                    let func_val = Value::function(func_ptr);

                    methods_map.insert(crate::vm::value::MapKey(Value::string_from_str(m_name.as_str())), func_val);
                }
                let methods_ptr = gc_allocate(GcData::Object(methods_map));
                let methods_val = Value::object(methods_ptr);
                let methods_idx = self.current_chunk().add_constant(methods_val);

                self.current_chunk().write_instruction(
                    OpCode::DefineStruct,
                    fields_idx as u8,
                    methods_idx as u8,
                    0,
                    name_idx as u32,
                );
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

    fn compile_await_handles(&mut self, handles_reg: usize) -> Result<(), String> {
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

                match &**inner {
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
            }
            Expr::Postfix(op, inner) => {
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

                match &**inner {
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
                let mut compiled_super = false;
                if let Expr::Get(obj, name) = &**callee {
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
                                        let func_ptr = gc_allocate(GcData::Function(func));
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
                let func_ptr = gc_allocate(GcData::Function(func));
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

    fn add_upvalue(&mut self, is_local: bool, index: u8, name: &str, is_const: bool, loc: crate::frontend::SourceLocation, ty: Option<String>) -> usize {
        for (i, uv) in self.upvalues.iter().enumerate() {
            if uv.is_local == is_local && uv.index == index {
                return i;
            }
        }
        self.upvalues.push(super::bytecode::UpvalueDescriptor { is_local, index });
        self.upvalue_names.push((name.to_string(), is_const, loc, ty));
        self.upvalues.len() - 1
    }

    fn resolve_upvalue(&mut self, name: &str) -> Option<usize> {
        if let Some(parent_ptr) = self.parent {
            unsafe {
                let parent = &mut *parent_ptr;
                if let Some(local_idx) = parent.resolve_local(name) {
                    let is_const = parent.locals[local_idx].is_const;
                    let loc = parent.locals[local_idx].loc.clone();
                    let ty = parent.locals[local_idx].ty.clone();
                    return Some(self.add_upvalue(true, local_idx as u8, name, is_const, loc, ty));
                }
                if let Some(parent_upval_idx) = parent.resolve_upvalue(name) {
                    let (ref u_name, is_const, ref loc, ref ty) = parent.upvalue_names[parent_upval_idx];
                    let loc_clone = loc.clone();
                    let ty_clone = ty.clone();
                    let u_name_clone = u_name.clone();
                    return Some(self.add_upvalue(false, parent_upval_idx as u8, &u_name_clone, is_const, loc_clone, ty_clone));
                }
            }
        }
        None
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

    fn patch_jump_to(&mut self, offset: usize, target_ip: usize) {
        let jump = target_ip - 1 - offset;
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

    fn format_const_assign_error(&self, name: &str, assign_loc: &crate::frontend::SourceLocation, decl_loc: &crate::frontend::SourceLocation) -> String {
        fn get_file_line(path: &str, line_num: usize) -> Option<String> {
            if let Ok(content) = std::fs::read_to_string(path) {
                content.lines().nth(line_num - 1).map(|s| s.to_string())
            } else {
                None
            }
        }
        
        fn format_snippet(loc: &crate::frontend::SourceLocation) -> String {
            let line_str = loc.line.to_string();
            let prefix = format!("{} | ", line_str);
            let mut result = String::new();
            if let Some(content) = get_file_line(&loc.file_path, loc.line) {
                result.push_str(&format!("{}{}\n", prefix, content));
                let spaces = " ".repeat(line_str.len() + 3 + loc.col - 1);
                result.push_str(&format!("{}^\n", spaces));
            }
            result
        }

        format!(
            "{}\
            error: This assignment will throw because \"{}\" is a constant\n\
            \x20\x20\x20\x20at {}:{}:{}\n\n\
            {}\
            note: The symbol \"{}\" was declared a constant here:\n\
            \x20\x20\x20at {}:{}:{}",
            format_snippet(assign_loc),
            name,
            assign_loc.file_path, assign_loc.line, assign_loc.col,
            format_snippet(decl_loc),
            name,
            decl_loc.file_path, decl_loc.line, decl_loc.col
        )
    }
}

fn collect_structs(stmts: &[Stmt], map: &mut std::collections::HashMap<String, RawStructInfo>) {
    for stmt in stmts {
        match stmt {
            Stmt::Struct(name, composed, fields, methods, _) => {
                map.insert(name.clone(), RawStructInfo {
                    composed: composed.clone(),
                    fields: fields.clone(),
                    methods: methods.clone(),
                });
            }
            Stmt::Block(inner_stmts) => {
                collect_structs(inner_stmts, map);
            }
            Stmt::If(_, then_branch, else_branch) => {
                collect_structs(std::slice::from_ref(then_branch), map);
                if let Some(eb) = else_branch {
                    collect_structs(std::slice::from_ref(eb), map);
                }
            }
            Stmt::While(_, body) => {
                collect_structs(std::slice::from_ref(body), map);
            }
            Stmt::For(_, _, _, body) => {
                collect_structs(std::slice::from_ref(body), map);
            }
            Stmt::ForIn(_, _, body) => {
                collect_structs(std::slice::from_ref(body), map);
            }
            Stmt::Try(try_body, catch_clause, finally_body) => {
                collect_structs(std::slice::from_ref(try_body), map);
                if let Some((_, catch_b)) = catch_clause {
                    collect_structs(std::slice::from_ref(catch_b), map);
                }
                if let Some(finally_b) = finally_body {
                    collect_structs(std::slice::from_ref(finally_b), map);
                }
            }
            Stmt::Switch(_, cases, default_body) => {
                for c in cases {
                    collect_structs(std::slice::from_ref(&c.body), map);
                }
                if let Some(def_b) = default_body {
                    collect_structs(std::slice::from_ref(def_b), map);
                }
            }
            Stmt::Export(inner) => {
                collect_structs(std::slice::from_ref(inner), map);
            }
            _ => {}
        }
    }
}

fn collect_interfaces(stmts: &[Stmt], map: &mut std::collections::HashMap<String, InterfaceInfo>) {
    for stmt in stmts {
        match stmt {
            Stmt::Interface(name, fields, methods, _) => {
                map.insert(name.clone(), InterfaceInfo {
                    fields: fields.clone(),
                    methods: methods.clone(),
                });
            }
            Stmt::Block(inner_stmts) => {
                collect_interfaces(inner_stmts, map);
            }
            Stmt::If(_, then_branch, else_branch) => {
                collect_interfaces(std::slice::from_ref(then_branch), map);
                if let Some(eb) = else_branch {
                    collect_interfaces(std::slice::from_ref(eb), map);
                }
            }
            Stmt::While(_, body) => {
                collect_interfaces(std::slice::from_ref(body), map);
            }
            Stmt::For(_, _, _, body) => {
                collect_interfaces(std::slice::from_ref(body), map);
            }
            Stmt::ForIn(_, _, body) => {
                collect_interfaces(std::slice::from_ref(body), map);
            }
            Stmt::Try(try_body, catch_clause, finally_body) => {
                collect_interfaces(std::slice::from_ref(try_body), map);
                if let Some((_, catch_b)) = catch_clause {
                    collect_interfaces(std::slice::from_ref(catch_b), map);
                }
                if let Some(finally_b) = finally_body {
                    collect_interfaces(std::slice::from_ref(finally_b), map);
                }
            }
            Stmt::Switch(_, cases, default_body) => {
                for c in cases {
                    collect_interfaces(std::slice::from_ref(&c.body), map);
                }
                if let Some(def_b) = default_body {
                    collect_interfaces(std::slice::from_ref(def_b), map);
                }
            }
            Stmt::Export(inner) => {
                collect_interfaces(std::slice::from_ref(inner), map);
            }
            _ => {}
        }
    }
}

fn flatten_struct(
    name: &str,
    structs: &std::collections::HashMap<String, RawStructInfo>,
    resolved: &mut std::collections::HashMap<String, FlattenedStructInfo>,
    visiting: &mut std::collections::HashSet<String>,
) -> Result<FlattenedStructInfo, String> {
    if let Some(res) = resolved.get(name) {
        return Ok(res.clone());
    }
    if visiting.contains(name) {
        return Err(format!("Circular dependency detected in struct composition: {}", name));
    }
    visiting.insert(name.to_string());

    let info = structs.get(name).ok_or_else(|| format!("Undefined struct: {}", name))?;
    let mut flat_fields = Vec::new();
    let mut flat_methods = Vec::new();

    let own_field_names: std::collections::HashSet<&str> = info.fields.iter().map(|(n, _)| n.as_str()).collect();
    for comp in &info.composed {
        let comp_flat = flatten_struct(comp, structs, resolved, visiting)?;
        for (f_name, f_type) in comp_flat.fields {
            if !own_field_names.contains(f_name.as_str()) {
                flat_fields.push((f_name, f_type));
            }
        }
    }
    flat_fields.extend(info.fields.clone());

    let own_method_names: std::collections::HashSet<&str> = info.methods.iter().map(|(n, _, _)| n.as_str()).collect();
    for comp in &info.composed {
        let comp_flat = flatten_struct(comp, structs, resolved, visiting)?;
        for (m_name, m_params, m_body) in comp_flat.methods {
            if !own_method_names.contains(m_name.as_str()) {
                flat_methods.push((m_name, m_params, m_body));
            }
        }
    }
    flat_methods.extend(info.methods.clone());

    let flat_info = FlattenedStructInfo {
        composed: info.composed.clone(),
        fields: flat_fields,
        methods: flat_methods,
    };

    resolved.insert(name.to_string(), flat_info.clone());
    visiting.remove(name);
    Ok(flat_info)
}

fn get_expr_type(
    expr: &Expr,
    locals: &[Local],
    global_types: &std::collections::HashMap<String, String>,
    structs: &std::collections::HashMap<String, FlattenedStructInfo>,
    interfaces: &std::collections::HashMap<String, InterfaceInfo>,
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

fn is_type_compatible(
    expected: &str,
    actual: &str,
    structs: &std::collections::HashMap<String, FlattenedStructInfo>,
    interfaces: &std::collections::HashMap<String, InterfaceInfo>,
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
            let struct_fields: std::collections::HashMap<String, String> = struct_info.fields.iter().cloned().collect();
            for (f_name, f_ty) in &iface.fields {
                if let Some(sf_ty) = struct_fields.get(f_name) {
                    if !is_type_compatible(f_ty, sf_ty, structs, interfaces) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            let struct_methods: std::collections::HashMap<String, usize> = struct_info.methods.iter().map(|(m, p, _)| (m.clone(), p.len())).collect();
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

fn check_type(
    expr: &Expr,
    expected_type: &str,
    structs: &std::collections::HashMap<String, FlattenedStructInfo>,
    interfaces: &std::collections::HashMap<String, InterfaceInfo>,
    locals: &[Local],
    global_types: &std::collections::HashMap<String, String>,
    loc: &crate::frontend::SourceLocation,
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
            let mut object_fields = std::collections::HashMap::new();
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
                let mut object_fields = std::collections::HashMap::new();
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
            let mut object_fields = std::collections::HashMap::new();
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
