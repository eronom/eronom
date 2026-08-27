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
    gc_allocate, gc_write_barrier, gc_blacken_object, mark_value, gc_with_state,
    GC_ROOTS, GC_NEEDS_STEP, GcColor, GcPhase, GcData, GcObject
};

use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicUsize, AtomicU64, Ordering};
use std::collections::BinaryHeap;

pub enum AsyncResult {
    Timeout,
    ResolvePromise(*mut crate::vm::gc::GcObject, Value),
    ResolveFetchPromise(*mut crate::vm::gc::GcObject, Result<String, String>),
    ResolveTextPromise(*mut crate::vm::gc::GcObject, Result<String, String>),
    ResolveJsonPromise(*mut crate::vm::gc::GcObject, Result<String, String>),
    ResolveWritePromise(*mut crate::vm::gc::GcObject, Result<usize, String>),
}

pub struct EventLoopTask {
    pub callback: Value,
    pub args: Vec<Value>,
    pub result: AsyncResult,
}

unsafe impl Send for EventLoopTask {}
unsafe impl Sync for EventLoopTask {}

#[derive(Clone)]
pub enum VmTimerAction {
    Callback { callback: Value, args: Vec<Value> },
    ResolvePromise { promise_ptr: *mut crate::vm::gc::GcObject, value: Value },
}

pub struct VmTimer {
    pub id: u64,
    pub due_time: Instant,
    pub action: VmTimerAction,
}

unsafe impl Send for VmTimer {}
unsafe impl Sync for VmTimer {}

impl PartialEq for VmTimer {
    fn eq(&self, other: &Self) -> bool {
        self.due_time == other.due_time && self.id == other.id
    }
}

impl Eq for VmTimer {}

impl PartialOrd for VmTimer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for VmTimer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap: earliest due_time has highest priority
        other.due_time.cmp(&self.due_time).then_with(|| other.id.cmp(&self.id))
    }
}

pub struct PendingAsync {
    pub callback: Value,
    pub args: Vec<Value>,
}

unsafe impl Send for PendingAsync {}
pub fn format_undeclared_var_error(name: &str) -> String {
    format!(
        "Variable '{}' not declared. It needs to be declared with 'let' or 'const', or by assigning to it.",
        name
    )
}

pub fn get_string_builtin_method_id(name: &str) -> Option<super::gc::BuiltinMethodId> {
    use super::gc::BuiltinMethodId::*;
    match name {
        "toUpperCase" => Some(StringToUpperCase),
        "toLowerCase" => Some(StringToLowerCase),
        "trim" => Some(StringTrim),
        "trimStart" | "trimLeft" => Some(StringTrimStart),
        "trimEnd" | "trimRight" => Some(StringTrimEnd),
        "split" => Some(StringSplit),
        "slice" => Some(StringSlice),
        "substring" => Some(StringSubstring),
        "indexOf" => Some(StringIndexOf),
        "lastIndexOf" => Some(StringLastIndexOf),
        "includes" => Some(StringIncludes),
        "startsWith" => Some(StringStartsWith),
        "endsWith" => Some(StringEndsWith),
        "replace" => Some(StringReplace),
        "replaceAll" => Some(StringReplaceAll),
        "charAt" => Some(StringCharAt),
        "charCodeAt" => Some(StringCharCodeAt),
        "repeat" => Some(StringRepeat),
        "padStart" => Some(StringPadStart),
        "padEnd" => Some(StringPadEnd),
        "concat" => Some(StringConcat),
        _ => None,
    }
}

pub fn get_array_builtin_method_id(name: &str) -> Option<super::gc::BuiltinMethodId> {
    use super::gc::BuiltinMethodId::*;
    match name {
        "push" => Some(ArrayPush),
        "pop" => Some(ArrayPop),
        "shift" => Some(ArrayShift),
        "unshift" => Some(ArrayUnshift),
        "map" => Some(ArrayMap),
        "filter" => Some(ArrayFilter),
        "reduce" => Some(ArrayReduce),
        "forEach" => Some(ArrayForEach),
        "find" => Some(ArrayFind),
        "findIndex" => Some(ArrayFindIndex),
        "some" => Some(ArraySome),
        "every" => Some(ArrayEvery),
        "includes" => Some(ArrayIncludes),
        "indexOf" => Some(ArrayIndexOf),
        "lastIndexOf" => Some(ArrayLastIndexOf),
        "slice" => Some(ArraySlice),
        "join" => Some(ArrayJoin),
        "concat" => Some(ArrayConcat),
        "reverse" => Some(ArrayReverse),
        "sort" => Some(ArraySort),
        "flat" => Some(ArrayFlat),
        "flatMap" => Some(ArrayFlatMap),
        "fill" => Some(ArrayFill),
        _ => None,
    }
}

pub fn get_object_builtin_method_id(name: &str) -> Option<super::gc::BuiltinMethodId> {
    use super::gc::BuiltinMethodId::*;
    match name {
        "keys" => Some(ObjectKeys),
        "values" => Some(ObjectValues),
        "entries" => Some(ObjectEntries),
        "hasOwnProperty" | "has" => Some(ObjectHasOwnProperty),
        _ => None,
    }
}

pub struct VM {
    /// Fast error flag at offset 0 — read directly by JIT code without FFI call.
    /// Set to 1 whenever `self.error` is set; cleared when error is consumed.
    pub has_error_flag: u8,
    pub frames: Vec<CallFrame>,
    pub stack: Vec<Value>,
    pub globals: FnvHashMap<Rc<str>, Value>,
    pub error: Option<String>,
    pub mir_ctx: Option<*mut std::ffi::c_void>,
    pub use_jit: bool,
    pub jit_threshold: usize,
    pub alloc_count_local: usize,
    pub use_evented_io: bool,
    pub structs: FnvHashMap<Rc<str>, Rc<super::gc::StructDescriptor>>,
    pub auto_shapes: FnvHashMap<Vec<super::value::MapKey>, (Rc<super::gc::StructDescriptor>, Vec<usize>)>,
    pub last_matched_keys: Vec<Value>,
    pub last_matched_descriptor: Option<Rc<super::gc::StructDescriptor>>,
    pub last_matched_offsets: Vec<usize>,
    pub open_upvalues: Vec<*mut GcObject>,
    pub thrown_value: Value,
    
    // Event loop fields
    pub event_loop_queue: Arc<Mutex<Vec<EventLoopTask>>>,
    pub event_loop_condvar: Arc<Condvar>,
    pub active_async_tasks: Arc<AtomicUsize>,
    pub pending_callbacks: Arc<Mutex<Vec<PendingAsync>>>,
    pub timers: Arc<Mutex<BinaryHeap<VmTimer>>>,
    pub next_timer_id: Arc<AtomicU64>,
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
        let jit_threshold = if let Ok(val) = std::env::var("ER_JIT_THRESHOLD") {
            val.parse::<usize>().unwrap_or(0)
        } else {
            0
        };
        Self {
            has_error_flag: 0,
            frames: Vec::new(),
            stack: Vec::with_capacity(1048576),
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

    pub fn capture_upvalue(&mut self, abs_slot: usize) -> *mut GcObject {
        for &upval_ptr in &self.open_upvalues {
            unsafe {
                if let GcData::Upvalue(ref u) = (*upval_ptr).data {
                    if let super::gc::UpvalueLocation::Open(slot) = u.location {
                        if slot == abs_slot {
                            return upval_ptr;
                        }
                    }
                }
            }
        }
        let upval_ptr = super::gc::gc_allocate(GcData::Upvalue(super::gc::GcUpvalue {
            location: super::gc::UpvalueLocation::Open(abs_slot),
        }));
        self.open_upvalues.push(upval_ptr);
        upval_ptr
    }

    pub fn close_upvalues(&mut self, from_slot: usize) {
        let mut i = 0;
        while i < self.open_upvalues.len() {
            let upval_ptr = self.open_upvalues[i];
            unsafe {
                let should_close = match &(*upval_ptr).data {
                    GcData::Upvalue(u) => match u.location {
                        super::gc::UpvalueLocation::Open(slot) => slot >= from_slot,
                        _ => false,
                    },
                    _ => false,
                };
                if should_close {
                    let slot = match &(*upval_ptr).data {
                        GcData::Upvalue(u) => match u.location {
                            super::gc::UpvalueLocation::Open(s) => s,
                            _ => 0,
                        },
                        _ => 0,
                    };
                    let val = if slot < self.stack.len() {
                        self.stack[slot]
                    } else {
                        Value::null()
                    };
                    (*upval_ptr).data = GcData::Upvalue(super::gc::GcUpvalue {
                        location: super::gc::UpvalueLocation::Closed(val),
                    });
                    self.open_upvalues.swap_remove(i);
                } else {
                    i += 1;
                }
            }
        }
    }

    pub fn reset_jit(&mut self) {
        crate::jit::reset_jit_state();
    }

    pub fn find_matching_struct(&self, keys: &[Value]) -> Option<Rc<super::gc::StructDescriptor>> {
        for desc in self.structs.values() {
            if desc.field_indices.len() == keys.len() {
                let mut all_match = true;
                for &key in keys {
                    if !desc.field_indices.contains_key(&super::value::MapKey(key)) {
                        all_match = false;
                        break;
                    }
                }
                if all_match {
                    return Some(desc.clone());
                }
            }
        }
        None
    }

    pub fn find_matching_struct_cached(&mut self, keys: &[Value]) -> Option<(Rc<super::gc::StructDescriptor>, &[usize])> {
        if keys.is_empty() {
            return None;
        }
        if self.last_matched_descriptor.is_some() && self.last_matched_keys.len() == keys.len() {
            let mut match_ok = true;
            for i in 0..keys.len() {
                if self.last_matched_keys[i].0 != keys[i].0 {
                    match_ok = false;
                    break;
                }
            }
            if match_ok {
                return Some((self.last_matched_descriptor.as_ref().unwrap().clone(), &self.last_matched_offsets));
            }
        }

        for desc in self.structs.values() {
            if desc.field_indices.len() == keys.len() {
                let mut all_match = true;
                let mut offsets = Vec::with_capacity(keys.len());
                for &key in keys {
                    if let Some(&idx) = desc.field_indices.get(&super::value::MapKey(key)) {
                        offsets.push(idx);
                    } else {
                        all_match = false;
                        break;
                    }
                }
                if all_match {
                    self.last_matched_keys = keys.to_vec();
                    self.last_matched_descriptor = Some(desc.clone());
                    self.last_matched_offsets = offsets;
                    return Some((desc.clone(), &self.last_matched_offsets));
                }
            }
        }

        let map_keys: Vec<super::value::MapKey> = keys.iter().map(|&k| super::value::MapKey(k)).collect();
        if let Some((desc, offsets)) = self.auto_shapes.get(&map_keys) {
            self.last_matched_keys = keys.to_vec();
            self.last_matched_descriptor = Some(desc.clone());
            self.last_matched_offsets = offsets.clone();
            return Some((desc.clone(), &self.last_matched_offsets));
        }

        let mut field_indices = FnvHashMap::default();
        let mut offsets = Vec::with_capacity(keys.len());
        for (idx, &key) in keys.iter().enumerate() {
            field_indices.insert(super::value::MapKey(key), idx);
            offsets.push(idx);
        }
        let desc = Rc::new(super::gc::StructDescriptor::new(
            Rc::from("Object"),
            field_indices,
            FnvHashMap::default(),
        ));

        self.auto_shapes.insert(map_keys, (desc.clone(), offsets.clone()));
        self.last_matched_keys = keys.to_vec();
        self.last_matched_descriptor = Some(desc.clone());
        self.last_matched_offsets = offsets;
        Some((desc, &self.last_matched_offsets))
    }

    pub fn register_global(&mut self, name: &str, value: Value) {
        self.globals.insert(Rc::from(name), value);
    }

    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    pub fn run(&mut self, function: Function) -> Result<Value, String> {
        let func_ptr = gc_allocate(GcData::Function(function));
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

    pub fn gc_step(&mut self) {
        let start_time = Instant::now();
        GC_COUNT.with(|c| c.set(c.get() + 1));
        let phase = gc_with_state(|state| state.phase);
        match phase {
            GcPhase::Pause => {
                let should_mark = gc_with_state(|state| {
                    if state.alloc_count >= 10000 {
                        state.phase = GcPhase::Mark;
                        state.gray_stack.clear();
                        true
                    } else {
                        false
                    }
                });
                if should_mark {
                    for val in &self.stack {
                        mark_value(val);
                    }
                    for val in self.globals.values() {
                        mark_value(val);
                    }
                    for frame in &self.frames {
                        mark_value(&Value::function(frame.function));
                    }
                    for &upval_ptr in &self.open_upvalues {
                        super::gc::mark_object(upval_ptr);
                    }
                    mark_value(&self.thrown_value);
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
                    if let Ok(timers) = self.timers.lock() {
                        for timer in timers.iter() {
                            match &timer.action {
                                VmTimerAction::Callback { callback, args } => {
                                    mark_value(callback);
                                    for arg in args {
                                        mark_value(arg);
                                    }
                                }
                                VmTimerAction::ResolvePromise { value, .. } => {
                                    mark_value(value);
                                }
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
                gc_with_state(|state| {
                    for _ in 0..128 {
                        if let Some(ptr) = state.gray_stack.pop() {
                            gc_blacken_object(ptr);
                        } else {
                            state.phase = GcPhase::Atomic;
                            break;
                        }
                    }
                });
            }
            GcPhase::Atomic => {
                gc_with_state(|state| {
                    state.phase = GcPhase::Sweep;
                    state.sweep_ptr = state.head;
                    state.prev_sweep_ptr = std::ptr::null_mut();
                });
                
                for val in &self.stack {
                    mark_value(val);
                }
                for val in self.globals.values() {
                    mark_value(val);
                }
                for frame in &self.frames {
                    mark_value(&Value::function(frame.function));
                }
                for &upval_ptr in &self.open_upvalues {
                    super::gc::mark_object(upval_ptr);
                }
                mark_value(&self.thrown_value);
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
                if let Ok(timers) = self.timers.lock() {
                    for timer in timers.iter() {
                        match &timer.action {
                            VmTimerAction::Callback { callback, args } => {
                                mark_value(callback);
                                for arg in args {
                                    mark_value(arg);
                                }
                            }
                            VmTimerAction::ResolvePromise { value, .. } => {
                                mark_value(value);
                            }
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

                loop {
                    let gray_opt = gc_with_state(|s| s.gray_stack.pop());
                    if let Some(ptr) = gray_opt {
                        gc_blacken_object(ptr);
                    } else {
                        break;
                    }
                }
                super::gc::gc_sweep_string_cache();
            }
            GcPhase::Sweep => {
                gc_with_state(|state| {
                    for _ in 0..256 {
                        let curr = state.sweep_ptr;
                        if curr.is_null() {
                            state.phase = GcPhase::Pause;
                            state.alloc_count = 0;
                            GC_NEEDS_STEP.store(false, Ordering::Relaxed);
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
                                super::gc::gc_dealloc_object(state, curr);
                                state.sweep_ptr = next;
                            } else {
                                (*curr).color = GcColor::White;
                                state.prev_sweep_ptr = curr;
                                state.sweep_ptr = next;
                            }
                        }
                    }
                });
            }
        }
        if cfg!(debug_assertions) {
            GC_TIME.with(|t| t.set(t.get() + start_time.elapsed()));
        }
    }

    pub fn collect_garbage(&mut self) {
        let start_time = Instant::now();
        gc_with_state(|state| {
            state.gray_stack.clear();
        });

        // 1. Mark phase: mark roots
        for val in &self.stack {
            mark_value(val);
        }
        for val in self.globals.values() {
            mark_value(val);
        }
        for frame in &self.frames {
            mark_value(&Value::function(frame.function));
        }
        for &upval_ptr in &self.open_upvalues {
            super::gc::mark_object(upval_ptr);
        }
        mark_value(&self.thrown_value);
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
        if let Ok(timers) = self.timers.lock() {
            for timer in timers.iter() {
                match &timer.action {
                    VmTimerAction::Callback { callback, args } => {
                        mark_value(callback);
                        for arg in args {
                            mark_value(arg);
                        }
                    }
                    VmTimerAction::ResolvePromise { value, .. } => {
                        mark_value(value);
                    }
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

        // 2. Trace phase: process gray stack until empty
        loop {
            let gray_opt = gc_with_state(|state| state.gray_stack.pop());
            if let Some(ptr) = gray_opt {
                gc_blacken_object(ptr);
            } else {
                break;
            }
        }

        // Evict dead entries from STRING_CACHE before sweeping
        super::gc::gc_sweep_string_cache();

        // 3. Sweep phase: sweep the entire linked list in one go
        gc_with_state(|state| {
            let mut curr = state.head;
            state.head = std::ptr::null_mut();
            let mut prev: *mut GcObject = std::ptr::null_mut();
            
            while !curr.is_null() {
                unsafe {
                    let next = (*curr).next;
                    if (*curr).color == GcColor::White {
                        super::gc::gc_dealloc_object(state, curr);
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

            // 4. Count live objects for adaptive threshold and reset GC state
            let mut live_objects: usize = 0;
            let mut curr_count = state.head;
            while !curr_count.is_null() {
                unsafe {
                    live_objects += 1;
                    curr_count = (*curr_count).next;
                }
            }
            state.live_count = live_objects;
            // Adaptive threshold: 2× the live set, but at least 10000
            state.alloc_threshold = (live_objects * 2).max(10000);
            state.alloc_count = 0;
            state.phase = GcPhase::Pause;
            state.sweep_ptr = std::ptr::null_mut();
            state.prev_sweep_ptr = std::ptr::null_mut();
        });
        GC_NEEDS_STEP.store(false, Ordering::Relaxed);
        GC_TIME.with(|t| t.set(t.get() + start_time.elapsed()));
    }

    #[inline(always)]
    pub fn gc_trigger(&mut self) {
        if GC_NEEDS_STEP.load(Ordering::Relaxed) {
            self.collect_garbage();
        }
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
                return self.execute_builtin_method(receiver, method, final_args);
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
        
        let old_fns: Vec<*mut GcObject> = old_frames.iter().map(|f| f.function).collect();
        super::gc::GC_ROOTS.with(|roots| {
            roots.borrow_mut().push(Box::new(move || {
                for &func in &old_fns {
                    super::gc::mark_value(&Value::function(func));
                }
            }));
        });
        
        if self.stack.len() < old_stack_len + 65536 {
            self.stack.resize(old_stack_len + 65536, Value::null());
        }

        let res = if self.use_jit {
            self.execute_loop(0)
        } else {
            self.execute_loop_interpreter(0)
        };
        
        super::gc::GC_ROOTS.with(|roots| {
            roots.borrow_mut().pop();
        });
        
        self.frames = old_frames;
        self.stack.truncate(old_stack_len);
        
        res
    }

    fn execute(&mut self) -> Result<Value, String> {
        let original_len = self.stack.len();
        self.stack.resize(original_len + 65536, Value::null());
        
        let res = if self.use_jit {
            self.execute_loop(0)
        } else {
            self.execute_loop_interpreter(0)
        };
        
        self.stack.truncate(original_len);
        res
    }

    pub fn execute_builtin_method(
        &mut self,
        receiver: Value,
        method: super::gc::BuiltinMethodId,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        use super::gc::BuiltinMethodId::*;
        match method {
            // String methods
            StringToUpperCase => {
                let s = receiver.as_str().unwrap_or("");
                let res = s.to_uppercase();
                let ptr = super::gc::gc_alloc_string(&res);
                Ok(Value::string(ptr))
            }
            StringToLowerCase => {
                let s = receiver.as_str().unwrap_or("");
                let res = s.to_lowercase();
                let ptr = super::gc::gc_alloc_string(&res);
                Ok(Value::string(ptr))
            }
            StringTrim => {
                let s = receiver.as_str().unwrap_or("");
                let res = s.trim();
                let ptr = super::gc::gc_alloc_string(res);
                Ok(Value::string(ptr))
            }
            StringTrimStart => {
                let s = receiver.as_str().unwrap_or("");
                let res = s.trim_start();
                let ptr = super::gc::gc_alloc_string(res);
                Ok(Value::string(ptr))
            }
            StringTrimEnd => {
                let s = receiver.as_str().unwrap_or("");
                let res = s.trim_end();
                let ptr = super::gc::gc_alloc_string(res);
                Ok(Value::string(ptr))
            }
            StringSplit => {
                let s = receiver.as_str().unwrap_or("");
                if args.is_empty() {
                    let ptr = super::gc::gc_alloc_array(&[receiver]);
                    Ok(Value::array(ptr))
                } else {
                    let sep = args[0].as_str().unwrap_or("");
                    let limit = args.get(1).and_then(|v| if v.is_number() { Some(v.as_number() as usize) } else { None });
                    let parts: Vec<Value> = if sep.is_empty() {
                        let mut p = Vec::new();
                        for c in s.chars() {
                            let s_c = c.to_string();
                            let ptr = super::gc::gc_alloc_string(&s_c);
                            p.push(Value::string(ptr));
                            if let Some(lim) = limit {
                                if p.len() >= lim { break; }
                            }
                        }
                        p
                    } else {
                        let mut p = Vec::new();
                        for part in s.split(sep) {
                            let ptr = super::gc::gc_alloc_string(part);
                            p.push(Value::string(ptr));
                            if let Some(lim) = limit {
                                if p.len() >= lim { break; }
                            }
                        }
                        p
                    };
                    let ptr = super::gc::gc_alloc_array(&parts);
                    Ok(Value::array(ptr))
                }
            }
            StringSlice => {
                let s = receiver.as_str().unwrap_or("");
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as isize;
                let start = args.get(0).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0);
                let end = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { len }).unwrap_or(len);
                let start_idx = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(chars.len()) };
                let end_idx = if end < 0 { (len + end).max(0) as usize } else { (end as usize).min(chars.len()) };
                if start_idx >= end_idx {
                    let ptr = super::gc::gc_alloc_string("");
                    Ok(Value::string(ptr))
                } else {
                    let sub: String = chars[start_idx..end_idx].iter().collect();
                    let ptr = super::gc::gc_alloc_string(&sub);
                    Ok(Value::string(ptr))
                }
            }
            StringSubstring => {
                let s = receiver.as_str().unwrap_or("");
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as isize;
                let mut start = args.get(0).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0).max(0) as usize;
                let mut end = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { len }).unwrap_or(len).max(0) as usize;
                start = start.min(chars.len());
                end = end.min(chars.len());
                if start > end {
                    std::mem::swap(&mut start, &mut end);
                }
                let sub: String = chars[start..end].iter().collect();
                let ptr = super::gc::gc_alloc_string(&sub);
                Ok(Value::string(ptr))
            }
            StringIndexOf => {
                let s = receiver.as_str().unwrap_or("");
                let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { 0 }).unwrap_or(0);
                if from_idx <= s.len() {
                    if let Some(pos) = s[from_idx..].find(search) {
                        return Ok(Value::number((from_idx + pos) as f64));
                    }
                }
                Ok(Value::number(-1.0))
            }
            StringLastIndexOf => {
                let s = receiver.as_str().unwrap_or("");
                let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { s.len() }).unwrap_or(s.len());
                let slice = if from_idx < s.len() { &s[..=from_idx] } else { s };
                if let Some(pos) = slice.rfind(search) {
                    Ok(Value::number(pos as f64))
                } else {
                    Ok(Value::number(-1.0))
                }
            }
            StringIncludes => {
                let s = receiver.as_str().unwrap_or("");
                let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { 0 }).unwrap_or(0);
                if from_idx <= s.len() {
                    Ok(Value::boolean(s[from_idx..].contains(search)))
                } else {
                    Ok(Value::boolean(false))
                }
            }
            StringStartsWith => {
                let s = receiver.as_str().unwrap_or("");
                let prefix = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { 0 }).unwrap_or(0);
                if from_idx <= s.len() {
                    Ok(Value::boolean(s[from_idx..].starts_with(prefix)))
                } else {
                    Ok(Value::boolean(false))
                }
            }
            StringEndsWith => {
                let s = receiver.as_str().unwrap_or("");
                let suffix = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let end_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { s.len() }).unwrap_or(s.len());
                let slice = if end_idx <= s.len() { &s[..end_idx] } else { s };
                Ok(Value::boolean(slice.ends_with(suffix)))
            }
            StringReplace => {
                let s = receiver.as_str().unwrap_or("");
                let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let replace = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                let res = s.replacen(search, replace, 1);
                let ptr = super::gc::gc_alloc_string(&res);
                Ok(Value::string(ptr))
            }
            StringReplaceAll => {
                let s = receiver.as_str().unwrap_or("");
                let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let replace = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                let res = s.replace(search, replace);
                let ptr = super::gc::gc_alloc_string(&res);
                Ok(Value::string(ptr))
            }
            StringCharAt => {
                let s = receiver.as_str().unwrap_or("");
                let idx = args.get(0).map(|v| if v.is_number() { v.as_number() as usize } else { 0 }).unwrap_or(0);
                if let Some(c) = s.chars().nth(idx) {
                    let mut buf = String::new();
                    buf.push(c);
                    let ptr = super::gc::gc_alloc_string(&buf);
                    Ok(Value::string(ptr))
                } else {
                    let ptr = super::gc::gc_alloc_string("");
                    Ok(Value::string(ptr))
                }
            }
            StringCharCodeAt => {
                let s = receiver.as_str().unwrap_or("");
                let idx = args.get(0).map(|v| if v.is_number() { v.as_number() as usize } else { 0 }).unwrap_or(0);
                if let Some(c) = s.chars().nth(idx) {
                    Ok(Value::number(c as u32 as f64))
                } else {
                    Ok(Value::null())
                }
            }
            StringRepeat => {
                let s = receiver.as_str().unwrap_or("");
                let count = args.get(0).map(|v| if v.is_number() { (v.as_number() as usize).max(0) } else { 0 }).unwrap_or(0);
                let res = s.repeat(count);
                let ptr = super::gc::gc_alloc_string(&res);
                Ok(Value::string(ptr))
            }
            StringPadStart => {
                let s = receiver.as_str().unwrap_or("");
                let target_len = args.get(0).map(|v| if v.is_number() { (v.as_number() as usize).max(0) } else { 0 }).unwrap_or(0);
                let pad_str = args.get(1).and_then(|v| v.as_str()).unwrap_or(" ");
                let curr_len = s.chars().count();
                if curr_len >= target_len || pad_str.is_empty() {
                    return Ok(receiver);
                }
                let needed = target_len - curr_len;
                let mut pad = String::new();
                while pad.chars().count() < needed {
                    pad.push_str(pad_str);
                }
                let pad_trimmed: String = pad.chars().take(needed).collect();
                let res = format!("{}{}", pad_trimmed, s);
                let ptr = super::gc::gc_alloc_string(&res);
                Ok(Value::string(ptr))
            }
            StringPadEnd => {
                let s = receiver.as_str().unwrap_or("");
                let target_len = args.get(0).map(|v| if v.is_number() { (v.as_number() as usize).max(0) } else { 0 }).unwrap_or(0);
                let pad_str = args.get(1).and_then(|v| v.as_str()).unwrap_or(" ");
                let curr_len = s.chars().count();
                if curr_len >= target_len || pad_str.is_empty() {
                    return Ok(receiver);
                }
                let needed = target_len - curr_len;
                let mut pad = String::new();
                while pad.chars().count() < needed {
                    pad.push_str(pad_str);
                }
                let pad_trimmed: String = pad.chars().take(needed).collect();
                let res = format!("{}{}", s, pad_trimmed);
                let ptr = super::gc::gc_alloc_string(&res);
                Ok(Value::string(ptr))
            }
            StringConcat => {
                let mut res = receiver.as_str().unwrap_or("").to_string();
                for arg in args {
                    if let Some(s) = arg.as_str() {
                        res.push_str(s);
                    } else {
                        res.push_str(&arg.to_string());
                    }
                }
                let ptr = super::gc::gc_alloc_string(&res);
                Ok(Value::string(ptr))
            }

            // Array methods
            ArrayPush => {
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            for arg in args {
                                super::gc::gc_write_barrier(ptr, &arg);
                                arr.push(arg);
                            }
                            Ok(Value::number(arr.len() as f64))
                        }
                        _ => Ok(Value::null()),
                    }
                }
            }
            ArrayPop => {
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            Ok(arr.pop().unwrap_or(Value::null()))
                        }
                        _ => Ok(Value::null()),
                    }
                }
            }
            ArrayShift => {
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            if arr.is_empty() {
                                Ok(Value::null())
                            } else {
                                Ok(arr.remove(0))
                            }
                        }
                        _ => Ok(Value::null()),
                    }
                }
            }
            ArrayUnshift => {
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            arr.splice(0..0, args.iter().copied());
                            for arg in &args {
                                super::gc::gc_write_barrier(ptr, arg);
                            }
                            Ok(Value::number(arr.len() as f64))
                        }
                        _ => Ok(Value::null()),
                    }
                }
            }
            ArrayMap => {
                let cb = args.get(0).copied().unwrap_or(Value::null());
                if !cb.is_function() && !cb.is_native_function() {
                    return Err("Array.map requires a function callback".to_string());
                }
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                let mut mapped = Vec::with_capacity(items.len());
                let mapped_ptr = &mapped as *const Vec<Value>;
                super::gc::GC_ROOTS.with(|roots| {
                    roots.borrow_mut().push(Box::new(move || {
                        let vec = unsafe { &*mapped_ptr };
                        for val in vec {
                            super::gc::mark_value(val);
                        }
                    }));
                });
                for (i, item) in items.iter().enumerate() {
                    let res = self.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                    mapped.push(res);
                }
                super::gc::GC_ROOTS.with(|roots| {
                    roots.borrow_mut().pop();
                });
                let ptr = super::gc::gc_alloc_array(&mapped);
                Ok(Value::array(ptr))
            }
            ArrayFilter => {
                let cb = args.get(0).copied().unwrap_or(Value::null());
                if !cb.is_function() && !cb.is_native_function() {
                    return Err("Array.filter requires a function callback".to_string());
                }
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                let mut filtered = Vec::new();
                let filtered_ptr = &filtered as *const Vec<Value>;
                super::gc::GC_ROOTS.with(|roots| {
                    roots.borrow_mut().push(Box::new(move || {
                        let vec = unsafe { &*filtered_ptr };
                        for val in vec {
                            super::gc::mark_value(val);
                        }
                    }));
                });
                for (i, item) in items.iter().enumerate() {
                    let res = self.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                    let is_truthy = !res.is_null() && (!res.is_boolean() || res.as_boolean());
                    if is_truthy {
                        filtered.push(*item);
                    }
                }
                super::gc::GC_ROOTS.with(|roots| {
                    roots.borrow_mut().pop();
                });
                let ptr = super::gc::gc_alloc_array(&filtered);
                Ok(Value::array(ptr))
            }
            ArrayReduce => {
                let cb = args.get(0).copied().unwrap_or(Value::null());
                if !cb.is_function() && !cb.is_native_function() {
                    return Err("Array.reduce requires a function callback".to_string());
                }
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                let has_init = args.len() >= 2;
                if items.is_empty() {
                    return if has_init { Ok(args[1]) } else { Err("Reduce of empty array with no initial value".to_string()) };
                }
                let (mut acc, start_idx) = if has_init { (args[1], 0) } else { (items[0], 1) };
                for i in start_idx..items.len() {
                    acc = self.call_function_reentrant(cb, vec![acc, items[i], Value::number(i as f64), receiver])?;
                }
                Ok(acc)
            }
            ArrayForEach => {
                let cb = args.get(0).copied().unwrap_or(Value::null());
                if !cb.is_function() && !cb.is_native_function() {
                    return Err("Array.forEach requires a function callback".to_string());
                }
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                for (i, item) in items.iter().enumerate() {
                    self.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                }
                Ok(Value::null())
            }
            ArrayFind => {
                let cb = args.get(0).copied().unwrap_or(Value::null());
                if !cb.is_function() && !cb.is_native_function() {
                    return Err("Array.find requires a function callback".to_string());
                }
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                for (i, item) in items.iter().enumerate() {
                    let res = self.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                    let is_truthy = !res.is_null() && (!res.is_boolean() || res.as_boolean());
                    if is_truthy {
                        return Ok(*item);
                    }
                }
                Ok(Value::null())
            }
            ArrayFindIndex => {
                let cb = args.get(0).copied().unwrap_or(Value::null());
                if !cb.is_function() && !cb.is_native_function() {
                    return Err("Array.findIndex requires a function callback".to_string());
                }
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                for (i, item) in items.iter().enumerate() {
                    let res = self.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                    let is_truthy = !res.is_null() && (!res.is_boolean() || res.as_boolean());
                    if is_truthy {
                        return Ok(Value::number(i as f64));
                    }
                }
                Ok(Value::number(-1.0))
            }
            ArraySome => {
                let cb = args.get(0).copied().unwrap_or(Value::null());
                if !cb.is_function() && !cb.is_native_function() {
                    return Err("Array.some requires a function callback".to_string());
                }
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                for (i, item) in items.iter().enumerate() {
                    let res = self.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                    let is_truthy = !res.is_null() && (!res.is_boolean() || res.as_boolean());
                    if is_truthy {
                        return Ok(Value::boolean(true));
                    }
                }
                Ok(Value::boolean(false))
            }
            ArrayEvery => {
                let cb = args.get(0).copied().unwrap_or(Value::null());
                if !cb.is_function() && !cb.is_native_function() {
                    return Err("Array.every requires a function callback".to_string());
                }
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                for (i, item) in items.iter().enumerate() {
                    let res = self.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                    let is_truthy = !res.is_null() && (!res.is_boolean() || res.as_boolean());
                    if !is_truthy {
                        return Ok(Value::boolean(false));
                    }
                }
                Ok(Value::boolean(true))
            }
            ArrayIncludes => {
                let search = args.get(0).copied().unwrap_or(Value::null());
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                let len = items.len() as isize;
                let from = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0);
                let start_idx = if from < 0 { (len + from).max(0) as usize } else { (from as usize).min(items.len()) };
                for i in start_idx..items.len() {
                    if items[i] == search {
                        return Ok(Value::boolean(true));
                    }
                }
                Ok(Value::boolean(false))
            }
            ArrayIndexOf => {
                let search = args.get(0).copied().unwrap_or(Value::null());
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                let len = items.len() as isize;
                let from = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0);
                let start_idx = if from < 0 { (len + from).max(0) as usize } else { (from as usize).min(items.len()) };
                for i in start_idx..items.len() {
                    if items[i] == search {
                        return Ok(Value::number(i as f64));
                    }
                }
                Ok(Value::number(-1.0))
            }
            ArrayLastIndexOf => {
                let search = args.get(0).copied().unwrap_or(Value::null());
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                if items.is_empty() {
                    return Ok(Value::number(-1.0));
                }
                let len = items.len() as isize;
                let from = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { len - 1 }).unwrap_or(len - 1);
                let end_idx = if from < 0 { (len + from).max(0) as usize } else { (from as usize).min(items.len() - 1) };
                for i in (0..=end_idx).rev() {
                    if items[i] == search {
                        return Ok(Value::number(i as f64));
                    }
                }
                Ok(Value::number(-1.0))
            }
            ArraySlice => {
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                let len = items.len() as isize;
                let start = args.get(0).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0);
                let end = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { len }).unwrap_or(len);
                let start_idx = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(items.len()) };
                let end_idx = if end < 0 { (len + end).max(0) as usize } else { (end as usize).min(items.len()) };
                if start_idx >= end_idx {
                    let ptr = super::gc::gc_alloc_array(&[]);
                    Ok(Value::array(ptr))
                } else {
                    let ptr = super::gc::gc_alloc_array(&items[start_idx..end_idx]);
                    Ok(Value::array(ptr))
                }
            }
            ArrayJoin => {
                let sep = args.get(0).and_then(|v| v.as_str()).unwrap_or(",");
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                let strings: Vec<String> = items.iter().map(|v| {
                    if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        v.to_string()
                    }
                }).collect();
                let joined = strings.join(sep);
                let ptr = super::gc::gc_alloc_string(&joined);
                Ok(Value::string(ptr))
            }
            ArrayConcat => {
                let mut items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                for arg in args {
                    if arg.is_array() {
                        unsafe {
                            if let GcData::Array(sub) = &(*arg.as_gc_ptr()).data {
                                items.extend_from_slice(sub);
                            }
                        }
                    } else {
                        items.push(arg);
                    }
                }
                let ptr = super::gc::gc_alloc_array(&items);
                Ok(Value::array(ptr))
            }
            ArrayReverse => {
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            arr.reverse();
                        }
                        _ => {}
                    }
                }
                Ok(receiver)
            }
            ArraySort => {
                let cb_opt = args.get(0).copied();
                let ptr = receiver.as_gc_ptr();
                let mut items: Vec<Value> = unsafe {
                    match &(*ptr).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                if let Some(cb) = cb_opt {
                    if cb.is_function() || cb.is_native_function() {
                        let len = items.len();
                        for i in 0..len {
                            for j in 0..len.saturating_sub(1 + i) {
                                let cmp_res = self.call_function_reentrant(cb, vec![items[j], items[j + 1]])?;
                                let cmp_val = cmp_res.as_number();
                                if cmp_val > 0.0 {
                                    items.swap(j, j + 1);
                                }
                            }
                        }
                    } else {
                        items.sort_by(|a, b| {
                            if a.is_number() && b.is_number() {
                                a.as_number().partial_cmp(&b.as_number()).unwrap_or(std::cmp::Ordering::Equal)
                            } else {
                                a.to_string().cmp(&b.to_string())
                            }
                        });
                    }
                } else {
                    items.sort_by(|a, b| {
                        if a.is_number() && b.is_number() {
                            a.as_number().partial_cmp(&b.as_number()).unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.to_string().cmp(&b.to_string())
                        }
                    });
                }
                unsafe {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            arr.clear();
                            arr.extend_from_slice(&items);
                        }
                        _ => {}
                    }
                }
                Ok(receiver)
            }
            ArrayFlat => {
                let depth = args.get(0).map(|v| if v.is_number() { (v.as_number() as usize).max(0) } else { 1 }).unwrap_or(1);
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                fn flatten_helper(val: Value, depth: usize, out: &mut Vec<Value>) {
                    if depth > 0 && val.is_array() {
                        unsafe {
                            if let GcData::Array(sub) = &(*val.as_gc_ptr()).data {
                                for item in sub {
                                    flatten_helper(*item, depth - 1, out);
                                }
                                return;
                            }
                        }
                    }
                    out.push(val);
                }
                let mut out = Vec::new();
                for item in items {
                    flatten_helper(item, depth, &mut out);
                }
                let ptr = super::gc::gc_alloc_array(&out);
                Ok(Value::array(ptr))
            }
            ArrayFlatMap => {
                let cb = args.get(0).copied().unwrap_or(Value::null());
                if !cb.is_function() && !cb.is_native_function() {
                    return Err("Array.flatMap requires a function callback".to_string());
                }
                let items: Vec<Value> = unsafe {
                    match &(*receiver.as_gc_ptr()).data {
                        GcData::Array(arr) => arr.clone(),
                        _ => vec![],
                    }
                };
                let mut mapped = Vec::with_capacity(items.len());
                let mapped_ptr = &mapped as *const Vec<Value>;
                super::gc::GC_ROOTS.with(|roots| {
                    roots.borrow_mut().push(Box::new(move || {
                        let vec = unsafe { &*mapped_ptr };
                        for val in vec {
                            super::gc::mark_value(val);
                        }
                    }));
                });
                for (i, item) in items.iter().enumerate() {
                    let res = self.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                    mapped.push(res);
                }
                super::gc::GC_ROOTS.with(|roots| {
                    roots.borrow_mut().pop();
                });
                let mut out = Vec::new();
                for val in mapped {
                    if val.is_array() {
                        unsafe {
                            if let GcData::Array(sub) = &(*val.as_gc_ptr()).data {
                                out.extend_from_slice(sub);
                                continue;
                            }
                        }
                    }
                    out.push(val);
                }
                let ptr = super::gc::gc_alloc_array(&out);
                Ok(Value::array(ptr))
            }
            ArrayFill => {
                let val = args.get(0).copied().unwrap_or(Value::null());
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            let len = arr.len() as isize;
                            let start = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0);
                            let end = args.get(2).map(|v| if v.is_number() { v.as_number() as isize } else { len }).unwrap_or(len);
                            let start_idx = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(arr.len()) };
                            let end_idx = if end < 0 { (len + end).max(0) as usize } else { (end as usize).min(arr.len()) };
                            for i in start_idx..end_idx {
                                arr[i] = val;
                                super::gc::gc_write_barrier(ptr, &val);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(receiver)
            }

            // Object methods
            ObjectKeys => {
                let mut keys_vec = Vec::new();
                if receiver.is_object() {
                    let ptr = receiver.as_gc_ptr();
                    unsafe {
                        match &(*ptr).data {
                            GcData::Object(map) => {
                                for key in map.keys() {
                                    keys_vec.push(key.0);
                                }
                            }
                            GcData::Struct(s) => {
                                for key in s.descriptor.field_indices.keys() {
                                    keys_vec.push(key.0);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                let ptr = super::gc::gc_alloc_array(&keys_vec);
                Ok(Value::array(ptr))
            }
            ObjectValues => {
                let mut vals_vec = Vec::new();
                if receiver.is_object() {
                    let ptr = receiver.as_gc_ptr();
                    unsafe {
                        match &(*ptr).data {
                            GcData::Object(map) => {
                                for val in map.values() {
                                    vals_vec.push(*val);
                                }
                            }
                            GcData::Struct(s) => {
                                for val in &s.fields {
                                    vals_vec.push(*val);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                let ptr = super::gc::gc_alloc_array(&vals_vec);
                Ok(Value::array(ptr))
            }
            ObjectEntries => {
                let mut entries_vec = Vec::new();
                if receiver.is_object() {
                    let ptr = receiver.as_gc_ptr();
                    unsafe {
                        match &(*ptr).data {
                            GcData::Object(map) => {
                                for (key, val) in map {
                                    let pair_ptr = super::gc::gc_alloc_array(&[key.0, *val]);
                                    entries_vec.push(Value::array(pair_ptr));
                                }
                            }
                            GcData::Struct(s) => {
                                for (key, &idx) in &s.descriptor.field_indices {
                                    let val = s.fields.get(idx).copied().unwrap_or(Value::null());
                                    let pair_ptr = super::gc::gc_alloc_array(&[key.0, val]);
                                    entries_vec.push(Value::array(pair_ptr));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                let ptr = super::gc::gc_alloc_array(&entries_vec);
                Ok(Value::array(ptr))
            }
            ObjectHasOwnProperty => {
                let key = args.get(0).copied().unwrap_or(Value::null());
                let mut has_prop = false;
                if receiver.is_object() {
                    let ptr = receiver.as_gc_ptr();
                    unsafe {
                        match &(*ptr).data {
                            GcData::Object(map) => {
                                has_prop = map.contains_key(&super::value::MapKey(key));
                            }
                            GcData::Struct(s) => {
                                has_prop = s.descriptor.field_indices.contains_key(&super::value::MapKey(key));
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Value::boolean(has_prop))
            }
        }
    }

    fn execute_loop(&mut self, target_depth: usize) -> Result<Value, String> {
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
                ($func_ptr:expr) => {{
                    let mut p = $func_ptr;
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
                }};
            }

            let mut func = get_func!(frame.function);
            let mut constants_ptr = func.chunk.constants.as_ptr();
            let mut slots_offset = frame.slots_offset;

            let mut stack_start;
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

                let native_ptr = if let Some(ptr) = raw_func.jit_ptr.get() {
                    ptr
                } else if self.jit_threshold == 0 || count >= self.jit_threshold || self.frames.len() <= 1 {
                    crate::jit::compile_function(self, raw_fn_ptr)
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
                    func = get_func!(frame.function);
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
                            let mut args = Vec::with_capacity(arg_count_out);
                            for i in 0..arg_count_out {
                                args.push(*frame_slots.add(func_reg_out + 1 + i));
                            }
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
                        let func_val = get_func!(func_ptr);
                        if actual_arg_count != func_val.arity {
                            return Err(format!(
                                "Expected {} args but got {}",
                                func_val.arity, actual_arg_count
                            ));
                        }
                        // Save current IP (call instruction index: ip_out)
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
                        let ptr = (callee.0 & super::value::PTR_MASK & !3) as *mut GcObject;
                        let method_sub_tag = callee.0 & 3;

                        let path_str = match &(*ptr).data {
                            GcData::Object(map) => {
                                let name_key = super::gc::get_or_create_string("name");
                                let name_val = map.get(&super::value::MapKey(Value::string(name_key))).cloned().unwrap_or(Value::null());
                                match name_val.as_str() {
                                    Some(s) => s.to_string(),
                                    None => "".to_string(),
                                }
                            }
                            GcData::Struct(s) => {
                                let name_key = super::gc::get_or_create_string("name");
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
                                    let ptr = super::gc::gc_allocate(GcData::String(std::rc::Rc::from(content)));
                                    Value::string(ptr)
                                } else {
                                    Value::null()
                                }
                            }
                            2 => { // json
                                if let Ok(content) = std::fs::read_to_string(&path_str) {
                                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
                                        super::gc::json_to_value(json_val)
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
                    func = get_func!(frame.function);
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
                                crate::vm::gc::GcData::Promise(prom) => {
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
                        let ptr = super::gc::gc_alloc_string(&err_msg);
                        Value::string(ptr)
                    } else {
                        let ptr = super::gc::intern_string("JIT execution error");
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
                        let curr_func = get_func!(self.frames[frame_idx].function);
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

    pub(crate) fn execute_loop_interpreter(&mut self, target_depth: usize) -> Result<Value, String> {
        unsafe {
            let mut frame_ptr = {
                let len = self.frames.len();
                self.frames.as_mut_ptr().add(len - 1)
            };
            let mut frame = &mut *frame_ptr;

            macro_rules! get_func {
                ($func_ptr:expr) => {{
                    let mut p = $func_ptr;
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
                }};
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

            macro_rules! handle_exception {
                ($thrown_val:expr) => {{
                    let thrown: Value = $thrown_val;
                    let mut handled = false;

                    let initial_frame_idx = self.frames.len() - 1;
                    while !self.frames.is_empty() {
                        let frame_idx = self.frames.len() - 1;
                        let curr_ip = if frame_idx == initial_frame_idx {
                            let offset = ip.offset_from(code_ptr) as usize;
                            if offset > 0 { offset - 1 } else { 0 }
                        } else {
                            self.frames[frame_idx].ip
                        };
                        let curr_func = get_func!(self.frames[frame_idx].function);

                        if let Some(handler) = curr_func.chunk.find_handler(curr_ip).cloned() {
                            while self.frames.len() > frame_idx + 1 {
                                self.frames.pop();
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
                            reload_stack!();

                            *frame_slots.add(handler.err_reg as usize) = thrown;
                            ip = code_ptr.add(handler.catch_ip);
                            handled = true;
                            break;
                        } else {
                            if self.frames.len() > 1 {
                                self.frames.pop();
                                if !self.frames.is_empty() {
                                    frame_ptr = {
                                        let len = self.frames.len();
                                        self.frames.as_mut_ptr().add(len - 1)
                                    };
                                    frame = &mut *frame_ptr;
                                    func = get_func!(frame.function);
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
                                let sa_str = a.as_str().unwrap_or("");
                                let val = super::value::ADD_SCRATCH.with(|scratch| {
                                    let mut s_ref = scratch.borrow_mut();
                                    s_ref.clear();
                                    s_ref.push_str(sa_str);
                                    if b.is_string() {
                                        if let Some(sb_str) = b.as_str() {
                                            s_ref.push_str(sb_str);
                                        }
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
                                    if let Some(inline) = Value::inline_string(s_ref.as_str()) {
                                        inline
                                    } else {
                                        let new_ptr = super::gc::gc_alloc_string(s_ref.as_str());
                                        Value::string(new_ptr)
                                    }
                                });
                                *frame_slots.add(dest) = val;
                            } else if b.is_string() {
                                let sb_str = b.as_str().unwrap_or("");
                                let val = super::value::ADD_SCRATCH.with(|scratch| {
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
                                    if let Some(inline) = Value::inline_string(s_ref.as_str()) {
                                        inline
                                    } else {
                                        let new_ptr = super::gc::gc_alloc_string(s_ref.as_str());
                                        Value::string(new_ptr)
                                    }
                                });
                                *frame_slots.add(dest) = val;
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
                    OpCode::Mod => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            *frame_slots.add(dest) = Value::number_unchecked(a.as_number() % b.as_number());
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::BitAnd => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            let res = ((a.as_number() as i64) & (b.as_number() as i64)) as f64;
                            *frame_slots.add(dest) = Value::number_unchecked(res);
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::BitOr => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            let res = ((a.as_number() as i64) | (b.as_number() as i64)) as f64;
                            *frame_slots.add(dest) = Value::number_unchecked(res);
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::BitXor => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            let res = ((a.as_number() as i64) ^ (b.as_number() as i64)) as f64;
                            *frame_slots.add(dest) = Value::number_unchecked(res);
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::BitNot => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        if a.is_number() {
                            let res = (!(a.as_number() as i64)) as f64;
                            *frame_slots.add(dest) = Value::number_unchecked(res);
                        } else {
                            return Err("Operand must be a number".into());
                        }
                    }
                    OpCode::ShiftLeft => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            let shift = (b.as_number() as u32) & 63;
                            let res = ((a.as_number() as i64).wrapping_shl(shift)) as f64;
                            *frame_slots.add(dest) = Value::number_unchecked(res);
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::ShiftRight => {
                        let dest = instruction.ra as usize;
                        let a = *frame_slots.add(instruction.rb as usize);
                        let b = *frame_slots.add(instruction.rc as usize);
                        if a.is_number() && b.is_number() {
                            let shift = (b.as_number() as u32) & 63;
                            let res = ((a.as_number() as i64).wrapping_shr(shift)) as f64;
                            *frame_slots.add(dest) = Value::number_unchecked(res);
                        } else {
                            return Err("Operands must be numbers".into());
                        }
                    }
                    OpCode::TypeOf => {
                        let dest = instruction.ra as usize;
                        let val = *frame_slots.add(instruction.rb as usize);
                        let type_str = if val.is_number() {
                            "number"
                        } else if val.is_string() {
                            "string"
                        } else if val.is_boolean() {
                            "boolean"
                        } else if val.is_null() {
                            "null"
                        } else if val.is_array() {
                            "array"
                        } else if val.is_object() {
                            "object"
                        } else if val.is_function() || val.is_native_function() {
                            "function"
                        } else {
                            "object"
                        };
                        let ptr = super::gc::get_or_create_string(type_str);
                        *frame_slots.add(dest) = Value::string(ptr);
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
                                GcData::String(s) => s.chars().map(|c| {
                                    let cp = super::gc::gc_alloc_string(&c.to_string());
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
                    OpCode::ArrayLen => {
                        let dest = instruction.ra as usize;
                        let src = instruction.rb as usize;
                        let val = *frame_slots.add(src);
                        if val.is_array() {
                            let arr_ptr = val.as_gc_ptr();
                            let len = match &(*arr_ptr).data {
                                GcData::Array(arr) => arr.len(),
                                _ => 0,
                            };
                            *frame_slots.add(dest) = Value::number(len as f64);
                        } else {
                            return Err("Expected array for length".into());
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
                        let is_false = val.0 == super::value::TAG_FALSE || val.0 == super::value::TAG_NULL;
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
                        if self.use_jit && !raw_func.is_async && raw_func.jit_ptr.get().is_none() && (self.jit_threshold == 0 || count >= self.jit_threshold) {
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

                        let ptr = if count == 0 {
                            let obj = super::gc::get_pooled_map(0);
                            gc_allocate(GcData::Object(obj))
                        } else if count <= 16 {
                            let mut keys = [Value::null(); 16];
                            let mut values = [Value::null(); 16];
                            for i in 0..count {
                                let key_val = *frame_slots.add(start_reg + i * 2);
                                let val = *frame_slots.add(start_reg + i * 2 + 1);
                                if !key_val.is_string() {
                                    return Err("Object key must be string".into());
                                }
                                keys[i] = key_val;
                                values[i] = val;
                            }

                            if let Some((desc, offsets)) = self.find_matching_struct_cached(&keys[..count]) {
                                let mut fields = super::gc::get_pooled_vec(count);
                                fields.resize(count, Value::null());
                                for i in 0..count {
                                    let val = values[i];
                                    let idx = offsets[i];
                                    fields[idx] = val;
                                }
                                gc_allocate(GcData::Struct(super::gc::GcStruct {
                                    descriptor: desc,
                                    fields,
                                }))
                            } else {
                                let (desc, offsets) = crate::vm::shape::get_or_create_anonymous_shape(&keys[..count]);
                                let mut fields = super::gc::get_pooled_vec(count);
                                fields.resize(count, Value::null());
                                for i in 0..count {
                                    fields[offsets[i]] = values[i];
                                }
                                gc_allocate(GcData::Struct(super::gc::GcStruct {
                                    descriptor: desc,
                                    fields,
                                }))
                            }
                        } else {
                            let mut keys = Vec::with_capacity(count);
                            let mut values = Vec::with_capacity(count);
                            for i in 0..count {
                                let key_val = *frame_slots.add(start_reg + i * 2);
                                let val = *frame_slots.add(start_reg + i * 2 + 1);
                                if !key_val.is_string() {
                                    return Err("Object key must be string".into());
                                }
                                keys.push(key_val);
                                values.push(val);
                            }

                            if let Some((desc, offsets)) = self.find_matching_struct_cached(&keys) {
                                let mut fields = super::gc::get_pooled_vec(keys.len());
                                fields.resize(keys.len(), Value::null());
                                for i in 0..count {
                                    let val = values[i];
                                    let idx = offsets[i];
                                    fields[idx] = val;
                                }
                                gc_allocate(GcData::Struct(super::gc::GcStruct {
                                    descriptor: desc,
                                    fields,
                                }))
                            } else {
                                let (desc, offsets) = crate::vm::shape::get_or_create_anonymous_shape(&keys);
                                let mut fields = super::gc::get_pooled_vec(keys.len());
                                fields.resize(keys.len(), Value::null());
                                for i in 0..count {
                                    fields[offsets[i]] = values[i];
                                }
                                gc_allocate(GcData::Struct(super::gc::GcStruct {
                                    descriptor: desc,
                                    fields,
                                }))
                            }
                        };
                        *frame_slots.add(dest) = Value::object(ptr);
                    }
                    OpCode::GetProperty => {
                        let dest = instruction.ra as usize;
                        let obj_reg = instruction.rb as usize;
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let obj = *frame_slots.add(obj_reg);
                        let name = name_val.as_str().unwrap_or("");
                        if obj.is_object() {
                            let ptr = obj.as_gc_ptr();
                            let mut is_json_method = false;
                            let mut is_text_method = false;
                            if name == "json" || name == "text" {
                                let body_key = super::gc::get_or_create_string("_body");
                                let is_response = match &(*ptr).data {
                                    GcData::Object(map) => map.contains_key(&super::value::MapKey(Value::string(body_key))),
                                    GcData::Struct(s) => s.get_field_by_name("_body").is_some(),
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
                            let mut is_file_method = false;
                            let mut file_method_sub_tag = 0;
                            if name == "exists" || name == "text" || name == "json" {
                                let is_file = match &(*ptr).data {
                                    GcData::Object(map) => {
                                        let file_key = super::gc::get_or_create_string("_isFile");
                                        map.get(&super::value::MapKey(Value::string(file_key)))
                                            .map(|v| v.as_boolean())
                                            .unwrap_or(false)
                                    }
                                    GcData::Struct(s) => {
                                        s.descriptor.name.as_ref() == "File"
                                    }
                                    _ => false,
                                };
                                if is_file {
                                    is_file_method = true;
                                    file_method_sub_tag = match name {
                                        "exists" => 0,
                                        "text" => 1,
                                        "json" => 2,
                                        _ => 3,
                                    };
                                }
                            }
                            if is_json_method {
                                *frame_slots.add(dest) = Value(super::value::TAG_METHOD_JSON | (ptr as u64 & super::value::PTR_MASK));
                            } else if is_text_method {
                                *frame_slots.add(dest) = Value(super::value::TAG_METHOD_TEXT | (ptr as u64 & super::value::PTR_MASK));
                            } else if is_file_method && file_method_sub_tag < 3 {
                                *frame_slots.add(dest) = Value(super::value::TAG_METHOD_FILE | (ptr as u64 & super::value::PTR_MASK & !3) | file_method_sub_tag);
                            } else {
                                match &(*ptr).data {
                                    GcData::Object(map) => {
                                        if let Some(val) = map.get(&super::value::MapKey(name_val)) {
                                            *frame_slots.add(dest) = *val;
                                        } else if let Some(m) = get_object_builtin_method_id(name) {
                                            let ptr = super::gc::gc_alloc_builtin_method(obj, m);
                                            *frame_slots.add(dest) = Value::function(ptr);
                                        } else {
                                            *frame_slots.add(dest) = Value::null();
                                        }
                                    }
                                    GcData::Struct(s) => {
                                         if let Some(val) = s.get_field(name_val) {
                                             *frame_slots.add(dest) = val;
                                         } else if let Some(&method_val) = s.descriptor.methods.get(&super::value::MapKey(name_val)) {
                                             let bound_method = super::gc::GcBoundMethod {
                                                 receiver: obj,
                                                 function: method_val.as_gc_ptr(),
                                             };
                                             let ptr = super::gc::gc_allocate(super::gc::GcData::BoundMethod(bound_method));
                                             *frame_slots.add(dest) = Value::function(ptr);
                                         } else if let Some(m) = get_object_builtin_method_id(name) {
                                             let ptr = super::gc::gc_alloc_builtin_method(obj, m);
                                             *frame_slots.add(dest) = Value::function(ptr);
                                         } else {
                                             *frame_slots.add(dest) = Value::null();
                                         }
                                    }
                                    _ => {
                                        *frame_slots.add(dest) = Value::null();
                                    }
                                }
                            }
                        } else if obj.is_array() {
                            let ptr = obj.as_gc_ptr();
                            match &(*ptr).data {
                                GcData::Array(arr) => {
                                    if name == "length" {
                                        *frame_slots.add(dest) = Value::number(arr.len() as f64);
                                    } else if let Some(m) = get_array_builtin_method_id(name) {
                                        let ptr = super::gc::gc_alloc_builtin_method(obj, m);
                                        *frame_slots.add(dest) = Value::function(ptr);
                                    } else if let Ok(idx) = name.parse::<usize>() {
                                        let val = arr.get(idx).cloned().unwrap_or(Value::null());
                                        *frame_slots.add(dest) = val;
                                    } else {
                                        *frame_slots.add(dest) = Value::null();
                                    }
                                }
                                _ => {
                                    *frame_slots.add(dest) = Value::null();
                                }
                            }
                        } else if obj.is_string() {
                            if name == "length" {
                                let s = obj.as_str().unwrap_or("");
                                *frame_slots.add(dest) = Value::number(s.chars().count() as f64);
                            } else if let Some(m) = get_string_builtin_method_id(name) {
                                let ptr = super::gc::gc_alloc_builtin_method(obj, m);
                                *frame_slots.add(dest) = Value::function(ptr);
                            } else {
                                *frame_slots.add(dest) = Value::null();
                            }
                        } else {
                            return Err("Only objects, arrays, and strings have properties".into());
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
                                GcData::Struct(s) => {
                                    if s.set_field(name_val, val) {
                                        gc_write_barrier(ptr, &val);
                                    } else if s.descriptor.name.as_ref() == "Anonymous" {
                                        let new_desc = crate::vm::shape::transition_shape_add_property(&s.descriptor, name_val);
                                        s.descriptor = new_desc;
                                        s.fields.push(val);
                                        gc_write_barrier(ptr, &val);
                                    } else {
                                        let name = name_val.as_str().unwrap_or("");
                                        return Err(format!("Struct has no field '{}'", name));
                                    }
                                }
                                _ => {}
                            }
                        } else if obj.is_array() {
                            let name_rc = name_val.as_str().unwrap_or("");
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
                                _ => {}
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
                                    _ => {
                                        *frame_slots.add(dest) = Value::null();
                                    }
                                }
                            } else if index.is_string() {
                                let s = index.as_str().unwrap_or("");
                                if let Ok(idx) = s.parse::<usize>() {
                                    match &(*ptr).data {
                                        GcData::Array(arr) => {
                                            let val = arr.get(idx).cloned().unwrap_or(Value::null());
                                            *frame_slots.add(dest) = val;
                                        }
                                        _ => {
                                            *frame_slots.add(dest) = Value::null();
                                        }
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
                                    GcData::Struct(s) => {
                                        let val = s.get_field(index).unwrap_or(Value::null());
                                        *frame_slots.add(dest) = val;
                                    }
                                    _ => {
                                        *frame_slots.add(dest) = Value::null();
                                    }
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
                                    _ => {}
                                }
                            } else if index.is_string() {
                                let s = index.as_str().unwrap_or("");
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
                                        _ => {}
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
                                    GcData::Struct(s) => {
                                        if s.set_field(index, val) {
                                            gc_write_barrier(ptr, &val);
                                        } else if s.descriptor.name.as_ref() == "Anonymous" {
                                            let new_desc = crate::vm::shape::transition_shape_add_property(&s.descriptor, index);
                                            s.descriptor = new_desc;
                                            s.fields.push(val);
                                            gc_write_barrier(ptr, &val);
                                        } else {
                                            let name = index.as_str().unwrap_or("");
                                            return Err(format!("Struct has no field '{}'", name));
                                        }
                                    }
                                    _ => {}
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
                            let mut func_ptr = callee.as_gc_ptr();
                            if let GcData::BuiltinMethod(builtin) = &(*func_ptr).data {
                                let receiver = builtin.receiver;
                                let method = builtin.method;
                                let mut args = Vec::with_capacity(arg_count);
                                for i in 0..arg_count {
                                    args.push(*frame_slots.add(func_reg + 1 + i));
                                }
                                sync_stack!();
                                frame.ip = ip.offset_from(code_ptr) as usize - 1;
                                let result = self.execute_builtin_method(receiver, method, args)?;
                                reload_stack!();
                                *frame_slots.add(dest) = result;
                                continue;
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
                            if actual_arg_count != raw_func.arity {
                                return Err(format!(
                                    "Expected {} args but got {}",
                                    raw_func.arity, actual_arg_count
                                ));
                            }

                            let count = raw_func.invocation_count.get() + 1;
                            raw_func.invocation_count.set(count);

                            if self.use_jit && !raw_func.is_async {
                                if raw_func.jit_ptr.get().is_none() && (self.jit_threshold == 0 || count >= self.jit_threshold) {
                                    crate::jit::compile_function(self, raw_fn_ptr);
                                }
                                if raw_func.jit_ptr.get().is_some() {
                                    frame.ip = ip.offset_from(code_ptr) as usize - 1;
                                    let new_slots_offset = slots_offset + func_reg + 1;
                                    self.frames.push(CallFrame {
                                        function: func_ptr,
                                        ip: 0,
                                        slots_offset: new_slots_offset,
                                        dest_reg: dest,
                                    });

                                    let initial_depth = self.frames.len() - 1;
                                    let res = self.execute_loop(initial_depth)?;
                                    if self.frames.len() <= target_depth {
                                        return Ok(res);
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
                                    reload_stack!();
                                    *frame_slots.add(dest) = res;
                                    ip = code_ptr.add(frame.ip + 1);
                                    continue;
                                }
                            }

                            frame.ip = ip.offset_from(code_ptr) as usize - 1;
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
                            frame.ip = ip.offset_from(code_ptr) as usize - 1;
                            let result = native(args);
                            reload_stack!();
                            if self.stack.is_empty() {
                                return Ok(Value::null());
                            }
                            *frame_slots.add(dest) = result;
                        } else if callee.is_method_file() {
                            let ptr = (callee.0 & super::value::PTR_MASK & !3) as *mut GcObject;
                            let method_sub_tag = callee.0 & 3;

                            let path_str = match &(*ptr).data {
                                GcData::Object(map) => {
                                    let name_key = super::gc::get_or_create_string("name");
                                    let name_val = map.get(&super::value::MapKey(Value::string(name_key))).cloned().unwrap_or(Value::null());
                                    match name_val.as_str() {
                                        Some(s) => s.to_string(),
                                        None => "".to_string(),
                                    }
                                }
                                GcData::Struct(s) => {
                                    let name_key = super::gc::get_or_create_string("name");
                                    let name_val = s.get_field(Value::string(name_key)).unwrap_or(Value::null());
                                    match name_val.as_str() {
                                        Some(s) => s.to_string(),
                                        None => "".to_string(),
                                    }
                                }
                                _ => "".to_string(),
                            };

                            sync_stack!();
                            let result = match method_sub_tag {
                                0 => { // exists
                                    Value::boolean(std::path::Path::new(&path_str).exists())
                                }
                                1 => { // text
                                    if let Ok(content) = std::fs::read_to_string(&path_str) {
                                        let ptr = super::gc::gc_allocate(GcData::String(std::rc::Rc::from(content)));
                                        Value::string(ptr)
                                    } else {
                                        Value::null()
                                    }
                                }
                                2 => { // json
                                    if let Ok(content) = std::fs::read_to_string(&path_str) {
                                        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
                                            super::gc::json_to_value(json_val)
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
                                            let s = body_val.as_str().unwrap_or("");
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
                        } else if callee.is_object() && matches!(&(*callee.as_gc_ptr()).data, crate::vm::gc::GcData::StructConstructor(_)) {
                            let ptr = callee.as_gc_ptr();
                            let descriptor = match &(*ptr).data {
                                crate::vm::gc::GcData::StructConstructor(desc) => desc.clone(),
                                _ => unreachable!(),
                            };
                            let mut args = Vec::with_capacity(arg_count);
                            for i in 0..arg_count {
                                args.push(*frame_slots.add(func_reg + 1 + i));
                            }
                            sync_stack!();
                            frame.ip = ip.offset_from(code_ptr) as usize - 1;
                            let result = crate::jit::helpers::construct_struct_from_args_helper(&descriptor, args)?;
                            reload_stack!();
                            *frame_slots.add(dest) = result;
                        } else {
                            return Err(format!("Can only call functions (callee: 0x{:x})", callee.0).into());
                        }
                    }
                    OpCode::Await => {
                        let await_value = *frame_slots.add(instruction.rb as usize);
                        if await_value.is_promise() {
                            let promise_ptr = await_value.as_gc_ptr();
                            let state = match &(*promise_ptr).data {
                                crate::vm::gc::GcData::Promise(prom) => prom.state.clone(),
                                _ => unreachable!(),
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
                                     frame.ip = ip.offset_from(code_ptr) as usize - 1;

                                     self.close_upvalues(0);
                                     let suspended_stack = std::mem::take(&mut self.stack);
                                     let suspended_frames = std::mem::take(&mut self.frames);

                                     match &mut (*promise_ptr).data {
                                         crate::vm::gc::GcData::Promise(prom) => {
                                             *prom.suspended_stack.lock().unwrap() = suspended_stack;
                                             *prom.suspended_frames.lock().unwrap() = suspended_frames;
                                         }
                                         _ => unreachable!(),
                                     }
                                     return Ok(Value::null());
                                }
                            }
                        } else {
                            *frame_slots.add(instruction.ra as usize) = await_value;
                        }
                    }
                    OpCode::DefineStruct => {
                        let name_val = *constants_ptr.add(instruction.operand as usize);
                        let fields_val = *constants_ptr.add(instruction.ra as usize);
                        let name_rc: Rc<str> = Rc::from(name_val.as_str().unwrap_or(""));
                        let fields_vec = match &(*fields_val.as_gc_ptr()).data {
                            GcData::Array(arr) => arr,
                            _ => unreachable!(),
                        };
                        
                        let mut field_indices = FnvHashMap::default();
                        for (idx, &f_val) in fields_vec.iter().enumerate() {
                            field_indices.insert(super::value::MapKey(f_val), idx);
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
                        
                        let descriptor = std::rc::Rc::new(super::gc::StructDescriptor::new(
                            name_rc.clone(),
                            field_indices,
                            methods,
                        ));
                        self.structs.insert(name_rc.clone(), descriptor.clone());
                        let ptr = gc_allocate(GcData::StructConstructor(descriptor));
                        self.globals.insert(name_rc, Value::object(ptr));
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
                                super::gc::UpvalueLocation::Open(slot) => *stack_start.add(slot),
                                super::gc::UpvalueLocation::Closed(val) => val,
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
                                super::gc::UpvalueLocation::Open(slot) => {
                                    *stack_start.add(slot) = val;
                                }
                                super::gc::UpvalueLocation::Closed(ref mut v) => {
                                    *v = val;
                                }
                            },
                            _ => unreachable!(),
                        }
                    }
                    OpCode::Closure => {
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
                                let parent_uv_ptr = match &(*frame.function).data {
                                    GcData::Closure(c) => c.upvalues[uv_desc.index as usize],
                                    _ => unreachable!(),
                                };
                                upvalue_ptrs.push(parent_uv_ptr);
                            }
                        }
                        let closure_ptr = super::gc::gc_allocate(GcData::Closure(super::gc::GcClosure {
                            function: raw_fn_ptr,
                            upvalues: upvalue_ptrs,
                        }));
                        *frame_slots.add(dest) = Value::function(closure_ptr);
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
                        func = get_func!(frame.function);
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
                }
            }
        }
    }

    pub fn run_event_loop(&mut self) -> Result<(), String> {
        let prev_vm = crate::vm::er_http::ACTIVE_VM.with(|active| active.replace(self as *mut VM));
        let has_server = crate::vm::er_http::ROUTES.with(|r| !r.borrow().is_empty()) 
            || crate::vm::er_http::WS_ROUTES.with(|r| !r.borrow().is_empty())
            || crate::vm::er_http::LISTEN_PORT.with(|p| p.get().is_some());
        let result = self.run_event_loop_inner(!has_server);
        crate::vm::er_http::ACTIVE_VM.with(|active| active.set(prev_vm));
        result
    }

    fn run_event_loop_inner(&mut self, wait_for_active: bool) -> Result<(), String> {
        loop {
            // 1. Process all expired timers from the min-heap
            loop {
                let now = Instant::now();
                let timer_opt = {
                    let mut timers = self.timers.lock().unwrap();
                    if let Some(top) = timers.peek() {
                        if top.due_time <= now {
                            timers.pop()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                let Some(timer) = timer_opt else { break; };

                self.active_async_tasks.fetch_sub(1, Ordering::SeqCst);
                match timer.action {
                    VmTimerAction::Callback { callback, args } => {
                        if let Err(e) = self.call_function_reentrant(callback, args) {
                            return Err(e);
                        }
                    }
                    VmTimerAction::ResolvePromise { promise_ptr, value } => {
                        let mut q = self.event_loop_queue.lock().unwrap();
                        q.push(EventLoopTask {
                            callback: Value::null(),
                            args: Vec::new(),
                            result: AsyncResult::ResolvePromise(promise_ptr, value),
                        });
                    }
                }
            }

            // 2. Process tasks from event loop queue (promises, I/O, etc.)
            let tasks = {
                let mut queue = self.event_loop_queue.lock().unwrap();
                std::mem::take(&mut *queue)
            };

            for task in tasks {
                match task.result {
                    AsyncResult::ResolvePromise(promise_ptr, _) |
                    AsyncResult::ResolveFetchPromise(promise_ptr, _) |
                    AsyncResult::ResolveTextPromise(promise_ptr, _) |
                    AsyncResult::ResolveJsonPromise(promise_ptr, _) |
                    AsyncResult::ResolveWritePromise(promise_ptr, _) => {
                        let resolved_value = match task.result {
                            AsyncResult::ResolvePromise(_, val) => val,
                            AsyncResult::ResolveFetchPromise(_, res) => {
                                match res {
                                    Ok(body_str) => {
                                        let mut map = crate::vm::gc::get_pooled_map(2);
                                        let body_key = crate::vm::gc::intern_string("_body");
                                        let body_val = crate::vm::gc::gc_alloc_string(&body_str);
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
                            AsyncResult::ResolveTextPromise(_, res) => {
                                match res {
                                    Ok(content) => {
                                        let ptr = crate::vm::gc::gc_alloc_string(&content);
                                        Value::string(ptr)
                                    }
                                    Err(e) => {
                                        eprintln!("[FileTextPromise] Error: {}", e);
                                        Value::null()
                                    }
                                }
                            }
                            AsyncResult::ResolveJsonPromise(_, res) => {
                                match res {
                                    Ok(content) => {
                                        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
                                            crate::vm::gc::json_to_value(json_val)
                                        } else {
                                            Value::null()
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[FileJsonPromise] Error: {}", e);
                                        Value::null()
                                    }
                                }
                            }
                            AsyncResult::ResolveWritePromise(_, res) => {
                                match res {
                                    Ok(bytes) => Value::number(bytes as f64),
                                    Err(e) => {
                                        eprintln!("[FileWritePromise] Error: {}", e);
                                        Value::number(0.0)
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
                                crate::vm::gc::GcData::Closure(c) => match &(*c.function).data {
                                    crate::vm::gc::GcData::Function(f) => f,
                                    _ => unreachable!(),
                                },
                                _ => unreachable!(),
                            }
                        };
                        let inst = func.chunk.code[frame.ip];
                        assert!(inst.op == OpCode::Await || inst.op == OpCode::Call);

                        // Write the resolved value to the destination register of the suspended instruction
                        self.stack[frame.slots_offset + inst.ra as usize] = resolved_value;

                        // Advance instruction pointer past Await/Call
                        frame.ip += 1;

                        // Resume execution directly at frame.ip (which points to the instruction right after Await/Call)
                        if let Err(e) = self.execute_loop_interpreter(0) {
                            return Err(e);
                        }
                        continue;
                    }
                    _ => {}
                }

                let mut args = Vec::new();
                match task.result {
                    AsyncResult::Timeout => {
                        args.extend(task.args);
                    }
                    _ => {}
                };

                if let Err(e) = self.call_function_reentrant(task.callback, args) {
                    return Err(e);
                }
            }

            let active = self.active_async_tasks.load(Ordering::SeqCst);
            if active == 0 || !wait_for_active {
                let queue_empty = self.event_loop_queue.lock().unwrap().is_empty();
                let has_due_timers = {
                    let timers = self.timers.lock().unwrap();
                    timers.peek().map_or(false, |t| t.due_time <= Instant::now())
                };
                if queue_empty && !has_due_timers {
                    break;
                }
            }

            let queue = self.event_loop_queue.lock().unwrap();
            if queue.is_empty() {
                let now = Instant::now();
                let wait_timeout = {
                    let timers = self.timers.lock().unwrap();
                    if let Some(top) = timers.peek() {
                        if top.due_time > now {
                            top.due_time.duration_since(now).min(Duration::from_millis(10))
                        } else {
                            Duration::from_millis(0)
                        }
                    } else {
                        Duration::from_millis(10)
                    }
                };
                if wait_timeout > Duration::from_millis(0) {
                    let _ = self.event_loop_condvar.wait_timeout(queue, wait_timeout);
                }
            }
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
    fn test_function_declaration() {
        let vm = run_code("fn add(a, b) {\n  return a + b\n}\nlet res = add(3, 4)\nlet res2 = (fn(x) { return x * 2 })(5)").unwrap();
        assert_eq!(vm.get_global("res").unwrap().as_number(), 7.0);
        assert_eq!(vm.get_global("res2").unwrap().as_number(), 10.0);
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
    fn test_struct() {
        let vm = run_code("struct Player {\n  name: string,\n  age: int,\n}\nlet p : Player = {\n  name: \"Vishnu\",\n  age: 25,\n}\nlet val = p.name").unwrap();
        assert_eq!(vm.get_global("val").unwrap().as_str().unwrap(), "Vishnu");
    }

    #[test]
    fn test_struct_type_safety() {
        let code = "struct Player {\n  name: string,\n  age: int,\n}\nlet p : Player = {\n  name: 67,\n  age: 25,\n}";
        let res = run_code(code);
        match res {
            Err(err) => {
                assert!(err.contains("Expected type \"string\" but got 67"));
            }
            Ok(_) => {
                panic!("Expected type error but code compiled successfully");
            }
        }
    }

    #[test]
    fn test_struct_mutation() {
        let vm = run_code("struct Player {\n  name: string,\n  age: int,\n}\nlet p : Player = {\n  name: \"Vishnu\",\n  age: 25,\n}\np.age = 26\nlet val = p.age").unwrap();
        assert_eq!(vm.get_global("val").unwrap().as_number(), 26.0);
    }

    #[test]
    fn test_struct_methods() {
        let vm = run_code("struct Player {\n  name: string,\n  age: int,\n  fn printPlayer() {\n    return this.name\n  }\n}\nlet p : Player = {\n  name: \"Vishnu\",\n  age: 25,\n}\nlet val = p.printPlayer()").unwrap();
        assert_eq!(vm.get_global("val").unwrap().as_str().unwrap(), "Vishnu");
    }

    #[test]
    fn test_struct_nested_typecheck() {
        let code = "struct Position {\n  x: int,\n  y: int,\n}\nstruct Player {\n  pos: Position,\n  name: string,\n}\nlet position : Position = {\n  x: 10,\n  y: 20,\n}\nlet p : Player = {\n  pos: position,\n  name: \"Vishnu\",\n}\nlet val = p.pos.x";
        let vm = run_code(code).unwrap();
        assert_eq!(vm.get_global("val").unwrap().as_number(), 10.0);
    }

    #[test]
    fn test_struct_composition() {
        let code = "struct Position {\n  x: int,\n  y: int,\n  fn printPos() {\n    return this.x\n  }\n}\nstruct Parent {\n  fn getVal() {\n    return 100\n  }\n}\nstruct Player embed Position, Parent {\n  name: string,\n  fn printPlayer() {\n    return this.name\n  }\n  fn getVal() {\n    return super.getVal() + 5\n  }\n}\nlet p : Player = {\n  x: 10,\n  y: 20,\n  name: \"Vishnu\",\n}\nlet val_x = p.printPos()\nlet val_name = p.printPlayer()\nlet val_super = p.getVal()";
        let vm = run_code(code).unwrap();
        assert_eq!(vm.get_global("val_x").unwrap().as_number(), 10.0);
        assert_eq!(vm.get_global("val_name").unwrap().as_str().unwrap(), "Vishnu");
        assert_eq!(vm.get_global("val_super").unwrap().as_number(), 105.0);
    }

    #[test]
    fn test_interfaces() {
        let code = "interface Barker {\n  name: string,\n  fn bark()\n}\nstruct Dog {\n  name: string,\n  age: int,\n  fn bark() {\n    return \"Woof! \" + this.name\n  }\n}\nlet pet: Barker = Dog({\n  name: \"Rex\",\n  age: 3,\n})\nlet message = pet.bark()";
        let vm = run_code(code).unwrap();
        assert_eq!(vm.get_global("message").unwrap().as_str().unwrap(), "Woof! Rex");
    }

    #[test]
    fn test_interfaces_invalid() {
        let code = "interface Barker {\n  name: string,\n  fn bark()\n}\nstruct Cat {\n  name: string,\n  age: int\n}\nlet pet: Barker = Cat({\n  name: \"Whiskers\",\n  age: 2,\n})";
        let result = run_code(code);
        assert!(result.is_err());
        assert!(result.err().unwrap().contains("does not implement interface"));
    }

    #[test]
    fn test_struct_new_constructor_syntax() {
        let code = r#"
            struct Dog {
                name: string,
                age: int
            }

            const d = Dog()
            let val_d_name = d.name

            user1 = Dog("Vishnu")
            let val_u1_name = user1.name
            let val_u1_age = user1.age

            const user2 = Dog({ name: "vishnu" })
            let val_u2_name = user2.name

            let user3 : Dog = []
            let val_u3_name = user3.name

            let user4 : Dog = [{}]
            let val_u4_name = user4[0].name

            user5 = Dog([ { name: "A" }, { name: "B" } ])
            let val_u5_name0 = user5[0].name
            let val_u5_name1 = user5[1].name
        "#;
        let vm = run_code(code).unwrap();
        assert!(vm.get_global("val_d_name").unwrap().is_null());
        assert_eq!(vm.get_global("val_u1_name").unwrap().as_str().unwrap(), "Vishnu");
        assert!(vm.get_global("val_u1_age").unwrap().is_null());
        assert_eq!(vm.get_global("val_u2_name").unwrap().as_str().unwrap(), "vishnu");
        assert!(vm.get_global("val_u3_name").unwrap().is_null());
        assert!(vm.get_global("val_u4_name").unwrap().is_null());
        assert_eq!(vm.get_global("val_u5_name0").unwrap().as_str().unwrap(), "A");
        assert_eq!(vm.get_global("val_u5_name1").unwrap().as_str().unwrap(), "B");
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

        assert_eq!(gc_with_state(|s| s.phase), GcPhase::Pause);

        for _ in 0..10000 {
            gc_allocate(GcData::Array(vec![]));
        }

        vm.gc_step();
        assert_eq!(gc_with_state(|s| s.phase), GcPhase::Mark);

        while gc_with_state(|s| s.phase) != GcPhase::Sweep {
            vm.gc_step();
        }

        while gc_with_state(|s| s.phase) == GcPhase::Sweep {
            vm.gc_step();
        }

        assert_eq!(gc_with_state(|s| s.phase), GcPhase::Pause);

        let mut found_parent = false;
        let mut found_garbage = false;
        unsafe {
            let mut curr = gc_with_state(|s| s.head);
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
    fn test_gc_stack_roots_deep_no_truncation() {
        gc_free_all();

        let mut vm = VM::new();
        // Fill stack with 500 null values (> 256)
        for _ in 0..500 {
            vm.stack.push(Value::null());
        }

        // Place a live object at slot 450 (which would have been ignored by 256 truncation)
        let deep_ptr = gc_allocate(GcData::Array(vec![Value::number(42.0)]));
        vm.stack[450] = Value::array(deep_ptr);

        let garbage_ptr = gc_allocate(GcData::Array(vec![Value::number(999.0)]));
        let _garbage = Value::array(garbage_ptr);

        // Run full GC collection
        vm.collect_garbage();

        let mut found_deep = false;
        let mut found_garbage = false;
        unsafe {
            let mut curr = gc_with_state(|s| s.head);
            while !curr.is_null() {
                if curr == deep_ptr {
                    found_deep = true;
                }
                if curr == garbage_ptr {
                    found_garbage = true;
                }
                curr = (*curr).next;
            }
        }
        assert!(found_deep, "Deep stack object (>256 slots) MUST be kept alive");
        assert!(!found_garbage, "Unreferenced garbage object must be reclaimed");

        gc_free_all();
    }

    #[test]
    fn test_gc_string_cache_sweep() {
        gc_free_all();

        let mut vm = VM::new();

        let live_ptr = crate::vm::gc::intern_string("live_constant_identifier");
        let _dead_ptr = crate::vm::gc::intern_string("transient_dead_identifier");

        // Reference live_ptr in global variables
        vm.globals.insert("live_id".into(), Value::string(live_ptr));

        // Ensure both are in cache initially
        crate::vm::gc::STRING_CACHE.with(|cache| {
            let c = cache.borrow();
            assert!(c.contains_key("live_constant_identifier"));
            assert!(c.contains_key("transient_dead_identifier"));
        });

        // Run GC collection
        vm.collect_garbage();

        // Verify cache sweeping: live retained, dead evicted
        crate::vm::gc::STRING_CACHE.with(|cache| {
            let c = cache.borrow();
            assert!(c.contains_key("live_constant_identifier"), "Live interned string must be retained");
            assert!(!c.contains_key("transient_dead_identifier"), "Dead interned string must be evicted");
        });

        // Verify that re-interning live string returns identical pointer
        let live_ptr_2 = crate::vm::gc::intern_string("live_constant_identifier");
        assert_eq!(live_ptr, live_ptr_2);

        gc_free_all();
    }

    #[test]
    fn test_gc_atomic_flag() {
        gc_free_all();

        assert_eq!(GC_NEEDS_STEP.load(Ordering::Relaxed), false);

        let threshold = gc_with_state(|s| s.alloc_threshold);
        for _ in 0..threshold {
            gc_allocate(GcData::Array(vec![]));
        }

        assert_eq!(GC_NEEDS_STEP.load(Ordering::Relaxed), true);

        let mut vm = VM::new();
        vm.collect_garbage();

        assert_eq!(GC_NEEDS_STEP.load(Ordering::Relaxed), false);

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

    #[test]
    fn test_concurrent_structured_syntax() {
        gc_free_all();
        let source = "
            let counter = 0
            const taskA = () => {
                counter = counter + 10
            }
            const taskB = () => {
                counter = counter + 20
            }
            concurrent {
                taskA()
                taskB()
            }
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
        vm.register_global("arrayPush", Value::native_function(crate::vm::er_http::native_array_push));
        vm.register_global("arrayLen", Value::native_function(crate::vm::er_http::native_array_len));
        vm.register_global("setIoMode", Value::native_function(crate::vm::er_http::native_set_io_mode));
        vm.register_global("getIoMode", Value::native_function(crate::vm::er_http::native_get_io_mode));
        crate::vm::er_http::register_eronom_file_api(&mut vm).unwrap();
        vm.use_jit = true;
        vm.run(function).unwrap();
        vm.run_event_loop().unwrap();

        assert_eq!(vm.get_global("counter").unwrap().as_number(), 30.0);
    }

    #[test]
    fn test_set_timeout_scale_and_ordering() {
        gc_free_all();
        let source = "
            let order = []
            setTimeout(() => {
                arrayPush(order, 3)
            }, 30)
            setTimeout(() => {
                arrayPush(order, 1)
            }, 10)
            setTimeout(() => {
                arrayPush(order, 2)
            }, 20)
        ";
        let tokens = crate::frontend::lex(source);
        let mut parser = crate::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = Compiler::new();
        let function = compiler.compile(&stmts).unwrap();

        let mut vm = VM::new();
        vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
        vm.register_global("clearTimeout", Value::native_function(crate::vm::er_http::native_clear_timeout));
        vm.register_global("arrayPush", Value::native_function(crate::vm::er_http::native_array_push));
        vm.use_jit = true;
        vm.run(function).unwrap();
        vm.run_event_loop().unwrap();

        let order_val = vm.get_global("order").unwrap();
        assert!(order_val.is_array());
        unsafe {
            if let GcData::Array(arr) = &(*order_val.as_gc_ptr()).data {
                assert_eq!(arr.len(), 3);
                assert_eq!(arr[0].as_number(), 1.0);
                assert_eq!(arr[1].as_number(), 2.0);
                assert_eq!(arr[2].as_number(), 3.0);
            } else {
                panic!("Expected array");
            }
        }
    }

    #[test]
    fn test_clear_timeout_functionality() {
        gc_free_all();
        let source = "
            let fired = []
            let t1 = setTimeout(() => {
                arrayPush(fired, 1)
            }, 10)
            let t2 = setTimeout(() => {
                arrayPush(fired, 2)
            }, 20)
            clearTimeout(t2)
        ";
        let tokens = crate::frontend::lex(source);
        let mut parser = crate::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = Compiler::new();
        let function = compiler.compile(&stmts).unwrap();

        let mut vm = VM::new();
        vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
        vm.register_global("clearTimeout", Value::native_function(crate::vm::er_http::native_clear_timeout));
        vm.register_global("arrayPush", Value::native_function(crate::vm::er_http::native_array_push));
        vm.use_jit = true;
        vm.run(function).unwrap();
        vm.run_event_loop().unwrap();

        let fired_val = vm.get_global("fired").unwrap();
        assert!(fired_val.is_array());
        unsafe {
            if let GcData::Array(arr) = &(*fired_val.as_gc_ptr()).data {
                assert_eq!(arr.len(), 1);
                assert_eq!(arr[0].as_number(), 1.0);
            } else {
                panic!("Expected array");
            }
        }
    }

    #[test]
    fn test_http_response_aborted_safety() {
        crate::vm::er_http::end_http_response_json(std::ptr::null_mut(), "{}");
        let null_args = vec![Value::null()];
        crate::vm::er_http::native_context_json(null_args.clone());
        crate::vm::er_http::native_context_html(null_args);
    }

    #[test]
    fn test_websocket_pubsub_api() {
        gc_free_all();
        let source = "
            let app = router()
            let open_called = false
            let msg_received = \"\"
            let is_binary_received = false
            
            app.ws(\"/chat\", {
                open: (ws) => {
                    open_called = true
                },
                message: (ws, msg, is_binary) => {
                    msg_received = msg
                    is_binary_received = is_binary
                },
                close: (ws, code, reason) => {
                }
            })

            let pub_res = app.publish(\"global_room\", \"Hello Everyone\")
            let subs_count = app.numSubscribers(\"global_room\")
        ";
        let tokens = crate::frontend::lex(source);
        let mut parser = crate::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = Compiler::new();
        let function = compiler.compile(&stmts).unwrap();

        let mut vm = VM::new();
        vm.register_global("router", Value::native_function(crate::vm::er_http::native_route));
        vm.use_jit = true;
        vm.run(function).unwrap();

        // Verify WebSocket route registered
        let has_ws_routes = crate::vm::er_http::WS_ROUTES.with(|routes| {
            routes.borrow().iter().any(|r| r.path == "/chat")
        });
        assert!(has_ws_routes);

        // Test Simulated open event
        let dummy_ws = 0x12345 as *mut std::ffi::c_void;
        let path_c = std::ffi::CString::new("/chat").unwrap();
        crate::vm::er_http::ACTIVE_VM.with(|active| active.set(&mut vm as *mut VM));
        crate::vm::er_http::er_ws_on_open(dummy_ws, path_c.as_ptr(), path_c.as_bytes().len());
        assert_eq!(vm.get_global("open_called").unwrap().as_boolean(), true);

        // Test Simulated text message event
        let text_msg = "Hello Eronom";
        let text_c = std::ffi::CString::new(text_msg).unwrap();
        crate::vm::er_http::er_ws_on_message(
            dummy_ws,
            path_c.as_ptr(),
            path_c.as_bytes().len(),
            text_c.as_ptr(),
            text_c.as_bytes().len(),
            0,
        );
        let received_val = vm.get_global("msg_received").unwrap();
        assert_eq!(received_val.as_str().unwrap(), "Hello Eronom");
        assert_eq!(vm.get_global("is_binary_received").unwrap().as_boolean(), false);

        // Test Simulated binary message event
        let binary_bytes: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];
        crate::vm::er_http::er_ws_on_message(
            dummy_ws,
            path_c.as_ptr(),
            path_c.as_bytes().len(),
            binary_bytes.as_ptr() as *const std::ffi::c_char,
            binary_bytes.len(),
            1,
        );
        let bin_msg_val = vm.get_global("msg_received").unwrap();
        assert!(bin_msg_val.is_array());
        unsafe {
            if let GcData::Array(arr) = &(*bin_msg_val.as_gc_ptr()).data {
                assert_eq!(arr.len(), 4);
                assert_eq!(arr[0].as_number(), 0xde as f64);
                assert_eq!(arr[1].as_number(), 0xad as f64);
                assert_eq!(arr[2].as_number(), 0xbe as f64);
                assert_eq!(arr[3].as_number(), 0xef as f64);
            } else {
                panic!("Expected Array for binary frame");
            }
        }
        assert_eq!(vm.get_global("is_binary_received").unwrap().as_boolean(), true);

        // Clean up
        crate::vm::er_http::ACTIVE_VM.with(|active| active.set(std::ptr::null_mut()));
    }

    #[test]
    fn test_extract_bytes_from_value() {
        gc_free_all();
        // 1. Test string extraction
        let s_ptr = crate::vm::gc::gc_alloc_string("hello");
        let (bytes, is_bin) = crate::vm::er_http::extract_bytes_from_value(Value::string(s_ptr), false);
        assert_eq!(bytes, b"hello");
        assert_eq!(is_bin, false);

        let (bytes_forced, is_bin_forced) = crate::vm::er_http::extract_bytes_from_value(Value::string(s_ptr), true);
        assert_eq!(bytes_forced, b"hello");
        assert_eq!(is_bin_forced, true);

        // 2. Test array of numbers extraction
        let mut arr_elems = Vec::new();
        arr_elems.push(Value::number(1.0));
        arr_elems.push(Value::number(2.0));
        arr_elems.push(Value::number(255.0));
        let arr_ptr = crate::vm::gc::gc_allocate(GcData::Array(arr_elems));
        let (arr_bytes, arr_is_bin) = crate::vm::er_http::extract_bytes_from_value(Value::array(arr_ptr), false);
        assert_eq!(arr_bytes, vec![1, 2, 255]);
        assert_eq!(arr_is_bin, true);
    }

    #[test]
    fn test_jit_closures_and_upvalues() {
        let code = "
            fn makeCounter(initial) {
                let count = initial
                fn inc(step) {
                    count = count + step
                    return count
                }
                return inc
            }

            let c1 = makeCounter(10)
            let res1 = c1(5)
            let res2 = c1(3)
        ";
        let vm = run_code(code).unwrap();
        assert_eq!(vm.get_global("res1").unwrap().as_number(), 15.0);
        assert_eq!(vm.get_global("res2").unwrap().as_number(), 18.0);
    }

    #[test]
    fn test_jit_struct_field_access_and_methods() {
        let code = "
            struct Point {
                x: int,
                y: int,
                fn sum() {
                    return this.x + this.y
                }
            }

            let pt : Point = { x: 10, y: 25 }
            let sum_val = pt.sum()
            pt.x = 40
            let sum_val2 = pt.sum()
        ";
        let vm = run_code(code).unwrap();
        assert_eq!(vm.get_global("sum_val").unwrap().as_number(), 35.0);
        assert_eq!(vm.get_global("sum_val2").unwrap().as_number(), 65.0);
    }


    #[test]
    fn test_jit_object_literals_and_array_methods() {
        let code = "
            let obj = { a: 1, b: \"hello\", c: [10, 20] }
            obj.c.push(30)
            let popped = obj.c.pop()
            let len = obj.c.length
            let val_a = obj.a
            let val_b = obj.b
        ";
        let vm = run_code(code).unwrap();
        assert_eq!(vm.get_global("popped").unwrap().as_number(), 30.0);
        assert_eq!(vm.get_global("len").unwrap().as_number(), 2.0);
        assert_eq!(vm.get_global("val_a").unwrap().as_number(), 1.0);
        assert_eq!(vm.get_global("val_b").unwrap().as_str().unwrap(), "hello");
    }

    #[test]
    fn test_jit_dynamic_type_bailout() {
        let code = "
            fn dynAdd(a, b) {
                return a + b
            }

            let r1 = dynAdd(10, 20)
            let r2 = dynAdd(\"Hello \", \"World\")
            let r3 = dynAdd(\"Number: \", 42)
        ";
        let vm = run_code(code).unwrap();
        assert_eq!(vm.get_global("r1").unwrap().as_number(), 30.0);
        assert_eq!(vm.get_global("r2").unwrap().as_str().unwrap(), "Hello World");
        assert_eq!(vm.get_global("r3").unwrap().as_str().unwrap(), "Number: 42");
    }

    #[test]
    fn test_jit_lifecycle_reset() {
        for i in 0..5 {
            let code = format!("let val = {} * 10 + 5", i);
            let vm = run_code(&code).unwrap();
            assert_eq!(vm.get_global("val").unwrap().as_number(), (i * 10 + 5) as f64);
            // Reset JIT context to test teardown and re-creation
            crate::jit::reset_jit_state();
        }
    }
}

