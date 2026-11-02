pub type JitResetIcFn = fn();

pub static mut JIT_RESET_IC_HOOK: Option<JitResetIcFn> = None;

pub fn set_jit_reset_ic_hook(hook: JitResetIcFn) {
    unsafe {
        JIT_RESET_IC_HOOK = Some(hook);
    }
}

pub fn trigger_jit_reset_ic() {
    unsafe {
        if let Some(reset_ic) = JIT_RESET_IC_HOOK {
            reset_ic();
        }
    }
}
