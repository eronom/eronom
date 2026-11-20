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

