pub mod types;
pub mod gc_integration;
pub mod upvalues;
pub mod struct_lookup;
pub mod builtins;
pub mod event_loop;
pub mod loop_ops_math;
pub mod loop_ops_obj;
pub mod loop_ops_call;
pub mod loop_jit;
pub mod loop_impl;

#[cfg(test)]
pub mod tests;

pub use types::{VM, CallFrame, AsyncResult, EventLoopTask, VmTimer, VmTimerAction, PendingAsync, format_undeclared_var_error};
pub use gc_integration::{GC_TIME, GC_COUNT, er_gc_reset_stats, er_gc_print_stats};
pub use builtins::{get_string_builtin_method_id, get_array_builtin_method_id, get_object_builtin_method_id};

use std::rc::Rc;
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicUsize, AtomicU64};
use std::collections::BinaryHeap;
use fnv::FnvHashMap;

use crate::vm::value::Value;
use crate::vm::bytecode::Function;
use crate::vm::gc::{gc_allocate, GcData, GcObject};

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

impl VM {
    pub fn new() -> Self {
        let use_jit = std::env::var("ER_NO_JIT").is_err();
        let jit_threshold = if let Ok(val) = std::env::var("ER_JIT_THRESHOLD") {
            val.parse::<usize>().unwrap_or(20)
        } else if std::env::var("ER_EAGER_JIT").is_ok() {
            0
        } else {
            20
        };
        crate::vm::alloc::init_allocator_options();
        Self {
            has_error_flag: 0,
            frames: Vec::new(),
            stack: Vec::with_capacity(1024),
            globals: FnvHashMap::default(),
            error: None,
            mir_ctx: None,
            use_jit,
            jit_threshold,
            alloc_count_local: 0,
            use_evented_io: true,
            structs: FnvHashMap::default(),
            auto_shapes: FnvHashMap::default(),
            last_matched_keys: Vec::new(),
            last_matched_descriptor: None,
            last_matched_offsets: Vec::new(),
            open_upvalues: Vec::new(),
            thrown_value: Value::null(),
            event_loop_queue: Arc::new(Mutex::new(Vec::new())),
            event_loop_condvar: Arc::new(Condvar::new()),
            active_async_tasks: Arc::new(AtomicUsize::new(0)),
            pending_callbacks: Arc::new(Mutex::new(Vec::new())),
            timers: Arc::new(Mutex::new(BinaryHeap::new())),
            next_timer_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn reset_jit(&mut self) {
        crate::jit::reset_jit_state();
    }

    pub fn register_global(&mut self, name: &str, value: Value) {
        self.globals.insert(Rc::from(name), value);
    }

    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    pub fn run(&mut self, function: Function) -> Result<Value, String> {
        let func_ptr = gc_allocate(GcData::Function(Box::new(function)));
        self.run_function_ptr(func_ptr)
    }

    pub fn run_function_ptr(&mut self, func_ptr: *mut GcObject) -> Result<Value, String> {
        let prev_vm = crate::vm::er_http::ACTIVE_VM.with(|active| active.replace(self as *mut VM));
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

    pub fn call_function_reentrant(&mut self, callee: Value, args: Vec<Value>) -> Result<Value, String> {
        if callee.is_method_resolve() {
            let promise_ptr = callee.as_gc_ptr();
            let arg = args.first().copied().unwrap_or(Value::null());
            let queue = self.event_loop_queue.clone();
            let condvar = self.event_loop_condvar.clone();
            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolvePromise(promise_ptr, arg),
            });
            condvar.notify_one();
            return Ok(Value::null());
        }

        if callee.is_array_method_push() || callee.is_array_method_pop() {
            let ptr = callee.as_gc_ptr();
            let result = unsafe {
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        if callee.is_array_method_push() {
                            for arg in &args {
                                crate::vm::gc::gc_write_barrier(ptr, arg);
                                arr.push(*arg);
                            }
                            Value::number(arr.len() as f64)
                        } else {
                            arr.pop().unwrap_or(Value::null())
                        }
                    }
                    _ => Value::null(),
                }
            };
            return Ok(result);
        }

        if !callee.is_function() {
            if callee.is_native_function() {
                let func = callee.as_native_fn();
                return Ok(func(args));
            }
            return Err("Callee is not a function".to_string());
        }

        let mut func_ptr = callee.as_gc_ptr();
        let mut final_args = args;
        unsafe {
            if let GcData::BuiltinMethod(builtin) = &(*func_ptr).data {
                let receiver = builtin.receiver;
                let method = builtin.method;
                return self.execute_builtin_method(receiver, method, &final_args);
            }
            if let GcData::BoundMethod(bound_method) = &(*func_ptr).data {
                final_args.insert(0, bound_method.receiver);
                func_ptr = bound_method.function;
            }
        }
        let raw_fn_ptr = match unsafe { &(*func_ptr).data } {
            GcData::Function(_) => func_ptr,
            GcData::Closure(c) => c.function,
            _ => return Err("Callee is not a callable function".to_string()),
        };
        let raw_func = match unsafe { &(*raw_fn_ptr).data } {
            GcData::Function(f) => f,
            _ => unreachable!(),
        };
        if final_args.len() < raw_func.arity {
            final_args.resize(raw_func.arity, Value::null());
        } else if final_args.len() > raw_func.arity {
            final_args.truncate(raw_func.arity);
        }

        let old_frames = std::mem::take(&mut self.frames);
        let old_stack_len = self.stack.len();
        
        for arg in &final_args {
            self.stack.push(*arg);
        }
        
        self.frames.push(CallFrame {
            function: func_ptr,
            ip: 0,
            slots_offset: old_stack_len,
            dest_reg: 0,
        });
        
        let old_fn_vals: Vec<Value> = old_frames.iter().map(|f| Value::function(f.function)).collect();
        crate::vm::gc::gc_push_temp_slice(old_fn_vals.as_ptr(), old_fn_vals.len());
        
        let needed = old_stack_len + 256;
        if self.stack.len() < needed {
            self.stack.resize(needed, Value::null());
        }

        let res = if self.use_jit {
            self.execute_loop(0)
        } else {
            self.execute_loop_interpreter(0)
        };
        
        crate::vm::gc::gc_pop_temp_slice();
        
        self.frames = old_frames;
        self.stack.truncate(old_stack_len);
        
        res
    }

    fn execute(&mut self) -> Result<Value, String> {
        let original_len = self.stack.len();
        let needed = original_len + 256;
        if self.stack.len() < needed {
            self.stack.resize(needed, Value::null());
        }
        
        let res = if self.use_jit {
            self.execute_loop(0)
        } else {
            self.execute_loop_interpreter(0)
        };
        
        self.stack.truncate(original_len);
        res
    }
}
