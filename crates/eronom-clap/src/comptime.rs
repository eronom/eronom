#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Values {
    None,
    One,
    Many,
    OneOptional,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Names {
    pub short: Option<u8>,
    pub long: Option<&'static [u8]>,
    pub long_aliases: &'static [&'static [u8]],
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Help {
    pub msg: &'static [u8],
    pub msg_plain: &'static [u8],
    pub value: &'static [u8],
}

#[derive(Copy, Clone, Debug)]
pub struct Param<Id> {
    pub id: Id,
    pub names: Names,
    pub takes_value: Values,
}

#[inline]
pub const fn is_named<Id>(p: &Param<Id>) -> bool {
    p.names.long.is_some() || p.names.short.is_some()
}

pub const fn const_bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[inline]
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

pub const fn count_flags<Id>(params: &[Param<Id>]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < params.len() {
        if is_named(&params[i]) && matches!(params[i].takes_value, Values::None) {
            n += 1;
        }
        i += 1;
    }
    n
}

pub const fn count_single<Id>(params: &[Param<Id>]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < params.len() {
        if is_named(&params[i])
            && matches!(params[i].takes_value, Values::One | Values::OneOptional)
        {
            n += 1;
        }
        i += 1;
    }
    n
}

pub const fn count_multi<Id>(params: &[Param<Id>]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < params.len() {
        if is_named(&params[i]) && matches!(params[i].takes_value, Values::Many) {
            n += 1;
        }
        i += 1;
    }
    n
}

pub const fn convert_params_array<Id, const N: usize>(params: &[Param<Id>]) -> [Param<usize>; N] {
    const DUMMY: Param<usize> = Param {
        id: 0,
        names: Names {
            short: None,
            long: None,
            long_aliases: &[],
        },
        takes_value: Values::None,
    };
    let mut out = [DUMMY; N];
    let mut flags = 0usize;
    let mut single = 0usize;
    let mut multi = 0usize;
    let mut i = 0;
    while i < params.len() {
        let p = &params[i];
        let mut index = 0usize;
        if is_named(p) {
            match p.takes_value {
                Values::None => {
                    index = flags;
                    flags += 1;
                }
                Values::One | Values::OneOptional => {
                    index = single;
                    single += 1;
                }
                Values::Many => {
                    index = multi;
                    multi += 1;
                }
            }
        }
        out[i] = Param {
            id: index,
            names: p.names,
            takes_value: p.takes_value,
        };
        i += 1;
    }
    out
}

pub const fn count_long_entries<Id>(params: &[Param<Id>]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < params.len() {
        if params[i].names.long.is_some() {
            n += 1;
        }
        n += params[i].names.long_aliases.len();
        i += 1;
    }
    n
}

#[derive(Copy, Clone, Debug)]
pub struct LongEntry {
    pub hash: u64,
    pub idx: u16,
}

pub const fn build_long_index<Id, const M: usize>(params: &[Param<Id>]) -> [LongEntry; M] {
    let mut out = [LongEntry { hash: 0, idx: 0 }; M];
    let mut w = 0;
    let mut i = 0;
    while i < params.len() {
        let n = &params[i].names;
        if let Some(l) = n.long {
            out[w] = LongEntry {
                hash: fnv1a64(l),
                idx: i as u16,
            };
            w += 1;
        }
        let mut a = 0;
        while a < n.long_aliases.len() {
            out[w] = LongEntry {
                hash: fnv1a64(n.long_aliases[a]),
                idx: i as u16,
            };
            w += 1;
            a += 1;
        }
        i += 1;
    }
    assert!(w == M);
    let mut j = 1;
    while j < M {
        let key = out[j];
        let mut k = j;
        while k > 0 && out[k - 1].hash > key.hash {
            out[k] = out[k - 1];
            k -= 1;
        }
        out[k] = key;
        j += 1;
    }
    out
}

pub const fn build_short_index(converted: &[Param<usize>]) -> [i16; 128] {
    let mut idx = [-1i16; 128];
    let mut i = 0;
    while i < converted.len() {
        if let Some(s) = converted[i].names.short {
            idx[(s & 0x7f) as usize] = i as i16;
        }
        i += 1;
    }
    idx
}

pub struct ConvertedTable {
    pub converted: &'static [Param<usize>],
    pub n_flags: usize,
    pub n_single: usize,
    pub n_multi: usize,
    pub long_index: &'static [LongEntry],
    pub short_index: [i16; 128],
}

impl ConvertedTable {
    pub const fn from_const(
        converted: &'static [Param<usize>],
        n_flags: usize,
        n_single: usize,
        n_multi: usize,
        long_index: &'static [LongEntry],
    ) -> Self {
        Self {
            converted,
            n_flags,
            n_single,
            n_multi,
            long_index,
            short_index: build_short_index(converted),
        }
    }

    #[inline]
    pub fn find_short(&self, short: u8) -> Option<&'static Param<usize>> {
        let i = self.short_index[(short & 0x7f) as usize];
        if i >= 0 {
            Some(&self.converted[i as usize])
        } else {
            None
        }
    }

    #[inline]
    pub fn find_long(&self, long: &[u8]) -> Option<&'static Param<usize>> {
        if self.long_index.is_empty() {
            return None;
        }
        let h = fnv1a64(long);
        let idx = self.long_index.partition_point(|e| e.hash < h);
        let mut j = idx;
        while j < self.long_index.len() && self.long_index[j].hash == h {
            let p = &self.converted[self.long_index[j].idx as usize];
            if p.names.long == Some(long) {
                return Some(p);
            }
            for alias in p.names.long_aliases {
                if *alias == long {
                    return Some(p);
                }
            }
            j += 1;
        }
        None
    }
}
