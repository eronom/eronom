use std::rc::Rc;
use std::time::Instant;
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicUsize, AtomicU64};
use std::collections::BinaryHeap;
use fnv::FnvHashMap;

use crate::vm::value::{Value, MapKey};
use crate::vm::gc::{GcObject, StructDescriptor};

pub enum AsyncResult {
    Timeout,
    ResolvePromise(*mut GcObject, Value),
    ResolveFetchPromise(*mut GcObject, Result<String, String>),
    ResolveTextPromise(*mut GcObject, Result<String, String>),
    ResolveJsonPromise(*mut GcObject, Result<String, String>),
    ResolveWritePromise(*mut GcObject, Result<usize, String>),
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
    ResolvePromise { promise_ptr: *mut GcObject, value: Value },
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

pub struct CallFrame {
    pub function: *mut GcObject,
    pub ip: usize,
    pub slots_offset: usize,
    pub dest_reg: usize,
}

unsafe impl Send for CallFrame {}
unsafe impl Sync for CallFrame {}

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
    pub structs: FnvHashMap<Rc<str>, Rc<StructDescriptor>>,
    pub auto_shapes: FnvHashMap<Vec<MapKey>, (Rc<StructDescriptor>, Vec<usize>)>,
    pub last_matched_keys: Vec<Value>,
    pub last_matched_descriptor: Option<Rc<StructDescriptor>>,
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

impl Drop for VM {
    fn drop(&mut self) {
        if let Some(ctx) = self.mir_ctx {
            crate::jit::cleanup_jit(ctx);
        }
    }
}

pub fn format_undeclared_var_error(name: &str) -> String {
    format!(
        "Variable '{}' not declared. It needs to be declared with 'let' or 'const', or by assigning to it.",
        name
    )
}
