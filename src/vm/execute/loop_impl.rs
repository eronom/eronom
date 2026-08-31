use std::rc::Rc;
use crate::vm::value::Value;
use crate::vm::bytecode::{Function, OpCode};
use crate::vm::gc::{gc_allocate, get_pooled_vec, GcData, GcObject};
use super::types::{VM, format_undeclared_var_error};
use super::loop_ops_math::execute_math_and_cmp_op;
use super::loop_ops_call::CallOpOutcome;

impl VM {
    pub(crate) fn execute_loop_interpreter(&mut self, target_depth: usize) -> Result<Value, String> {
        unsafe {
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

            macro_rules! handle_exception {
                ($thrown_val:expr) => {{
                    let thrown: Value = $thrown_val;
                    let mut handled = false;

                    let initial_frame_idx = self.frames.len() - 1;
                    while self.frames.len() > target_depth {
                        let frame_idx = self.frames.len() - 1;
                        let curr_ip = if frame_idx == initial_frame_idx {
                            let offset = ip.offset_from(code_ptr) as usize;
                            if offset > 0 { offset - 1 } else { 0 }
                        } else {
                            self.frames[frame_idx].ip
                        };
                        let curr_func = get_raw_func(self.frames[frame_idx].function);

                        if let Some(handler) = curr_func.chunk.find_handler(curr_ip).cloned() {
                            while self.frames.len() > frame_idx + 1 {
                                self.frames.pop();
                            }
                            frame_ptr = {
                                let len = self.frames.len();
                                self.frames.as_mut_ptr().add(len - 1)
                            };
                            frame = &mut *frame_ptr;
                            func = get_raw_func(frame.function);
                            code_ptr = func.chunk.code.as_ptr();
                            constants_ptr = func.chunk.constants.as_ptr();
                            slots_offset = frame.slots_offset;
                            reload_stack!();

                            *frame_slots.add(handler.err_reg as usize) = thrown;
                            ip = code_ptr.add(handler.catch_ip);
                            handled = true;
                            break;
                        } else {
                            if self.frames.len() > target_depth + 1 {
                                self.frames.pop();
                                if !self.frames.is_empty() {
                                    frame_ptr = {
                                        let len = self.frames.len();
                                        self.frames.as_mut_ptr().add(len - 1)
                                    };
                                    frame = &mut *frame_ptr;
                                    func = get_raw_func(frame.function);
                                    code_ptr = func.chunk.code.as_ptr();
                                    constants_ptr = func.chunk.constants.as_ptr();
                                    slots_offset = frame.slots_offset;
                                    reload_stack!();
                                }
                            } else {
                                break;
                            }
                        }
                    }

                    if !handled {
                        self.thrown_value = thrown;
                        let err_str = match thrown.as_str() {
                            Some(s) => s.to_string(),
                            None => format!("{}", thrown),
                        };
                        return Err(format!("Uncaught exception: {}", err_str));
                    }
                }};
            }

            loop {
                let instruction = *ip;
                ip = ip.add(1);

                if execute_math_and_cmp_op(&instruction, frame_slots)? {
                    continue;
                }

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
                    OpCode::ToIter => {
                        let dest = instruction.ra as usize;
                        let src = instruction.rb as usize;
                        let val = *frame_slots.add(src);
                        if val.is_array() {
                            *frame_slots.add(dest) = val;
                        } else if val.is_object() {
                            let obj_ptr = val.as_gc_ptr();
                            sync_stack!();
                            self.gc_trigger();
                            reload_stack!();
                            let keys: Vec<Value> = match &(*obj_ptr).data {
                                GcData::Object(map) => map.keys().map(|k| k.0).collect(),
                                GcData::Struct(s) => s.descriptor.field_indices.keys().map(|k| k.0).collect(),
                                _ => Vec::new(),
                            };
                            let arr_ptr = gc_allocate(GcData::Array(keys));
                            *frame_slots.add(dest) = Value::array(arr_ptr);
                        } else if val.is_string() {
                            let s_ptr = val.as_gc_ptr();
                            sync_stack!();
                            self.gc_trigger();
                            reload_stack!();
                            let chars: Vec<Value> = match &(*s_ptr).data {
                                GcData::String(s) => s.as_str().chars().map(|c| {
                                    let cp = crate::vm::gc::gc_alloc_string(&c.to_string());
                                    Value::string(cp)
                                }).collect(),
                                _ => Vec::new(),
                            };
                            let arr_ptr = gc_allocate(GcData::Array(chars));
                            *frame_slots.add(dest) = Value::array(arr_ptr);
                        } else {
                            return Err("Cannot iterate over non-iterable value".into());
                        }
                    }
                    OpCode::DefineGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name_str = name_val.as_str().unwrap_or("");
                        let name: Rc<str> = Rc::from(name_str);
                        let val = *frame_slots.add(instruction.ra as usize);
                        self.globals.insert(name, val);
                    }
                    OpCode::GetGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = name_val.as_str().unwrap_or("");
                        if let Some(val) = self.globals.get(name) {
                            *frame_slots.add(instruction.ra as usize) = *val;
                        } else {
                            return Err(format!("Undefined variable '{}'", name));
                        }
                    }
                    OpCode::SetGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = name_val.as_str().unwrap_or("");
                        let val = *frame_slots.add(instruction.ra as usize);
                        match self.globals.get_mut(name) {
                            Some(entry) => {
                                *entry = val;
                            }
                            None => {
                                return Err(format_undeclared_var_error(name));
                            }
                        }
                    }
                    OpCode::Jump => {
                        ip = ip.add(instruction.operand as usize);
                    }
                    OpCode::JumpIfFalse => {
                        let val = *frame_slots.add(instruction.ra as usize);
                        let is_false = val.0 == crate::vm::value::TAG_FALSE || val.0 == crate::vm::value::TAG_NULL;
                        if is_false {
                            ip = ip.add(instruction.operand as usize);
                        }
                    }
                    OpCode::Loop => {
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
                        if self.use_jit && !raw_func.is_async && raw_func.chunk.handlers.is_empty() && raw_func.jit_ptr.get().is_none() && (self.jit_threshold == 0 || raw_func.has_loop || count >= self.jit_threshold) {
                            crate::jit::compile_function(self, raw_fn_ptr);
                        }
                        ip = ip.sub(instruction.operand as usize);
                    }
                    OpCode::MakeArray => {
                        let dest = instruction.ra as usize;
                        let start_reg = instruction.rb as usize;
                        let count = instruction.operand as usize;
                        sync_stack!();
                        self.gc_trigger();
                        reload_stack!();

                        let mut elements = get_pooled_vec(count);
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
                        self.execute_make_object(dest, start_reg, count, frame_slots)?;
                    }
                    OpCode::GetProperty => {
                        let dest = instruction.ra as usize;
                        let obj_reg = instruction.rb as usize;
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let obj = *frame_slots.add(obj_reg);
                        self.execute_get_property(dest, obj, name_val, frame_slots)?;
                    }
                    OpCode::SetProperty => {
                        let obj_reg = instruction.ra as usize;
                        let val_reg = instruction.rb as usize;
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let obj = *frame_slots.add(obj_reg);
                        let val = *frame_slots.add(val_reg);
                        self.execute_set_property(obj, val, name_val)?;
                    }
                    OpCode::GetIndex => {
                        let dest = instruction.ra as usize;
                        let obj = *frame_slots.add(instruction.rb as usize);
                        let index = *frame_slots.add(instruction.rc as usize);
                        self.execute_get_index(dest, obj, index, frame_slots)?;
                    }
                    OpCode::SetIndex => {
                        let obj = *frame_slots.add(instruction.ra as usize);
                        let index = *frame_slots.add(instruction.rb as usize);
                        let val = *frame_slots.add(instruction.rc as usize);
                        self.execute_set_index(obj, index, val)?;
                    }
                    OpCode::Call => {
                        match self.execute_call_op(
                            &instruction,
                            frame_slots,
                            slots_offset,
                            code_ptr,
                            ip,
                            target_depth,
                            |p| get_raw_func(p),
                        )? {
                            CallOpOutcome::ContinueLoop => {
                                reload_stack!();
                                continue;
                            }
                            CallOpOutcome::ReturnResult(res) => {
                                return Ok(res);
                            }
                            CallOpOutcome::EnterInterpreterFrame => {
                                frame_ptr = {
                                    let len = self.frames.len();
                                    self.frames.as_mut_ptr().add(len - 1)
                                };
                                frame = &mut *frame_ptr;
                                func = get_raw_func(frame.function);
                                code_ptr = func.chunk.code.as_ptr();
                                constants_ptr = func.chunk.constants.as_ptr();
                                slots_offset = frame.slots_offset;
                                reload_stack!();
                                ip = code_ptr.add(frame.ip);
                            }
                        }
                    }
                    OpCode::Await => {
                        let curr_ip = ip.offset_from(code_ptr) as usize - 1;
                        if let Some(early_ret) = self.execute_await_op(&instruction, frame_slots, curr_ip)? {
                            return Ok(early_ret);
                        }
                    }
                    OpCode::DefineStruct => {
                        self.execute_define_struct_op(&instruction, constants_ptr);
                    }
                    OpCode::GetUpvalue => {
                        let dest = instruction.ra as usize;
                        let upval_idx = instruction.operand as usize;
                        let upval_ptr = match &(*frame.function).data {
                            GcData::Closure(c) => c.upvalues[upval_idx],
                            _ => unreachable!(),
                        };
                        let val = match &(*upval_ptr).data {
                            GcData::Upvalue(u) => match u.location {
                                crate::vm::gc::UpvalueLocation::Open(slot) => *stack_start.add(slot),
                                crate::vm::gc::UpvalueLocation::Closed(val) => val,
                            },
                            _ => unreachable!(),
                        };
                        *frame_slots.add(dest) = val;
                    }
                    OpCode::SetUpvalue => {
                        let src = instruction.ra as usize;
                        let upval_idx = instruction.operand as usize;
                        let val = *frame_slots.add(src);
                        let upval_ptr = match &(*frame.function).data {
                            GcData::Closure(c) => c.upvalues[upval_idx],
                            _ => unreachable!(),
                        };
                        match &mut (*upval_ptr).data {
                            GcData::Upvalue(u) => match u.location {
                                crate::vm::gc::UpvalueLocation::Open(slot) => {
                                    *stack_start.add(slot) = val;
                                }
                                crate::vm::gc::UpvalueLocation::Closed(ref mut v) => {
                                    *v = val;
                                }
                            },
                            _ => unreachable!(),
                        }
                    }
                    OpCode::Closure => {
                        self.execute_closure_op(
                            &instruction,
                            frame.function,
                            slots_offset,
                            constants_ptr,
                            frame_slots,
                        );
                    }
                    OpCode::CloseUpvalue => {
                        let slot = slots_offset + instruction.operand as usize;
                        self.close_upvalues(slot);
                    }
                    OpCode::Return => {
                        let result = *frame_slots.add(instruction.ra as usize);
                        let caller_dest_reg = frame.dest_reg;

                        self.close_upvalues(frame.slots_offset);

                        self.frames.pop();
                        if self.frames.len() <= target_depth {
                            return Ok(result);
                        }

                        frame_ptr = {
                            let len = self.frames.len();
                            self.frames.as_mut_ptr().add(len - 1)
                        };
                        frame = &mut *frame_ptr;
                        func = get_raw_func(frame.function);
                        code_ptr = func.chunk.code.as_ptr();
                        constants_ptr = func.chunk.constants.as_ptr();
                        slots_offset = frame.slots_offset;
                        frame_slots = stack_start.add(slots_offset);
                        ip = code_ptr.add(frame.ip + 1);

                        *frame_slots.add(caller_dest_reg) = result;
                    }
                    OpCode::Throw => {
                        let thrown = *frame_slots.add(instruction.ra as usize);
                        handle_exception!(thrown);
                    }
                    _ => {}
                }
            }
        }
    }
}
