use std::alloc::{GlobalAlloc, Layout, System};
use mimalloc::MiMalloc;
use std::sync::atomic::{AtomicU8, Ordering};

const UNINITIALIZED: u8 = 0;
const USE_SYSTEM: u8 = 1;
const USE_MIMALLOC: u8 = 2;

static ALLOCATOR_STATE: AtomicU8 = AtomicU8::new(UNINITIALIZED);

unsafe extern "C" {
    fn getenv(name: *const i8) -> *mut i8;
}

fn use_mimalloc() -> bool {
    match ALLOCATOR_STATE.load(Ordering::Relaxed) {
        USE_SYSTEM => false,
        USE_MIMALLOC => true,
        UNINITIALIZED => {
            ALLOCATOR_STATE.store(USE_SYSTEM, Ordering::Relaxed);
            let use_mi = unsafe {
                let jit_var = getenv(b"ER_NO_JIT\0".as_ptr() as *const i8);
                let mi_var = getenv(b"ER_NO_MIMALLOC\0".as_ptr() as *const i8);
                let mi_var_case = getenv(b"no_MiMalloc\0".as_ptr() as *const i8);
                jit_var.is_null() && mi_var.is_null() && mi_var_case.is_null()
            };
            if use_mi {
                ALLOCATOR_STATE.store(USE_MIMALLOC, Ordering::Relaxed);
                true
            } else {
                ALLOCATOR_STATE.store(USE_SYSTEM, Ordering::Relaxed);
                false
            }
        }
        _ => unreachable!(),
    }
}

struct CondAllocator;

unsafe impl GlobalAlloc for CondAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if use_mimalloc() {
            unsafe { MiMalloc.alloc(layout) }
        } else {
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if use_mimalloc() {
            unsafe { MiMalloc.dealloc(ptr, layout) }
        } else {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if use_mimalloc() {
            unsafe { MiMalloc.alloc_zeroed(layout) }
        } else {
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if use_mimalloc() {
            unsafe { MiMalloc.realloc(ptr, layout, new_size) }
        } else {
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }
}

#[global_allocator]
static GLOBAL: CondAllocator = CondAllocator;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.er>", args[0]);
        std::process::exit(1);
    }
    if let Err(e) = er::run_file(&args[1]) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
