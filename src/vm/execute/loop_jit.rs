use crate::vm::value::Value;
use crate::vm::bytecode::Function;
use crate::vm::gc::{gc_write_barrier, GcData, GcObject};
use super::types::{VM, CallFrame};

impl VM {
    #[allow(unused_assignments)]
    pub fn execute_loop(&mut self, target_depth: usize) -> Result<Value, String> {
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

            #[inline(always)]
            unsafe fn get_raw_func<'a>(mut p: *mut GcObject) -> &'a Function {
                if let GcData::BoundMethod(bm) = &(*p).data {
                    p = bm.function;
                }
                if let GcData::Closure(c) = &(*p).data {
                    p = c.function;
                }
                match &(*p).data {
                    GcData::Function(func) => func,
                    _ => unreachable!(),
                }
            }

            let mut func = get_raw_func(frame.function);
            let mut constants_ptr = func.chunk.constants.as_ptr();
            let mut slots_offset = frame.slots_offset;

            let mut stack_start;
            #[allow(unused_assignments)]
            let mut frame_slots;

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

                let raw_fn_ptr = match &(*frame.function).data {
                    GcData::Function(_) => frame.function,
                    GcData::Closure(c) => c.function,
                    _ => frame.function,
                };
                let raw_func = match &(*raw_fn_ptr).data {
                    GcData::Function(f) => f,
                    _ => unreachable!(),
                };

                let count = raw_func.invocation_count.get() + 1;
                raw_func.invocation_count.set(count);

                let can_jit = self.use_jit && !raw_func.is_async && raw_func.chunk.handlers.is_empty();
                let native_ptr = if !can_jit {
                    None
                } else if let Some(ptr) = raw_func.jit_ptr.get() {
                    Some(ptr)
                } else if self.jit_threshold == 0 || raw_func.has_loop || count >= self.jit_threshold || self.frames.len() <= 1 {
                    Some(crate::jit::compile_function(self, raw_fn_ptr))
                } else {
                    None
                };

                let native_ptr = if let Some(ptr) = native_ptr {
                    ptr
                } else {
                    let child_dest_reg = frame.dest_reg;
                    let initial_depth = self.frames.len() - 1;
                    let res = self.execute_loop_interpreter(initial_depth)?;
                    if self.frames.len() <= target_depth {
                        return Ok(res);
                    }
                    frame_ptr = {
                        let len = self.frames.len();
                        self.frames.as_mut_ptr().add(len - 1)
                    };
                    frame = &mut *frame_ptr;
                    func = get_raw_func(frame.function);
                    constants_ptr = func.chunk.constants.as_ptr();
                    slots_offset = frame.slots_offset;
                    reload_stack!();
                    *frame_slots.add(child_dest_reg) = res;
                    frame.ip += 1;
                    ip_val = frame.ip;
                    continue;
                };
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
                        let mut func_ptr = callee.as_gc_ptr();
                        if let GcData::BuiltinMethod(builtin) = &(*func_ptr).data {
                            let receiver = builtin.receiver;
                            let method = builtin.method;
                            let args = std::slice::from_raw_parts(frame_slots.add(func_reg_out + 1), arg_count_out);
                            frame.ip = ip_out;
                            let result = self.execute_builtin_method(receiver, method, args)?;
                            reload_stack!();
                            *frame_slots.add(dest_reg_out) = result;
                            frame.ip = ip_out + 1;
                            ip_val = frame.ip;
                            continue;
                        }
                        let mut actual_arg_count = arg_count_out;
                        if let GcData::BoundMethod(bound_method) = &(*func_ptr).data {
                            for i in (0..arg_count_out).rev() {
                                *frame_slots.add(func_reg_out + 2 + i) = *frame_slots.add(func_reg_out + 1 + i);
                            }
                            *frame_slots.add(func_reg_out + 1) = bound_method.receiver;
                            func_ptr = bound_method.function;
                            actual_arg_count = arg_count_out + 1;
                        }
                        let func_val = get_raw_func(func_ptr);
                        if actual_arg_count < func_val.arity {
                            for i in actual_arg_count..func_val.arity {
                                *frame_slots.add(func_reg_out + 1 + i) = Value::null();
                            }
                        } else if actual_arg_count > func_val.arity {
                            return Err(format!(
                                "Expected {} args but got {}",
                                func_val.arity, actual_arg_count
                            ));
                        }
                        // Save current IP (call instruction index: ip_out)
                        frame.ip = ip_out;
                        let new_slots_offset = slots_offset + func_reg_out + 1;
                        let needed = new_slots_offset + 256;
                        if self.stack.len() < needed {
                            self.stack.resize(needed, Value::null());
                        }
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
                        func = get_raw_func(frame.function);
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
                        frame.ip = ip_out + 1;
                        ip_val = frame.ip;
                    } else if callee.is_method_file() {
                        let ptr = (callee.0 & crate::vm::value::PTR_MASK & !3) as *mut GcObject;
                        let method_sub_tag = callee.0 & 3;

                        let path_str = match &(*ptr).data {
                            GcData::Object(map) => {
                                let name_key = crate::vm::gc::get_or_create_string("name");
                                let name_val = map.get(&crate::vm::value::MapKey(Value::string(name_key))).cloned().unwrap_or(Value::null());
                                match name_val.as_str() {
                                    Some(s) => s.to_string(),
                                    None => "".to_string(),
                                }
                            }
                            GcData::Struct(s) => {
                                let name_key = crate::vm::gc::get_or_create_string("name");
                                let name_val = s.get_field(Value::string(name_key)).unwrap_or(Value::null());
                                match name_val.as_str() {
                                    Some(s) => s.to_string(),
                                    None => "".to_string(),
                                }
                            }
                            _ => "".to_string(),
                        };

                        let result = match method_sub_tag {
                            0 => { // exists
                                Value::boolean(std::path::Path::new(&path_str).exists())
                            }
                            1 => { // text
                                if let Ok(content) = std::fs::read_to_string(&path_str) {
                                    let ptr = crate::vm::gc::gc_alloc_string(&content);
                                    Value::string(ptr)
                                } else {
                                    Value::null()
                                }
                            }
                            2 => { // json
                                if let Ok(content) = std::fs::read_to_string(&path_str) {
                                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
                                        crate::vm::gc::json_to_value(json_val)
                                    } else {
                                        Value::null()
                                    }
                                } else {
                                    Value::null()
                                }
                            }
                            _ => Value::null(),
                        };
                        reload_stack!();
                        *frame_slots.add(dest_reg_out) = result;
                        frame.ip = ip_out + 1;
                        ip_val = frame.ip;
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
                        frame.ip = ip_out + 1;
                        ip_val = frame.ip;
                    } else {
                        return Err(format!("[JIT] Can only call functions (callee: 0x{:x})", callee.0).into());
                    }
                } else if status == 1 {
                    // YieldReturn: a Return instruction yielded to the JIT orchestrator.
                    let caller_dest_reg = frame.dest_reg;
                    self.close_upvalues(frame.slots_offset);
                    self.frames.pop();
                    if self.frames.len() <= target_depth {
                        return Ok(ret_val_out);
                    }

                    frame_ptr = {
                        let len = self.frames.len();
                        self.frames.as_mut_ptr().add(len - 1)
                    };
                    frame = &mut *frame_ptr;
                    func = get_raw_func(frame.function);
                    constants_ptr = func.chunk.constants.as_ptr();
                    slots_offset = frame.slots_offset;
                    reload_stack!();

                    *frame_slots.add(caller_dest_reg) = ret_val_out;
                    frame.ip = frame.ip + 1;
                    ip_val = frame.ip;
                } else if status == 2 {
                    // YieldGc / YieldLoop
                    frame.ip = ip_out;
                    ip_val = ip_out;
                } else if status == 3 {
                    // YieldSuspend: an async Await or native function suspended the VM during JIT execution.
                    frame.ip = ip_out;
                    if !self.stack.is_empty() && func_reg_out < self.stack.len() {
                        let await_val = *frame_slots.add(func_reg_out);
                        if await_val.is_promise() {
                            let promise_ptr = await_val.as_gc_ptr();
                            self.close_upvalues(0);
                            let suspended_stack = std::mem::take(&mut self.stack);
                            let suspended_frames = std::mem::take(&mut self.frames);

                            match &mut (*promise_ptr).data {
                                GcData::Promise(prom) => {
                                    *prom.suspended_stack.lock().unwrap() = suspended_stack;
                                    *prom.suspended_frames.lock().unwrap() = suspended_frames;
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                    return Ok(Value::null());
                } else if status == 4 {
                    // YieldDeopt: Dynamic type bailout back to bytecode interpreter
                    frame.ip = ip_out;
                    return self.execute_loop_interpreter(target_depth);
                } else {
                    // RuntimeError, JIT Throw, or JIT execution error.
                    let thrown = if !ret_val_out.is_null() {
                        ret_val_out
                    } else if !self.thrown_value.is_null() {
                        let t = self.thrown_value;
                        self.thrown_value = Value::null();
                        t
                    } else if let Some(err_msg) = self.error.take() {
                        self.has_error_flag = 0;
                        let ptr = crate::vm::gc::gc_alloc_string(&err_msg);
                        Value::string(ptr)
                    } else {
                        let ptr = crate::vm::gc::intern_string("JIT execution error");
                        Value::string(ptr)
                    };

                    let initial_frame_idx = self.frames.len() - 1;
                    while !self.frames.is_empty() {
                        let frame_idx = self.frames.len() - 1;
                        let curr_ip = if frame_idx == initial_frame_idx {
                            ip_out
                        } else {
                            self.frames[frame_idx].ip
                        };
                        let curr_func = get_raw_func(self.frames[frame_idx].function);
                        if let Some(handler) = curr_func.chunk.find_handler(curr_ip).cloned() {
                            while self.frames.len() > frame_idx + 1 {
                                self.frames.pop();
                            }
                            let frame_slots_target = self.stack.as_mut_ptr().add(self.frames[frame_idx].slots_offset);
                            *frame_slots_target.add(handler.err_reg as usize) = thrown;
                            self.frames[frame_idx].ip = handler.catch_ip;
                            return self.execute_loop_interpreter(0);
                        } else {
                            if self.frames.len() > 1 {
                                self.frames.pop();
                            } else {
                                break;
                            }
                        }
                    }

                    let err_str = match thrown.as_str() {
                        Some(s) => s.to_string(),
                        None => format!("{}", thrown),
                    };
                    return Err(format!("Uncaught exception: {}", err_str));
                }
            }
        }
    }
}
