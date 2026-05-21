use super::value::{
    Value, TAG_NUMBER_MASK, TAG_STRING, TAG_FUNCTION, TAG_METHOD_PUSH, TAG_METHOD_POP
};
use super::bytecode::Function;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcColor {
    White,
    Gray,
    Black,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcPhase {
    Pause,
    Mark,
    Atomic,
    Sweep,
}

pub enum GcData {
    String(String),
    Array(Vec<Value>),
    Object(HashMap<Rc<str>, Value>),
    Function(Function),
}

pub struct GcObject {
    pub color: GcColor,
    pub next: *mut GcObject,
    pub data: GcData,
}

thread_local! {
    pub static GC_HEAD: std::cell::Cell<*mut GcObject> = std::cell::Cell::new(std::ptr::null_mut());
    pub static ALLOC_COUNT: std::cell::Cell<usize> = std::cell::Cell::new(0);
    pub static GC_ROOTS: std::cell::RefCell<Vec<Box<dyn Fn()>>> = std::cell::RefCell::new(Vec::new());
    pub static GC_PHASE: std::cell::Cell<GcPhase> = std::cell::Cell::new(GcPhase::Pause);
    pub static GRAY_STACK: std::cell::RefCell<Vec<*mut GcObject>> = std::cell::RefCell::new(Vec::new());
    pub static SWEEP_PTR: std::cell::Cell<*mut GcObject> = std::cell::Cell::new(std::ptr::null_mut());
    pub static PREV_SWEEP_PTR: std::cell::Cell<*mut GcObject> = std::cell::Cell::new(std::ptr::null_mut());
}

pub fn gc_allocate(data: GcData) -> *mut GcObject {
    GC_HEAD.with(|head| {
        let obj = Box::new(GcObject {
            color: GcColor::White,
            next: head.get(),
            data,
        });
        let ptr = Box::into_raw(obj);
        head.set(ptr);

        if GC_PHASE.with(|phase| phase.get()) == GcPhase::Sweep {
            PREV_SWEEP_PTR.with(|prev| {
                if prev.get().is_null() {
                    prev.set(ptr);
                }
            });
        }

        ALLOC_COUNT.with(|c| c.set(c.get() + 1));
        ptr
    })
}

pub fn gc_free_all() {
    unsafe {
        GC_HEAD.with(|head| {
            let mut curr = head.get();
            head.set(std::ptr::null_mut());
            while !curr.is_null() {
                let next = (*curr).next;
                let _ = Box::from_raw(curr);
                curr = next;
            }
        });
    }
    ALLOC_COUNT.with(|c| c.set(0));
    GC_PHASE.with(|p| p.set(GcPhase::Pause));
    GRAY_STACK.with(|gs| gs.borrow_mut().clear());
    SWEEP_PTR.with(|s| s.set(std::ptr::null_mut()));
    PREV_SWEEP_PTR.with(|p| p.set(std::ptr::null_mut()));
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_mark_value(val: &Value) {
    if (val.0 & TAG_NUMBER_MASK) == TAG_NUMBER_MASK {
        let tag = val.0 & 0xffff_0000_0000_0000;
        if (tag >= TAG_STRING && tag <= TAG_FUNCTION) || tag == TAG_METHOD_PUSH || tag == TAG_METHOD_POP {
            let ptr = val.as_gc_ptr();
            unsafe {
                if !ptr.is_null() && (*ptr).color == GcColor::White {
                    (*ptr).color = GcColor::Gray;
                    GRAY_STACK.with(|gs| gs.borrow_mut().push(ptr));
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
            GRAY_STACK.with(|gs| gs.borrow_mut().push(ptr));
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
            GcData::String(_) => {}
            GcData::Array(arr) => {
                for val in arr {
                    gc_mark_value(val);
                }
            }
            GcData::Object(obj) => {
                for val in obj.values() {
                    gc_mark_value(val);
                }
            }
            GcData::Function(func) => {
                for constant in &func.chunk.constants {
                    gc_mark_value(constant);
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
        if (*parent).color == GcColor::Black {
            if (child.0 & TAG_NUMBER_MASK) == TAG_NUMBER_MASK {
                let tag = child.0 & 0xffff_0000_0000_0000;
                if (tag >= TAG_STRING && tag <= TAG_FUNCTION) || tag == TAG_METHOD_PUSH || tag == TAG_METHOD_POP {
                    let child_ptr = child.as_gc_ptr();
                    if !child_ptr.is_null() && (*child_ptr).color == GcColor::White {
                        (*child_ptr).color = GcColor::Gray;
                        GRAY_STACK.with(|stack| {
                            stack.borrow_mut().push(child_ptr);
                        });
                    }
                }
            }
        }
    }
}
