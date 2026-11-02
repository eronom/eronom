use std::time::Duration;
use std::cell::RefCell;

#[derive(Default, Clone, Copy)]
pub struct JitProfileStats {
    pub make_array_count: u64,
    pub make_array_time: Duration,
    pub make_object_count: u64,
    pub make_object_time: Duration,
    pub get_property_count: u64,
    pub get_property_time: Duration,
    pub call_non_vm_count: u64,
    pub call_non_vm_time: Duration,
    pub add_count: u64,
    pub add_time: Duration,
}

pub const JIT_PROFILING: bool = false;

thread_local! {
    pub static JIT_PROFILER: RefCell<JitProfileStats> = RefCell::new(JitProfileStats::default());
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_reset_profiler() {
    JIT_PROFILER.with(|p| *p.borrow_mut() = JitProfileStats::default());
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_print_profiler() {
    JIT_PROFILER.with(|p| {
        let stats = p.borrow();
        println!("=== JIT FFI Helper Profiler Stats ===");
        println!("  MakeArray:   count={:<8} time={:?}", stats.make_array_count, stats.make_array_time);
        println!("  MakeObject:  count={:<8} time={:?}", stats.make_object_count, stats.make_object_time);
        println!("  GetProperty: count={:<8} time={:?}", stats.get_property_count, stats.get_property_time);
        println!("  CallNonVM:   count={:<8} time={:?}", stats.call_non_vm_count, stats.call_non_vm_time);
        println!("  Add:         count={:<8} time={:?}", stats.add_count, stats.add_time);
        println!("=====================================");
    });
}
