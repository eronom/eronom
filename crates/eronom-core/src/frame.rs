use crate::gc::GcObject;

#[derive(Debug, Clone, Copy)]
pub struct CallFrame {
    pub function: *mut GcObject,
    pub ip: usize,
    pub slots_offset: usize,
    pub dest_reg: usize,
}

unsafe impl Send for CallFrame {}
unsafe impl Sync for CallFrame {}
