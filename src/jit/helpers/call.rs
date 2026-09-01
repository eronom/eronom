use std::time::Instant;
use crate::vm::execute::VM;
use crate::vm::value::Value;
use crate::vm::gc::{gc_write_barrier, GcData, get_or_create_string};
use crate::vm::value::MapKey;
use crate::jit::profile::{JIT_PROFILING, JIT_PROFILER};
use super::property::construct_struct_from_args_helper;

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_call_fast(
    vm: *mut VM,
    callee: Value,
    callee_frame_slots: *mut Value,
    dest: *mut Value,
    inst_idx: i64,
    dest_reg: i64,
) -> i64 {
    unsafe {
        if let Some(frame) = (*vm).frames.last_mut() {
            frame.ip = inst_idx as usize;
        }
        if !callee.is_function() {
            return -1;
        }
        let func_ptr = (callee.0 & crate::vm::value::PTR_MASK) as *mut crate::vm::gc::GcObject;
        let (raw_fn_ptr, func) = match &(*func_ptr).data {
            GcData::Function(f) => (func_ptr, f),
            GcData::Closure(c) => match &(*c.function).data {
                GcData::Function(f) => (c.function, f),
                _ => return -1,
            },
            _ => return -1,
        };

        if func.is_async || !func.chunk.handlers.is_empty() {
            return -1;
        }

        let count = func.invocation_count.get() + 1;
        func.invocation_count.set(count);

        let native_ptr = if let Some(ptr) = func.jit_ptr.get() {
            ptr
        } else if (*vm).jit_threshold == 0 || func.has_loop || count >= (*vm).jit_threshold {
            crate::jit::compile_function(&mut *vm, raw_fn_ptr)
        } else {
            return -1;
        };

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

        let jit_fn: JitFn = std::mem::transmute(native_ptr);
        let constants_ptr = func.chunk.constants.as_ptr();

        let mut ip_out: usize = 0;
        let mut dest_reg_out: usize = 0;
        let mut func_reg_out: usize = 0;
        let mut arg_count_out: usize = 0;
        let mut ret_val_out: Value = Value::null();

        let slots_offset = callee_frame_slots.offset_from((*vm).stack.as_ptr()) as usize;
        (*vm).frames.push(crate::vm::execute::CallFrame {
            function: func_ptr,
            ip: 0,
            slots_offset,
            dest_reg: dest_reg as usize,
        });

        let res = jit_fn(
            vm,
            callee_frame_slots,
            constants_ptr,
            0,
            &mut ip_out,
            &mut dest_reg_out,
            &mut func_reg_out,
            &mut arg_count_out,
            &mut ret_val_out,
        );

        if res == 1 {
            if !(*vm).stack.is_empty() {
                (*vm).close_upvalues(slots_offset);
                (*vm).frames.pop();
            }
            *dest = ret_val_out;
            0
        } else if res == 3 {
            -3
        } else if res == 4 {
            let initial_depth = (*vm).frames.len() - 1;
            match (*vm).execute_loop_interpreter(initial_depth) {
                Ok(val) => {
                    *dest = val;
                    0
                }
                Err(e) => {
                    (*vm).error = Some(e);
                    -2
                }
            }
        } else {
            if !ret_val_out.is_null() {
                (*vm).thrown_value = ret_val_out;
            }
            -2
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_call_non_vm(
    _vm: *mut VM,
    dest: *mut Value,
    callee: Value,
    func_reg: i64,
    arg_count: i64,
    frame_slots: *mut Value,
    inst_idx: i64,
    dest_reg: i64,
) -> i64 {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        if let Some(frame) = (*_vm).frames.last_mut() {
            frame.ip = inst_idx as usize;
        }
        let status = if callee.is_function() {
            let mut func_ptr = callee.as_gc_ptr();
            if let GcData::BuiltinMethod(builtin) = &(*func_ptr).data {
                let receiver = builtin.receiver;
                let method = builtin.method;
                let args = std::slice::from_raw_parts(frame_slots.offset((func_reg + 1) as isize), arg_count as usize);
                match (*_vm).execute_builtin_method(receiver, method, args) {
                    Ok(res) => {
                        *dest = res;
                        0
                    }
                    Err(err) => {
                        (*_vm).error = Some(err);
                        (*_vm).has_error_flag = 1;
                        -2
                    }
                }
            } else {
                let mut actual_arg_count = arg_count as usize;
                let mut callee_frame_slots = frame_slots.offset((func_reg + 1) as isize);

                if let GcData::BoundMethod(bound_method) = &(*func_ptr).data {
                    for i in (0..actual_arg_count).rev() {
                        *frame_slots.offset((func_reg + 2 + i as i64) as isize) = *frame_slots.offset((func_reg + 1 + i as i64) as isize);
                    }
                    *frame_slots.offset((func_reg + 1) as isize) = bound_method.receiver;
                    func_ptr = bound_method.function;
                    actual_arg_count += 1;
                }

                let raw_fn_ptr = match &(*func_ptr).data {
                    GcData::Function(_) => func_ptr,
                    GcData::Closure(c) => c.function,
                    _ => return -1,
                };

                let func_val = match &(*raw_fn_ptr).data {
                    GcData::Function(func) => func,
                    _ => return -1,
                };

                if actual_arg_count < func_val.arity {
                    for i in actual_arg_count..func_val.arity {
                        *callee_frame_slots.add(i) = Value::null();
                    }
                } else if actual_arg_count > func_val.arity {
                    (*_vm).error = Some(format!(
                        "Expected {} args but got {}",
                        func_val.arity, actual_arg_count
                    ));
                    (*_vm).has_error_flag = 1;
                    return -2;
                }

                if func_val.is_async || !func_val.chunk.handlers.is_empty() {
                    return -1; // Fallback to host VM loop for async or exception handlers
                } else {
                    let offset_from_base = callee_frame_slots.offset_from((*_vm).stack.as_ptr()) as usize;
                    if offset_from_base + 512 >= (*_vm).stack.len() {
                        let new_len = (*_vm).stack.len() * 2;
                        (*_vm).stack.resize(new_len, Value::null());
                        callee_frame_slots = (*_vm).stack.as_mut_ptr().add(offset_from_base);
                    }

                    let count = func_val.invocation_count.get() + 1;
                    func_val.invocation_count.set(count);

                    let native_ptr = if let Some(ptr) = func_val.jit_ptr.get() {
                        ptr
                    } else if (*_vm).jit_threshold == 0 || func_val.has_loop || count >= (*_vm).jit_threshold {
                        crate::jit::compile_function(&mut *_vm, raw_fn_ptr)
                    } else {
                        return -1;
                    };

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

                    let jit_fn: JitFn = std::mem::transmute(native_ptr);
                    let constants_ptr = func_val.chunk.constants.as_ptr();

                    let mut ip_out: usize = 0;
                    let mut dest_reg_out: usize = 0;
                    let mut func_reg_out: usize = 0;
                    let mut arg_count_out: usize = 0;
                    let mut ret_val_out: Value = Value::null();

                    let slots_offset = callee_frame_slots.offset_from((*_vm).stack.as_ptr()) as usize;
                    (*_vm).frames.push(crate::vm::execute::CallFrame {
                        function: func_ptr,
                        ip: 0,
                        slots_offset,
                        dest_reg: dest_reg as usize,
                    });

                    let jit_res = jit_fn(
                        _vm,
                        callee_frame_slots,
                        constants_ptr,
                        0,
                        &mut ip_out,
                        &mut dest_reg_out,
                        &mut func_reg_out,
                        &mut arg_count_out,
                        &mut ret_val_out,
                    );

                    if jit_res == 1 {
                        if !(*_vm).stack.is_empty() {
                            (*_vm).close_upvalues(slots_offset);
                            (*_vm).frames.pop();
                        }
                        *dest = ret_val_out;
                        0
                    } else if jit_res == 3 {
                        -3
                    } else if jit_res == 4 {
                        let initial_depth = (*_vm).frames.len() - 1;
                        match (*_vm).execute_loop_interpreter(initial_depth) {
                            Ok(val) => {
                                *dest = val;
                                0
                            }
                            Err(e) => {
                                (*_vm).error = Some(e);
                                -2
                            }
                        }
                    } else {
                        if !ret_val_out.is_null() {
                            (*_vm).thrown_value = ret_val_out;
                        }
                        -2
                    }
                }
            }
        } else if callee.is_native_function() {
            let native = callee.as_native_fn();
            let mut args = Vec::with_capacity(arg_count as usize);
            for i in 0..arg_count {
                args.push(*frame_slots.offset((func_reg + 1 + i) as isize));
            }
            let result = native(args);
            if (*_vm).stack.is_empty() {
                -3
            } else {
                *dest = result;
                0
            }
        } else if callee.is_object() && matches!(&(*callee.as_gc_ptr()).data, GcData::StructConstructor(_)) {
            let ptr = callee.as_gc_ptr();
            let descriptor = match &(*ptr).data {
                GcData::StructConstructor(desc) => desc.clone(),
                _ => return -1,
            };
            let mut args = Vec::with_capacity(arg_count as usize);
            for i in 0..arg_count {
                args.push(*frame_slots.offset((func_reg + 1 + i) as isize));
            }
            match construct_struct_from_args_helper(&descriptor, args) {
                Ok(result) => {
                    *dest = result;
                    0
                }
                Err(err) => {
                    (*_vm).error = Some(err);
                    -2
                }
            }
        } else if callee.is_method_json() || callee.is_method_text() {
            let ptr = callee.as_gc_ptr();
            let result = match &(*ptr).data {
                GcData::Object(map) => {
                    let body_key = get_or_create_string("_body");
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
                _ => Value::null(),
            };
            *dest = result;
            0
        } else if callee.is_method_send_json() {
            let res_ptr = (callee.0 & crate::vm::value::PTR_MASK) as *mut std::ffi::c_void;
            if !res_ptr.is_null() {
                let arg = if arg_count > 0 {
                    *frame_slots.offset((func_reg + 1) as isize)
                } else {
                    Value::null()
                };
                let json_val = crate::vm::er_http::value_to_json(arg);
                let json_str = serde_json::to_string(&json_val).unwrap_or_else(|_| "null".to_string());
                crate::vm::er_http::end_http_response_json(res_ptr, &json_str);
            }
            *dest = Value::null();
            0
        } else if callee.is_method_resolve() {
            let promise_ptr = callee.as_gc_ptr();
            let arg = if arg_count > 0 {
                *frame_slots.offset((func_reg + 1) as isize)
            } else {
                Value::null()
            };
            let queue = (*_vm).event_loop_queue.clone();
            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolvePromise(promise_ptr, arg),
            });
            *dest = Value::null();
            0
        } else if callee.is_array_method_push() || callee.is_array_method_pop() {
            let ptr = callee.as_gc_ptr();
            let result = match &mut (*ptr).data {
                GcData::Array(arr) => {
                    if callee.is_array_method_push() {
                        for i in 0..arg_count {
                            let arg = *frame_slots.offset((func_reg + 1 + i) as isize);
                            gc_write_barrier(ptr, &arg);
                            arr.push(arg);
                        }
                        Value::number(arr.len() as f64)
                    } else {
                        arr.pop().unwrap_or(Value::null())
                    }
                }
                _ => Value::null(),
            };
            *dest = result;
            0
        } else {
            -1 // Not a native function or method, needs fallback
        };
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.call_non_vm_count += 1;
                s.call_non_vm_time += start_time.unwrap().elapsed();
            });
        }
        status
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_array_push(arr_val: Value, arg: Value) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let ptr = arr_val.as_gc_ptr();
        let res = match &mut (*ptr).data {
            GcData::Array(arr) => {
                gc_write_barrier(ptr, &arg);
                arr.push(arg);
                Value::number(arr.len() as f64)
            }
            _ => Value::null(),
        };
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.call_non_vm_count += 1;
                s.call_non_vm_time += start_time.unwrap().elapsed();
            });
        }
        res
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_array_pop(arr_val: Value) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let ptr = arr_val.as_gc_ptr();
        let res = match &mut (*ptr).data {
            GcData::Array(arr) => {
                arr.pop().unwrap_or(Value::null())
            }
            _ => Value::null(),
        };
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.call_non_vm_count += 1;
                s.call_non_vm_time += start_time.unwrap().elapsed();
            });
        }
        res
    }
}
