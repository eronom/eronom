use crate::vm::execute::VM;
use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, gc_write_barrier, GcData};

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_upvalue(vm: *mut VM, upval_idx: i64) -> Value {
    unsafe {
        let frame = (*vm).frames.last().unwrap();
        let upval_ptr = match &(*frame.function).data {
            GcData::Closure(c) => c.upvalues[upval_idx as usize],
            _ => return Value::null(),
        };
        let val = match &(*upval_ptr).data {
            GcData::Upvalue(u) => match u.location {
                crate::vm::gc::UpvalueLocation::Open(slot) => (*vm).stack.as_ptr().add(slot).read(),
                crate::vm::gc::UpvalueLocation::Closed(val) => val,
            },
            _ => Value::null(),
        };
        val
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_upvalue(vm: *mut VM, upval_idx: i64, val: Value) -> i64 {
    unsafe {
        let frame = (*vm).frames.last().unwrap();
        let upval_ptr = match &(*frame.function).data {
            GcData::Closure(c) => c.upvalues[upval_idx as usize],
            _ => return -1,
        };
        match &mut (*upval_ptr).data {
            GcData::Upvalue(u) => match u.location {
                crate::vm::gc::UpvalueLocation::Open(slot) => {
                    (*vm).stack.as_mut_ptr().add(slot).write(val);
                }
                crate::vm::gc::UpvalueLocation::Closed(ref mut v) => {
                    *v = val;
                }
            },
            _ => return -1,
        }
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_make_closure(vm: *mut VM, raw_fn_val: Value) -> Value {
    unsafe {
        let raw_fn_ptr = raw_fn_val.as_gc_ptr();
        let fn_proto = match &(*raw_fn_ptr).data {
            GcData::Function(f) => f,
            _ => return Value::null(),
        };
        let frame = (*vm).frames.last().unwrap();
        let slots_offset = frame.slots_offset;
        let mut upvalue_ptrs = Vec::with_capacity(fn_proto.upvalues.len());
        for uv_desc in &fn_proto.upvalues {
            if uv_desc.is_local {
                let abs_slot = slots_offset + uv_desc.index as usize;
                let uv_ptr = (*vm).capture_upvalue(abs_slot);
                upvalue_ptrs.push(uv_ptr);
            } else {
                let parent_uv_ptr = match &(*frame.function).data {
                    GcData::Closure(c) => c.upvalues[uv_desc.index as usize],
                    _ => return Value::null(),
                };
                upvalue_ptrs.push(parent_uv_ptr);
            }
        }
        let closure_ptr = gc_allocate(GcData::Closure(crate::vm::gc::GcClosure {
            function: raw_fn_ptr,
            upvalues: upvalue_ptrs,
        }));
        Value::function(closure_ptr)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_close_upvalues(vm: *mut VM, rel_slot: i64) -> i64 {
    unsafe {
        if !(*vm).open_upvalues.is_empty() {
            if let Some(frame) = (*vm).frames.last() {
                let slot = frame.slots_offset + rel_slot as usize;
                (*vm).close_upvalues(slot);
            }
        }
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_await(vm: *mut VM, await_val: Value, dest: *mut Value) -> i64 {
    unsafe {
        if await_val.is_promise() {
            let promise_ptr = await_val.as_gc_ptr();
            let state = match &(*promise_ptr).data {
                crate::vm::gc::GcData::Promise(prom) => prom.state.clone(),
                _ => return -1,
            };
            let promise_status = {
                let lock = state.lock().unwrap();
                lock.clone()
            };
            match promise_status {
                crate::vm::gc::PromiseState::Fulfilled(val) => {
                    *dest = val;
                    0
                }
                crate::vm::gc::PromiseState::Rejected(err) => {
                    (*vm).has_error_flag = 1; (*vm).error = Some(err);
                    -1
                }
                crate::vm::gc::PromiseState::Pending => {
                    // Pending promise: Needs suspend
                    -3
                }
            }
        } else {
            *dest = await_val;
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_reset_context() {
    crate::jit::compiler::reset_jit_state();
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_write_barrier(parent: *mut crate::vm::gc::GcObject, child: Value) {
    gc_write_barrier(parent, &child);
}
