use std::collections::HashMap;
use std::rc::Rc;
use super::value::Value;
use super::bytecode::Function;
use super::gc::{
    gc_allocate, gc_write_barrier, gc_blacken_object, mark_value,
    GC_HEAD, GC_PHASE, GC_ROOTS, GRAY_STACK, ALLOC_COUNT, SWEEP_PTR, PREV_SWEEP_PTR,
    GcColor, GcPhase, GcData, GcObject
};

pub struct VM {
    pub frames: Vec<CallFrame>,
    pub stack: Vec<Value>,
    pub globals: HashMap<Rc<str>, Value>,
    pub error: Option<String>,
    pub mir_ctx: Option<*mut std::ffi::c_void>,
}

pub struct CallFrame {
    pub function: *mut GcObject,
    pub ip: usize,
    pub slots_offset: usize,
    pub dest_reg: usize,
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for VM {
    fn drop(&mut self) {
        if let Some(ctx) = self.mir_ctx {
            super::jit::cleanup_jit(ctx);
        }
    }
}

impl VM {
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            stack: Vec::new(),
            globals: HashMap::new(),
            error: None,
            mir_ctx: None,
        }
    }

    pub fn register_global(&mut self, name: &str, value: Value) {
        self.globals.insert(Rc::from(name), value);
    }

    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    pub fn run(&mut self, function: Function) -> Result<Value, String> {
        let func_ptr = gc_allocate(GcData::Function(function));
        self.frames.push(CallFrame {
            function: func_ptr,
            ip: 0,
            slots_offset: self.stack.len(),
            dest_reg: 0,
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
                        mark_value(&Value::function(frame.function));
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

    fn _gc_trigger(&mut self) {
        self.gc_step();
        self.gc_step();
    }

    fn execute(&mut self) -> Result<Value, String> {
        let original_len = self.stack.len();
        self.stack.resize(4096, Value::null());
        
        let res = self.execute_loop(original_len);
        
        self.stack.truncate(self.stack.len());
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
                // Ensure the current function is JIT compiled
                let native_ptr = super::jit::compile_function(self, frame.function);
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
                        if arg_count_out != func_val.arity {
                            return Err(format!(
                                "Expected {} args but got {}",
                                func_val.arity, arg_count_out
                            ));
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
                        *frame_slots.add(dest_reg_out) = result;
                        ip_val = ip_out;
                    } else if callee.is_array_method_push() || callee.is_array_method_pop() {
                        let ptr = callee.as_gc_ptr();
                        let mut args = Vec::with_capacity(arg_count_out);
                        for i in 0..arg_count_out {
                            args.push(*frame_slots.add(func_reg_out + 1 + i));
                        }
                        let result = match &mut (*ptr).data {
                            GcData::Array(arr) => {
                                if callee.is_array_method_push() {
                                    for arg in args {
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
                } else {
                    // RuntimeError or JIT compilation/execution error.
                    let err_msg = self.error.take().unwrap_or_else(|| "JIT execution error".into());
                    return Err(err_msg);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::gc::gc_free_all;
    use super::super::compiler::Compiler;

    fn run_code(source: &str) -> Result<VM, String> {
        let tokens = crate::frontend::lex(source);
        let mut parser = crate::frontend::Parser::new(tokens);
        let stmts = parser.parse().map_err(|e| e.to_string())?;
        let compiler = Compiler::new();
        let function = compiler.compile(&stmts)?;
        let mut vm = VM::new();
        vm.run(function)?;
        Ok(vm)
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

        assert_eq!(GC_PHASE.with(|p| p.get()), GcPhase::Pause);

        for _ in 0..10 {
            gc_allocate(GcData::Array(vec![]));
        }

        vm.gc_step();
        assert_eq!(GC_PHASE.with(|p| p.get()), GcPhase::Mark);

        while GC_PHASE.with(|p| p.get()) != GcPhase::Sweep {
            vm.gc_step();
        }

        while GC_PHASE.with(|p| p.get()) == GcPhase::Sweep {
            vm.gc_step();
        }

        assert_eq!(GC_PHASE.with(|p| p.get()), GcPhase::Pause);

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

        gc_free_all();
    }
}
