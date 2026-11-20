pub mod args;
pub mod comptime;
pub mod error;

pub use args::{ArgIter, OsIterator, SliceIterator};
pub use comptime::{ConvertedTable, Help, LongEntry, Names, Param, Values};
pub use error::{Error, Result};

#[doc(hidden)]
pub use eronom_clap_macros::{__parse_param_impl, __parse_params_impl};

#[macro_export]
macro_rules! parse_param {
    ($lit:literal $(,)?) => {
        $crate::__parse_param_impl!($crate, $lit)
    };
}

#[macro_export]
macro_rules! param {
    ($lit:literal $(,)?) => {
        $crate::__parse_param_impl!($crate, $lit)
    };
}

#[macro_export]
macro_rules! params {
    ($($lit:literal),* $(,)?) => {
        &[
            $( $crate::__parse_param_impl!($crate, $lit) ),*
        ]
    };
}

#[macro_export]
macro_rules! concat_params {
    ($($part:expr),* $(,)?) => {{
        const __PARTS: &[&[$crate::Param<$crate::Help>]] = &[$($part),*];
        const __ARR: [$crate::Param<$crate::Help>; $crate::__param_slices_len(__PARTS)] =
            $crate::__param_slices_concat::<{ $crate::__param_slices_len(__PARTS) }>(__PARTS);
        &__ARR
    }};
}

#[macro_export]
macro_rules! comptime_table {
    ($params:expr) => {
        $crate::comptime_table!(
            @build
            { #[cfg_attr(any(target_os = "linux", target_os = "android"), unsafe(link_section = ".rodata.startup"))] }
            $params
        )
    };
    ($params:expr, cold) => {
        $crate::comptime_table!(@build { } $params)
    };
    (@build { $(#[$attr:meta])* } $params:expr) => {{
        const __P: &[$crate::Param<$crate::Help>] = $params;
        const __N: usize = __P.len();
        $(#[$attr])*
        static __CONV: [$crate::Param<usize>; __N] =
            $crate::comptime::convert_params_array::<$crate::Help, __N>(__P);
        const __M: usize = $crate::comptime::count_long_entries(__P);
        $(#[$attr])*
        static __LONG: [$crate::comptime::LongEntry; __M] =
            $crate::comptime::build_long_index::<$crate::Help, __M>(__P);
        $(#[$attr])*
        static __TABLE: $crate::ConvertedTable = $crate::ConvertedTable::from_const(
            &__CONV,
            $crate::comptime::count_flags(__P),
            $crate::comptime::count_single(__P),
            $crate::comptime::count_multi(__P),
            &__LONG,
        );
        &__TABLE
    }};
}

#[doc(hidden)]
pub const fn __param_slices_len(parts: &[&[Param<Help>]]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < parts.len() {
        n += parts[i].len();
        i += 1;
    }
    n
}

#[doc(hidden)]
pub const fn __param_slices_concat<const N: usize>(parts: &[&[Param<Help>]]) -> [Param<Help>; N] {
    const DUMMY: Param<Help> = Param {
        id: Help {
            msg: b"",
            msg_plain: b"",
            value: b"",
        },
        names: Names {
            short: None,
            long: None,
            long_aliases: &[],
        },
        takes_value: Values::None,
    };
    let mut arr = [DUMMY; N];
    let mut out_i = 0;
    let mut p_i = 0;
    while p_i < parts.len() {
        let part = parts[p_i];
        let mut in_i = 0;
        while in_i < part.len() {
            arr[out_i] = part[in_i];
            out_i += 1;
            in_i += 1;
        }
        p_i += 1;
    }
    arr
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PARAMS: &[Param<Help>] = concat_params!(params!(
        "-v, --version      Print version",
        "-h, --help         Print help",
        "-p, --port <port>  Specify port",
    ));

    static TEST_TABLE: &ConvertedTable = comptime_table!(TEST_PARAMS);

    #[test]
    fn test_table_generation() {
        assert_eq!(TEST_TABLE.n_flags, 2);
        assert_eq!(TEST_TABLE.n_single, 1);
        assert_eq!(TEST_TABLE.n_multi, 0);

        let p_ver = TEST_TABLE.find_short(b'v').unwrap();
        assert_eq!(p_ver.names.short, Some(b'v'));
        assert_eq!(p_ver.names.long, Some(b"version" as &'static [u8]));

        let p_port = TEST_TABLE.find_long(b"port").unwrap();
        assert_eq!(p_port.names.short, Some(b'p'));
        assert_eq!(p_port.takes_value, Values::One);
    }
}
