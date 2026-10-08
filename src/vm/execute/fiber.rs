use std::sync::atomic::{AtomicU64, Ordering};
use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, GcData, GcObject, GcPromise, PromiseState};
use super::types::CallFrame;

static NEXT_FIBER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberStatus {
    Ready,
    Running,
    Suspended,
    Completed,
    Cancelled,
    Failed,
}

impl std::fmt::Display for FiberStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FiberStatus::Ready => write!(f, "ready"),
            FiberStatus::Running => write!(f, "running"),
            FiberStatus::Suspended => write!(f, "suspended"),
            FiberStatus::Completed => write!(f, "completed"),
            FiberStatus::Cancelled => write!(f, "cancelled"),
            FiberStatus::Failed => write!(f, "failed"),
        }
    }
}

pub struct Fiber {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub status: FiberStatus,
    pub stack: Vec<Value>,
    pub frames: Vec<CallFrame>,
    pub open_upvalues: Vec<*mut GcObject>,
    pub children: Vec<u64>,
    pub completion_promise: *mut GcObject,
    pub waiting_on: Option<*mut GcObject>,
    pub dest_reg: usize,
    pub result: Value,
    pub error: Option<String>,
    pub interruption_masks: usize,
    pub interrupted: bool,
    pub finalizers: Vec<Value>,
}

unsafe impl Send for Fiber {}
unsafe impl Sync for Fiber {}

impl Fiber {
    pub fn new(parent_id: Option<u64>) -> Self {
        let id = NEXT_FIBER_ID.fetch_add(1, Ordering::Relaxed);
        let prom = GcPromise {
            state: std::sync::Arc::new(std::sync::Mutex::new(PromiseState::Pending)),
            suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let completion_promise = gc_allocate(GcData::Promise(prom));

        Self {
            id,
            parent_id,
            status: FiberStatus::Ready,
            stack: Vec::with_capacity(256),
            frames: Vec::with_capacity(16),
            open_upvalues: Vec::new(),
            children: Vec::new(),
            completion_promise,
            waiting_on: None,
            dest_reg: 0,
            result: Value::null(),
            error: None,
            interruption_masks: 0,
            interrupted: false,
            finalizers: Vec::new(),
        }
    }

    pub fn complete(&mut self, val: Value) {
        self.status = FiberStatus::Completed;
        self.result = val;
        unsafe {
            if !self.completion_promise.is_null() {
                if let GcData::Promise(p) = &(*self.completion_promise).data {
                    if let Ok(mut state) = p.state.lock() {
                        *state = PromiseState::Fulfilled(val);
                    }
                }
            }
        }
    }

    pub fn fail(&mut self, err: String) {
        self.status = FiberStatus::Failed;
        self.error = Some(err.clone());
        unsafe {
            if !self.completion_promise.is_null() {
                if let GcData::Promise(p) = &(*self.completion_promise).data {
                    if let Ok(mut state) = p.state.lock() {
                        *state = PromiseState::Rejected(err);
                    }
                }
            }
        }
    }
}

pub fn native_fiber_spawn(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let callee = args[0];
    let arg_list = if args.len() > 1 && args[1].is_array() {
        unsafe {
            match &(*args[1].as_gc_ptr()).data {
                GcData::Array(arr) => arr.clone(),
                _ => Vec::new(),
            }
        }
    } else if args.len() > 1 {
        args[1..].to_vec()
    } else {
        Vec::new()
    };

    let vm_ptr = crate::vm::er_http::ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let func_ptr = if callee.is_function() {
        callee.as_gc_ptr()
    } else {
        return Value::null();
    };

    let parent_id = Some(vm.scheduler.current_fiber_id);
    let fiber_id = vm.scheduler.spawn_fiber(parent_id, func_ptr, &arg_list);
    let completion_promise = vm.scheduler.get_fiber_mut(fiber_id).map(|f| f.completion_promise).unwrap_or(std::ptr::null_mut());

    let mut map = crate::vm::gc::get_pooled_map(4);
    let id_key = crate::vm::gc::intern_string("id");
    let prom_key = crate::vm::gc::intern_string("_promise");
    map.insert(crate::vm::value::MapKey(Value::string(id_key)), Value::number(fiber_id as f64));
    if !completion_promise.is_null() {
        map.insert(crate::vm::value::MapKey(Value::string(prom_key)), Value::promise(completion_promise));
    }
    let ptr = gc_allocate(GcData::Object(map));
    Value::object(ptr)
}

pub fn native_fiber_yield(_args: Vec<Value>) -> Value {
    let vm_ptr = crate::vm::er_http::ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let current_id = vm.scheduler.current_fiber_id;
    if let Some(fiber) = vm.scheduler.fibers.get_mut(&current_id) {
        if let Some(frame) = vm.frames.last_mut() {
            let func_ptr = frame.function;
            let ip = frame.ip;
            let func = unsafe {
                match &(*func_ptr).data {
                    crate::vm::gc::GcData::Function(f) => f,
                    crate::vm::gc::GcData::Closure(c) => match &(*c.function).data {
                        crate::vm::gc::GcData::Function(f) => f,
                        _ => unreachable!(),
                    },
                    _ => unreachable!(),
                }
            };
            let inst = func.chunk.code[ip];
            let dest = inst.ra as usize;
            let dest_idx = frame.slots_offset + dest;
            if dest_idx < vm.stack.len() {
                vm.stack[dest_idx] = Value::null();
            }
            frame.ip += 1;
        }

        fiber.stack = std::mem::take(&mut vm.stack);
        fiber.frames = std::mem::take(&mut vm.frames);
        fiber.open_upvalues = std::mem::take(&mut vm.open_upvalues);
        fiber.status = FiberStatus::Ready;
    }
    vm.scheduler.ready_queue.push_back(current_id);

    Value::null()
}

pub fn register_fiber_natives(vm: &mut super::types::VM) {
    vm.register_global("Eronom_fiberSpawn", Value::native_function(native_fiber_spawn));
    vm.register_global("Eronom_fiberYield", Value::native_function(native_fiber_yield));
    vm.register_global("Eronom_fiberCurrentId", Value::native_function(native_fiber_current_id));
    vm.register_global("Eronom_fiberStatus", Value::native_function(native_fiber_status));
    vm.register_global("Eronom_fiberInterrupt", Value::native_function(native_fiber_interrupt));
    vm.register_global("Eronom_fiberMask", Value::native_function(native_fiber_mask));
    vm.register_global("Eronom_fiberAddFinalizer", Value::native_function(native_fiber_add_finalizer));
    vm.register_global("Eronom_bracket", Value::native_function(native_bracket));
}

pub fn extract_fiber_id(target: Value) -> u64 {
    if target.is_number() {
        target.as_number() as u64
    } else if target.is_object() {
        unsafe {
            match &(*target.as_gc_ptr()).data {
                GcData::Object(map) => {
                    let id_key = crate::vm::gc::intern_string("id");
                    map.get(&crate::vm::value::MapKey(Value::string(id_key)))
                        .and_then(|v| if v.is_number() { Some(v.as_number() as u64) } else { None })
                        .unwrap_or(0)
                }
                _ => 0,
            }
        }
    } else {
        0
    }
}

pub fn native_fiber_current_id(_args: Vec<Value>) -> Value {
    let vm_ptr = crate::vm::er_http::ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::number(0.0);
    }
    let vm = unsafe { &mut *vm_ptr };
    Value::number(vm.scheduler.current_fiber_id as f64)
}

pub fn native_fiber_status(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let fiber_id = extract_fiber_id(args[0]);

    let vm_ptr = crate::vm::er_http::ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() || fiber_id == 0 {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };
    if let Some(fiber) = vm.scheduler.fibers.get(&fiber_id) {
        let s = fiber.status.to_string();
        let ptr = crate::vm::gc::gc_alloc_string(&s);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_fiber_interrupt(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let fiber_id = extract_fiber_id(args[0]);
    let vm_ptr = crate::vm::er_http::ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() || fiber_id == 0 {
        return Value::boolean(false);
    }
    let vm = unsafe { &mut *vm_ptr };

    if let Some(fiber) = vm.scheduler.fibers.get_mut(&fiber_id) {
        fiber.interrupted = true;
        if fiber.status == FiberStatus::Suspended && fiber.interruption_masks == 0 {
            fiber.status = FiberStatus::Ready;
            vm.scheduler.ready_queue.push_back(fiber_id);
        }
        Value::boolean(true)
    } else {
        Value::boolean(false)
    }
}

pub fn native_fiber_mask(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let closure = args[0];
    let vm_ptr = crate::vm::er_http::ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };
    let cid = vm.scheduler.current_fiber_id;

    if let Some(f) = vm.scheduler.fibers.get_mut(&cid) {
        f.interruption_masks += 1;
    }

    let res = vm.call_function_reentrant(closure, Vec::new());

    if let Some(f) = vm.scheduler.fibers.get_mut(&cid) {
        f.interruption_masks = f.interruption_masks.saturating_sub(1);
    }

    match res {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[FiberMask] Error: {}", e);
            Value::null()
        }
    }
}

pub fn native_fiber_add_finalizer(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let finalizer = args[0];
    let vm_ptr = crate::vm::er_http::ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };
    let cid = vm.scheduler.current_fiber_id;
    if let Some(f) = vm.scheduler.fibers.get_mut(&cid) {
        f.finalizers.push(finalizer);
    }
    Value::null()
}

pub fn native_bracket(args: Vec<Value>) -> Value {
    if args.len() < 3 {
        return Value::null();
    }
    let acquire = args[0];
    let use_fn = args[1];
    let release = args[2];

    let vm_ptr = crate::vm::er_http::ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    // 1. Mask interruption during acquire
    let cid = vm.scheduler.current_fiber_id;
    if let Some(f) = vm.scheduler.fibers.get_mut(&cid) {
        f.interruption_masks += 1;
    }

    let resource = match vm.call_function_reentrant(acquire, Vec::new()) {
        Ok(res) => res,
        Err(e) => {
            if let Some(f) = vm.scheduler.fibers.get_mut(&cid) {
                f.interruption_masks = f.interruption_masks.saturating_sub(1);
            }
            eprintln!("[Bracket] Acquire error: {}", e);
            return Value::null();
        }
    };

    // 2. Unmask interruption for use
    if let Some(f) = vm.scheduler.fibers.get_mut(&cid) {
        f.interruption_masks = f.interruption_masks.saturating_sub(1);
    }

    // 3. Execute use_fn
    let use_res = vm.call_function_reentrant(use_fn, vec![resource]);

    // 4. Mask interruption during release
    if let Some(f) = vm.scheduler.fibers.get_mut(&cid) {
        f.interruption_masks += 1;
    }

    let _ = vm.call_function_reentrant(release, vec![resource]);

    if let Some(f) = vm.scheduler.fibers.get_mut(&cid) {
        f.interruption_masks = f.interruption_masks.saturating_sub(1);
    }

    match use_res {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[Bracket] Use error: {}", e);
            Value::null()
        }
    }
}
