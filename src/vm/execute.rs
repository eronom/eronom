use fnv::FnvHashMap;
use std::rc::Rc;
use std::time::{Instant, Duration};
use std::cell::Cell;
use super::value::{Value, push_positive_integer};
use super::bytecode::{Function, OpCode};

thread_local! {
    pub static GC_TIME: Cell<Duration> = Cell::new(Duration::default());
    pub static GC_COUNT: Cell<u64> = Cell::new(0);
}

#[unsafe(no_mangle)]
pub extern "C" fn er_gc_reset_stats() {
    GC_COUNT.with(|c| c.set(0));
    GC_TIME.with(|t| t.set(Duration::default()));
}

#[unsafe(no_mangle)]
pub extern "C" fn er_gc_print_stats() {
    GC_COUNT.with(|c| {
        GC_TIME.with(|t| {
            println!("=== GC Profiler Stats ===");
            println!("  GC Steps: count={:<8} time={:?}", c.get(), t.get());
            println!("=========================");
        });
    });
}
use super::gc::{
    gc_allocate, gc_write_barrier, gc_blacken_object, mark_value,
    GC_STATE, GC_ROOTS, GC_NEEDS_STEP, GcColor, GcPhase, GcData, GcObject
};

use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicUsize;

pub enum AsyncResult {
    Timeout,
    Fetch(Result<String, String>),
    ResolvePromise(*mut crate::vm::gc::GcObject, Value),
    ResolveFetchPromise(*mut crate::vm::gc::GcObject, Result<String, String>),
    ChildProcessStdout(*mut crate::vm::gc::GcObject, String),
    ChildProcessStderr(*mut crate::vm::gc::GcObject, String),
    ChildProcessExit(*mut crate::vm::gc::GcObject, i32),
}

pub struct EventLoopTask {
    pub callback: Value,
    pub args: Vec<Value>,
    pub result: AsyncResult,
}

unsafe impl Send for EventLoopTask {}
unsafe impl Sync for EventLoopTask {}

pub struct PendingAsync {
    pub callback: Value,
    pub args: Vec<Value>,
}

unsafe impl Send for PendingAsync {}
unsafe impl Sync for PendingAsync {}

pub struct VM {
    pub frames: Vec<CallFrame>,
    pub stack: Vec<Value>,
    pub globals: FnvHashMap<Rc<str>, Value>,
    pub error: Option<String>,
    pub mir_ctx: Option<*mut std::ffi::c_void>,
    pub use_jit: bool,
    pub alloc_count_local: usize,
    pub use_evented_io: bool,
    
    // Event loop fields
    pub event_loop_queue: Arc<Mutex<Vec<EventLoopTask>>>,
    pub active_async_tasks: Arc<AtomicUsize>,
    pub pending_callbacks: Arc<Mutex<Vec<PendingAsync>>>,
}

pub struct CallFrame {
    pub function: *mut GcObject,
    pub ip: usize,
    pub slots_offset: usize,
    pub dest_reg: usize,
}

unsafe impl Send for CallFrame {}
unsafe impl Sync for CallFrame {}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for VM {
    fn drop(&mut self) {
        if let Some(ctx) = self.mir_ctx {
            crate::jit::cleanup_jit(ctx);
        }
    }
}

impl VM {
    pub fn new() -> Self {
        let use_jit = std::env::var("ER_NO_JIT").is_err();
        Self {
            frames: Vec::new(),
            stack: Vec::new(),
            globals: FnvHashMap::default(),
            error: None,
            mir_ctx: None,
            use_jit,
            alloc_count_local: 0,
            use_evented_io: true,
            event_loop_queue: Arc::new(Mutex::new(Vec::new())),
            active_async_tasks: Arc::new(AtomicUsize::new(0)),
            pending_callbacks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn register_global(&mut self, name: &str, value: Value) {
        self.globals.insert(Rc::from(name), value);
    }

    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    pub fn run(&mut self, function: Function) -> Result<Value, String> {
        let prev_vm = crate::vm::er_http::ACTIVE_VM.with(|active| active.replace(self as *mut VM));
        let func_ptr = gc_allocate(GcData::Function(function));
        self.frames.push(CallFrame {
            function: func_ptr,
            ip: 0,
            slots_offset: self.stack.len(),
            dest_reg: 0,
        });

        let res = self.execute();
        crate::vm::er_http::ACTIVE_VM.with(|active| active.set(prev_vm));
        res
    }

    pub fn gc_step(&mut self) {
        let start_time = Instant::now();
        GC_COUNT.with(|c| c.set(c.get() + 1));
        GC_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let phase = state.phase;
            match phase {
                GcPhase::Pause => {
                    if state.alloc_count >= 10000 {
                        super::gc::gc_clear_string_cache();
                        state.phase = GcPhase::Mark;
                        state.gray_stack.clear();

                        let stack_len = if let Some(last_frame) = self.frames.last() {
                            (last_frame.slots_offset + 256).min(self.stack.len())
                        } else {
                            self.stack.len()
                        };
                        
                        drop(state);
                        
                        for val in &self.stack[..stack_len] {
                            mark_value(val);
                        }
                        for val in self.globals.values() {
                            mark_value(val);
                        }
                        for frame in &self.frames {
                            mark_value(&Value::function(frame.function));
                        }
                        if let Ok(queue) = self.event_loop_queue.lock() {
                            for task in queue.iter() {
                                mark_value(&task.callback);
                                for arg in task.args.iter() {
                                    mark_value(arg);
                                }
                            }
                        }
                        if let Ok(pending) = self.pending_callbacks.lock() {
                            for item in pending.iter() {
                                mark_value(&item.callback);
                                for arg in item.args.iter() {
                                    mark_value(arg);
                                }
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
                    let gray_opt = state.gray_stack.pop();
                    if let Some(ptr) = gray_opt {
                        drop(state);
                        gc_blacken_object(ptr);
                    } else {
                        state.phase = GcPhase::Atomic;
                    }
                }
                GcPhase::Atomic => {
                    state.phase = GcPhase::Sweep;
                    state.sweep_ptr = state.head;
                    state.prev_sweep_ptr = std::ptr::null_mut();
                    
                    drop(state);
                    
                    let stack_len = if let Some(last_frame) = self.frames.last() {
                        (last_frame.slots_offset + 256).min(self.stack.len())
                    } else {
                        self.stack.len()
                    };
                    for val in &self.stack[..stack_len] {
                        mark_value(val);
                    }
                    for val in self.globals.values() {
                        mark_value(val);
                    }
                    for frame in &self.frames {
                        mark_value(&Value::function(frame.function));
                    }
                    GC_ROOTS.with(|roots| {
                        if let Ok(borrowed) = roots.try_borrow() {
                            for root_fn in borrowed.iter() {
                                root_fn();
                            }
                        }
                    });

                    loop {
                        let gray_opt = GC_STATE.with(|s| s.borrow_mut().gray_stack.pop());
                        if let Some(ptr) = gray_opt {
                            gc_blacken_object(ptr);
                        } else {
                            break;
                        }
                    }
                }
                GcPhase::Sweep => {
                    for _ in 0..5 {
                        let curr = state.sweep_ptr;
                        if curr.is_null() {
                            state.phase = GcPhase::Pause;
                            state.alloc_count = 0;
                            unsafe { GC_NEEDS_STEP = false; }
                            break;
                        }

                        unsafe {
                            let next = (*curr).next;
                            if (*curr).color == GcColor::White {
                                let prev = state.prev_sweep_ptr;
                                if prev.is_null() {
                                    state.head = next;
                                } else {
                                    (*prev).next = next;
                                }
                                super::gc::gc_dealloc_object(&mut state, curr);
                                state.sweep_ptr = next;
                            } else {
                                (*curr).color = GcColor::White;
                                state.prev_sweep_ptr = curr;
                                state.sweep_ptr = next;
                            }
                        }
                    }
                }
            }
        });
        GC_TIME.with(|t| t.set(t.get() + start_time.elapsed()));
    }

    pub fn collect_garbage(&mut self) {
        let start_time = Instant::now();
        super::gc::gc_clear_string_cache();
        GC_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.gray_stack.clear();
        });

        // 1. Mark phase: mark roots
        let stack_len = if let Some(last_frame) = self.frames.last() {
            (last_frame.slots_offset + 256).min(self.stack.len())
        } else {
            self.stack.len()
        };
        for val in &self.stack[..stack_len] {
            mark_value(val);
        }
        for val in self.globals.values() {
            mark_value(val);
        }
        for frame in &self.frames {
            mark_value(&Value::function(frame.function));
        }
        GC_ROOTS.with(|roots| {
            if let Ok(borrowed) = roots.try_borrow() {
                for root_fn in borrowed.iter() {
                    root_fn();
                }
            }
        });

        // 2. Trace phase: process gray stack until empty
        loop {
            let gray_opt = GC_STATE.with(|state| state.borrow_mut().gray_stack.pop());
            if let Some(ptr) = gray_opt {
                gc_blacken_object(ptr);
            } else {
                break;
            }
        }

        // 3. Sweep phase: sweep the entire linked list in one go
        GC_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let mut curr = state.head;
            state.head = std::ptr::null_mut();
            let mut prev: *mut GcObject = std::ptr::null_mut();
            
            while !curr.is_null() {
                unsafe {
                    let next = (*curr).next;
                    if (*curr).color == GcColor::White {
                        super::gc::gc_dealloc_object(&mut state, curr);
                    } else {
                        (*curr).color = GcColor::White;
                        (*curr).next = std::ptr::null_mut();
                        if prev.is_null() {
                            state.head = curr;
                        } else {
                            (*prev).next = curr;
                        }
                        prev = curr;
                    }
                    curr = next;
                }
            }

            // 4. Reset GC state
            state.alloc_count = 0;
            state.phase = GcPhase::Pause;
            state.sweep_ptr = std::ptr::null_mut();
            state.prev_sweep_ptr = std::ptr::null_mut();
        });
        unsafe { GC_NEEDS_STEP = false; }
        GC_TIME.with(|t| t.set(t.get() + start_time.elapsed()));
    }

    #[inline(always)]
    pub fn gc_trigger(&mut self) {
        if unsafe { GC_NEEDS_STEP } {
            self.collect_garbage();
        }
    }

    pub fn call_function_reentrant(&mut self, callee: Value, args: Vec<Value>) -> Result<Value, String> {
        let func_ptr = callee.as_gc_ptr();
        
        let old_frames = std::mem::take(&mut self.frames);
        let old_stack_len = self.stack.len();
        
        for arg in &args {
            self.stack.push(*arg);
        }
        
        self.frames.push(CallFrame {
            function: func_ptr,
            ip: 0,
            slots_offset: old_stack_len,
            dest_reg: 0,
        });
        
        let old_fns: Vec<*mut GcObject> = old_frames.iter().map(|f| f.function).collect();
        super::gc::GC_ROOTS.with(|roots| {
            roots.borrow_mut().push(Box::new(move || {
                for &func in &old_fns {
                    super::gc::mark_value(&Value::function(func));
                }
            }));
        });
        
        let res = self.execute();
        
        super::gc::GC_ROOTS.with(|roots| {
            roots.borrow_mut().pop();
        });
        
        self.frames = old_frames;
        self.stack.truncate(old_stack_len);
        
        res
    }

    fn execute(&mut self) -> Result<Value, String> {
        let original_len = self.stack.len();
        self.stack.resize(original_len + 4096, Value::null());
        
        let res = if self.use_jit {
            self.execute_loop(original_len)
        } else {
            self.execute_loop_interpreter(original_len)
        };
        
        self.stack.truncate(original_len);
        res
    }

    fn execute_loop(&mut self, _original_len: usize) -> Result<Value, String> {
        unsafe {
            type JitFn = unsafe extern "C" fn(
                vm: *mut VM,
                frame_slots: *mut Value,
                constants_ptr: *const Value,
                start_ip: usize,
                ip_out: *mut usize,
                dest_reg_out: *mut usize,
                func_reg_out: *mut usize,
                arg_count_out: *mut usize,
                ret_val_out: *mut Value,
            ) -> i64;

            let mut frame_ptr = {
                let len = self.frames.len();
                self.frames.as_mut_ptr().add(len - 1)
            };
            let mut frame = &mut *frame_ptr;

            macro_rules! get_func {
                ($func_ptr:expr) => {
                    match &(*$func_ptr).data {
                        GcData::Function(func) => func,
                        _ => unreachable!(),
                    }
                };
            }

            let mut func = get_func!(frame.function);
            let mut constants_ptr = func.chunk.constants.as_ptr();
            let mut slots_offset = frame.slots_offset;

            let mut stack_start = self.stack.as_mut_ptr();
            let mut frame_slots = stack_start.add(slots_offset);

            macro_rules! reload_stack {
                () => {
                    stack_start = self.stack.as_mut_ptr();
                    frame_slots = stack_start.add(slots_offset);
                };
            }

            let mut ip_val = frame.ip;

            loop {
                self.gc_trigger();
                reload_stack!();

                let is_async = unsafe {
                    match &(*frame.function).data {
                        GcData::Function(f) => f.is_async,
                        _ => false,
                    }
                };
                if is_async {
                    return self.execute_loop_interpreter(0);
                }

                // Ensure the current function is JIT compiled
                let native_ptr = crate::jit::compile_function(self, frame.function);
                let jit_fn: JitFn = std::mem::transmute(native_ptr);

                let mut ip_out: usize = ip_val;
                let mut dest_reg_out: usize = 0;
                let mut func_reg_out: usize = 0;
                let mut arg_count_out: usize = 0;
                let mut ret_val_out: Value = Value::null();

                let status = jit_fn(
                    self as *mut VM,
                    frame_slots,
                    constants_ptr,
                    ip_val,
                    &mut ip_out,
                    &mut dest_reg_out,
                    &mut func_reg_out,
                    &mut arg_count_out,
                    &mut ret_val_out,
                );

                if status == 0 {
                    // YieldCall: a Call instruction yielded to the JIT orchestrator.
                    let callee = *frame_slots.add(func_reg_out);
                    if callee.is_function() {
                        let func_ptr = callee.as_gc_ptr();
                        let func_val = get_func!(func_ptr);
                        if arg_count_out > func_val.arity {
                            return Err(format!(
                                "Expected at most {} args but got {}",
                                func_val.arity, arg_count_out
                            ));
                        }
                        if arg_count_out < func_val.arity {
                            let new_slots_offset = slots_offset + func_reg_out + 1;
                            for i in arg_count_out..func_val.arity {
                                *stack_start.add(new_slots_offset + i) = Value::null();
                            }
                        }
                        // Save current IP (resume position: ip_out)
                        frame.ip = ip_out;
                        let new_slots_offset = slots_offset + func_reg_out + 1;
                        self.frames.push(CallFrame {
                            function: func_ptr,
                            ip: 0,
                            slots_offset: new_slots_offset,
                            dest_reg: dest_reg_out,
                        });
                        frame_ptr = {
                            let len = self.frames.len();
                            self.frames.as_mut_ptr().add(len - 1)
                        };
                        frame = &mut *frame_ptr;
                        func = get_func!(frame.function);
                        constants_ptr = func.chunk.constants.as_ptr();
                        slots_offset = frame.slots_offset;
                        reload_stack!();
                        ip_val = 0;
                    } else if callee.is_native_function() {
                        let native = callee.as_native_fn();
                        let mut args = Vec::with_capacity(arg_count_out);
                        for i in 0..arg_count_out {
                            args.push(*frame_slots.add(func_reg_out + 1 + i));
                        }
                        let result = native(args);
                        reload_stack!();
                        if self.stack.is_empty() {
                            frame.ip = ip_out - 1;
                            return Ok(Value::null());
                        }
                        *frame_slots.add(dest_reg_out) = result;
                        ip_val = ip_out;
                    } else if callee.is_array_method_push() || callee.is_array_method_pop() {
                        let ptr = callee.as_gc_ptr();
                        let result = match &mut (*ptr).data {
                            GcData::Array(arr) => {
                                if callee.is_array_method_push() {
                                    for i in 0..arg_count_out {
                                        let arg = *frame_slots.add(func_reg_out + 1 + i);
                                        gc_write_barrier(ptr, &arg);
                                        arr.push(arg);
                                    }
                                    Value::number(arr.len() as f64)
                                } else {
                                    arr.pop().unwrap_or(Value::null())
                                }
                            }
                            _ => unreachable!(),
                        };
                        reload_stack!();
                        *frame_slots.add(dest_reg_out) = result;
                        ip_val = ip_out;
                    } else {
                        return Err("Can only call functions".into());
                    }
                } else if status == 1 {
                    // YieldReturn: a Return instruction yielded to the JIT orchestrator.
                    let caller_dest_reg = frame.dest_reg;
                    self.frames.pop();
                    if self.frames.is_empty() {
                        return Ok(ret_val_out);
                    }

                    frame_ptr = {
                        let len = self.frames.len();
                        self.frames.as_mut_ptr().add(len - 1)
                    };
                    frame = &mut *frame_ptr;
                    func = get_func!(frame.function);
                    constants_ptr = func.chunk.constants.as_ptr();
                    slots_offset = frame.slots_offset;
                    reload_stack!();

                    *frame_slots.add(caller_dest_reg) = ret_val_out;
                    ip_val = frame.ip;
                } else if status == 2 {
                    // YieldGc / YieldLoop
                    frame.ip = ip_out;
                    ip_val = ip_out;
                } else if status == 3 {
                    // YieldSuspend: a native function suspended the VM during JIT execution.
                    frame.ip = ip_out;
                    return Ok(Value::null());
                } else {
                    // RuntimeError or JIT compilation/execution error.
                    let err_msg = self.error.take().unwrap_or_else(|| "JIT execution error".into());
                    return Err(err_msg);
                }
            }
        }
    }

    fn execute_loop_interpreter(&mut self, _original_len: usize) -> Result<Value, String> {
        unsafe {
            let mut frame_ptr = {
                let len = self.frames.len();
                self.frames.as_mut_ptr().add(len - 1)
            };
            let mut frame = &mut *frame_ptr;

            macro_rules! get_func {
                ($func_ptr:expr) => {
                    match &(*$func_ptr).data {
                        GcData::Function(func) => func,
                        _ => unreachable!(),
                    }
                };
            }

            let mut func = get_func!(frame.function);
            let mut code_ptr = func.chunk.code.as_ptr();
            let mut constants_ptr = func.chunk.constants.as_ptr();
            let mut slots_offset = frame.slots_offset;
            let mut ip = code_ptr.add(frame.ip);

            let mut stack_start = self.stack.as_mut_ptr();
            let mut frame_slots = stack_start.add(slots_offset);

            macro_rules! sync_stack {
                () => {};
            }

            macro_rules! reload_stack {
                () => {
                    stack_start = self.stack.as_mut_ptr();
                    frame_slots = stack_start.add(slots_offset);
                };
            }

            loop {
                let instruction = *ip;
                ip = ip.add(1);

                match instruction.op {
                    OpCode::LoadConst => {
                        let dest = instruction.ra as usize;
                        let val = *constants_ptr.add(instruction.operand as usize);
                        *frame_slots.add(dest) = val;
                    }
                    OpCode::LoadNull => {
                        let dest = instruction.ra as usize;
                        *frame_slots.add(dest) = Value::null();
                    }
                    OpCode::LoadBool => {
                        let dest = instruction.ra as usize;
                        *frame_slots.add(dest) = Value::boolean(instruction.rb != 0);
                    }
                    OpCode::Move => {
                        let dest = instruction.ra as usize;
                        let src = instruction.rb as usize;
                        *frame_slots.add(dest) = *frame_slots.add(src);
                    }
                    OpCode::Negate => {
                        let dest = instruction.ra as usize;
                        let src = instruction.rb as usize;
                        let val = *frame_slots.add(src);
                        if val.is_number() {
                            *frame_slots.add(dest) = Value::number_unchecked(-val.as_number());
                        } else {
                            return Err("Operand must be a number".into());
                        }
                    }
                    OpCode::Not => {
                        let dest = instruction.ra as usize;
                        let src = instruction.rb as usize;
                        let val = *frame_slots.add(src);
                        let res = if val.is_boolean() {
                            !val.as_boolean()
                        } else if val.is_null() {
                            true
                        } else {
                            false
                        };
                        *frame_slots.add(dest) = Value::boolean(res);
                    }
                    OpCode::Add => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            *frame_slots.add(dest) = Value::number_unchecked(a.as_number() + b.as_number());
                        } else {
                            use std::fmt::Write;
                            if a.is_string() {
                                let sa_str = match &(*a.as_gc_ptr()).data {
                                    GcData::String(s) => s,
                                    _ => unreachable!(),
                                };
                                let new_ptr = super::value::ADD_SCRATCH.with(|scratch| {
                                    let mut s_ref = scratch.borrow_mut();
                                    s_ref.clear();
                                    s_ref.push_str(sa_str);
                                    if b.is_string() {
                                        let sb_str = match &(*b.as_gc_ptr()).data {
                                            GcData::String(s) => s,
                                            _ => unreachable!(),
                                        };
                                        s_ref.push_str(sb_str);
                                    } else if b.is_number() {
                                        let val = b.as_number();
                                        if val >= 0.0 && val == val.trunc() && val < 1.8446744073709552e19 {
                                            push_positive_integer(&mut s_ref, val as u64);
                                        } else {
                                            let _ = write!(&mut s_ref, "{}", val);
                                        }
                                    } else {
                                        let _ = write!(&mut s_ref, "{}", b);
                                    }
                                    super::gc::get_or_create_string(s_ref.as_str())
                                });
                                *frame_slots.add(dest) = Value::string(new_ptr);
                            } else if b.is_string() {
                                let sb_str = match &(*b.as_gc_ptr()).data {
                                    GcData::String(s) => s,
                                    _ => unreachable!(),
                                };
                                let new_ptr = super::value::ADD_SCRATCH.with(|scratch| {
                                    let mut s_ref = scratch.borrow_mut();
                                    s_ref.clear();
                                    if a.is_number() {
                                        let val = a.as_number();
                                        if val >= 0.0 && val == val.trunc() && val < 1.8446744073709552e19 {
                                            push_positive_integer(&mut s_ref, val as u64);
                                        } else {
                                            let _ = write!(&mut s_ref, "{}", val);
                                        }
                                    } else {
                                        let _ = write!(&mut s_ref, "{}", a);
                                    }
                                    s_ref.push_str(sb_str);
                                    super::gc::get_or_create_string(s_ref.as_str())
                                });
                                *frame_slots.add(dest) = Value::string(new_ptr);
                            } else {
                                return Err("Operands must be numbers or strings".into());
                            }
                        }
                    }
                    OpCode::Sub => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            *frame_slots.add(dest) = Value::number_unchecked(a.as_number() - b.as_number());
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::Mul => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            *frame_slots.add(dest) = Value::number_unchecked(a.as_number() * b.as_number());
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::Div => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            *frame_slots.add(dest) = Value::number_unchecked(a.as_number() / b.as_number());
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::Equal => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        *frame_slots.add(dest) = Value::boolean(a == b);
                    }
                    OpCode::Greater => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            *frame_slots.add(dest) = Value::boolean(a.as_number() > b.as_number());
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::Less => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            *frame_slots.add(dest) = Value::boolean(a.as_number() < b.as_number());
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::DefineGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => s.clone(),
                            _ => unreachable!(),
                        };
                        let val = *frame_slots.add(instruction.ra as usize);
                        self.globals.insert(name, val);
                    }
                    OpCode::GetGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => s,
                            _ => unreachable!(),
                        };
                        if let Some(val) = self.globals.get(name) {
                            *frame_slots.add(instruction.ra as usize) = *val;
                        } else {
                            return Err(format!("Undefined variable '{}'", name));
                        }
                    }
                    OpCode::SetGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => s.clone(),
                            _ => unreachable!(),
                        };
                        let val = *frame_slots.add(instruction.ra as usize);
                        match self.globals.entry(name.clone()) {
                            std::collections::hash_map::Entry::Occupied(mut entry) => {
                                entry.insert(val);
                            }
                            std::collections::hash_map::Entry::Vacant(_) => {
                                return Err(format!(
                                    "Variable '{}' not declared. It needs to be declared with 'let' or 'const'.",
                                    name
                                ));
                            }
                        }
                    }
                    OpCode::Jump => {
                        ip = ip.add(instruction.operand as usize);
                    }
                    OpCode::JumpIfFalse => {
                        let val = *frame_slots.add(instruction.ra as usize);
                        let is_false = val.0 == super::value::TAG_FALSE || val.0 == super::value::TAG_NULL;
                        if is_false {
                            ip = ip.add(instruction.operand as usize);
                        }
                    }
                    OpCode::Loop => {
                        ip = ip.sub(instruction.operand as usize);
                    }
                    OpCode::MakeArray => {
                        let dest = instruction.ra as usize;
                        let start_reg = instruction.rb as usize;
                        let count = instruction.operand as usize;
                        sync_stack!();
                        self.gc_trigger();
                        reload_stack!();

                        let mut elements = super::gc::get_pooled_vec(count);
                        for i in 0..count {
                            elements.push(*frame_slots.add(start_reg + i));
                        }
                        let ptr = gc_allocate(GcData::Array(elements));
                        *frame_slots.add(dest) = Value::array(ptr);
                    }
                    OpCode::MakeObject => {
                        let dest = instruction.ra as usize;
                        let start_reg = instruction.rb as usize;
                        let count = instruction.operand as usize;
                        sync_stack!();
                        self.gc_trigger();
                        reload_stack!();

                        let mut obj = super::gc::get_pooled_map(count);
                        for i in 0..count {
                            let key_val = *frame_slots.add(start_reg + i * 2);
                            let val = *frame_slots.add(start_reg + i * 2 + 1);
                            if !key_val.is_string() {
                                return Err("Object key must be string".into());
                            }
                            obj.insert(super::value::MapKey(key_val), val);
                        }
                        let ptr = gc_allocate(GcData::Object(obj));
                        *frame_slots.add(dest) = Value::object(ptr);
                    }
                    OpCode::GetProperty => {
                        let dest = instruction.ra as usize;
                        let obj_reg = instruction.rb as usize;
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let obj = *frame_slots.add(obj_reg);
                        if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            let name = match &(*name_val.as_gc_ptr()).data {
                                GcData::String(s) => s.as_ref(),
                                _ => "",
                            };
                            let mut is_json_method = false;
                            let mut is_text_method = false;
                            if name == "json" || name == "text" {
                                let body_key = super::gc::get_or_create_string("_body");
                                let is_response = match &(*ptr).data {
                                    GcData::Object(map) => map.contains_key(&super::value::MapKey(Value::string(body_key))),
                                    _ => false,
                                };
                                if is_response {
                                    if name == "json" {
                                        is_json_method = true;
                                    } else {
                                        is_text_method = true;
                                    }
                                }
                            }
                            if is_json_method {
                                *frame_slots.add(dest) = Value(super::value::TAG_METHOD_JSON | (ptr as u64 & super::value::PTR_MASK));
                            } else if is_text_method {
                                *frame_slots.add(dest) = Value(super::value::TAG_METHOD_TEXT | (ptr as u64 & super::value::PTR_MASK));
                            } else {
                                match &(*ptr).data {
                                    GcData::Object(map) => {
                                        let val = map.get(&super::value::MapKey(name_val)).cloned().unwrap_or(Value::null());
                                        *frame_slots.add(dest) = val;
                                    }
                                    _ => unreachable!(),
                                }
                            }
                        } else if obj.is_array() {
                            let name = match &(*name_val.as_gc_ptr()).data {
                                GcData::String(s) => s.as_ref(),
                                _ => unreachable!(),
                            };
                            let ptr = obj.as_gc_ptr();
                            match &(*ptr).data {
                                GcData::Array(arr) => {
                                    if name == "push" {
                                        *frame_slots.add(dest) = Value::array_method_push(ptr);
                                    } else if name == "pop" {
                                        *frame_slots.add(dest) = Value::array_method_pop(ptr);
                                    } else if name == "length" {
                                        *frame_slots.add(dest) = Value::number(arr.len() as f64);
                                    } else if let Ok(idx) = name.parse::<usize>() {
                                        let val = arr.get(idx).cloned().unwrap_or(Value::null());
                                        *frame_slots.add(dest) = val;
                                    } else {
                                        *frame_slots.add(dest) = Value::null();
                                    }
                                }
                                _ => unreachable!(),
                            }
                        } else {
                            return Err("Only objects and arrays have properties".into());
                        }
                    }
                    OpCode::SetProperty => {
                        let obj_reg = instruction.ra as usize;
                        let val_reg = instruction.rb as usize;
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let obj = *frame_slots.add(obj_reg);
                        let val = *frame_slots.add(val_reg);
                        if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            match &mut (*ptr).data {
                                GcData::Object(map) => {
                                    map.insert(super::value::MapKey(name_val), val);
                                    gc_write_barrier(ptr, &val);
                                }
                                _ => unreachable!(),
                            }
                        } else if obj.is_array() {
                            let name_rc = match &(*name_val.as_gc_ptr()).data {
                                GcData::String(s) => s.as_ref(),
                                _ => unreachable!(),
                            };
                            let ptr = obj.as_gc_ptr();
                            match &mut (*ptr).data {
                                GcData::Array(arr) => {
                                    if let Ok(idx) = name_rc.parse::<usize>() {
                                        if idx < arr.len() {
                                            arr[idx] = val;
                                        } else if idx == arr.len() {
                                            arr.push(val);
                                        } else {
                                            return Err(format!(
                                                "Index {} out of bounds for array of length {}",
                                                idx,
                                                arr.len()
                                            ));
                                        }
                                        gc_write_barrier(ptr, &val);
                                    } else {
                                        return Err("Cannot set non-numeric property on array".into());
                                    }
                                }
                                _ => unreachable!(),
                            }
                        } else {
                            return Err("Only objects and arrays have properties".into());
                        }
                    }
                    OpCode::GetIndex => {
                        let dest = instruction.ra as usize;
                        let obj = *frame_slots.add(instruction.rb as usize);
                        let index = *frame_slots.add(instruction.rc as usize);
                        if obj.is_array() {
                            let ptr = obj.as_gc_ptr();
                            if index.is_number() {
                                let idx = index.as_number() as usize;
                                match &(*ptr).data {
                                    GcData::Array(arr) => {
                                        let val = arr.get(idx).cloned().unwrap_or(Value::null());
                                        *frame_slots.add(dest) = val;
                                    }
                                    _ => unreachable!(),
                                }
                            } else if index.is_string() {
                                let s = match &(*index.as_gc_ptr()).data {
                                    GcData::String(st) => st,
                                    _ => unreachable!(),
                                };
                                if let Ok(idx) = s.parse::<usize>() {
                                    match &(*ptr).data {
                                        GcData::Array(arr) => {
                                            let val = arr.get(idx).cloned().unwrap_or(Value::null());
                                            *frame_slots.add(dest) = val;
                                        }
                                        _ => unreachable!(),
                                    }
                                } else {
                                    *frame_slots.add(dest) = Value::null();
                                }
                            } else {
                                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                            }
                        } else if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            if index.is_string() {
                                match &(*ptr).data {
                                    GcData::Object(map) => {
                                        let val = map.get(&super::value::MapKey(index)).cloned().unwrap_or(Value::null());
                                        *frame_slots.add(dest) = val;
                                    }
                                    _ => unreachable!(),
                                }
                            } else {
                                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                            }
                        } else {
                            return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                        }
                    }
                    OpCode::SetIndex => {
                        let obj = *frame_slots.add(instruction.ra as usize);
                        let index = *frame_slots.add(instruction.rb as usize);
                        let val = *frame_slots.add(instruction.rc as usize);
                        if obj.is_array() {
                            let ptr = obj.as_gc_ptr();
                            if index.is_number() {
                                let idx = index.as_number() as usize;
                                match &mut (*ptr).data {
                                    GcData::Array(arr) => {
                                        if idx < arr.len() {
                                            arr[idx] = val;
                                        } else if idx == arr.len() {
                                            arr.push(val);
                                        } else {
                                            return Err(format!(
                                                "Index {} out of bounds for array of length {}",
                                                idx,
                                                arr.len()
                                            ));
                                        }
                                        gc_write_barrier(ptr, &val);
                                    }
                                    _ => unreachable!(),
                                }
                            } else if index.is_string() {
                                let s = match &(*index.as_gc_ptr()).data {
                                    GcData::String(st) => st,
                                    _ => unreachable!(),
                                };
                                if let Ok(idx) = s.parse::<usize>() {
                                    match &mut (*ptr).data {
                                        GcData::Array(arr) => {
                                            if idx < arr.len() {
                                                arr[idx] = val;
                                            } else if idx == arr.len() {
                                                arr.push(val);
                                            } else {
                                                return Err(format!(
                                                    "Index {} out of bounds for array of length {}",
                                                    idx,
                                                    arr.len()
                                                ));
                                            }
                                            gc_write_barrier(ptr, &val);
                                        }
                                        _ => unreachable!(),
                                    }
                                } else {
                                    return Err("Cannot set non-numeric property on array".into());
                                }
                            } else {
                                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                            }
                        } else if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            if index.is_string() {
                                match &mut (*ptr).data {
                                    GcData::Object(map) => {
                                        map.insert(super::value::MapKey(index), val);
                                        gc_write_barrier(ptr, &val);
                                    }
                                    _ => unreachable!(),
                                }
                            } else {
                                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                            }
                        } else {
                            return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                        }
                    }
                    OpCode::Call => {
                        let dest = instruction.ra as usize;
                        let func_reg = instruction.rb as usize;
                        let arg_count = instruction.operand as usize;
                        let callee = *frame_slots.add(func_reg);
                        if callee.is_function() {
                            let func_ptr = callee.as_gc_ptr();
                            let func_val = get_func!(func_ptr);
                            if arg_count > func_val.arity {
                                return Err(format!(
                                    "Expected at most {} args but got {}",
                                    func_val.arity, arg_count
                                ));
                            }
                            if arg_count < func_val.arity {
                                let new_slots_offset = slots_offset + func_reg + 1;
                                for i in arg_count..func_val.arity {
                                    *stack_start.add(new_slots_offset + i) = Value::null();
                                }
                            }
                            frame.ip = ip.offset_from(code_ptr) as usize;
                            let new_slots_offset = slots_offset + func_reg + 1;
                            self.frames.push(CallFrame {
                                function: func_ptr,
                                ip: 0,
                                slots_offset: new_slots_offset,
                                dest_reg: dest,
                            });
                            frame_ptr = {
                                let len = self.frames.len();
                                self.frames.as_mut_ptr().add(len - 1)
                            };
                            frame = &mut *frame_ptr;
                            func = get_func!(frame.function);
                            code_ptr = func.chunk.code.as_ptr();
                            constants_ptr = func.chunk.constants.as_ptr();
                            slots_offset = frame.slots_offset;
                            frame_slots = stack_start.add(slots_offset);
                            ip = code_ptr.add(frame.ip);
                        } else if callee.is_native_function() {
                            let native = callee.as_native_fn();
                            let mut args = Vec::with_capacity(arg_count);
                            for i in 0..arg_count {
                                args.push(*frame_slots.add(func_reg + 1 + i));
                            }
                            sync_stack!();
                            frame.ip = unsafe { ip.offset_from(code_ptr) } as usize - 1;
                            let result = native(args);
                            reload_stack!();
                            if self.stack.is_empty() {
                                return Ok(Value::null());
                            }
                            *frame_slots.add(dest) = result;
                        } else if callee.is_method_json() || callee.is_method_text() {
                            let ptr = callee.as_gc_ptr();
                            sync_stack!();
                            let result = match &(*ptr).data {
                                GcData::Object(map) => {
                                    let body_key = super::gc::get_or_create_string("_body");
                                    let body_val = map.get(&super::value::MapKey(Value::string(body_key))).cloned().unwrap_or(Value::null());
                                    if callee.is_method_json() {
                                        if body_val.is_string() {
                                            let s = match &(*body_val.as_gc_ptr()).data {
                                                GcData::String(st) => st.as_ref(),
                                                _ => "",
                                            };
                                            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(s) {
                                                super::gc::json_to_value(json_val)
                                            } else {
                                                Value::null()
                                            }
                                        } else {
                                            Value::null()
                                        }
                                    } else {
                                        body_val
                                    }
                                }
                                _ => unreachable!(),
                            };
                            reload_stack!();
                            *frame_slots.add(dest) = result;
                        } else if callee.is_method_send_json() {
                            let res_ptr = (callee.0 & super::value::PTR_MASK) as *mut std::ffi::c_void;
                            if !res_ptr.is_null() {
                                let arg = if arg_count > 0 {
                                    *frame_slots.add(func_reg + 1)
                                } else {
                                    Value::null()
                                };
                                sync_stack!();
                                let json_val = super::er_http::value_to_json(arg);
                                let json_str = serde_json::to_string(&json_val).unwrap_or_else(|_| "null".to_string());
                                super::er_http::end_http_response_json(res_ptr, &json_str);
                                reload_stack!();
                            }
                            *frame_slots.add(dest) = Value::null();
                        } else if callee.is_method_resolve() {
                            let promise_ptr = callee.as_gc_ptr();
                            let arg = if arg_count > 0 {
                                *frame_slots.add(func_reg + 1)
                            } else {
                                Value::null()
                            };
                            let queue = self.event_loop_queue.clone();
                            let mut q = queue.lock().unwrap();
                            q.push(crate::vm::execute::EventLoopTask {
                                callback: Value::null(),
                                args: Vec::new(),
                                result: crate::vm::execute::AsyncResult::ResolvePromise(promise_ptr, arg),
                            });
                            *frame_slots.add(dest) = Value::null();
                        } else if callee.is_array_method_push() || callee.is_array_method_pop() {
                            let ptr = callee.as_gc_ptr();
                            sync_stack!();
                            let result = match &mut (*ptr).data {
                                GcData::Array(arr) => {
                                    if callee.is_array_method_push() {
                                        for i in 0..arg_count {
                                            let arg = *frame_slots.add(func_reg + 1 + i);
                                            gc_write_barrier(ptr, &arg);
                                            arr.push(arg);
                                        }
                                        Value::number(arr.len() as f64)
                                    } else {
                                        arr.pop().unwrap_or(Value::null())
                                    }
                                }
                                _ => unreachable!(),
                            };
                            reload_stack!();
                            *frame_slots.add(dest) = result;
                        } else {
                            return Err("Can only call functions".into());
                        }
                    }
                    OpCode::Await => {
                        let await_value = *frame_slots.add(instruction.rb as usize);
                        if await_value.is_promise() {
                            let promise_ptr = await_value.as_gc_ptr();
                            let state = unsafe {
                                match &(*promise_ptr).data {
                                    crate::vm::gc::GcData::Promise(prom) => prom.state.clone(),
                                    _ => unreachable!(),
                                }
                            };
                            let promise_status = {
                                let lock = state.lock().unwrap();
                                lock.clone()
                            };
                            match promise_status {
                                crate::vm::gc::PromiseState::Fulfilled(val) => {
                                    *frame_slots.add(instruction.ra as usize) = val;
                                }
                                crate::vm::gc::PromiseState::Rejected(err) => {
                                    return Err(err);
                                }
                                crate::vm::gc::PromiseState::Pending => {
                                     frame.ip = unsafe { ip.offset_from(code_ptr) } as usize - 1;

                                    let mut suspended_stack = std::mem::take(&mut self.stack);
                                    let mut suspended_frames = std::mem::take(&mut self.frames);

                                    unsafe {
                                        match &mut (*promise_ptr).data {
                                            crate::vm::gc::GcData::Promise(prom) => {
                                                *prom.suspended_stack.lock().unwrap() = suspended_stack;
                                                *prom.suspended_frames.lock().unwrap() = suspended_frames;
                                            }
                                            _ => unreachable!(),
                                        }
                                    }
                                    return Ok(Value::null());
                                }
                            }
                        } else {
                            *frame_slots.add(instruction.ra as usize) = await_value;
                        }
                    }
                    OpCode::Return => {
                        let result = *frame_slots.add(instruction.ra as usize);
                        let caller_dest_reg = frame.dest_reg;

                        self.frames.pop();
                        if self.frames.is_empty() {
                            return Ok(result);
                        }

                        frame_ptr = {
                            let len = self.frames.len();
                            self.frames.as_mut_ptr().add(len - 1)
                        };
                        frame = &mut *frame_ptr;
                        func = get_func!(frame.function);
                        code_ptr = func.chunk.code.as_ptr();
                        constants_ptr = func.chunk.constants.as_ptr();
                        slots_offset = frame.slots_offset;
                        frame_slots = stack_start.add(slots_offset);
                        ip = code_ptr.add(frame.ip);

                        *frame_slots.add(caller_dest_reg) = result;
                    }
                }
            }
        }
    }

fn get_property_val(obj: Value, name: &str) -> Value {
    if obj.is_object() {
        let ptr = obj.as_gc_ptr();
        unsafe {
            match &(*ptr).data {
                GcData::Object(map) => {
                    let name_ptr = crate::vm::gc::get_or_create_string(name);
                    return map.get(&crate::vm::value::MapKey(Value::string(name_ptr))).cloned().unwrap_or(Value::null());
                }
                _ => {}
            }
        }
    }
    Value::null()
}

    pub fn run_event_loop(&mut self) -> Result<(), String> {
        let prev_vm = crate::vm::er_http::ACTIVE_VM.with(|active| active.replace(self as *mut VM));
        let result = self.run_event_loop_inner();
        crate::vm::er_http::ACTIVE_VM.with(|active| active.set(prev_vm));
        result
    }

    fn run_event_loop_inner(&mut self) -> Result<(), String> {
        loop {
            let tasks = {
                let mut queue = self.event_loop_queue.lock().unwrap();
                std::mem::take(&mut *queue)
            };

            for task in tasks {
                match task.result {
                    AsyncResult::ResolvePromise(promise_ptr, _) |
                    AsyncResult::ResolveFetchPromise(promise_ptr, _) => {
                        let resolved_value = match task.result {
                            AsyncResult::ResolvePromise(_, val) => val,
                            AsyncResult::ResolveFetchPromise(_, res) => {
                                match res {
                                    Ok(body_str) => {
                                        let mut map = crate::vm::gc::get_pooled_map(2);
                                        let body_key = crate::vm::gc::get_or_create_string("_body");
                                        let body_val = crate::vm::gc::get_or_create_string(&body_str);
                                        map.insert(crate::vm::value::MapKey(Value::string(body_key)), Value::string(body_val));
                                        let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Object(map));
                                        Value::object(ptr)
                                    }
                                    Err(e) => {
                                        eprintln!("[FetchAsyncPromise] Error: {}", e);
                                        Value::null()
                                    }
                                }
                            }
                            _ => unreachable!(),
                        };

                        let (suspended_stack, suspended_frames) = unsafe {
                            match &(*promise_ptr).data {
                                crate::vm::gc::GcData::Promise(prom) => {
                                    let mut state = prom.state.lock().unwrap();
                                    match *state {
                                        crate::vm::gc::PromiseState::Pending => {
                                            *state = crate::vm::gc::PromiseState::Fulfilled(resolved_value);
                                            (
                                                std::mem::take(&mut *prom.suspended_stack.lock().unwrap()),
                                                std::mem::take(&mut *prom.suspended_frames.lock().unwrap())
                                            )
                                        }
                                        _ => continue, // already resolved
                                    }
                                }
                                _ => unreachable!(),
                            }
                        };

                        if suspended_frames.is_empty() {
                            continue;
                        }

                        // Restore stack and frames
                        self.stack = suspended_stack;
                        self.frames = suspended_frames;

                        // Find the destination register from the Await or Call instruction
                        let frame = self.frames.last_mut().unwrap();
                        let func = unsafe {
                            match &(*frame.function).data {
                                crate::vm::gc::GcData::Function(f) => f,
                                _ => unreachable!(),
                            }
                        };
                        let inst = func.chunk.code[frame.ip];
                        assert!(inst.op == OpCode::Await || inst.op == OpCode::Call);

                        // Write the resolved value to the destination register
                        self.stack[frame.slots_offset + inst.ra as usize] = resolved_value;

                        // Advance instruction pointer past Await
                        frame.ip += 1;

                        // Resume execution!
                        if let Err(e) = self.execute_loop_interpreter(0) {
                            return Err(e);
                        }
                        continue;
                    }
                    AsyncResult::ChildProcessStdout(child_ptr, data) => {
                        let child_val = Value::object(child_ptr);
                        let stdout_val = Self::get_property_val(child_val, "stdout");
                        if stdout_val.is_object() {
                            let on_data_val = Self::get_property_val(stdout_val, "_onData");
                            if on_data_val.is_function() || on_data_val.is_native_function() {
                                let str_ptr = crate::vm::gc::get_or_create_string(&data);
                                let args = vec![Value::string(str_ptr)];
                                if let Err(e) = self.call_function_reentrant(on_data_val, args) {
                                    eprintln!("[ChildProcess Stdout] Error calling callback: {}", e);
                                }
                            }
                        }
                        continue;
                    }
                    AsyncResult::ChildProcessStderr(child_ptr, data) => {
                        let child_val = Value::object(child_ptr);
                        let stderr_val = Self::get_property_val(child_val, "stderr");
                        if stderr_val.is_object() {
                            let on_data_val = Self::get_property_val(stderr_val, "_onData");
                            if on_data_val.is_function() || on_data_val.is_native_function() {
                                let str_ptr = crate::vm::gc::get_or_create_string(&data);
                                let args = vec![Value::string(str_ptr)];
                                if let Err(e) = self.call_function_reentrant(on_data_val, args) {
                                    eprintln!("[ChildProcess Stderr] Error calling callback: {}", e);
                                }
                            }
                        }
                        continue;
                    }
                    AsyncResult::ChildProcessExit(child_ptr, code) => {
                        let child_val = Value::object(child_ptr);
                        let on_exit_val = Self::get_property_val(child_val, "_onExit");
                        if on_exit_val.is_function() || on_exit_val.is_native_function() {
                            let args = vec![Value::number(code as f64)];
                            if let Err(e) = self.call_function_reentrant(on_exit_val, args) {
                                    eprintln!("[ChildProcess Exit] Error calling callback: {}", e);
                            }
                        }
                        let on_close_val = Self::get_property_val(child_val, "_onClose");
                        if on_close_val.is_function() || on_close_val.is_native_function() {
                            let args = vec![Value::number(code as f64)];
                            if let Err(e) = self.call_function_reentrant(on_close_val, args) {
                                    eprintln!("[ChildProcess Close] Error calling callback: {}", e);
                            }
                        }
                        crate::vm::er_http::remove_active_process(child_ptr);
                        continue;
                    }
                    _ => {}
                }

                let mut args = Vec::new();
                match task.result {
                    AsyncResult::Timeout => {
                        args.extend(task.args);
                    }
                    AsyncResult::Fetch(res) => {
                        match res {
                            Ok(body_str) => {
                                let mut map = crate::vm::gc::get_pooled_map(2);
                                let body_key = crate::vm::gc::get_or_create_string("_body");
                                let body_val = crate::vm::gc::get_or_create_string(&body_str);
                                map.insert(crate::vm::value::MapKey(Value::string(body_key)), Value::string(body_val));
                                let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Object(map));
                                args.push(Value::object(ptr));
                            }
                            Err(e) => {
                                eprintln!("[FetchAsync] Error: {}", e);
                                args.push(Value::null());
                            }
                        }
                        args.extend(task.args);
                    }
                    _ => unreachable!(),
                };

                if let Err(e) = self.call_function_reentrant(task.callback, args) {
                    return Err(e);
                }
            }

            let active = self.active_async_tasks.load(std::sync::atomic::Ordering::SeqCst);
            if active == 0 {
                let queue_empty = self.event_loop_queue.lock().unwrap().is_empty();
                if queue_empty {
                    break;
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::gc::gc_free_all;
    use super::super::compiler::Compiler;



    fn run_code(source: &str) -> Result<VM, String> {
        gc_free_all();
        let tokens = crate::frontend::lex(source);
        let mut parser = crate::frontend::Parser::new(tokens);
        let stmts = parser.parse().map_err(|e| e.to_string())?;
        let compiler = Compiler::new();
        let function = compiler.compile(&stmts)?;
        
        let mut vm = VM::new();
        vm.use_jit = true;
        vm.run(function)?;

        let mut jit_globals = std::collections::HashMap::new();
        for (k, v) in &vm.globals {
            jit_globals.insert(k.clone(), v.to_string());
        }

        // Clean up JIT allocations before running Interpreter
        gc_free_all();

        // Recompile to get fresh constants for the Interpreter run
        let tokens = crate::frontend::lex(source);
        let mut parser = crate::frontend::Parser::new(tokens);
        let stmts = parser.parse().map_err(|e| e.to_string())?;
        let compiler = Compiler::new();
        let function_interp = compiler.compile(&stmts)?;

        let mut vm_interp = VM::new();
        vm_interp.use_jit = false;
        vm_interp.run(function_interp)?;

        for (k, v_interp) in &vm_interp.globals {
            let v_jit_str = jit_globals.get(k).expect("Missing global in JIT");
            assert_eq!(v_jit_str, &v_interp.to_string(), "Global mismatch for '{}': JIT={}, Interpreter={}", k, v_jit_str, v_interp);
        }

        Ok(vm_interp)
    }

    #[test]
    fn test_arithmetic() {
        let vm = run_code("let res = 5 + 10 * 2").unwrap();
        assert_eq!(vm.get_global("res").unwrap().as_number(), 25.0);
    }

    #[test]
    fn test_logical() {
        let vm = run_code("let res1 = true and false\nlet res2 = false or true").unwrap();
        assert_eq!(vm.get_global("res1").unwrap().as_boolean(), false);
        assert_eq!(vm.get_global("res2").unwrap().as_boolean(), true);
    }

    #[test]
    fn test_for_loop() {
        let vm = run_code("let sum = 0\nfor i in 1..5 {\n  sum = sum + i\n}").unwrap();
        assert_eq!(vm.get_global("sum").unwrap().as_number(), 10.0);
    }

    #[test]
    fn test_if_else() {
        let vm = run_code("let res = 0\nif (1 < 2) {\n  res = 10\n} else {\n  res = 20\n}").unwrap();
        assert_eq!(vm.get_global("res").unwrap().as_number(), 10.0);
    }

    #[test]
    fn test_function_call() {
        let vm = run_code("let add = (a, b) => {\n  return a + b\n}\nlet res = add(3, 4)").unwrap();
        assert_eq!(vm.get_global("res").unwrap().as_number(), 7.0);
    }

    #[test]
    fn test_recursion() {
        let vm = run_code("let fib = (n) => {\n  if (n < 2) { return n }\n  return fib(n - 1) + fib(n - 2)\n}\nlet res = fib(5)").unwrap();
        assert_eq!(vm.get_global("res").unwrap().as_number(), 5.0);
    }

    #[test]
    fn test_array() {
        let vm = run_code("let arr = [10, 20]\narr.push(30)\nlet l = arr.length\nlet val = arr[1]").unwrap();
        assert_eq!(vm.get_global("l").unwrap().as_number(), 3.0);
        assert_eq!(vm.get_global("val").unwrap().as_number(), 20.0);
    }

    #[test]
    fn test_object() {
        let vm = run_code("let obj = { x: 100 }\nobj.x = 200\nlet val = obj.x").unwrap();
        assert_eq!(vm.get_global("val").unwrap().as_number(), 200.0);
    }

    #[test]
    fn test_incremental_garbage_collector() {
        gc_free_all();

        let parent_ptr = gc_allocate(GcData::Array(vec![]));
        let parent = Value::array(parent_ptr);

        let garbage_ptr = gc_allocate(GcData::Array(vec![]));
        let _garbage = Value::array(garbage_ptr);

        let mut vm = VM::new();
        vm.stack.push(parent);

        assert_eq!(GC_STATE.with(|s| s.borrow().phase), GcPhase::Pause);

        for _ in 0..10000 {
            gc_allocate(GcData::Array(vec![]));
        }

        vm.gc_step();
        assert_eq!(GC_STATE.with(|s| s.borrow().phase), GcPhase::Mark);

        while GC_STATE.with(|s| s.borrow().phase) != GcPhase::Sweep {
            vm.gc_step();
        }

        while GC_STATE.with(|s| s.borrow().phase) == GcPhase::Sweep {
            vm.gc_step();
        }

        assert_eq!(GC_STATE.with(|s| s.borrow().phase), GcPhase::Pause);

        let mut found_parent = false;
        let mut found_garbage = false;
        unsafe {
            let mut curr = GC_STATE.with(|s| s.borrow().head);
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

        gc_free_all();
    }

    #[test]
    fn test_imports_exports() {
        use std::fs;
        let dir = std::env::current_dir().unwrap().join("target").join("test_imports_exports");
        fs::create_dir_all(&dir).unwrap();
        
        let lib_path = dir.join("lib.er");
        fs::write(&lib_path, "export const value = 42\nexport const other = 100").unwrap();
        
        let main_path = dir.join("main.er");
        fs::write(&main_path, "import { value } from \"./lib.er\"\nlet res = value + 10").unwrap();

        let stmts = crate::frontend::parse_and_resolve_imports(&main_path).unwrap();
        let compiler = Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        
        let mut vm = VM::new();
        vm.use_jit = true;
        vm.run(function.clone()).unwrap();
        assert_eq!(vm.get_global("res").unwrap().as_number(), 52.0);

        // Test failing when name is not exported
        let main_bad_path = dir.join("main_bad.er");
        fs::write(&main_bad_path, "import { not_exist } from \"./lib.er\"\n").unwrap();
        assert!(crate::frontend::parse_and_resolve_imports(&main_bad_path).is_err());
        
        // Clean up
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_async_await_event_loop() {
        gc_free_all();
        let source = "
            let result = 0
            const get_val = () => {
                return 42
            }
            const main = () => {
                const pair = createPromisePair()
                setTimeout((resolve) => {
                    const x = get_val()
                    resolve(x)
                }, 0, pair.resolve)
                let x = futureAwait(pair.promise)
                result = x + 10
            }
            main()
        ";
        let tokens = crate::frontend::lex(source);
        let mut parser = crate::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        
        let mut vm = VM::new();
        vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
        vm.register_global("futureAwait", Value::native_function(crate::vm::er_http::native_future_await));
        vm.register_global("createPromisePair", Value::native_function(crate::vm::er_http::native_create_promise_pair));
        vm.use_jit = true;
        vm.run(function).unwrap();
        vm.run_event_loop().unwrap();
        
        assert_eq!(vm.get_global("result").unwrap().as_number(), 52.0);
    }
}
