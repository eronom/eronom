use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicUsize, Ordering};
use ::eronom::vm as backend;

pub struct Counter {
    pub allocated: AtomicUsize,
    pub peak: AtomicUsize,
}

impl Counter {
    pub const fn new() -> Self {
        Self {
            allocated: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }
    }

    pub fn add(&self, size: usize) {
        let prev = self.allocated.fetch_add(size, Ordering::SeqCst);
        let current = prev + size;
        let mut peak = self.peak.load(Ordering::SeqCst);
        while current > peak {
            match self.peak.compare_exchange_weak(peak, current, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(actual) => peak = actual,
            }
        }
    }

    pub fn sub(&self, size: usize) {
        self.allocated.fetch_sub(size, Ordering::SeqCst);
    }

    pub fn reset_peak(&self) {
        self.peak.store(self.allocated.load(Ordering::SeqCst), Ordering::SeqCst);
    }
}

pub static COUNTER: Counter = Counter::new();

pub struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        backend::alloc::init_allocator_options();
        let ptr = unsafe { mimalloc::MiMalloc.alloc(layout) };
        if !ptr.is_null() {
            COUNTER.add(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { mimalloc::MiMalloc.dealloc(ptr, layout) };
        COUNTER.sub(layout.size());
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        backend::alloc::init_allocator_options();
        let ptr = unsafe { mimalloc::MiMalloc.alloc_zeroed(layout) };
        if !ptr.is_null() {
            COUNTER.add(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        backend::alloc::init_allocator_options();
        let new_ptr = unsafe { mimalloc::MiMalloc.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            COUNTER.sub(layout.size());
            COUNTER.add(new_size);
        }
        new_ptr
    }
}

pub fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_rss(kb: Option<usize>) -> String {
    match kb {
        Some(k) => {
            if k >= 1024 {
                format!("{:.2} MB", k as f64 / 1024.0)
            } else {
                format!("{} KB", k)
            }
        }
        None => "-".to_string(),
    }
}

pub fn run_command_with_metrics(
    cmd: &str,
    args: &[&str],
) -> (Option<String>, Option<usize>) {
    let mut time_cmd = std::process::Command::new("/usr/bin/time");
    time_cmd.arg("-f").arg("%M");
    time_cmd.arg(cmd).args(args);
    if let Ok(output) = time_cmd.output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut rss = None;
            if let Some(last_line) = stderr.lines().last() {
                if let Ok(kb) = last_line.trim().parse::<usize>() {
                    rss = Some(kb);
                }
            }
            return (Some(stdout), rss);
        }
    }

    if let Ok(output) = std::process::Command::new(cmd).args(args).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            return (Some(stdout), None);
        }
    }

    (None, None)
}
