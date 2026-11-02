use crate::vm::value::{Value, TAG_STRING, TAG_NATIVE};
use super::types::{GcColor, GcData, GcObject, UpvalueLocation};
use super::GC_STATE;

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_mark_value(val: &Value) {
    if !val.is_number() && !val.is_inline_string() {
        let tag = val.0 & 0xffff_0000_0000_0000;
        if tag >= TAG_STRING && tag != TAG_NATIVE {
            let ptr = val.as_gc_ptr();
            unsafe {
                if !ptr.is_null() && (*ptr).color == GcColor::White {
                    (*ptr).color = GcColor::Gray;
                    GC_STATE.with(|state| (*state.get()).gray_stack.push(ptr));
                }
            }
        }
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_mark_object(ptr: *mut GcObject) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        if (*ptr).color == GcColor::White {
            (*ptr).color = GcColor::Gray;
            GC_STATE.with(|state| (*state.get()).gray_stack.push(ptr));
        }
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_blacken_object(ptr: *mut GcObject) {
    unsafe {
        if ptr.is_null() {
            return;
        }
        (*ptr).color = GcColor::Black;
        match &(*ptr).data {
            GcData::Empty => {}
            GcData::String(_) => {}
            GcData::Array(arr) => {
                for val in arr {
                    gc_mark_value(val);
                }
            }
            GcData::Object(obj) => {
                for (key, val) in obj {
                    gc_mark_value(&key.0);
                    gc_mark_value(val);
                }
            }
            GcData::Struct(s) => {
                for val in &s.fields {
                    gc_mark_value(val);
                }
            }
            GcData::Function(func) => {
                for constant in &func.chunk.constants {
                    gc_mark_value(constant);
                }
            }
            GcData::Promise(prom) => {
                if let Ok(stack) = prom.suspended_stack.lock() {
                    for val in stack.iter() {
                        gc_mark_value(val);
                    }
                }
                if let Ok(frames) = prom.suspended_frames.lock() {
                    for frame in frames.iter() {
                        let fn_val = Value::function(frame.function);
                        gc_mark_value(&fn_val);
                    }
                }
            }
            GcData::BoundMethod(bm) => {
                gc_mark_value(&bm.receiver);
                gc_mark_object(bm.function);
            }
            GcData::BuiltinMethod(bm) => {
                gc_mark_value(&bm.receiver);
            }
            GcData::StructConstructor(_) => {}
            GcData::Closure(c) => {
                gc_mark_object(c.function);
                for &upval_ptr in &c.upvalues {
                    gc_mark_object(upval_ptr);
                }
            }
            GcData::Upvalue(u) => {
                if let UpvalueLocation::Closed(ref val) = u.location {
                    gc_mark_value(val);
                }
            }
        }
    }
}

pub fn mark_value(val: &Value) {
    gc_mark_value(val);
}

pub fn mark_object(ptr: *mut GcObject) {
    gc_mark_object(ptr);
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_write_barrier(parent: *mut GcObject, child: &Value) {
    unsafe {
        if parent.is_null() {
            return;
        }
        if (*parent).color == GcColor::Black && !child.is_number() && !child.is_inline_string() {
            let tag = child.0 & 0xffff_0000_0000_0000;
            if tag >= TAG_STRING && tag != TAG_NATIVE {
                let child_ptr = child.as_gc_ptr();
                if !child_ptr.is_null() && (*child_ptr).color == GcColor::White {
                    (*child_ptr).color = GcColor::Gray;
                    GC_STATE.with(|state| {
                        (*state.get()).gray_stack.push(child_ptr);
                    });
                }
            }
        }
    }
}
