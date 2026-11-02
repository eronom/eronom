use std::collections::VecDeque;
use fnv::FnvHashMap;
use crate::vm::value::Value;
use crate::vm::gc::GcObject;
use super::fiber::{Fiber, FiberStatus};
use super::types::CallFrame;

pub struct FiberScheduler {
    pub current_fiber_id: u64,
    pub fibers: FnvHashMap<u64, Box<Fiber>>,
    pub ready_queue: VecDeque<u64>,
    pub waiting_fibers: FnvHashMap<*mut GcObject, Vec<u64>>,
}

impl Default for FiberScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl FiberScheduler {
    pub fn new() -> Self {
        let mut sched = Self {
            current_fiber_id: 0,
            fibers: FnvHashMap::default(),
            ready_queue: VecDeque::new(),
            waiting_fibers: FnvHashMap::default(),
        };
        // Create root main fiber (ID 1)
        let main_fiber = Box::new(Fiber::new(None));
        sched.current_fiber_id = main_fiber.id;
        sched.fibers.insert(main_fiber.id, main_fiber);
        sched
    }

    pub fn current_fiber(&mut self) -> Option<&mut Fiber> {
        self.fibers.get_mut(&self.current_fiber_id).map(|b| b.as_mut())
    }

    pub fn get_fiber_mut(&mut self, id: u64) -> Option<&mut Fiber> {
        self.fibers.get_mut(&id).map(|b| b.as_mut())
    }

    pub fn spawn_fiber(&mut self, parent_id: Option<u64>, func_ptr: *mut GcObject, args: &[Value]) -> u64 {
        let mut fiber = Box::new(Fiber::new(parent_id));
        let id = fiber.id;

        for arg in args {
            fiber.stack.push(*arg);
        }
        let needed = fiber.stack.len().max(256);
        if fiber.stack.len() < needed {
            fiber.stack.resize(needed, Value::null());
        }

        fiber.frames.push(CallFrame {
            function: func_ptr,
            ip: 0,
            slots_offset: 0,
            dest_reg: 0,
        });

        fiber.status = FiberStatus::Ready;
        self.fibers.insert(id, fiber);
        self.ready_queue.push_back(id);

        if let Some(pid) = parent_id {
            if let Some(parent) = self.fibers.get_mut(&pid) {
                parent.children.push(id);
            }
        }

        id
    }

    pub fn suspend_current(&mut self, promise_ptr: *mut GcObject, dest_reg: usize) {
        let cid = self.current_fiber_id;
        if let Some(fiber) = self.fibers.get_mut(&cid) {
            fiber.status = FiberStatus::Suspended;
            fiber.waiting_on = Some(promise_ptr);
            fiber.dest_reg = dest_reg;
        }
        self.waiting_fibers.entry(promise_ptr).or_default().push(cid);
    }

    pub fn wake_promise(&mut self, promise_ptr: *mut GcObject, value: Value) -> Vec<u64> {
        let mut awakened = Vec::new();
        if let Some(fiber_ids) = self.waiting_fibers.remove(&promise_ptr) {
            for fid in fiber_ids {
                if let Some(fiber) = self.fibers.get_mut(&fid) {
                    if fiber.status == FiberStatus::Suspended {
                        fiber.status = FiberStatus::Ready;
                        fiber.waiting_on = None;
                        if let Some(frame) = fiber.frames.last_mut() {
                            let reg_idx = frame.slots_offset + fiber.dest_reg;
                            if reg_idx < fiber.stack.len() {
                                fiber.stack[reg_idx] = value;
                            }
                            frame.ip += 1;
                        }
                        self.ready_queue.push_back(fid);
                        awakened.push(fid);
                    }
                }
            }
        }
        awakened
    }
}
