use std::ffi::{c_char, c_void};

#[repr(C)]
pub struct MirDlist {
    pub head: *mut c_void,
    pub tail: *mut c_void,
}

#[repr(C)]
pub struct MirModule {
    pub data: *mut c_void,
    pub name: *const c_char,
    pub head_item: *mut c_void,
    pub tail_item: *mut c_void,
}

unsafe extern "C" {
    pub fn _MIR_init(alloc: *mut c_void, code_alloc: *mut c_void) -> *mut c_void;
    pub fn MIR_finish(ctx: *mut c_void);
    pub fn MIR_scan_string(ctx: *mut c_void, str: *const c_char);
    pub fn MIR_load_module(ctx: *mut c_void, module: *mut c_void);
    pub fn MIR_load_external(ctx: *mut c_void, name: *const c_char, addr: *mut c_void);
    pub fn MIR_link(
        ctx: *mut c_void,
        set_interface: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
        import_resolver: Option<unsafe extern "C" fn(*const c_char) -> *mut c_void>,
    );
    pub fn MIR_gen_init(ctx: *mut c_void);
    pub fn MIR_gen(ctx: *mut c_void, func_item: *mut c_void) -> *mut c_void;
    pub fn MIR_gen_finish(ctx: *mut c_void);
    pub fn MIR_get_module_list(ctx: *mut c_void) -> *mut c_void;
    pub fn MIR_set_gen_interface(ctx: *mut c_void, func_item: *mut c_void);
    pub fn MIR_gen_set_optimize_level(ctx: *mut c_void, level: u32);
}

// Clean up MIR context when VM is dropped
pub fn cleanup_jit(mir_ctx: *mut c_void) {
    unsafe {
        MIR_gen_finish(mir_ctx);
        MIR_finish(mir_ctx);
    }
}
