use std::rc::Rc;
use std::time::{Instant, Duration};
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicUsize, AtomicU64, Ordering};
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

#[derive(Clone)]
pub struct VmEventLoop {
    pub queue: Arc<Mutex<Vec<EventLoopTask>>>,
    pub condvar: Arc<Condvar>,
    pub active_async_tasks: Arc<AtomicUsize>,
    pub pending_callbacks: Arc<Mutex<Vec<PendingAsync>>>,
    pub timers: Arc<Mutex<BinaryHeap<VmTimer>>>,
    pub next_timer_id: Arc<AtomicU64>,
}

impl Default for VmEventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl VmEventLoop {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
            condvar: Arc::new(Condvar::new()),
            active_async_tasks: Arc::new(AtomicUsize::new(0)),
            pending_callbacks: Arc::new(Mutex::new(Vec::new())),
            timers: Arc::new(Mutex::new(BinaryHeap::new())),
            next_timer_id: Arc::new(AtomicU64::new(1)),
        }
    }

    #[inline]
    pub fn enqueue_task(&self, task: EventLoopTask) {
        if let Ok(mut q) = self.queue.lock() {
            q.push(task);
        }
        self.condvar.notify_all();
    }

    #[inline]
    pub fn post_and_decrement(&self, task: EventLoopTask) {
        if let Ok(mut q) = self.queue.lock() {
            q.push(task);
        }
        self.active_async_tasks.fetch_sub(1, Ordering::SeqCst);
        self.condvar.notify_all();
    }

    #[inline]
    pub fn increment_active_tasks(&self) {
        self.active_async_tasks.fetch_add(1, Ordering::SeqCst);
    }

    #[inline]
    pub fn decrement_active_tasks(&self) {
        self.active_async_tasks.fetch_sub(1, Ordering::SeqCst);
    }

    #[inline]
    pub fn active_tasks(&self) -> usize {
        self.active_async_tasks.load(Ordering::SeqCst)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.lock().map_or(true, |q| q.is_empty())
    }

    pub fn take_tasks(&self) -> Vec<EventLoopTask> {
        let mut queue = self.queue.lock().unwrap();
        std::mem::take(&mut *queue)
    }

    pub fn pop_expired_timer(&self) -> Option<VmTimer> {
        let now = Instant::now();
        let mut timers = self.timers.lock().unwrap();
        if let Some(top) = timers.peek() {
            if top.due_time <= now {
                return timers.pop();
            }
        }
        None
    }

    pub fn has_due_timers(&self) -> bool {
        let timers = self.timers.lock().unwrap();
        timers.peek().map_or(false, |t| t.due_time <= Instant::now())
    }

    pub fn next_wait_timeout(&self) -> Duration {
        let now = Instant::now();
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
    }

    pub fn wait_timeout(&self, timeout: Duration) {
        if timeout > Duration::from_millis(0) {
            let queue = self.queue.lock().unwrap();
            let _ = self.condvar.wait_timeout(queue, timeout);
        }
    }

    pub fn schedule_timer(&self, due_time: Instant, action: VmTimerAction) -> u64 {
        let timer_id = self.next_timer_id.fetch_add(1, Ordering::SeqCst);
        self.timers.lock().unwrap().push(VmTimer {
            id: timer_id,
            due_time,
            action,
        });
        self.active_async_tasks.fetch_add(1, Ordering::SeqCst);
        self.condvar.notify_all();
        timer_id
    }

    pub fn cancel_timer(&self, timer_id: u64) -> bool {
        let mut timers = self.timers.lock().unwrap();
        let mut remaining: Vec<VmTimer> = timers.drain().collect();
        let mut found = false;
        if let Some(pos) = remaining.iter().position(|t| t.id == timer_id) {
            remaining.swap_remove(pos);
            self.active_async_tasks.fetch_sub(1, Ordering::SeqCst);
            found = true;
        }
        for t in remaining {
            timers.push(t);
        }
        found
    }

    pub fn trace_roots<F>(&self, mut mark_value: F)
    where
        F: FnMut(&Value),
    {
        if let Ok(queue) = self.queue.try_lock() {
            if !queue.is_empty() {
                for task in queue.iter() {
                    mark_value(&task.callback);
                    for arg in task.args.iter() {
                        mark_value(arg);
                    }
                    match &task.result {
                        AsyncResult::ResolvePromise(ptr, val) => {
                            if !ptr.is_null() {
                                crate::vm::gc::mark_object(*ptr);
                            }
                            mark_value(val);
                        }
                        AsyncResult::ResolveFetchPromise(ptr, _)
                        | AsyncResult::ResolveTextPromise(ptr, _)
                        | AsyncResult::ResolveJsonPromise(ptr, _)
                        | AsyncResult::ResolveWritePromise(ptr, _) => {
                            if !ptr.is_null() {
                                crate::vm::gc::mark_object(*ptr);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Ok(pending) = self.pending_callbacks.try_lock() {
            if !pending.is_empty() {
                for item in pending.iter() {
                    mark_value(&item.callback);
                    for arg in item.args.iter() {
                        mark_value(arg);
                    }
                }
            }
        }
        if let Ok(timers) = self.timers.try_lock() {
            if !timers.is_empty() {
                for timer in timers.iter() {
                    match &timer.action {
                        VmTimerAction::Callback { callback, args } => {
                            mark_value(callback);
                            for arg in args {
                                mark_value(arg);
                            }
                        }
                        VmTimerAction::ResolvePromise { promise_ptr, value } => {
                            if !promise_ptr.is_null() {
                                crate::vm::gc::mark_object(*promise_ptr);
                            }
                            mark_value(value);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Default)]
pub struct ShapeRegistry {
    pub structs: FnvHashMap<Rc<str>, Rc<StructDescriptor>>,
    pub auto_shapes: FnvHashMap<Vec<MapKey>, (Rc<StructDescriptor>, Vec<usize>)>,
    pub last_matched_keys: Vec<Value>,
    pub last_matched_descriptor: Option<Rc<StructDescriptor>>,
    pub last_matched_offsets: Vec<usize>,
}

impl ShapeRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}
pub use crate::vm::frame::CallFrame;

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
    pub open_upvalues: Vec<*mut GcObject>,
    pub thrown_value: Value,

    // Subsystems
    pub shapes: ShapeRegistry,
    pub event_loop: VmEventLoop,
    pub scheduler: super::scheduler::FiberScheduler,
}

pub type JitCompileFn = fn(&mut VM, *mut crate::vm::gc::GcObject) -> *const std::ffi::c_void;
pub type JitResetFn = fn();
pub type JitCleanupFn = unsafe fn(*mut std::ffi::c_void);
pub type JitResetIcFn = fn();

pub static mut JIT_COMPILE_HOOK: Option<JitCompileFn> = None;
pub static mut JIT_RESET_HOOK: Option<JitResetFn> = None;
pub static mut JIT_CLEANUP_HOOK: Option<JitCleanupFn> = None;
pub static mut JIT_RESET_IC_HOOK: Option<JitResetIcFn> = None;

pub fn set_jit_hooks(
    compile: JitCompileFn,
    reset: JitResetFn,
    cleanup: JitCleanupFn,
    reset_ic: JitResetIcFn,
) {
    unsafe {
        JIT_COMPILE_HOOK = Some(compile);
        JIT_RESET_HOOK = Some(reset);
        JIT_CLEANUP_HOOK = Some(cleanup);
        JIT_RESET_IC_HOOK = Some(reset_ic);
    }
}

pub fn trigger_jit_reset() {
    unsafe {
        if let Some(reset) = JIT_RESET_HOOK {
            reset();
        }
    }
}

pub fn trigger_jit_reset_ic() {
    unsafe {
        if let Some(reset_ic) = JIT_RESET_IC_HOOK {
            reset_ic();
        }
    }
}

impl Drop for VM {
    fn drop(&mut self) {
        trigger_jit_reset_ic();
        if let Some(ctx) = self.mir_ctx {
            unsafe {
                if let Some(cleanup) = JIT_CLEANUP_HOOK {
                    cleanup(ctx);
                }
            }
        }
    }
}

pub fn format_undeclared_var_error(name: &str) -> String {
    format!(
        "Variable '{}' not declared. It needs to be declared with 'let' or 'const', or by assigning to it.",
        name
    )
}
