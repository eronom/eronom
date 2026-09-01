use std::cell::Cell;
use std::time::{Instant, Duration};
use std::sync::atomic::Ordering;

use crate::vm::value::Value;
use crate::vm::gc::{
    mark_value, gc_with_state, gc_blacken_object,
    GC_ROOTS, GC_NEEDS_STEP, GcColor, GcPhase, GcObject
};
use super::types::{VM, VmTimerAction};

thread_local! {
    pub static GC_TIME: Cell<Duration> = const { Cell::new(Duration::from_nanos(0)) };
    pub static GC_COUNT: Cell<u64> = const { Cell::new(0) };
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

impl VM {
    pub fn gc_step(&mut self) {
        let start_time = Instant::now();
        GC_COUNT.with(|c| c.set(c.get() + 1));
        let phase = gc_with_state(|state| state.phase);
        match phase {
            GcPhase::Pause => {
                let should_mark = gc_with_state(|state| {
                    if state.alloc_count >= state.alloc_threshold {
                        state.phase = GcPhase::Mark;
                        state.gray_stack.clear();
                        true
                    } else {
                        false
                    }
                });
                if should_mark {
                    self.mark_roots();
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
                
                self.mark_roots();

                loop {
                    let gray_opt = gc_with_state(|s| s.gray_stack.pop());
                    if let Some(ptr) = gray_opt {
                        gc_blacken_object(ptr);
                    } else {
                        break;
                    }
                }
                crate::vm::gc::gc_sweep_string_cache();
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
                                crate::vm::gc::gc_dealloc_object(state, curr);
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

    fn mark_roots(&self) {
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
            crate::vm::gc::mark_object(upval_ptr);
        }
        mark_value(&self.thrown_value);
        if let Ok(queue) = self.event_loop_queue.try_lock() {
            if !queue.is_empty() {
                for task in queue.iter() {
                    mark_value(&task.callback);
                    for arg in task.args.iter() {
                        mark_value(arg);
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
                        VmTimerAction::ResolvePromise { value, .. } => {
                            mark_value(value);
                        }
                    }
                }
            }
        }
        crate::vm::gc::GC_TEMP_SLICES.with(|slices| {
            if let Ok(borrowed) = slices.try_borrow() {
                for &(ptr, len) in borrowed.iter() {
                    if !ptr.is_null() && len > 0 {
                        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
                        for val in slice {
                            mark_value(val);
                        }
                    }
                }
            }
        });
        GC_ROOTS.with(|roots| {
            if let Ok(borrowed) = roots.try_borrow() {
                for root_fn in borrowed.iter() {
                    root_fn();
                }
            }
        });
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
            crate::vm::gc::mark_object(upval_ptr);
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
        crate::vm::gc::gc_sweep_string_cache();

        // 3. Sweep phase: sweep the entire linked list in one go
        gc_with_state(|state| {
            let mut curr = state.head;
            state.head = std::ptr::null_mut();
            let mut prev: *mut GcObject = std::ptr::null_mut();
            
            while !curr.is_null() {
                unsafe {
                    let next = (*curr).next;
                    if (*curr).color == GcColor::White {
                        crate::vm::gc::gc_dealloc_object(state, curr);
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
            // Adaptive threshold: 2× the live set, bounded between 512 and 2048
            state.alloc_threshold = (live_objects * 2).max(512).min(2048);
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
}
