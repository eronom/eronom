use crate::frontend::{Expr, LiteralValue, Stmt, TokenType};
use super::value::Value;
use super::bytecode::{Function, Chunk, OpCode};
use super::gc::{gc_allocate, GcData, get_or_create_string};

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

pub struct Compiler {
    function: Function,
    locals: Vec<Local>,
    scope_depth: usize,
    next_reg: usize,
    const_globals: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, crate::frontend::SourceLocation>>>,
    structs: std::collections::HashMap<String, FlattenedStructInfo>,
    interfaces: std::collections::HashMap<String, InterfaceInfo>,
    global_types: std::collections::HashMap<String, String>,
    current_struct: Option<String>,
    concurrent_scopes: Vec<usize>,
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
            function: Function {
                name: None,
                chunk: Chunk::default(),
                arity: 0,
                jit_ptr: std::cell::Cell::new(None),
                is_async: false,
            },
            locals: Vec::new(),
            scope_depth: 0,
            next_reg: 0,
            const_globals: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
            structs: std::collections::HashMap::new(),
            interfaces: std::collections::HashMap::new(),
            global_types: std::collections::HashMap::new(),
            current_struct: None,
            concurrent_scopes: Vec::new(),
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
                    .add_constant(Value::string(get_or_create_string("print")));
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
                        .add_constant(Value::string(get_or_create_string(name.as_str())));
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
                let name_val = Value::string(get_or_create_string(name.as_str()));
                let name_idx = self.current_chunk().add_constant(name_val);
                
                let mut field_names_vals = Vec::new();
                for (field_name, _) in &flat.fields {
                    let f_ptr = get_or_create_string(field_name.as_str());
                    field_names_vals.push(Value::string(f_ptr));
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

                    let m_name_ptr = get_or_create_string(m_name.as_str());
                    methods_map.insert(crate::vm::value::MapKey(Value::string(m_name_ptr)), func_val);
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
            .add_constant(Value::string(get_or_create_string("arrayLen")));
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
            .add_constant(Value::string(get_or_create_string("futureAwait")));
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
                            .add_constant(Value::string(get_or_create_string(s.as_str())));
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
                } else {
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::string(get_or_create_string(name.as_str())));
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
                } else {
                    if let Some(decl_loc) = self.const_globals.borrow().get(name).cloned() {
                        return Err(self.format_const_assign_error(name, assign_loc, &decl_loc));
                    }
                    self.compile_expr(val, dest)?;
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::string(get_or_create_string(name.as_str())));
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
                    .add_constant(Value::string(get_or_create_string(name.as_str())));
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
                    .add_constant(Value::string(get_or_create_string(name.as_str())));
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
                        .add_constant(Value::string(get_or_create_string(key.as_str())));
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
                compiler.const_globals = self.const_globals.clone();
                compiler.structs = self.structs.clone();
                compiler.interfaces = self.interfaces.clone();
                compiler.global_types = self.global_types.clone();
                compiler.current_struct = self.current_struct.clone();
                compiler.function.arity = params.len();
                compiler.function.is_async = false;
                compiler.next_reg = params.len();
                compiler.begin_scope();
                for param in params {
                    compiler.locals.push(Local {
                        name: param.clone(),
                        depth: compiler.scope_depth,
                        is_const: false,
                        loc: crate::frontend::SourceLocation::default(),
                        ty: None,
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
            Expr::Spawn(inner) => {
                let (callee, args) = match &**inner {
                    Expr::Call(callee, args) => (callee.as_ref().clone(), args.clone()),
                    _ => return Err("spawn expects a function call expression".to_string()),
                };

                let spawn_call = Expr::Call(
                    Box::new(Expr::Get(
                        Box::new(Expr::Variable("Io".to_string(), crate::frontend::SourceLocation::default())),
                        "spawn".to_string(),
                    )),
                    vec![callee, Expr::Array(args)],
                );

                self.compile_expr(&spawn_call, dest)?;

                if let Some(&handles_reg) = self.concurrent_scopes.last() {
                    let push_fn_reg = std::cmp::max(self.next_reg, dest + 1);
                    let push_name_idx = self
                        .current_chunk()
                        .add_constant(Value::string(get_or_create_string("arrayPush")));
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
            Stmt::For(_, _, _, body) => {
                collect_structs(std::slice::from_ref(body), map);
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
            Stmt::For(_, _, _, body) => {
                collect_interfaces(std::slice::from_ref(body), map);
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
) -> Option<String> {
    match expr {
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
            }
            None
        }
        _ => None,
    }
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
    if let Expr::StructInst(struct_name, pairs, loc) = expr {
        if let Some(s_info) = structs.get(struct_name) {
            let mut object_fields = std::collections::HashMap::new();
            for (k, v) in pairs {
                object_fields.insert(k.clone(), v);
            }
            for (field_name, field_type) in &s_info.fields {
                if let Some(field_val_expr) = object_fields.remove(field_name) {
                    check_type(field_val_expr, field_type, structs, interfaces, locals, global_types, loc)?;
                } else {
                    return Err(format!(
                        "error: Missing field \"{}\" of type \"{}\" in struct \"{}\"\n    at {}:{}:{}",
                        field_name, field_type, struct_name, loc.file_path, loc.line, loc.col
                    ));
                }
            }
            if !object_fields.is_empty() {
                let extra_fields: Vec<String> = object_fields.keys().cloned().collect();
                return Err(format!(
                    "error: Extra fields {:?} not declared in struct \"{}\"\n    at {}:{}:{}",
                    extra_fields, struct_name, loc.file_path, loc.line, loc.col
                ));
            }
        } else {
            return Err(format!(
                "error: Unknown struct type \"{}\"\n    at {}:{}:{}",
                struct_name, loc.file_path, loc.line, loc.col
            ));
        }
    }

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

        if let Some(src_type) = get_expr_type(expr, locals, global_types, structs) {
            if src_type == expected_type {
                return Ok(());
            }

            if let Some(struct_info) = structs.get(&src_type) {
                let struct_fields: std::collections::HashMap<String, String> = struct_info.fields.iter().cloned().collect();
                for (field_name, field_type) in &interface_info.fields {
                    if let Some(struct_field_type) = struct_fields.get(field_name) {
                        if struct_field_type != field_type {
                            return Err(format!(
                                "error: Type mismatch for field \"{}\" in struct \"{}\" (expected \"{}\" from interface \"{}\" but got \"{}\")\n    at {}:{}:{}",
                                field_name, src_type, field_type, expected_type, struct_field_type, loc.file_path, loc.line, loc.col
                            ));
                        }
                    } else {
                        return Err(format!(
                            "error: Struct \"{}\" does not implement interface \"{}\" because it is missing field \"{}\"\n    at {}:{}:{}",
                            src_type, expected_type, field_name, loc.file_path, loc.line, loc.col
                        ));
                    }
                }

                let struct_methods: std::collections::HashMap<String, Vec<String>> = struct_info.methods.iter().map(|(m_name, m_params, _)| (m_name.clone(), m_params.clone())).collect();
                for (method_name, method_params) in &interface_info.methods {
                    if let Some(struct_method_params) = struct_methods.get(method_name) {
                        if struct_method_params.len() != method_params.len() {
                            return Err(format!(
                                "error: Method \"{}\" in struct \"{}\" has {} parameters, but interface \"{}\" expects {}\n    at {}:{}:{}",
                                method_name, src_type, struct_method_params.len(), expected_type, method_params.len(), loc.file_path, loc.line, loc.col
                            ));
                        }
                    } else {
                        return Err(format!(
                            "error: Struct \"{}\" does not implement interface \"{}\" because it is missing method \"{}\"\n    at {}:{}:{}",
                            src_type, expected_type, method_name, loc.file_path, loc.line, loc.col
                        ));
                    }
                }

                return Ok(());
            }

            if let Some(src_interface_info) = interfaces.get(&src_type) {
                let src_fields: std::collections::HashMap<String, String> = src_interface_info.fields.iter().cloned().collect();
                for (field_name, field_type) in &interface_info.fields {
                    if let Some(src_field_type) = src_fields.get(field_name) {
                        if src_field_type != field_type {
                            return Err(format!(
                                "error: Type mismatch for field \"{}\" (expected \"{}\" but got \"{}\")\n    at {}:{}:{}",
                                field_name, field_type, src_field_type, loc.file_path, loc.line, loc.col
                            ));
                        }
                    } else {
                        return Err(format!(
                            "error: Interface \"{}\" does not satisfy interface \"{}\" because it is missing field \"{}\"\n    at {}:{}:{}",
                            src_type, expected_type, field_name, loc.file_path, loc.line, loc.col
                        ));
                    }
                }

                let src_methods: std::collections::HashMap<String, Vec<String>> = src_interface_info.methods.iter().cloned().collect();
                for (method_name, method_params) in &interface_info.methods {
                    if let Some(src_method_params) = src_methods.get(method_name) {
                        if src_method_params.len() != method_params.len() {
                            return Err(format!(
                                "error: Method \"{}\" has parameter count mismatch (expected {} but got {})\n    at {}:{}:{}",
                                method_name, method_params.len(), src_method_params.len(), loc.file_path, loc.line, loc.col
                            ));
                        }
                    } else {
                        return Err(format!(
                            "error: Interface \"{}\" does not satisfy interface \"{}\" because it is missing method \"{}\"\n    at {}:{}:{}",
                            src_type, expected_type, method_name, loc.file_path, loc.line, loc.col
                        ));
                    }
                }

                return Ok(());
            }

            return Err(format!(
                "error: Expected interface \"{}\" but got unknown type \"{}\"\n    at {}:{}:{}",
                expected_type, src_type, loc.file_path, loc.line, loc.col
            ));
        }

        if let Expr::Literal(LiteralValue::Null) = expr {
            return Ok(());
        }

        return Ok(());
    }

    if let Some(struct_info) = structs.get(expected_type) {
        let struct_fields = &struct_info.fields;
        match expr {
            Expr::Object(pairs) => {
                let mut object_fields = std::collections::HashMap::new();
                for (k, v) in pairs {
                    object_fields.insert(k.clone(), v);
                }

                for (field_name, field_type) in struct_fields {
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
            }
            Expr::StructInst(struct_name, _, loc) => {
                if struct_name != expected_type {
                    return Err(format!(
                        "error: Expected struct \"{}\" but got struct \"{}\"\n    at {}:{}:{}",
                        expected_type, struct_name, loc.file_path, loc.line, loc.col
                    ));
                }
            }
            Expr::Literal(LiteralValue::Null) => {}
            Expr::Literal(val) => {
                let got_str = match val {
                    LiteralValue::Number(n) => n.to_string(),
                    LiteralValue::String(s) => format!("\"{}\"", s),
                    LiteralValue::Boolean(b) => b.to_string(),
                    LiteralValue::Null => "null".to_string(),
                };
                return Err(format!(
                    "error: Expected struct \"{}\" but got {}\n    at {}:{}:{}",
                    expected_type, got_str, loc.file_path, loc.line, loc.col
                ));
            }
            _ => {}
        }
    } else {
        match expected_type {
            "string" => {
                match expr {
                    Expr::Literal(LiteralValue::String(_)) => {}
                    Expr::Literal(LiteralValue::Null) => {}
                    Expr::Literal(val) => {
                        let got_str = match val {
                            LiteralValue::Number(n) => n.to_string(),
                            LiteralValue::String(s) => format!("\"{}\"", s),
                            LiteralValue::Boolean(b) => b.to_string(),
                            LiteralValue::Null => "null".to_string(),
                        };
                        return Err(format!(
                            "error: Expected type \"string\" but got {}\n    at {}:{}:{}",
                            got_str, loc.file_path, loc.line, loc.col
                        ));
                    }
                    _ => {}
                }
            }
            "int" | "number" | "float" => {
                match expr {
                    Expr::Literal(LiteralValue::Number(_)) => {}
                    Expr::Literal(LiteralValue::Null) => {}
                    Expr::Literal(val) => {
                        let got_str = match val {
                            LiteralValue::Number(n) => n.to_string(),
                            LiteralValue::String(s) => format!("\"{}\"", s),
                            LiteralValue::Boolean(b) => b.to_string(),
                            LiteralValue::Null => "null".to_string(),
                        };
                        return Err(format!(
                            "error: Expected type \"{}\" but got {}\n    at {}:{}:{}",
                            expected_type, got_str, loc.file_path, loc.line, loc.col
                        ));
                    }
                    _ => {}
                }
            }
            "bool" | "boolean" => {
                match expr {
                    Expr::Literal(LiteralValue::Boolean(_)) => {}
                    Expr::Literal(LiteralValue::Null) => {}
                    Expr::Literal(val) => {
                        let got_str = match val {
                            LiteralValue::Number(n) => n.to_string(),
                            LiteralValue::String(s) => format!("\"{}\"", s),
                            LiteralValue::Boolean(b) => b.to_string(),
                            LiteralValue::Null => "null".to_string(),
                        };
                        return Err(format!(
                            "error: Expected type \"{}\" but got {}\n    at {}:{}:{}",
                            expected_type, got_str, loc.file_path, loc.line, loc.col
                        ));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}
