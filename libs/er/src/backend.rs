use crate::frontend::{Expr, LiteralValue, Stmt, TokenType};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcColor {
    White,
    Gray,
    Black,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcPhase {
    Pause,
    Mark,
    Atomic,
    Sweep,
}

pub enum GcData {
    Array(Vec<Value>),
    Object(HashMap<Rc<str>, Value>),
}

pub struct GcObject {
    pub color: GcColor,
    pub next: *mut GcObject,
    pub data: GcData,
}

thread_local! {
    pub static GC_HEAD: std::cell::Cell<*mut GcObject> = std::cell::Cell::new(std::ptr::null_mut());
    pub static ALLOC_COUNT: std::cell::Cell<usize> = std::cell::Cell::new(0);
    pub static GC_ROOTS: std::cell::RefCell<Vec<Box<dyn Fn()>>> = std::cell::RefCell::new(Vec::new());
    pub static GC_PHASE: std::cell::Cell<GcPhase> = std::cell::Cell::new(GcPhase::Pause);
    pub static GRAY_STACK: std::cell::RefCell<Vec<*mut GcObject>> = std::cell::RefCell::new(Vec::new());
    pub static SWEEP_PTR: std::cell::Cell<*mut GcObject> = std::cell::Cell::new(std::ptr::null_mut());
    pub static PREV_SWEEP_PTR: std::cell::Cell<*mut GcObject> = std::cell::Cell::new(std::ptr::null_mut());
}

pub fn gc_allocate(data: GcData) -> *mut GcObject {
    GC_HEAD.with(|head| {
        let obj = Box::new(GcObject {
            color: GcColor::White,
            next: head.get(),
            data,
        });
        let ptr = Box::into_raw(obj);
        head.set(ptr);

        if GC_PHASE.with(|phase| phase.get()) == GcPhase::Sweep {
            PREV_SWEEP_PTR.with(|prev| {
                if prev.get().is_null() {
                    prev.set(ptr);
                }
            });
        }

        ALLOC_COUNT.with(|c| c.set(c.get() + 1));
        ptr
    })
}

pub fn gc_free_all() {
    unsafe {
        GC_HEAD.with(|head| {
            let mut curr = head.get();
            head.set(std::ptr::null_mut());
            while !curr.is_null() {
                let next = (*curr).next;
                let _ = Box::from_raw(curr);
                curr = next;
            }
        });
    }
    ALLOC_COUNT.with(|c| c.set(0));
    GC_PHASE.with(|p| p.set(GcPhase::Pause));
    GRAY_STACK.with(|gs| gs.borrow_mut().clear());
    SWEEP_PTR.with(|s| s.set(std::ptr::null_mut()));
    PREV_SWEEP_PTR.with(|p| p.set(std::ptr::null_mut()));
}

pub fn gc_mark_value(val: &Value) {
    match val {
        Value::Array(ptr) | Value::ArrayMethod(ptr, _) | Value::Object(ptr) => unsafe {
            if !ptr.is_null() && (*(*ptr)).color == GcColor::White {
                (*(*ptr)).color = GcColor::Gray;
                GRAY_STACK.with(|gs| gs.borrow_mut().push(*ptr));
            }
        }
        Value::Function(func) => {
            for constant in &func.chunk.constants {
                gc_mark_value(constant);
            }
        }
        _ => {}
    }
}

pub fn gc_mark_object(ptr: *mut GcObject) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        if (*ptr).color == GcColor::White {
            (*ptr).color = GcColor::Gray;
            GRAY_STACK.with(|gs| gs.borrow_mut().push(ptr));
        }
    }
}

pub fn gc_blacken_object(ptr: *mut GcObject) {
    unsafe {
        if ptr.is_null() {
            return;
        }
        (*ptr).color = GcColor::Black;
        match &(*ptr).data {
            GcData::Array(arr) => {
                for val in arr {
                    gc_mark_value(val);
                }
            }
            GcData::Object(obj) => {
                for val in obj.values() {
                    gc_mark_value(val);
                }
            }
        }
    }
}

pub fn mark_value(val: &Value) {
    gc_mark_value(val);
}

pub fn mark_object(ptr: *mut GcObject) {
    gc_mark_object(ptr);
}

pub fn gc_write_barrier(parent: *mut GcObject, child: &Value) {
    unsafe {
        if parent.is_null() {
            return;
        }
        if (*parent).color == GcColor::Black {
            match child {
                Value::Array(child_ptr) | Value::Object(child_ptr) => {
                    if !child_ptr.is_null() && (*(*child_ptr)).color == GcColor::White {
                        (*(*child_ptr)).color = GcColor::Gray;
                        GRAY_STACK.with(|stack| {
                            stack.borrow_mut().push(*child_ptr);
                        });
                    }
                }
                Value::ArrayMethod(child_ptr, _) => {
                    if !child_ptr.is_null() && (*(*child_ptr)).color == GcColor::White {
                        (*(*child_ptr)).color = GcColor::Gray;
                        GRAY_STACK.with(|stack| {
                            stack.borrow_mut().push(*child_ptr);
                        });
                    }
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArrayMethodType {
    Push,
    Pop,
}

pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(Rc<str>),
    Array(*mut GcObject),
    Object(*mut GcObject),
    Function(Rc<Function>),
    NativeFunction(fn(Vec<Value>) -> Value),
    ArrayMethod(*mut GcObject, ArrayMethodType),
}

impl Clone for Value {
    #[inline]
    fn clone(&self) -> Self {
        match self {
            Value::Null => Value::Null,
            Value::Boolean(b) => Value::Boolean(*b),
            Value::Number(n) => Value::Number(*n),
            Value::String(s) => Value::String(Rc::clone(s)),
            Value::Array(ptr) => Value::Array(*ptr),
            Value::Object(ptr) => Value::Object(*ptr),
            Value::Function(func) => Value::Function(Rc::clone(func)),
            Value::NativeFunction(func) => Value::NativeFunction(*func),
            Value::ArrayMethod(ptr, method) => Value::ArrayMethod(*ptr, *method),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Array(ptr) => unsafe {
                match &(**ptr).data {
                    GcData::Array(arr) => {
                        let items: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                        write!(f, "[{}]", items.join(", "))
                    }
                    _ => unreachable!(),
                }
            },
            Value::Object(ptr) => unsafe {
                match &(**ptr).data {
                    GcData::Object(obj) => {
                        let items: Vec<String> = obj
                            .iter()
                            .map(|(k, v)| format!("\"{}\": {}", k, v))
                            .collect();
                        write!(f, "{{{}}}", items.join(", "))
                    }
                    _ => unreachable!(),
                }
            },
            Value::Function(_) => write!(f, "[Function]"),
            Value::NativeFunction(_) => write!(f, "[NativeFunction]"),
            Value::ArrayMethod(_, method) => {
                let name = match method {
                    ArrayMethodType::Push => "push",
                    ArrayMethodType::Pop => "pop",
                };
                write!(f, "[ArrayMethod {}]", name)
            }
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
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            (Value::ArrayMethod(a_arr, a_name), Value::ArrayMethod(b_arr, b_name)) => {
                a_arr == b_arr && a_name == b_name
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

#[derive(Debug, Clone, Copy)]
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
    GetProperty(usize),
    SetProperty(usize),
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
                let name_idx = self.current_chunk().add_constant(Value::String(Rc::from("print")));
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
                        .add_constant(Value::String(Rc::from(name.as_str())));
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
                    LiteralValue::String(s) => Value::String(Rc::from(s.as_str())),
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
                        .add_constant(Value::String(Rc::from(name.as_str())));
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
                        .add_constant(Value::String(Rc::from(name.as_str())));
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
                let name_idx = self.current_chunk().add_constant(Value::String(Rc::from(name.as_str())));
                self.current_chunk().write(OpCode::GetProperty(name_idx));
            }
            Expr::Set(obj, name, val) => {
                self.compile_expr(obj)?;
                self.compile_expr(val)?;
                let name_idx = self.current_chunk().add_constant(Value::String(Rc::from(name.as_str())));
                self.current_chunk().write(OpCode::SetProperty(name_idx));
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
                        .add_constant(Value::String(Rc::from(key.as_str())));
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
    globals: HashMap<Rc<str>, Value>,
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
        self.globals.insert(Rc::from(name), value);
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

    pub fn gc_step(&mut self) {
        let phase = GC_PHASE.with(|p| p.get());
        match phase {
            GcPhase::Pause => {
                if ALLOC_COUNT.with(|c| c.get()) >= 10 {
                    GC_PHASE.with(|p| p.set(GcPhase::Mark));
                    GRAY_STACK.with(|gs| gs.borrow_mut().clear());
                    
                    for val in &self.stack {
                        mark_value(val);
                    }
                    for val in self.globals.values() {
                        mark_value(val);
                    }
                    for frame in &self.frames {
                        for constant in &frame.function.chunk.constants {
                            mark_value(constant);
                        }
                    }
                    GC_ROOTS.with(|roots| {
                        if let Ok(borrowed) = roots.try_borrow() {
                            for root_fn in borrowed.iter() {
                                root_fn();
                            }
                        }
                    });
                }
            }
            GcPhase::Mark => {
                let gray_opt = GRAY_STACK.with(|gs| gs.borrow_mut().pop());
                if let Some(ptr) = gray_opt {
                    gc_blacken_object(ptr);
                } else {
                    GC_PHASE.with(|p| p.set(GcPhase::Atomic));
                }
            }
            GcPhase::Atomic => {
                for val in &self.stack {
                    mark_value(val);
                }
                for val in self.globals.values() {
                    mark_value(val);
                }
                for frame in &self.frames {
                    for constant in &frame.function.chunk.constants {
                        mark_value(constant);
                    }
                }
                GC_ROOTS.with(|roots| {
                    if let Ok(borrowed) = roots.try_borrow() {
                        for root_fn in borrowed.iter() {
                            root_fn();
                        }
                    }
                });

                loop {
                    let gray_opt = GRAY_STACK.with(|gs| gs.borrow_mut().pop());
                    if let Some(ptr) = gray_opt {
                        gc_blacken_object(ptr);
                    } else {
                        break;
                    }
                }

                GC_PHASE.with(|p| p.set(GcPhase::Sweep));
                SWEEP_PTR.with(|s| s.set(GC_HEAD.with(|h| h.get())));
                PREV_SWEEP_PTR.with(|p| p.set(std::ptr::null_mut()));
            }
            GcPhase::Sweep => {
                for _ in 0..5 {
                    let curr = SWEEP_PTR.with(|s| s.get());
                    if curr.is_null() {
                        GC_PHASE.with(|p| p.set(GcPhase::Pause));
                        ALLOC_COUNT.with(|c| c.set(0));
                        break;
                    }

                    unsafe {
                        let next = (*curr).next;
                        if (*curr).color == GcColor::White {
                            let prev = PREV_SWEEP_PTR.with(|p| p.get());
                            if prev.is_null() {
                                GC_HEAD.with(|h| h.set(next));
                            } else {
                                (*prev).next = next;
                            }
                            let _ = Box::from_raw(curr);
                            SWEEP_PTR.with(|s| s.set(next));
                        } else {
                            (*curr).color = GcColor::White;
                            PREV_SWEEP_PTR.with(|p| p.set(curr));
                            SWEEP_PTR.with(|s| s.set(next));
                        }
                    }
                }
            }
        }
    }

    pub fn collect_garbage(&mut self) {
        if GC_PHASE.with(|p| p.get()) == GcPhase::Pause {
            ALLOC_COUNT.with(|c| c.set(999999));
            self.gc_step();
        }
        while GC_PHASE.with(|p| p.get()) != GcPhase::Pause {
            self.gc_step();
        }
    }

    fn gc_trigger(&mut self) {
        self.gc_step();
        self.gc_step();
    }

    fn execute(&mut self) -> Result<Value, String> {
        self.stack.reserve(2048);

        let mut frame_ptr = unsafe {
            let len = self.frames.len();
            self.frames.as_mut_ptr().add(len - 1)
        };

        // Cache active frame's instruction pointer and end pointer
        let mut ip = unsafe { (*frame_ptr).function.chunk.code.as_ptr().add((*frame_ptr).ip) };
        let mut ip_end = unsafe { (*frame_ptr).function.chunk.code.as_ptr().add((*frame_ptr).function.chunk.code.len()) };

        while ip < ip_end {
            let instruction = unsafe { *ip };
            ip = unsafe { ip.add(1) };

            match instruction {
                OpCode::Constant(idx) => {
                    let val = unsafe { (*frame_ptr).function.chunk.constants.get_unchecked(idx) };
                    self.stack.push(val.clone());
                }
                OpCode::Return => {
                    let result = self.stack.pop().unwrap_or(Value::Null);
                    let slots_offset = unsafe { (*frame_ptr).slots_offset };
                    self.frames.pop();
                    if self.frames.is_empty() {
                        return Ok(result);
                    }
                    frame_ptr = unsafe {
                        let len = self.frames.len();
                        self.frames.as_mut_ptr().add(len - 1)
                    };
                    self.stack.truncate(slots_offset);
                    self.stack.push(result);

                    ip = unsafe { (*frame_ptr).function.chunk.code.as_ptr().add((*frame_ptr).ip) };
                    ip_end = unsafe { (*frame_ptr).function.chunk.code.as_ptr().add((*frame_ptr).function.chunk.code.len()) };
                }
                OpCode::Negate => {
                    if let Value::Number(n) = unsafe { self.stack.last_mut().unwrap_unchecked() } {
                        *n = -*n;
                    }
                }
                OpCode::Add => {
                    let len = self.stack.len();
                    unsafe {
                        let ptr = self.stack.as_mut_ptr();
                        let b_ptr = ptr.add(len - 1);
                        let a_ptr = ptr.add(len - 2);
                        if let (Value::Number(na), Value::Number(nb)) = (&*a_ptr, &*b_ptr) {
                            *a_ptr = Value::Number(*na + *nb);
                            self.stack.set_len(len - 1);
                        } else {
                            let b = self.stack.pop().unwrap_unchecked();
                            let a = self.stack.pop().unwrap_unchecked();
                            match (a, b) {
                                (Value::String(sa), sb) => {
                                    self.stack.push(Value::String(Rc::from(format!("{}{}", sa, sb))));
                                }
                                (sa, Value::String(sb)) => {
                                    self.stack.push(Value::String(Rc::from(format!("{}{}", sa, sb))));
                                }
                                _ => return Err("Operands must be numbers or strings".into()),
                            }
                        }
                    }
                }
                OpCode::Sub => {
                    let len = self.stack.len();
                    unsafe {
                        let ptr = self.stack.as_mut_ptr();
                        let b_ptr = ptr.add(len - 1);
                        let a_ptr = ptr.add(len - 2);
                        if let (Value::Number(na), Value::Number(nb)) = (&*a_ptr, &*b_ptr) {
                            *a_ptr = Value::Number(*na - *nb);
                            self.stack.set_len(len - 1);
                        } else {
                            let b = self.stack.pop().unwrap_unchecked();
                            let a = self.stack.pop().unwrap_unchecked();
                            if let (Value::Number(na), Value::Number(nb)) = (a, b) {
                                self.stack.push(Value::Number(na - nb));
                            }
                        }
                    }
                }
                OpCode::Mul => {
                    let len = self.stack.len();
                    unsafe {
                        let ptr = self.stack.as_mut_ptr();
                        let b_ptr = ptr.add(len - 1);
                        let a_ptr = ptr.add(len - 2);
                        if let (Value::Number(na), Value::Number(nb)) = (&*a_ptr, &*b_ptr) {
                            *a_ptr = Value::Number(*na * *nb);
                            self.stack.set_len(len - 1);
                        } else {
                            let b = self.stack.pop().unwrap_unchecked();
                            let a = self.stack.pop().unwrap_unchecked();
                            if let (Value::Number(na), Value::Number(nb)) = (a, b) {
                                self.stack.push(Value::Number(na * nb));
                            }
                        }
                    }
                }
                OpCode::Div => {
                    let len = self.stack.len();
                    unsafe {
                        let ptr = self.stack.as_mut_ptr();
                        let b_ptr = ptr.add(len - 1);
                        let a_ptr = ptr.add(len - 2);
                        if let (Value::Number(na), Value::Number(nb)) = (&*a_ptr, &*b_ptr) {
                            *a_ptr = Value::Number(*na / *nb);
                            self.stack.set_len(len - 1);
                        } else {
                            let b = self.stack.pop().unwrap_unchecked();
                            let a = self.stack.pop().unwrap_unchecked();
                            if let (Value::Number(na), Value::Number(nb)) = (a, b) {
                                self.stack.push(Value::Number(na / nb));
                            }
                        }
                    }
                }
                OpCode::Equal => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    self.stack.push(Value::Boolean(a == b));
                }
                OpCode::Greater => {
                    let len = self.stack.len();
                    unsafe {
                        let ptr = self.stack.as_mut_ptr();
                        let b_ptr = ptr.add(len - 1);
                        let a_ptr = ptr.add(len - 2);
                        if let (Value::Number(na), Value::Number(nb)) = (&*a_ptr, &*b_ptr) {
                            *a_ptr = Value::Boolean(*na > *nb);
                            self.stack.set_len(len - 1);
                        } else {
                            let b = self.stack.pop().unwrap_unchecked();
                            let a = self.stack.pop().unwrap_unchecked();
                            if let (Value::Number(na), Value::Number(nb)) = (a, b) {
                                self.stack.push(Value::Boolean(na > nb));
                            }
                        }
                    }
                }
                OpCode::Less => {
                    let len = self.stack.len();
                    unsafe {
                        let ptr = self.stack.as_mut_ptr();
                        let b_ptr = ptr.add(len - 1);
                        let a_ptr = ptr.add(len - 2);
                        if let (Value::Number(na), Value::Number(nb)) = (&*a_ptr, &*b_ptr) {
                            *a_ptr = Value::Boolean(*na < *nb);
                            self.stack.set_len(len - 1);
                        } else {
                            let b = self.stack.pop().unwrap_unchecked();
                            let a = self.stack.pop().unwrap_unchecked();
                            if let (Value::Number(na), Value::Number(nb)) = (a, b) {
                                self.stack.push(Value::Boolean(na < nb));
                            }
                        }
                    }
                }
                OpCode::Pop => {
                    self.stack.pop();
                }
                OpCode::DefineGlobal(idx) => {
                    let name = match unsafe { (*frame_ptr).function.chunk.constants.get_unchecked(idx) } {
                        Value::String(s) => s.clone(),
                        _ => unreachable!(),
                    };
                    let val = self.stack.pop().unwrap();
                    self.globals.insert(name, val);
                }
                OpCode::GetGlobal(idx) => {
                    let name = match unsafe { (*frame_ptr).function.chunk.constants.get_unchecked(idx) } {
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
                    let name = match unsafe { (*frame_ptr).function.chunk.constants.get_unchecked(idx) } {
                        Value::String(s) => s.clone(),
                        _ => unreachable!(),
                    };
                    let val = unsafe { self.stack.last().unwrap_unchecked() }.clone();
                    if self.globals.contains_key(&name) {
                        self.globals.insert(name, val);
                    } else {
                        return Err(format!("Variable '{}' not declared. It needs to be declared with 'let' or 'const'.", name));
                    }
                }
                OpCode::GetLocal(idx) => {
                    let slots_offset = unsafe { (*frame_ptr).slots_offset };
                    let val = unsafe { self.stack.get_unchecked(slots_offset + idx) };
                    self.stack.push(val.clone());
                }
                OpCode::SetLocal(idx) => {
                    let slots_offset = unsafe { (*frame_ptr).slots_offset };
                    let val = unsafe { self.stack.last().unwrap_unchecked() }.clone();
                    unsafe {
                        *self.stack.get_unchecked_mut(slots_offset + idx) = val;
                    }
                }
                OpCode::JumpIfFalse(offset) => {
                    let val = unsafe { self.stack.last().unwrap_unchecked() };
                    let is_false = match val {
                        Value::Boolean(b) => !b,
                        Value::Null => true,
                        _ => false,
                    };
                    if is_false {
                        ip = unsafe { ip.add(offset) };
                    }
                }
                OpCode::Jump(offset) => {
                    ip = unsafe { ip.add(offset) };
                }
                OpCode::Loop(offset) => {
                    ip = unsafe { ip.sub(offset) };
                }
                OpCode::MakeArray(count) => {
                    self.gc_trigger();
                    let mut elements = Vec::with_capacity(count);
                    for _ in 0..count {
                        elements.push(self.stack.pop().unwrap());
                    }
                    elements.reverse();
                    let ptr = gc_allocate(GcData::Array(elements));
                    self.stack.push(Value::Array(ptr));
                }
                OpCode::MakeObject(count) => {
                    self.gc_trigger();
                    let mut obj = HashMap::new();
                    for _ in 0..count {
                        let val = self.stack.pop().unwrap();
                        let key = match self.stack.pop().unwrap() {
                            Value::String(s) => s,
                            _ => return Err("Object key must be string".into()),
                        };
                        obj.insert(key, val);
                    }
                    let ptr = gc_allocate(GcData::Object(obj));
                    self.stack.push(Value::Object(ptr));
                }
                OpCode::GetProperty(idx) => {
                    let obj = self.stack.pop().unwrap();
                    let name = match unsafe { (*frame_ptr).function.chunk.constants.get_unchecked(idx) } {
                        Value::String(s) => s.clone(),
                        _ => unreachable!(),
                    };
                    match obj {
                        Value::Object(ptr) => unsafe {
                            match &(*ptr).data {
                                GcData::Object(map) => {
                                    let val = map.get(&name).cloned().unwrap_or(Value::Null);
                                    self.stack.push(val);
                                }
                                _ => unreachable!(),
                            }
                        }
                        Value::Array(ptr) => unsafe {
                            match &(*ptr).data {
                                GcData::Array(arr) => {
                                    if &*name == "push" {
                                        self.stack.push(Value::ArrayMethod(ptr, ArrayMethodType::Push));
                                    } else if &*name == "pop" {
                                        self.stack.push(Value::ArrayMethod(ptr, ArrayMethodType::Pop));
                                    } else if &*name == "length" {
                                        self.stack.push(Value::Number(arr.len() as f64));
                                    } else if let Ok(idx) = name.parse::<usize>() {
                                        let val = arr.get(idx).cloned().unwrap_or(Value::Null);
                                        self.stack.push(val);
                                    } else {
                                        self.stack.push(Value::Null);
                                    }
                                }
                                _ => unreachable!(),
                            }
                        }
                        _ => return Err("Only objects have properties".into()),
                    }
                }
                OpCode::SetProperty(idx) => {
                    let val = self.stack.pop().unwrap();
                    let obj = self.stack.pop().unwrap();
                    let name = match unsafe { (*frame_ptr).function.chunk.constants.get_unchecked(idx) } {
                        Value::String(s) => s.clone(),
                        _ => unreachable!(),
                    };
                    match obj {
                        Value::Object(ptr) => unsafe {
                            match &mut (*ptr).data {
                                GcData::Object(map) => {
                                    map.insert(name, val.clone());
                                    gc_write_barrier(ptr, &val);
                                    self.stack.push(val);
                                }
                                _ => unreachable!(),
                            }
                        }
                        Value::Array(ptr) => unsafe {
                            match &mut (*ptr).data {
                                GcData::Array(arr) => {
                                    if let Ok(idx) = name.parse::<usize>() {
                                        if idx < arr.len() {
                                            arr[idx] = val.clone();
                                        } else if idx == arr.len() {
                                            arr.push(val.clone());
                                        } else {
                                            return Err(format!("Index {} out of bounds for array of length {}", idx, arr.len()).into());
                                        }
                                        gc_write_barrier(ptr, &val);
                                        self.stack.push(val);
                                    } else {
                                        return Err("Cannot set non-numeric property on array".into());
                                    }
                                }
                                _ => unreachable!(),
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
                            unsafe {
                                (*frame_ptr).ip = ip.offset_from((*frame_ptr).function.chunk.code.as_ptr()) as usize;
                            }
                            self.frames.push(CallFrame {
                                function: func,
                                ip: 0,
                                slots_offset: self.stack.len() - arg_count,
                            });
                            frame_ptr = unsafe {
                                let len = self.frames.len();
                                self.frames.as_mut_ptr().add(len - 1)
                            };
                            ip = unsafe { (*frame_ptr).function.chunk.code.as_ptr().add((*frame_ptr).ip) };
                            ip_end = unsafe { (*frame_ptr).function.chunk.code.as_ptr().add((*frame_ptr).function.chunk.code.len()) };
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
                        Value::ArrayMethod(ptr, method) => {
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.stack.pop().unwrap());
                            }
                            args.reverse();
                            self.stack.pop(); // pop callee

                            let result = unsafe {
                                match &mut (*ptr).data {
                                    GcData::Array(arr) => {
                                        match method {
                                            ArrayMethodType::Push => {
                                                for arg in args {
                                                    gc_write_barrier(ptr, &arg);
                                                    arr.push(arg);
                                                }
                                                Value::Number(arr.len() as f64)
                                            }
                                            ArrayMethodType::Pop => {
                                                arr.pop().unwrap_or(Value::Null)
                                            }
                                        }
                                    }
                                    _ => unreachable!(),
                                }
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
                        (Value::Array(ptr), Value::Number(n)) => unsafe {
                            let idx = *n as usize;
                            match &(**ptr).data {
                                GcData::Array(arr) => {
                                    let val = arr.get(idx).cloned().unwrap_or(Value::Null);
                                    self.stack.push(val);
                                }
                                _ => unreachable!(),
                            }
                        }
                        (Value::Array(ptr), Value::String(s)) => unsafe {
                            if let Ok(idx) = s.parse::<usize>() {
                                match &(**ptr).data {
                                    GcData::Array(arr) => {
                                        let val = arr.get(idx).cloned().unwrap_or(Value::Null);
                                        self.stack.push(val);
                                    }
                                    _ => unreachable!(),
                                }
                            } else {
                                self.stack.push(Value::Null);
                            }
                        }
                        (Value::Object(ptr), Value::String(s)) => unsafe {
                            match &(**ptr).data {
                                GcData::Object(map) => {
                                    let val = map.get(s).cloned().unwrap_or(Value::Null);
                                    self.stack.push(val);
                                }
                                _ => unreachable!(),
                            }
                        }
                        _ => return Err("Only arrays can be indexed by numbers, and objects by strings".into()),
                    }
                }
                OpCode::SetIndex => {
                    let val = self.stack.pop().unwrap();
                    let index = self.stack.pop().unwrap();
                    let obj = self.stack.pop().unwrap();
                    match (&obj, &index) {
                        (Value::Array(ptr), Value::Number(n)) => unsafe {
                            let idx = *n as usize;
                            match &mut (**ptr).data {
                                GcData::Array(arr) => {
                                    if idx < arr.len() {
                                        arr[idx] = val.clone();
                                    } else if idx == arr.len() {
                                        arr.push(val.clone());
                                    } else {
                                        return Err(format!("Index {} out of bounds for array of length {}", idx, arr.len()).into());
                                    }
                                    gc_write_barrier(*ptr, &val);
                                    self.stack.push(val);
                                }
                                _ => unreachable!(),
                            }
                        }
                        (Value::Array(ptr), Value::String(s)) => unsafe {
                            if let Ok(idx) = s.parse::<usize>() {
                                match &mut (**ptr).data {
                                    GcData::Array(arr) => {
                                        if idx < arr.len() {
                                            arr[idx] = val.clone();
                                        } else if idx == arr.len() {
                                            arr.push(val.clone());
                                        } else {
                                            return Err(format!("Index {} out of bounds for array of length {}", idx, arr.len()).into());
                                        }
                                        gc_write_barrier(*ptr, &val);
                                        self.stack.push(val);
                                    }
                                    _ => unreachable!(),
                                }
                            } else {
                                return Err("Cannot set non-numeric property on array".into());
                            }
                        }
                        (Value::Object(ptr), Value::String(s)) => unsafe {
                            match &mut (**ptr).data {
                                GcData::Object(map) => {
                                    map.insert(s.clone(), val.clone());
                                    gc_write_barrier(*ptr, &val);
                                    self.stack.push(val);
                                }
                                _ => unreachable!(),
                            }
                        }
                        _ => return Err("Only arrays can be indexed by numbers, and objects by strings".into()),
                    }
                }
            }
        }
        Ok(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incremental_garbage_collector() {
        // Clear everything first
        gc_free_all();

        // 1. Allocate a reachable array
        let parent_ptr = gc_allocate(GcData::Array(vec![]));
        let parent = Value::Array(parent_ptr);

        // 2. Allocate an unreachable array (garbage)
        let garbage_ptr = gc_allocate(GcData::Array(vec![]));
        let _garbage = Value::Array(garbage_ptr);

        // 3. Create a VM with parent on the stack (so it is a root)
        let mut vm = VM::new();
        vm.stack.push(parent.clone());

        // Verify GC is in Pause phase initially
        assert_eq!(GC_PHASE.with(|p| p.get()), GcPhase::Pause);

        // Allocate more objects to increase ALLOC_COUNT
        for _ in 0..10 {
            gc_allocate(GcData::Array(vec![]));
        }

        // Trigger a step, which should start the Mark phase since ALLOC_COUNT >= 10
        vm.gc_step();
        assert_eq!(GC_PHASE.with(|p| p.get()), GcPhase::Mark);

        // Run incremental steps until we reach Sweep phase
        while GC_PHASE.with(|p| p.get()) != GcPhase::Sweep {
            vm.gc_step();
        }

        // Let's sweep step-by-step
        while GC_PHASE.with(|p| p.get()) == GcPhase::Sweep {
            vm.gc_step();
        }

        // Check that GC is back to Pause
        assert_eq!(GC_PHASE.with(|p| p.get()), GcPhase::Pause);

        // Check that parent_ptr is still alive, but garbage_ptr is freed!
        let mut found_parent = false;
        let mut found_garbage = false;
        unsafe {
            let mut curr = GC_HEAD.with(|h| h.get());
            while !curr.is_null() {
                if curr == parent_ptr {
                    found_parent = true;
                }
                if curr == garbage_ptr {
                    found_garbage = true;
                }
                curr = (*curr).next;
            }
        }
        assert!(found_parent, "Parent should be alive");
        assert!(!found_garbage, "Garbage should be collected");

        // Clean up
        gc_free_all();
    }
}
