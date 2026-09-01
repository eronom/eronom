use std::path::PathBuf;
use std::sync::Mutex;

pub static BASE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
pub static DEFAULT_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
pub static IS_PROD: Mutex<bool> = Mutex::new(false);
pub static HMR_QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());
pub static ACTIVE_CONNECTIONS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

pub struct GcGuard;
impl Drop for GcGuard {
    fn drop(&mut self) {
        crate::vm::gc::gc_free_all();
    }
}

#[repr(C)]
pub struct Tm {
    pub tm_sec: std::ffi::c_int,
    pub tm_min: std::ffi::c_int,
    pub tm_hour: std::ffi::c_int,
    pub tm_mday: std::ffi::c_int,
    pub tm_mon: std::ffi::c_int,
    pub tm_year: std::ffi::c_int,
    pub tm_wday: std::ffi::c_int,
    pub tm_yday: std::ffi::c_int,
    pub tm_isdst: std::ffi::c_int,
    #[cfg(unix)]
    pub tm_gmtoff: std::ffi::c_long,
    #[cfg(unix)]
    pub tm_zone: *const std::ffi::c_char,
}

#[cfg(unix)]
unsafe extern "C" {
    pub fn time(time: *mut std::ffi::c_long) -> std::ffi::c_long;
    pub fn localtime_r(timep: *const std::ffi::c_long, result: *mut Tm) -> *mut Tm;
}

#[cfg(windows)]
unsafe extern "C" {
    pub fn _time64(time: *mut i64) -> i64;
    pub fn _localtime64_s(result: *mut Tm, timep: *const i64) -> std::ffi::c_int;
}

pub fn get_local_time_string() -> String {
    unsafe {
        let mut tm_val = std::mem::zeroed::<Tm>();
        #[cfg(unix)]
        {
            let mut t: std::ffi::c_long = 0;
            time(&mut t);
            localtime_r(&t, &mut tm_val);
        }
        #[cfg(windows)]
        {
            let mut t: i64 = 0;
            _time64(&mut t);
            _localtime64_s(&mut tm_val, &t);
        }
        let hour = tm_val.tm_hour;
        let min = tm_val.tm_min;
        let sec = tm_val.tm_sec;
        let am_pm = if hour >= 12 { "PM" } else { "AM" };
        let display_hour = if hour == 0 {
            12
        } else if hour > 12 {
            hour - 12
        } else {
            hour
        };
        format!("{:02}:{:02}:{:02} {}", display_hour, min, sec, am_pm)
    }
}
