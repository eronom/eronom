use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, GcData, GcObject, GcUpvalue, UpvalueLocation};
use super::types::VM;

impl VM {
    pub fn capture_upvalue(&mut self, abs_slot: usize) -> *mut GcObject {
        for &upval_ptr in &self.open_upvalues {
            unsafe {
                if let GcData::Upvalue(ref u) = (*upval_ptr).data {
                    if let UpvalueLocation::Open(slot) = u.location {
                        if slot == abs_slot {
                            return upval_ptr;
                        }
                    }
                }
            }
        }
        let upval_ptr = gc_allocate(GcData::Upvalue(GcUpvalue {
            location: UpvalueLocation::Open(abs_slot),
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
                        UpvalueLocation::Open(slot) => slot >= from_slot,
                        _ => false,
                    },
                    _ => false,
                };
                if should_close {
                    let slot = match &(*upval_ptr).data {
                        GcData::Upvalue(u) => match u.location {
                            UpvalueLocation::Open(s) => s,
                            _ => 0,
                        },
                        _ => 0,
                    };
                    let val = if slot < self.stack.len() {
                        self.stack[slot]
                    } else {
                        Value::null()
                    };
                    (*upval_ptr).data = GcData::Upvalue(GcUpvalue {
                        location: UpvalueLocation::Closed(val),
                    });
                    self.open_upvalues.swap_remove(i);
                } else {
                    i += 1;
                }
            }
        }
    }
}
