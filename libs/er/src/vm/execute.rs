use std::collections::HashMap;
use std::rc::Rc;
use super::value::Value;
use super::bytecode::{Function, OpCode};
use super::gc::{
    gc_allocate, gc_write_barrier, gc_blacken_object, mark_value,
    GC_HEAD, GC_PHASE, GC_ROOTS, GRAY_STACK, ALLOC_COUNT, SWEEP_PTR, PREV_SWEEP_PTR,
    GcColor, GcPhase, GcData, GcObject
};

pub struct VM {
    pub frames: Vec<CallFrame>,
    pub stack: Vec<Value>,
    pub globals: HashMap<Rc<str>, Value>,
}

pub struct CallFrame {
    pub function: *mut GcObject,
    pub ip: usize,
    pub slots_offset: usize,
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

    pub fn run(&mut self, function: Function) -> Result<Value, String> {
        let func_ptr = gc_allocate(GcData::Function(function));
        self.frames.push(CallFrame {
            function: func_ptr,
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

    fn gc_trigger(&mut self) {
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

    fn execute_loop(&mut self, original_len: usize) -> Result<Value, String> {
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
            let mut stack_top = stack_start.add(original_len);
            let mut frame_slots = stack_start.add(slots_offset);

            macro_rules! sync_stack {
                () => {
                    let len = stack_top.offset_from(stack_start) as usize;
                    self.stack.set_len(len);
                };
            }

            macro_rules! reload_stack {
                () => {
                    stack_start = self.stack.as_mut_ptr();
                    stack_top = stack_start.add(self.stack.len());
                    frame_slots = stack_start.add(slots_offset);
                };
            }

            loop {
                let instruction = *ip;
                ip = ip.add(1);

                match instruction.op {
                    OpCode::Constant => {
                        let val = *constants_ptr.add(instruction.operand as usize);
                        *stack_top = val;
                        stack_top = stack_top.add(1);
                    }
                    OpCode::Return => {
                        let result = {
                            stack_top = stack_top.sub(1);
                            *stack_top
                        };
                        let frame_slots_offset = slots_offset;

                        self.frames.pop();
                        if self.frames.is_empty() {
                            let len = stack_top.offset_from(stack_start) as usize;
                            self.stack.set_len(len);
                            return Ok(result);
                        }

                        frame_ptr = {
                            let len = self.frames.len();
                            self.frames.as_mut_ptr().add(len - 1)
                        };
                        
                        stack_top = stack_start.add(frame_slots_offset);
                        *stack_top = result;
                        stack_top = stack_top.add(1);

                        frame = &mut *frame_ptr;
                        func = get_func!(frame.function);
                        code_ptr = func.chunk.code.as_ptr();
                        constants_ptr = func.chunk.constants.as_ptr();
                        slots_offset = frame.slots_offset;
                        frame_slots = stack_start.add(slots_offset);
                        ip = code_ptr.add(frame.ip);
                    }
                    OpCode::Negate => {
                        let val_ptr = stack_top.sub(1);
                        if (*val_ptr).is_number() {
                            *val_ptr = Value::number(-(*val_ptr).as_number());
                        }
                    }
                    OpCode::Add => {
                        stack_top = stack_top.sub(1);
                        let b = *stack_top;
                        let a_ptr = stack_top.sub(1);
                        if (*a_ptr).is_number() && b.is_number() {
                            *a_ptr = Value::number((*a_ptr).as_number() + b.as_number());
                        } else {
                            let a = *a_ptr;
                            if a.is_string() {
                                let sa_str = match &(*a.as_gc_ptr()).data {
                                    GcData::String(s) => s,
                                    _ => unreachable!(),
                                };
                                let sb_str = b.to_string();
                                let new_str = format!("{}{}", sa_str, sb_str);
                                let new_ptr = gc_allocate(GcData::String(new_str));
                                *a_ptr = Value::string(new_ptr);
                            } else if b.is_string() {
                                let sa_str = a.to_string();
                                let sb_str = match &(*b.as_gc_ptr()).data {
                                    GcData::String(s) => s,
                                    _ => unreachable!(),
                                };
                                let new_str = format!("{}{}", sa_str, sb_str);
                                let new_ptr = gc_allocate(GcData::String(new_str));
                                *a_ptr = Value::string(new_ptr);
                            } else {
                                sync_stack!();
                                return Err("Operands must be numbers or strings".into());
                            }
                        }
                    }
                    OpCode::Sub => {
                        stack_top = stack_top.sub(1);
                        let b = *stack_top;
                        let a_ptr = stack_top.sub(1);
                        if (*a_ptr).is_number() && b.is_number() {
                            *a_ptr = Value::number((*a_ptr).as_number() - b.as_number());
                        }
                    }
                    OpCode::Mul => {
                        stack_top = stack_top.sub(1);
                        let b = *stack_top;
                        let a_ptr = stack_top.sub(1);
                        if (*a_ptr).is_number() && b.is_number() {
                            *a_ptr = Value::number((*a_ptr).as_number() * b.as_number());
                        }
                    }
                    OpCode::Div => {
                        stack_top = stack_top.sub(1);
                        let b = *stack_top;
                        let a_ptr = stack_top.sub(1);
                        if (*a_ptr).is_number() && b.is_number() {
                            *a_ptr = Value::number((*a_ptr).as_number() / b.as_number());
                        }
                    }
                    OpCode::Equal => {
                        stack_top = stack_top.sub(1);
                        let b = *stack_top;
                        let a_ptr = stack_top.sub(1);
                        *a_ptr = Value::boolean(*a_ptr == b);
                    }
                    OpCode::Greater => {
                        stack_top = stack_top.sub(1);
                        let b = *stack_top;
                        let a_ptr = stack_top.sub(1);
                        if (*a_ptr).is_number() && b.is_number() {
                            *a_ptr = Value::boolean((*a_ptr).as_number() > b.as_number());
                        }
                    }
                    OpCode::Less => {
                        stack_top = stack_top.sub(1);
                        let b = *stack_top;
                        let a_ptr = stack_top.sub(1);
                        if (*a_ptr).is_number() && b.is_number() {
                            *a_ptr = Value::boolean((*a_ptr).as_number() < b.as_number());
                        }
                    }
                    OpCode::Pop => {
                        stack_top = stack_top.sub(1);
                    }
                    OpCode::DefineGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => Rc::from(s.as_str()),
                            _ => unreachable!(),
                        };
                        stack_top = stack_top.sub(1);
                        let val = *stack_top;
                        self.globals.insert(name, val);
                    }
                    OpCode::GetGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => s.as_str(),
                            _ => unreachable!(),
                        };
                        if let Some(val) = self.globals.get(name) {
                            *stack_top = *val;
                            stack_top = stack_top.add(1);
                        } else {
                            sync_stack!();
                            return Err(format!("Undefined variable '{}'", name));
                        }
                    }
                    OpCode::SetGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => Rc::from(s.as_str()),
                            _ => unreachable!(),
                        };
                        let val = *stack_top.sub(1);
                        if self.globals.contains_key(&name) {
                            self.globals.insert(name, val);
                        } else {
                            sync_stack!();
                            return Err(format!(
                                "Variable '{}' not declared. It needs to be declared with 'let' or 'const'.",
                                name
                            ));
                        }
                    }
                    OpCode::GetLocal => {
                        let val = *frame_slots.add(instruction.operand as usize);
                        *stack_top = val;
                        stack_top = stack_top.add(1);
                    }
                    OpCode::SetLocal => {
                        let val = *stack_top.sub(1);
                        *frame_slots.add(instruction.operand as usize) = val;
                    }
                    OpCode::JumpIfFalse => {
                        let val = *stack_top.sub(1);
                        let is_false = if val.is_boolean() {
                            !val.as_boolean()
                        } else if val.is_null() {
                            true
                        } else {
                            false
                        };
                        if is_false {
                            ip = ip.add(instruction.operand as usize);
                        }
                    }
                    OpCode::Jump => {
                        ip = ip.add(instruction.operand as usize);
                    }
                    OpCode::Loop => {
                        ip = ip.sub(instruction.operand as usize);
                    }
                    OpCode::MakeArray => {
                        let count = instruction.operand as usize;
                        sync_stack!();
                        self.gc_trigger();
                        reload_stack!();
                        
                        let mut elements = Vec::with_capacity(count);
                        stack_top = stack_top.sub(count);
                        for i in 0..count {
                            elements.push(*stack_top.add(i));
                        }
                        let ptr = gc_allocate(GcData::Array(elements));
                        *stack_top = Value::array(ptr);
                        stack_top = stack_top.add(1);
                    }
                    OpCode::MakeObject => {
                        let count = instruction.operand as usize;
                        sync_stack!();
                        self.gc_trigger();
                        reload_stack!();

                        let mut obj = HashMap::new();
                        stack_top = stack_top.sub(count * 2);
                        for i in 0..count {
                            let key_val = *stack_top.add(i * 2);
                            let val = *stack_top.add(i * 2 + 1);
                            if !key_val.is_string() {
                                sync_stack!();
                                return Err("Object key must be string".into());
                            }
                            let key = match &(*key_val.as_gc_ptr()).data {
                                GcData::String(s) => Rc::from(s.as_str()),
                                _ => unreachable!(),
                            };
                            obj.insert(key, val);
                        }
                        let ptr = gc_allocate(GcData::Object(obj));
                        *stack_top = Value::object(ptr);
                        stack_top = stack_top.add(1);
                    }
                    OpCode::GetProperty => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => s.as_str(),
                            _ => unreachable!(),
                        };
                        let obj_ptr = stack_top.sub(1);
                        let obj = *obj_ptr;
                        if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            match &(*ptr).data {
                                GcData::Object(map) => {
                                    let val = map.get(name).cloned().unwrap_or(Value::null());
                                    *obj_ptr = val;
                                }
                                _ => unreachable!(),
                            }
                        } else if obj.is_array() {
                            let ptr = obj.as_gc_ptr();
                            match &(*ptr).data {
                                GcData::Array(arr) => {
                                    if name == "push" {
                                        *obj_ptr = Value::array_method_push(ptr);
                                    } else if name == "pop" {
                                        *obj_ptr = Value::array_method_pop(ptr);
                                    } else if name == "length" {
                                        *obj_ptr = Value::number(arr.len() as f64);
                                    } else if let Ok(idx) = name.parse::<usize>() {
                                        let val = arr.get(idx).cloned().unwrap_or(Value::null());
                                        *obj_ptr = val;
                                    } else {
                                        *obj_ptr = Value::null();
                                    }
                                }
                                _ => unreachable!(),
                            }
                        } else {
                            sync_stack!();
                            return Err("Only objects have properties".into());
                        }
                    }
                    OpCode::SetProperty => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => s.as_str(),
                            _ => unreachable!(),
                        };
                        stack_top = stack_top.sub(2);
                        let obj = *stack_top;
                        let val = *stack_top.add(1);
                        if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            match &mut (*ptr).data {
                                GcData::Object(map) => {
                                    map.insert(Rc::from(name), val);
                                    gc_write_barrier(ptr, &val);
                                    *stack_top = val;
                                    stack_top = stack_top.add(1);
                                }
                                _ => unreachable!(),
                            }
                        } else if obj.is_array() {
                            let ptr = obj.as_gc_ptr();
                            match &mut (*ptr).data {
                                GcData::Array(arr) => {
                                    if let Ok(idx) = name.parse::<usize>() {
                                        if idx < arr.len() {
                                            arr[idx] = val;
                                        } else if idx == arr.len() {
                                            arr.push(val);
                                        } else {
                                            sync_stack!();
                                            return Err(format!(
                                                "Index {} out of bounds for array of length {}",
                                                idx,
                                                arr.len()
                                            ));
                                        }
                                        gc_write_barrier(ptr, &val);
                                        *stack_top = val;
                                        stack_top = stack_top.add(1);
                                    } else {
                                        sync_stack!();
                                        return Err("Cannot set non-numeric property on array".into());
                                    }
                                }
                                _ => unreachable!(),
                            }
                        } else {
                            sync_stack!();
                            return Err("Only objects have properties".into());
                        }
                    }
                    OpCode::Call => {
                        let arg_count = instruction.operand as usize;
                        let callee = *stack_top.sub(arg_count + 1);
                        if callee.is_function() {
                            let func_ptr = callee.as_gc_ptr();
                            let func_val = get_func!(func_ptr);
                            if arg_count != func_val.arity {
                                sync_stack!();
                                return Err(format!(
                                    "Expected {} args but got {}",
                                    func_val.arity, arg_count
                                ));
                            }
                            frame.ip = ip.offset_from(code_ptr) as usize;
                            let current_stack_len = stack_top.offset_from(stack_start) as usize;
                            self.frames.push(CallFrame {
                                function: func_ptr,
                                ip: 0,
                                slots_offset: current_stack_len - arg_count,
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
                            stack_top = stack_top.sub(arg_count);
                            for i in 0..arg_count {
                                args.push(*stack_top.add(i));
                            }
                            stack_top = stack_top.sub(1); // pop function
                            sync_stack!();
                            let result = native(args);
                            reload_stack!();
                            *stack_top = result;
                            stack_top = stack_top.add(1);
                        } else if callee.is_array_method_push() || callee.is_array_method_pop() {
                            let ptr = callee.as_gc_ptr();
                            let mut args = Vec::with_capacity(arg_count);
                            stack_top = stack_top.sub(arg_count);
                            for i in 0..arg_count {
                                args.push(*stack_top.add(i));
                            }
                            stack_top = stack_top.sub(1); // pop callee

                            sync_stack!();
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
                            *stack_top = result;
                            stack_top = stack_top.add(1);
                        } else {
                            sync_stack!();
                            return Err("Can only call functions".into());
                        }
                    }
                    OpCode::Not => {
                        stack_top = stack_top.sub(1);
                        let val = *stack_top;
                        let res = if val.is_boolean() {
                            !val.as_boolean()
                        } else if val.is_null() {
                            true
                        } else {
                            false
                        };
                        *stack_top = Value::boolean(res);
                        stack_top = stack_top.add(1);
                    }
                    OpCode::GetIndex => {
                        stack_top = stack_top.sub(2);
                        let obj = *stack_top;
                        let index = *stack_top.add(1);
                        if obj.is_array() {
                            let ptr = obj.as_gc_ptr();
                            if index.is_number() {
                                let idx = index.as_number() as usize;
                                match &(*ptr).data {
                                    GcData::Array(arr) => {
                                        let val = arr.get(idx).cloned().unwrap_or(Value::null());
                                        *stack_top = val;
                                        stack_top = stack_top.add(1);
                                    }
                                    _ => unreachable!(),
                                }
                            } else if index.is_string() {
                                let s = match &(*index.as_gc_ptr()).data {
                                    GcData::String(st) => st.as_str(),
                                    _ => unreachable!(),
                                };
                                if let Ok(idx) = s.parse::<usize>() {
                                    match &(*ptr).data {
                                        GcData::Array(arr) => {
                                            let val = arr.get(idx).cloned().unwrap_or(Value::null());
                                            *stack_top = val;
                                            stack_top = stack_top.add(1);
                                        }
                                        _ => unreachable!(),
                                    }
                                } else {
                                    *stack_top = Value::null();
                                    stack_top = stack_top.add(1);
                                }
                            } else {
                                sync_stack!();
                                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                            }
                        } else if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            if index.is_string() {
                                let s = match &(*index.as_gc_ptr()).data {
                                    GcData::String(st) => st.as_str(),
                                    _ => unreachable!(),
                                };
                                match &(*ptr).data {
                                    GcData::Object(map) => {
                                        let val = map.get(s).cloned().unwrap_or(Value::null());
                                        *stack_top = val;
                                        stack_top = stack_top.add(1);
                                    }
                                    _ => unreachable!(),
                                }
                            } else {
                                sync_stack!();
                                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                            }
                        } else {
                            sync_stack!();
                            return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                        }
                    }
                    OpCode::SetIndex => {
                        stack_top = stack_top.sub(3);
                        let obj = *stack_top;
                        let index = *stack_top.add(1);
                        let val = *stack_top.add(2);
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
                                            sync_stack!();
                                            return Err(format!(
                                                "Index {} out of bounds for array of length {}",
                                                idx,
                                                arr.len()
                                            ));
                                        }
                                        gc_write_barrier(ptr, &val);
                                        *stack_top = val;
                                        stack_top = stack_top.add(1);
                                    }
                                    _ => unreachable!(),
                                }
                            } else if index.is_string() {
                                let s = match &(*index.as_gc_ptr()).data {
                                    GcData::String(st) => st.as_str(),
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
                                                sync_stack!();
                                                return Err(format!(
                                                    "Index {} out of bounds for array of length {}",
                                                    idx,
                                                    arr.len()
                                                ));
                                            }
                                            gc_write_barrier(ptr, &val);
                                            *stack_top = val;
                                            stack_top = stack_top.add(1);
                                        }
                                        _ => unreachable!(),
                                    }
                                } else {
                                    sync_stack!();
                                    return Err("Cannot set non-numeric property on array".into());
                                }
                            } else {
                                sync_stack!();
                                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                            }
                        } else if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            if index.is_string() {
                                let s = match &(*index.as_gc_ptr()).data {
                                    GcData::String(st) => Rc::from(st.as_str()),
                                    _ => unreachable!(),
                                };
                                match &mut (*ptr).data {
                                    GcData::Object(map) => {
                                        map.insert(s, val);
                                        gc_write_barrier(ptr, &val);
                                        *stack_top = val;
                                        stack_top = stack_top.add(1);
                                    }
                                    _ => unreachable!(),
                                }
                            } else {
                                sync_stack!();
                                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                            }
                        } else {
                            sync_stack!();
                            return Err("Only arrays can be indexed by numbers, and objects by strings".into());
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::gc::gc_free_all;

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
