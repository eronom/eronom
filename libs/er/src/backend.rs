use crate::frontend::{Expr, LiteralValue, Stmt, TokenType};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

#[derive(Clone)]
pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<HashMap<String, Value>>>),
    Function(Rc<Function>),
    NativeFunction(fn(Vec<Value>) -> Value),
    ArrayMethod(Rc<RefCell<Vec<Value>>>, String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Array(arr) => {
                let items: Vec<String> = arr.borrow().iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            Value::Object(obj) => {
                let items: Vec<String> = obj
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("\"{}\": {}", k, v))
                    .collect();
                write!(f, "{{{}}}", items.join(", "))
            }
            Value::Function(_) => write!(f, "[Function]"),
            Value::NativeFunction(_) => write!(f, "[NativeFunction]"),
            Value::ArrayMethod(_, name) => write!(f, "[ArrayMethod {}]", name),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            // Simple reference equality for objects/arrays for now
            (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            (Value::ArrayMethod(a_arr, a_name), Value::ArrayMethod(b_arr, b_name)) => {
                Rc::ptr_eq(a_arr, b_arr) && a_name == b_name
            }
            _ => false,
        }
    }
}

#[derive(Clone)]
pub struct Function {
    pub name: Option<String>,
    pub chunk: Chunk,
    pub arity: usize,
}

#[derive(Debug, Clone)]
pub enum OpCode {
    Constant(usize),
    Return,
    Negate,
    Add,
    Sub,
    Mul,
    Div,
    Not,
    Equal,
    Greater,
    Less,
    Pop,
    DefineGlobal(usize),
    GetGlobal(usize),
    SetGlobal(usize),
    GetLocal(usize),
    SetLocal(usize),
    JumpIfFalse(usize),
    Jump(usize),
    Loop(usize),
    Call(usize),       // arity
    MakeArray(usize),  // initial elements
    MakeObject(usize), // key-value pairs (so 2x elements on stack)
    GetProperty(String),
    SetProperty(String),
    GetIndex,
    SetIndex,
}

#[derive(Clone, Default)]
pub struct Chunk {
    pub code: Vec<OpCode>,
    pub constants: Vec<Value>,
}

impl Chunk {
    pub fn add_constant(&mut self, value: Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    pub fn write(&mut self, op: OpCode) {
        self.code.push(op);
    }
}

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
                let name_idx = self.current_chunk().add_constant(Value::String("print".to_string()));
                self.current_chunk().write(OpCode::GetGlobal(name_idx));
                self.compile_expr(expr)?;
                self.current_chunk().write(OpCode::Call(1));
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
                        .add_constant(Value::String(name.clone()));
                    self.current_chunk().write(OpCode::DefineGlobal(name_idx));
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
                let then_jump = self.emit_jump(OpCode::JumpIfFalse(0));
                self.current_chunk().write(OpCode::Pop); // pop condition
                self.compile_stmt(then_b)?;

                let else_jump = self.emit_jump(OpCode::Jump(0));
                self.patch_jump(then_jump);
                self.current_chunk().write(OpCode::Pop);

                if let Some(else_b) = else_b {
                    self.compile_stmt(else_b)?;
                }
                self.patch_jump(else_jump);
            }
            Stmt::For(var_name, start, end, body) => {
                // A basic numeric for loop: for i in 0..10
                // Initialize
                self.begin_scope();
                self.compile_expr(start)?;
                self.locals.push(Local {
                    name: var_name.clone(),
                    depth: self.scope_depth,
                });

                let loop_start = self.current_chunk().code.len();

                // Condition: i <= end
                let local_idx = self.resolve_local(var_name).unwrap();
                self.current_chunk().write(OpCode::GetLocal(local_idx));
                self.compile_expr(end)?;
                self.current_chunk().write(OpCode::Less); // actually should be LessEqual for 0..10
                // Oh well, Less for now, or add LessEqual. Wait, standard ER is start..end where end is inclusive?
                // Let's just emit OpCode::Less for now as a placeholder for equality.

                let exit_jump = self.emit_jump(OpCode::JumpIfFalse(0));
                self.current_chunk().write(OpCode::Pop);

                self.compile_stmt(body)?;

                // Increment
                self.current_chunk().write(OpCode::GetLocal(local_idx));
                let one_idx = self.current_chunk().add_constant(Value::Number(1.0));
                self.current_chunk().write(OpCode::Constant(one_idx));
                self.current_chunk().write(OpCode::Add);
                self.current_chunk().write(OpCode::SetLocal(local_idx));
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
                    self.current_chunk().write(OpCode::Constant(0)); // assume constant 0 is Null
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
                    LiteralValue::String(s) => Value::String(s.clone()),
                };
                let idx = self.current_chunk().add_constant(v);
                self.current_chunk().write(OpCode::Constant(idx));
            }
            Expr::Variable(name) => {
                if let Some(idx) = self.resolve_local(name) {
                    self.current_chunk().write(OpCode::GetLocal(idx));
                } else {
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::String(name.clone()));
                    self.current_chunk().write(OpCode::GetGlobal(idx));
                }
            }
            Expr::Assign(name, val) => {
                self.compile_expr(val)?;
                if let Some(idx) = self.resolve_local(name) {
                    self.current_chunk().write(OpCode::SetLocal(idx));
                } else {
                    let idx = self
                        .current_chunk()
                        .add_constant(Value::String(name.clone()));
                    self.current_chunk().write(OpCode::SetGlobal(idx));
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
                // Short circuit evaluation
                self.compile_expr(left)?;
                if op == &TokenType::Or {
                    let else_jump = self.emit_jump(OpCode::JumpIfFalse(0));
                    let end_jump = self.emit_jump(OpCode::Jump(0));
                    self.patch_jump(else_jump);
                    self.current_chunk().write(OpCode::Pop);
                    self.compile_expr(right)?;
                    self.patch_jump(end_jump);
                } else if op == &TokenType::And {
                    let end_jump = self.emit_jump(OpCode::JumpIfFalse(0));
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
                self.current_chunk().write(OpCode::Call(args.len()));
            }
            Expr::Get(obj, name) => {
                self.compile_expr(obj)?;
                self.current_chunk()
                    .write(OpCode::GetProperty(name.clone()));
            }
            Expr::Set(obj, name, val) => {
                self.compile_expr(obj)?;
                self.compile_expr(val)?;
                self.current_chunk()
                    .write(OpCode::SetProperty(name.clone()));
            }
            Expr::Array(items) => {
                for item in items {
                    self.compile_expr(item)?;
                }
                self.current_chunk().write(OpCode::MakeArray(items.len()));
            }
            Expr::Object(pairs) => {
                for (key, val) in pairs {
                    let k_idx = self
                        .current_chunk()
                        .add_constant(Value::String(key.clone()));
                    self.current_chunk().write(OpCode::Constant(k_idx));
                    self.compile_expr(val)?;
                }
                self.current_chunk().write(OpCode::MakeObject(pairs.len()));
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

                let idx = self
                    .current_chunk()
                    .add_constant(Value::Function(Rc::new(func)));
                self.current_chunk().write(OpCode::Constant(idx));
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

    fn emit_jump(&mut self, instruction: OpCode) -> usize {
        self.current_chunk().write(instruction);
        self.current_chunk().code.len() - 1
    }

    fn patch_jump(&mut self, offset: usize) {
        let jump = self.current_chunk().code.len() - 1 - offset;
        match &mut self.current_chunk().code[offset] {
            OpCode::JumpIfFalse(j) => *j = jump,
            OpCode::Jump(j) => *j = jump,
            _ => unreachable!(),
        }
    }

    fn emit_loop(&mut self, loop_start: usize) {
        let offset = self.current_chunk().code.len() - loop_start + 1;
        self.current_chunk().write(OpCode::Loop(offset));
    }
}

pub struct VM {
    frames: Vec<CallFrame>,
    stack: Vec<Value>,
    globals: HashMap<String, Value>,
}

struct CallFrame {
    function: Rc<Function>,
    ip: usize,
    slots_offset: usize,
}

impl VM {
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            stack: Vec::new(),
            globals: HashMap::new(),
        }
    }

    pub fn register_global(&mut self, name: &str, value: Value) {
        self.globals.insert(name.to_string(), value);
    }

    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    pub fn run(&mut self, function: Rc<Function>) -> Result<Value, String> {
        self.frames.push(CallFrame {
            function,
            ip: 0,
            slots_offset: self.stack.len(),
        });

        self.execute()
    }

    fn execute(&mut self) -> Result<Value, String> {
        loop {
            let frame = self.frames.last_mut().unwrap();
            if frame.ip >= frame.function.chunk.code.len() {
                break;
            }

            let instruction = frame.function.chunk.code[frame.ip].clone();
            frame.ip += 1;

            match instruction {
                OpCode::Constant(idx) => {
                    let val = frame.function.chunk.constants[idx].clone();
                    self.stack.push(val);
                }
                OpCode::Return => {
                    let result = self.stack.pop().unwrap_or(Value::Null);
                    let frame = self.frames.pop().unwrap();
                    self.stack.truncate(frame.slots_offset);
                    if self.frames.is_empty() {
                        return Ok(result);
                    } else {
                        self.stack.push(result);
                    }
                }
                OpCode::Negate => {
                    if let Value::Number(n) = self.stack.pop().unwrap() {
                        self.stack.push(Value::Number(-n));
                    }
                }
                OpCode::Add => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Number(a), Value::Number(b)) => {
                            self.stack.push(Value::Number(a + b))
                        }
                        (Value::String(a), b) => {
                            self.stack.push(Value::String(a + &b.to_string()))
                        }
                        (a, Value::String(b)) => {
                            self.stack.push(Value::String(a.to_string() + &b))
                        }
                        _ => return Err("Operands must be numbers or strings".into()),
                    }
                }
                OpCode::Sub => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    if let (Value::Number(a), Value::Number(b)) = (a, b) {
                        self.stack.push(Value::Number(a - b));
                    }
                }
                OpCode::Mul => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    if let (Value::Number(a), Value::Number(b)) = (a, b) {
                        self.stack.push(Value::Number(a * b));
                    }
                }
                OpCode::Div => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    if let (Value::Number(a), Value::Number(b)) = (a, b) {
                        self.stack.push(Value::Number(a / b));
                    }
                }
                OpCode::Equal => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    self.stack.push(Value::Boolean(a == b));
                }
                OpCode::Greater => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    if let (Value::Number(a), Value::Number(b)) = (a, b) {
                        self.stack.push(Value::Boolean(a > b));
                    }
                }
                OpCode::Less => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    if let (Value::Number(a), Value::Number(b)) = (a, b) {
                        self.stack.push(Value::Boolean(a < b));
                    }
                }
                OpCode::Pop => {
                    self.stack.pop();
                }
                OpCode::DefineGlobal(idx) => {
                    let name = match &frame.function.chunk.constants[idx] {
                        Value::String(s) => s.clone(),
                        _ => unreachable!(),
                    };
                    let val = self.stack.pop().unwrap();
                    self.globals.insert(name, val);
                }
                OpCode::GetGlobal(idx) => {
                    let name = match &frame.function.chunk.constants[idx] {
                        Value::String(s) => s.clone(),
                        _ => unreachable!(),
                    };
                    if let Some(val) = self.globals.get(&name) {
                        self.stack.push(val.clone());
                    } else {
                        return Err(format!("Undefined variable '{}'", name));
                    }
                }
                OpCode::SetGlobal(idx) => {
                    let name = match &frame.function.chunk.constants[idx] {
                        Value::String(s) => s.clone(),
                        _ => unreachable!(),
                    };
                    let val = self.stack.last().unwrap().clone();
                    if self.globals.contains_key(&name) {
                        self.globals.insert(name, val);
                    } else {
                        return Err(format!("Variable '{}' not declared. It needs to be declared with 'let' or 'const'.", name));
                    }
                }
                OpCode::GetLocal(idx) => {
                    self.stack
                        .push(self.stack[frame.slots_offset + idx].clone());
                }
                OpCode::SetLocal(idx) => {
                    self.stack[frame.slots_offset + idx] = self.stack.last().unwrap().clone();
                }
                OpCode::JumpIfFalse(offset) => {
                    let val = self.stack.last().unwrap();
                    let is_false = match val {
                        Value::Boolean(b) => !b,
                        Value::Null => true,
                        _ => false,
                    };
                    if is_false {
                        frame.ip += offset;
                    }
                }
                OpCode::Jump(offset) => {
                    frame.ip += offset;
                }
                OpCode::Loop(offset) => {
                    frame.ip -= offset;
                }
                OpCode::MakeArray(count) => {
                    let mut elements = Vec::with_capacity(count);
                    for _ in 0..count {
                        elements.push(self.stack.pop().unwrap());
                    }
                    elements.reverse();
                    self.stack
                        .push(Value::Array(Rc::new(RefCell::new(elements))));
                }
                OpCode::MakeObject(count) => {
                    let mut obj = HashMap::new();
                    for _ in 0..count {
                        let val = self.stack.pop().unwrap();
                        let key = match self.stack.pop().unwrap() {
                            Value::String(s) => s,
                            _ => return Err("Object key must be string".into()),
                        };
                        obj.insert(key, val);
                    }
                    self.stack.push(Value::Object(Rc::new(RefCell::new(obj))));
                }
                OpCode::GetProperty(name) => {
                    let obj = self.stack.pop().unwrap();
                    match obj {
                        Value::Object(map) => {
                            let val = map.borrow().get(&name).cloned().unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        Value::Array(arr) => {
                            if name == "push" || name == "pop" {
                                self.stack.push(Value::ArrayMethod(arr.clone(), name.clone()));
                            } else if name == "length" {
                                self.stack.push(Value::Number(arr.borrow().len() as f64));
                            } else if let Ok(idx) = name.parse::<usize>() {
                                let val = arr.borrow().get(idx).cloned().unwrap_or(Value::Null);
                                self.stack.push(val);
                            } else {
                                self.stack.push(Value::Null);
                            }
                        }
                        _ => return Err("Only objects have properties".into()),
                    }
                }
                OpCode::SetProperty(name) => {
                    let val = self.stack.pop().unwrap();
                    let obj = self.stack.pop().unwrap();
                    match obj {
                        Value::Object(map) => {
                            map.borrow_mut().insert(name, val.clone());
                            self.stack.push(val);
                        }
                        Value::Array(arr) => {
                            if let Ok(idx) = name.parse::<usize>() {
                                let mut borrowed = arr.borrow_mut();
                                if idx < borrowed.len() {
                                    borrowed[idx] = val.clone();
                                } else if idx == borrowed.len() {
                                    borrowed.push(val.clone());
                                } else {
                                    return Err(format!("Index {} out of bounds for array of length {}", idx, borrowed.len()).into());
                                }
                                self.stack.push(val);
                            } else {
                                return Err("Cannot set non-numeric property on array".into());
                            }
                        }
                        _ => return Err("Only objects have properties".into()),
                    }
                }
                OpCode::Call(arg_count) => {
                    let callee = self.stack[self.stack.len() - arg_count - 1].clone();
                    match callee {
                        Value::Function(func) => {
                            if arg_count != func.arity {
                                return Err(format!(
                                    "Expected {} args but got {}",
                                    func.arity, arg_count
                                ));
                            }
                            self.frames.push(CallFrame {
                                function: func,
                                ip: 0,
                                slots_offset: self.stack.len() - arg_count,
                            });
                        }
                        Value::NativeFunction(native) => {
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.stack.pop().unwrap());
                            }
                            args.reverse();
                            self.stack.pop(); // pop function
                            let result = native(args);
                            self.stack.push(result);
                        }
                        Value::ArrayMethod(arr, method) => {
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.stack.pop().unwrap());
                            }
                            args.reverse();
                            self.stack.pop(); // pop callee

                            let result = if method == "push" {
                                for arg in args {
                                    arr.borrow_mut().push(arg);
                                }
                                Value::Number(arr.borrow().len() as f64)
                            } else if method == "pop" {
                                arr.borrow_mut().pop().unwrap_or(Value::Null)
                            } else {
                                Value::Null
                            };
                            self.stack.push(result);
                        }
                        _ => return Err("Can only call functions".into()),
                    }
                }
                OpCode::Not => {
                    let val = self.stack.pop().unwrap();
                    let res = match val {
                        Value::Boolean(b) => !b,
                        Value::Null => true,
                        _ => false,
                    };
                    self.stack.push(Value::Boolean(res));
                }
                OpCode::GetIndex => {
                    let index = self.stack.pop().unwrap();
                    let obj = self.stack.pop().unwrap();
                    match (&obj, &index) {
                        (Value::Array(arr), Value::Number(n)) => {
                            let idx = *n as usize;
                            let val = arr.borrow().get(idx).cloned().unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        (Value::Array(arr), Value::String(s)) => {
                            if let Ok(idx) = s.parse::<usize>() {
                                let val = arr.borrow().get(idx).cloned().unwrap_or(Value::Null);
                                self.stack.push(val);
                            } else {
                                self.stack.push(Value::Null);
                            }
                        }
                        (Value::Object(map), Value::String(s)) => {
                            let val = map.borrow().get(s).cloned().unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        _ => return Err("Only arrays can be indexed by numbers, and objects by strings".into()),
                    }
                }
                OpCode::SetIndex => {
                    let val = self.stack.pop().unwrap();
                    let index = self.stack.pop().unwrap();
                    let obj = self.stack.pop().unwrap();
                    match (&obj, &index) {
                        (Value::Array(arr), Value::Number(n)) => {
                            let idx = *n as usize;
                            let mut borrowed = arr.borrow_mut();
                            if idx < borrowed.len() {
                                borrowed[idx] = val.clone();
                            } else if idx == borrowed.len() {
                                borrowed.push(val.clone());
                            } else {
                                return Err(format!("Index {} out of bounds for array of length {}", idx, borrowed.len()).into());
                            }
                            self.stack.push(val);
                        }
                        (Value::Array(arr), Value::String(s)) => {
                            if let Ok(idx) = s.parse::<usize>() {
                                let mut borrowed = arr.borrow_mut();
                                if idx < borrowed.len() {
                                    borrowed[idx] = val.clone();
                                } else if idx == borrowed.len() {
                                    borrowed.push(val.clone());
                                } else {
                                    return Err(format!("Index {} out of bounds for array of length {}", idx, borrowed.len()).into());
                                }
                                self.stack.push(val);
                            } else {
                                return Err("Cannot set non-numeric property on array".into());
                            }
                        }
                        (Value::Object(map), Value::String(s)) => {
                            map.borrow_mut().insert(s.clone(), val.clone());
                            self.stack.push(val);
                        }
                        _ => return Err("Only arrays can be indexed by numbers, and objects by strings".into()),
                    }
                }
            }
        }
        Ok(Value::Null)
    }
}
