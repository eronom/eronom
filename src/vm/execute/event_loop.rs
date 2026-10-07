use std::time::{Instant, Duration};
use std::sync::atomic::Ordering;

use crate::vm::value::Value;
use crate::vm::bytecode::OpCode;
use super::types::{VM, AsyncResult, EventLoopTask, VmTimerAction};

impl VM {
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

                        // 1. Check if any fiber in scheduler was waiting on this promise
                        let awakened_fibers = self.scheduler.wake_promise(promise_ptr, resolved_value);
                        if !awakened_fibers.is_empty() {
                            continue;
                        }

                        if suspended_frames.is_empty() {
                            continue;
                        }

                        // Restore stack and frames (legacy single-VM suspension fallback)
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

            // 3. Process any ready fibers scheduled in ready_queue (bounded tick to prevent starvation)
            let fibers_to_run = self.scheduler.ready_queue.len();
            for _ in 0..fibers_to_run {
                let next_fid = match self.scheduler.ready_queue.pop_front() {
                    Some(fid) => fid,
                    None => break,
                };
                let is_cancelled = if let Some(f) = self.scheduler.fibers.get(&next_fid) {
                    f.interrupted && f.interruption_masks == 0
                } else {
                    false
                };

                if is_cancelled {
                    if let Some(f) = self.scheduler.fibers.get_mut(&next_fid) {
                        f.status = crate::vm::execute::fiber::FiberStatus::Cancelled;
                    }
                    self.run_fiber_finalizers(next_fid);
                    continue;
                }

                self.save_active_fiber();
                self.load_fiber(next_fid);
                let exec_res = self.execute_loop_interpreter(0);
                match exec_res {
                    Ok(val) => {
                        let completed = if let Some(f) = self.scheduler.fibers.get_mut(&next_fid) {
                            if f.status == crate::vm::execute::fiber::FiberStatus::Running
                                && self.frames.is_empty()
                                && f.frames.is_empty()
                            {
                                let comp_prom = f.completion_promise;
                                f.complete(val);
                                if !comp_prom.is_null() {
                                    self.scheduler.wake_promise(comp_prom, val);
                                }
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if completed {
                            self.run_fiber_finalizers(next_fid);
                        }
                    }
                    Err(e) => {
                        let is_interrupted = e == "FiberInterrupted";
                        if let Some(f) = self.scheduler.fibers.get_mut(&next_fid) {
                            if is_interrupted {
                                f.status = crate::vm::execute::fiber::FiberStatus::Cancelled;
                            } else {
                                f.fail(e.clone());
                            }
                        }
                        self.run_fiber_finalizers(next_fid);
                        if !is_interrupted {
                            return Err(e);
                        }
                    }
                }
                self.save_active_fiber();
            }

            let has_ready_fibers = !self.scheduler.ready_queue.is_empty();
            let active = self.active_async_tasks.load(Ordering::SeqCst);
            if (active == 0 && !has_ready_fibers) || !wait_for_active {
                let queue_empty = self.event_loop_queue.lock().unwrap().is_empty();
                let has_due_timers = {
                    let timers = self.timers.lock().unwrap();
                    timers.peek().map_or(false, |t| t.due_time <= Instant::now())
                };
                if queue_empty && !has_due_timers && !has_ready_fibers {
                    break;
                }
            }

            let queue = self.event_loop_queue.lock().unwrap();
            if queue.is_empty() && !has_ready_fibers {
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
