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
    pub dest_reg: usize,
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
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

    fn execute_loop(&mut self, _original_len: usize) -> Result<Value, String> {
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
                            if a.is_string() {
                                let sa_str = match &(*a.as_gc_ptr()).data {
                                    GcData::String(s) => s,
                                    _ => unreachable!(),
                                };
                                let sb_str = b.to_string();
                                let new_str = format!("{}{}", sa_str, sb_str);
                                let new_ptr = gc_allocate(GcData::String(new_str));
                                *frame_slots.add(dest) = Value::string(new_ptr);
                            } else if b.is_string() {
                                let sa_str = a.to_string();
                                let sb_str = match &(*b.as_gc_ptr()).data {
                                    GcData::String(s) => s,
                                    _ => unreachable!(),
                                };
                                let new_str = format!("{}{}", sa_str, sb_str);
                                let new_ptr = gc_allocate(GcData::String(new_str));
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
                            GcData::String(s) => Rc::from(s.as_str()),
                            _ => unreachable!(),
                        };
                        let val = *frame_slots.add(instruction.ra as usize);
                        self.globals.insert(name, val);
                    }
                    OpCode::GetGlobal => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => s.as_str(),
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
                        let name: Rc<str> = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => Rc::from(s.as_str()),
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

                        let mut elements = Vec::with_capacity(count);
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

                        let mut obj = HashMap::new();
                        for i in 0..count {
                            let key_val = *frame_slots.add(start_reg + i * 2);
                            let val = *frame_slots.add(start_reg + i * 2 + 1);
                            if !key_val.is_string() {
                                return Err("Object key must be string".into());
                            }
                            let key = match &(*key_val.as_gc_ptr()).data {
                                GcData::String(s) => Rc::from(s.as_str()),
                                _ => unreachable!(),
                            };
                            obj.insert(key, val);
                        }
                        let ptr = gc_allocate(GcData::Object(obj));
                        *frame_slots.add(dest) = Value::object(ptr);
                    }
                    OpCode::GetProperty => {
                        let dest = instruction.ra as usize;
                        let obj_reg = instruction.rb as usize;
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => s.as_str(),
                            _ => unreachable!(),
                        };
                        let obj = *frame_slots.add(obj_reg);
                        if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            match &(*ptr).data {
                                GcData::Object(map) => {
                                    let val = map.get(name).cloned().unwrap_or(Value::null());
                                    *frame_slots.add(dest) = val;
                                }
                                _ => unreachable!(),
                            }
                        } else if obj.is_array() {
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
                        let name = match &(*name_val.as_gc_ptr()).data {
                            GcData::String(s) => s.as_str(),
                            _ => unreachable!(),
                        };
                        let obj = *frame_slots.add(obj_reg);
                        let val = *frame_slots.add(val_reg);
                        if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            match &mut (*ptr).data {
                                GcData::Object(map) => {
                                    map.insert(Rc::from(name), val);
                                    gc_write_barrier(ptr, &val);
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
                                    GcData::String(st) => st.as_str(),
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
                                let s = match &(*index.as_gc_ptr()).data {
                                    GcData::String(st) => st.as_str(),
                                    _ => unreachable!(),
                                };
                                match &(*ptr).data {
                                    GcData::Object(map) => {
                                        let val = map.get(s).cloned().unwrap_or(Value::null());
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
                                let s = match &(*index.as_gc_ptr()).data {
                                    GcData::String(st) => Rc::from(st.as_str()),
                                    _ => unreachable!(),
                                };
                                match &mut (*ptr).data {
                                    GcData::Object(map) => {
                                        map.insert(s, val);
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
                            if arg_count != func_val.arity {
                                return Err(format!(
                                    "Expected {} args but got {}",
                                    func_val.arity, arg_count
                                ));
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
                            let result = native(args);
                            reload_stack!();
                            *frame_slots.add(dest) = result;
                        } else if callee.is_array_method_push() || callee.is_array_method_pop() {
                            let ptr = callee.as_gc_ptr();
                            let mut args = Vec::with_capacity(arg_count);
                            for i in 0..arg_count {
                                args.push(*frame_slots.add(func_reg + 1 + i));
                            }
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
                            *frame_slots.add(dest) = result;
                        } else {
                            return Err("Can only call functions".into());
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
