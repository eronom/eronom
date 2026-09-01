use std::rc::Rc;
use fnv::FnvHashMap;
use crate::vm::value::{Value, MapKey};
use crate::vm::bytecode::{Function, Instruction};
use crate::vm::gc::{gc_allocate, gc_write_barrier, GcData, GcObject, GcClosure, StructDescriptor};
use super::types::{VM, CallFrame};

pub enum CallOpOutcome {
    ContinueLoop,
    ReturnResult(Value),
    EnterInterpreterFrame,
}

impl VM {
    pub unsafe fn execute_call_op(
        &mut self,
        instruction: &Instruction,
        frame_slots: *mut Value,
        slots_offset: usize,
        code_ptr: *const Instruction,
        ip: *const Instruction,
        target_depth: usize,
        _get_raw_func: impl Fn(*mut GcObject) -> &'static Function,
    ) -> Result<CallOpOutcome, String> { unsafe {
        let dest = instruction.ra as usize;
        let func_reg = instruction.rb as usize;
        let arg_count = instruction.operand as usize;
        let callee = *frame_slots.add(func_reg);
        if callee.is_function() {
            let mut func_ptr = callee.as_gc_ptr();
            if let GcData::BuiltinMethod(builtin) = &(*func_ptr).data {
                let receiver = builtin.receiver;
                let method = builtin.method;
                let args = std::slice::from_raw_parts(frame_slots.add(func_reg + 1), arg_count);
                let frame = self.frames.last_mut().unwrap();
                frame.ip = ip.offset_from(code_ptr) as usize - 1;
                let result = self.execute_builtin_method(receiver, method, args)?;
                let stack_start = self.stack.as_mut_ptr();
                let slots = stack_start.add(slots_offset);
                *slots.add(dest) = result;
                return Ok(CallOpOutcome::ContinueLoop);
            }
            let mut actual_arg_count = arg_count;
            if let GcData::BoundMethod(bound_method) = &(*func_ptr).data {
                for i in (0..arg_count).rev() {
                    *frame_slots.add(func_reg + 2 + i) = *frame_slots.add(func_reg + 1 + i);
                }
                *frame_slots.add(func_reg + 1) = bound_method.receiver;
                func_ptr = bound_method.function;
                actual_arg_count = arg_count + 1;
            }
            let raw_fn_ptr = match &(*func_ptr).data {
                GcData::Function(_) => func_ptr,
                GcData::Closure(c) => c.function,
                _ => func_ptr,
            };
            let raw_func = match &(*raw_fn_ptr).data {
                GcData::Function(f) => f,
                _ => unreachable!(),
            };
            if actual_arg_count < raw_func.arity {
                for i in actual_arg_count..raw_func.arity {
                    *frame_slots.add(func_reg + 1 + i) = Value::null();
                }
            } else if actual_arg_count > raw_func.arity {
                return Err(format!(
                    "Expected {} args but got {}",
                    raw_func.arity, actual_arg_count
                ));
            }

            let count = raw_func.invocation_count.get() + 1;
            raw_func.invocation_count.set(count);

            if self.use_jit && !raw_func.is_async && raw_func.chunk.handlers.is_empty() {
                if raw_func.jit_ptr.get().is_none() && (self.jit_threshold == 0 || raw_func.has_loop || count >= self.jit_threshold) {
                    crate::jit::compile_function(self, raw_fn_ptr);
                }
                if raw_func.jit_ptr.get().is_some() {
                    let frame = self.frames.last_mut().unwrap();
                    frame.ip = ip.offset_from(code_ptr) as usize - 1;
                    let new_slots_offset = slots_offset + func_reg + 1;
                    let needed = new_slots_offset + 256;
                    if self.stack.len() < needed {
                        self.stack.resize(needed, Value::null());
                    }
                    self.frames.push(CallFrame {
                        function: func_ptr,
                        ip: 0,
                        slots_offset: new_slots_offset,
                        dest_reg: dest,
                    });

                    let initial_depth = self.frames.len() - 1;
                    let res = self.execute_loop(initial_depth)?;
                    if self.frames.len() <= target_depth {
                        return Ok(CallOpOutcome::ReturnResult(res));
                    }
                    let stack_start = self.stack.as_mut_ptr();
                    let slots = stack_start.add(slots_offset);
                    *slots.add(dest) = res;
                    let frame = self.frames.last_mut().unwrap();
                    frame.ip += 1;
                    return Ok(CallOpOutcome::ContinueLoop);
                }
            }

            let frame = self.frames.last_mut().unwrap();
            frame.ip = ip.offset_from(code_ptr) as usize - 1;
            let new_slots_offset = slots_offset + func_reg + 1;
            let needed = new_slots_offset + 256;
            if self.stack.len() < needed {
                self.stack.resize(needed, Value::null());
            }
            self.frames.push(CallFrame {
                function: func_ptr,
                ip: 0,
                slots_offset: new_slots_offset,
                dest_reg: dest,
            });
            return Ok(CallOpOutcome::EnterInterpreterFrame);
        } else if callee.is_native_function() {
            let native = callee.as_native_fn();
            let mut args = Vec::with_capacity(arg_count);
            for i in 0..arg_count {
                args.push(*frame_slots.add(func_reg + 1 + i));
            }
            let frame = self.frames.last_mut().unwrap();
            frame.ip = ip.offset_from(code_ptr) as usize - 1;
            let result = native(args);
            if self.stack.is_empty() {
                return Ok(CallOpOutcome::ReturnResult(Value::null()));
            }
            let stack_start = self.stack.as_mut_ptr();
            let slots = stack_start.add(slots_offset);
            *slots.add(dest) = result;
        } else if callee.is_method_file() {
            let ptr = (callee.0 & crate::vm::value::PTR_MASK & !3) as *mut GcObject;
            let method_sub_tag = callee.0 & 3;

            let path_str = match &(*ptr).data {
                GcData::Object(map) => {
                    let name_key = crate::vm::gc::get_or_create_string("name");
                    let name_val = map.get(&MapKey(Value::string(name_key))).cloned().unwrap_or(Value::null());
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
            let stack_start = self.stack.as_mut_ptr();
            let slots = stack_start.add(slots_offset);
            *slots.add(dest) = result;
        } else if callee.is_method_json() || callee.is_method_text() {
            let ptr = callee.as_gc_ptr();
            let result = match &(*ptr).data {
                GcData::Object(map) => {
                    let body_key = crate::vm::gc::get_or_create_string("_body");
                    let body_val = map.get(&MapKey(Value::string(body_key))).cloned().unwrap_or(Value::null());
                    if callee.is_method_json() {
                        if body_val.is_string() {
                            let s = body_val.as_str().unwrap_or("");
                            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(s) {
                                crate::vm::gc::json_to_value(json_val)
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
            let stack_start = self.stack.as_mut_ptr();
            let slots = stack_start.add(slots_offset);
            *slots.add(dest) = result;
        } else if callee.is_method_send_json() {
            let res_ptr = (callee.0 & crate::vm::value::PTR_MASK) as *mut std::ffi::c_void;
            if !res_ptr.is_null() {
                let arg = if arg_count > 0 {
                    *frame_slots.add(func_reg + 1)
                } else {
                    Value::null()
                };
                let json_val = crate::vm::er_http::value_to_json(arg);
                let json_str = serde_json::to_string(&json_val).unwrap_or_else(|_| "null".to_string());
                crate::vm::er_http::end_http_response_json(res_ptr, &json_str);
            }
            let stack_start = self.stack.as_mut_ptr();
            let slots = stack_start.add(slots_offset);
            *slots.add(dest) = Value::null();
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
            let stack_start = self.stack.as_mut_ptr();
            let slots = stack_start.add(slots_offset);
            *slots.add(dest) = Value::null();
        } else if callee.is_array_method_push() || callee.is_array_method_pop() {
            let ptr = callee.as_gc_ptr();
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
            let stack_start = self.stack.as_mut_ptr();
            let slots = stack_start.add(slots_offset);
            *slots.add(dest) = result;
        } else if callee.is_object() && matches!(&(*callee.as_gc_ptr()).data, GcData::StructConstructor(_)) {
            let ptr = callee.as_gc_ptr();
            let descriptor = match &(*ptr).data {
                GcData::StructConstructor(desc) => desc.clone(),
                _ => unreachable!(),
            };
            let mut args = Vec::with_capacity(arg_count);
            for i in 0..arg_count {
                args.push(*frame_slots.add(func_reg + 1 + i));
            }
            let frame = self.frames.last_mut().unwrap();
            frame.ip = ip.offset_from(code_ptr) as usize - 1;
            let result = crate::jit::helpers::construct_struct_from_args_helper(&descriptor, args)?;
            let stack_start = self.stack.as_mut_ptr();
            let slots = stack_start.add(slots_offset);
            *slots.add(dest) = result;
        } else {
            return Err(format!("Can only call functions (callee: 0x{:x})", callee.0).into());
        }
        Ok(CallOpOutcome::ContinueLoop)
    }}

    pub unsafe fn execute_await_op(
        &mut self,
        instruction: &Instruction,
        frame_slots: *mut Value,
        curr_ip: usize,
    ) -> Result<Option<Value>, String> { unsafe {
        let await_value = *frame_slots.add(instruction.rb as usize);
        if await_value.is_promise() {
            let promise_ptr = await_value.as_gc_ptr();
            let state = match &(*promise_ptr).data {
                GcData::Promise(prom) => prom.state.clone(),
                _ => unreachable!(),
            };
            let promise_status = {
                let lock = state.lock().unwrap();
                lock.clone()
            };
            match promise_status {
                crate::vm::gc::PromiseState::Fulfilled(val) => {
                    *frame_slots.add(instruction.ra as usize) = val;
                    Ok(None)
                }
                crate::vm::gc::PromiseState::Rejected(err) => {
                    Err(err)
                }
                crate::vm::gc::PromiseState::Pending => {
                    let frame = self.frames.last_mut().unwrap();
                    frame.ip = curr_ip;

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
                    Ok(Some(Value::null()))
                }
            }
        } else {
            *frame_slots.add(instruction.ra as usize) = await_value;
            Ok(None)
        }
    }}

    pub unsafe fn execute_define_struct_op(
        &mut self,
        instruction: &Instruction,
        constants_ptr: *const Value,
    ) { unsafe {
        let name_val = *constants_ptr.add(instruction.operand as usize);
        let fields_val = *constants_ptr.add(instruction.ra as usize);
        let name_rc: Rc<str> = Rc::from(name_val.as_str().unwrap_or(""));
        let fields_vec = match &(*fields_val.as_gc_ptr()).data {
            GcData::Array(arr) => arr,
            _ => unreachable!(),
        };
        
        let mut field_indices = FnvHashMap::default();
        for (idx, &f_val) in fields_vec.iter().enumerate() {
            field_indices.insert(MapKey(f_val), idx);
        }

        let methods_val = *constants_ptr.add(instruction.rb as usize);
        let mut methods = FnvHashMap::default();
        if methods_val.is_object() {
            let methods_ptr = methods_val.as_gc_ptr();
            if let GcData::Object(map) = &(*methods_ptr).data {
                for (k, &v) in map {
                    methods.insert(*k, v);
                }
            }
        }
        
        let descriptor = std::rc::Rc::new(StructDescriptor::new(
            name_rc.clone(),
            field_indices,
            methods,
        ));
        self.structs.insert(name_rc.clone(), descriptor.clone());
        let ptr = gc_allocate(GcData::StructConstructor(descriptor));
        self.globals.insert(name_rc, Value::object(ptr));
    }}

    pub unsafe fn execute_closure_op(
        &mut self,
        instruction: &Instruction,
        frame_fn_ptr: *mut GcObject,
        slots_offset: usize,
        constants_ptr: *const Value,
        frame_slots: *mut Value,
    ) { unsafe {
        let dest = instruction.ra as usize;
        let const_idx = instruction.operand as usize;
        let raw_fn_val = *constants_ptr.add(const_idx);
        let raw_fn_ptr = raw_fn_val.as_gc_ptr();
        let fn_proto = match &(*raw_fn_ptr).data {
            GcData::Function(f) => f,
            _ => unreachable!(),
        };
        let mut upvalue_ptrs = Vec::with_capacity(fn_proto.upvalues.len());
        for uv_desc in &fn_proto.upvalues {
            if uv_desc.is_local {
                let abs_slot = slots_offset + uv_desc.index as usize;
                let uv_ptr = self.capture_upvalue(abs_slot);
                upvalue_ptrs.push(uv_ptr);
            } else {
                let parent_uv_ptr = match &(*frame_fn_ptr).data {
                    GcData::Closure(c) => c.upvalues[uv_desc.index as usize],
                    _ => unreachable!(),
                };
                upvalue_ptrs.push(parent_uv_ptr);
            }
        }
        let closure_ptr = gc_allocate(GcData::Closure(GcClosure {
            function: raw_fn_ptr,
            upvalues: upvalue_ptrs,
        }));
        *frame_slots.add(dest) = Value::function(closure_ptr);
    }}
}
