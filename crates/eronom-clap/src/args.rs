pub trait ArgIter<'a> {
    fn next(&mut self) -> Option<&'a [u8]>;
    fn remain(&self) -> &[&'a [u8]];
}

#[inline]
fn pop_first<'a>(remain: &mut &'a [&'a [u8]]) -> Option<&'a [u8]> {
    if remain.is_empty() {
        return None;
    }
    let res = remain[0];
    *remain = &remain[1..];
    Some(res)
}

pub struct SliceIterator<'a> {
    pub remain: &'a [&'a [u8]],
}

impl<'a> SliceIterator<'a> {
    pub fn new(remain: &'a [&'a [u8]]) -> Self {
        Self { remain }
    }
}

impl<'a> ArgIter<'a> for SliceIterator<'a> {
    #[inline]
    fn next(&mut self) -> Option<&'a [u8]> {
        pop_first(&mut self.remain)
    }

    #[inline]
    fn remain(&self) -> &[&'a [u8]] {
        self.remain
    }
}

pub struct OsIterator {
    pub remain: &'static [&'static [u8]],
}

#[cfg(target_os = "linux")]
static mut RAW_ARGS_STORAGE: [&'static [u8]; 128] = [&[]; 128];
#[cfg(target_os = "linux")]
static mut RAW_ARGS_LEN: usize = 0;

#[cfg(target_os = "linux")]
#[used]
#[unsafe(link_section = ".init_array")]
static INIT_HOOK: extern "C" fn(i32, *const *const u8) = capture_raw_argv;

#[cfg(target_os = "linux")]
extern "C" fn capture_raw_argv(argc: i32, argv: *const *const u8) {
    if argc <= 0 || argv.is_null() {
        return;
    }
    let count = (argc as usize).min(128);
    for i in 0..count {
        unsafe {
            let ptr = *argv.add(i);
            if !ptr.is_null() {
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                RAW_ARGS_STORAGE[i] = std::slice::from_raw_parts(ptr, len);
            }
        }
    }
    unsafe {
        RAW_ARGS_LEN = count;
    }
}

fn init_os_argv() -> &'static [&'static [u8]] {
    let args: Vec<&'static [u8]> = std::env::args_os()
        .map(|os_str| {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                let bytes = os_str.as_bytes();
                let leaked: &'static [u8] = Box::leak(bytes.to_vec().into_boxed_slice());
                leaked
            }
            #[cfg(not(unix))]
            {
                let s = os_str.to_string_lossy().into_owned();
                let leaked: &'static [u8] = Box::leak(s.into_bytes().into_boxed_slice());
                leaked
            }
        })
        .collect();
    Box::leak(args.into_boxed_slice())
}

impl OsIterator {
    pub fn init() -> Self {
        #[cfg(target_os = "linux")]
        {
            let count = unsafe { RAW_ARGS_LEN };
            if count > 1 {
                let slice = unsafe { &RAW_ARGS_STORAGE[1..count] };
                return Self { remain: slice };
            } else if count == 1 {
                return Self { remain: &[] };
            }
        }

        static ONCE: std::sync::OnceLock<&'static [&'static [u8]]> = std::sync::OnceLock::new();
        let argv = ONCE.get_or_init(init_os_argv);
        let mut iter = Self { remain: argv };
        let _ = iter.next(); // Skip argv[0] (executable path)
        iter
    }
}

impl ArgIter<'static> for OsIterator {
    #[inline]
    fn next(&mut self) -> Option<&'static [u8]> {
        pop_first(&mut self.remain)
    }

    #[inline]
    fn remain(&self) -> &[&'static [u8]] {
        self.remain
    }
}

