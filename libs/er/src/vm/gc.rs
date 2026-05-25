use super::value::{
    Value, TAG_NUMBER_MASK, TAG_STRING, TAG_FUNCTION, TAG_METHOD_PUSH, TAG_METHOD_POP
};
use super::bytecode::Function;
use fnv::FnvHashMap;
use std::rc::Rc;
use std::cell::RefCell;

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
    String(Rc<str>),
    Array(Vec<Value>),
    Object(FnvHashMap<Rc<str>, Value>),
    Function(Function),
}

pub struct GcObject {
    pub color: GcColor,
    pub next: *mut GcObject,
    pub data: GcData,
}

pub struct GcState {
    pub head: *mut GcObject,
    pub alloc_count: usize,
    pub phase: GcPhase,
    pub gray_stack: Vec<*mut GcObject>,
    pub sweep_ptr: *mut GcObject,
    pub prev_sweep_ptr: *mut GcObject,
}

thread_local! {
    pub static GC_STATE: RefCell<GcState> = RefCell::new(GcState {
        head: std::ptr::null_mut(),
        alloc_count: 0,
        phase: GcPhase::Pause,
        gray_stack: Vec::new(),
        sweep_ptr: std::ptr::null_mut(),
        prev_sweep_ptr: std::ptr::null_mut(),
    });
    pub static GC_ROOTS: RefCell<Vec<Box<dyn Fn()>>> = RefCell::new(Vec::new());
    pub static GC_NEEDS_STEP: std::cell::Cell<bool> = std::cell::Cell::new(false);
}

pub fn gc_allocate(data: GcData) -> *mut GcObject {
    GC_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let obj = Box::new(GcObject {
            color: GcColor::White,
            next: state.head,
            data,
        });
        let ptr = Box::into_raw(obj);
        state.head = ptr;

        if let GcPhase::Sweep = state.phase {
            if state.prev_sweep_ptr.is_null() {
                state.prev_sweep_ptr = ptr;
            }
        }

        state.alloc_count += 1;
        if state.alloc_count >= 10000 {
            GC_NEEDS_STEP.with(|n| n.set(true));
        }
        ptr
    })
}

pub fn gc_free_all() {
    unsafe {
        GC_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let mut curr = state.head;
            state.head = std::ptr::null_mut();
            while !curr.is_null() {
                let next = (*curr).next;
                let _ = Box::from_raw(curr);
                curr = next;
            }
            state.alloc_count = 0;
            state.phase = GcPhase::Pause;
            state.gray_stack.clear();
            state.sweep_ptr = std::ptr::null_mut();
            state.prev_sweep_ptr = std::ptr::null_mut();
            GC_NEEDS_STEP.with(|n| n.set(false));
        });
    }
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
                    GC_STATE.with(|state| state.borrow_mut().gray_stack.push(ptr));
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
            GC_STATE.with(|state| state.borrow_mut().gray_stack.push(ptr));
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
                        GC_STATE.with(|state| {
                            state.borrow_mut().gray_stack.push(child_ptr);
                        });
                    }
                }
            }
        }
    }
}
