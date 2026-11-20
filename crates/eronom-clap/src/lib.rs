pub mod args;
pub mod comptime;
pub mod error;
pub mod streaming;

pub use args::{ArgIter, OsIterator, SliceIterator};
pub use comptime::{ConvertedTable, Help, LongEntry, Names, Param, Values};
pub use error::{Error, Result};
pub use streaming::{ArgItem, StreamingClap};

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

#[derive(Debug, Clone)]
pub struct ParsedArgs<'a> {
    pub table: &'static ConvertedTable,
    pub flags: Vec<bool>,
    pub singles: Vec<Option<&'a [u8]>>,
    pub multis: Vec<Vec<&'a [u8]>>,
    pub positionals: Vec<&'a [u8]>,
}

impl<'a> ParsedArgs<'a> {
    pub fn flag_by_short(&self, short: u8) -> bool {
        if let Some(p) = self.table.find_short(short) {
            if p.takes_value == Values::None {
                return self.flags[p.id];
            }
        }
        false
    }

    pub fn flag_by_long(&self, long: &[u8]) -> bool {
        if let Some(p) = self.table.find_long(long) {
            if p.takes_value == Values::None {
                return self.flags[p.id];
            }
        }
        false
    }

    pub fn option_by_short(&self, short: u8) -> Option<&'a str> {
        if let Some(p) = self.table.find_short(short) {
            if matches!(p.takes_value, Values::One | Values::OneOptional) {
                return self.singles[p.id].and_then(|b| std::str::from_utf8(b).ok());
            }
        }
        None
    }

    pub fn option_by_long(&self, long: &[u8]) -> Option<&'a str> {
        if let Some(p) = self.table.find_long(long) {
            if matches!(p.takes_value, Values::One | Values::OneOptional) {
                return self.singles[p.id].and_then(|b| std::str::from_utf8(b).ok());
            }
        }
        None
    }

    pub fn options_multi_by_long(&self, long: &[u8]) -> Vec<&'a str> {
        if let Some(p) = self.table.find_long(long) {
            if p.takes_value == Values::Many {
                return self.multis[p.id]
                    .iter()
                    .filter_map(|b| std::str::from_utf8(b).ok())
                    .collect();
            }
        }
        Vec::new()
    }

    pub fn flag(&self, name: &str) -> bool {
        let bytes = name.as_bytes();
        if bytes.starts_with(b"--") {
            self.flag_by_long(&bytes[2..])
        } else if bytes.starts_with(b"-") && bytes.len() == 2 {
            self.flag_by_short(bytes[1])
        } else {
            self.flag_by_long(bytes)
        }
    }

    pub fn option(&self, name: &str) -> Option<&'a str> {
        let bytes = name.as_bytes();
        if bytes.starts_with(b"--") {
            self.option_by_long(&bytes[2..])
        } else if bytes.starts_with(b"-") && bytes.len() == 2 {
            self.option_by_short(bytes[1])
        } else {
            self.option_by_long(bytes)
        }
    }

    pub fn positionals(&self) -> &[&'a [u8]] {
        &self.positionals
    }

    pub fn positionals_str(&self) -> Vec<&'a str> {
        self.positionals
            .iter()
            .filter_map(|b| std::str::from_utf8(b).ok())
            .collect()
    }
}

pub fn parse_with_table<'p, 'a, I: ArgIter<'a>>(
    table: &'static ConvertedTable,
    iter: I,
) -> Result<ParsedArgs<'a>> {
    let mut streaming = StreamingClap::new(table, iter);
    let mut flags = vec![false; table.n_flags];
    let mut singles = vec![None; table.n_single];
    let mut multis = vec![Vec::new(); table.n_multi];
    let mut positionals = Vec::new();

    while let Some(item) = streaming.next()? {
        match item {
            ArgItem::Option { param, value } => match param.takes_value {
                Values::None => {
                    flags[param.id] = true;
                }
                Values::One | Values::OneOptional => {
                    singles[param.id] = value;
                }
                Values::Many => {
                    if let Some(v) = value {
                        multis[param.id].push(v);
                    }
                }
            },
            ArgItem::Positional(p) => {
                positionals.push(p);
            }
        }
    }

    Ok(ParsedArgs {
        table,
        flags,
        singles,
        multis,
        positionals,
    })
}

pub fn format_help(usage: &str, params: &[Param<Help>], commands: &[(&str, &str)]) -> String {
    let mut out = String::new();
    out.push_str("Usage: ");
    out.push_str(usage);
    out.push_str("\n\n");

    if !commands.is_empty() {
        out.push_str("Commands:\n");
        let max_cmd_len = commands.iter().map(|(c, _)| c.len()).max().unwrap_or(12);
        for (cmd, desc) in commands {
            out.push_str("  ");
            out.push_str(cmd);
            let spaces = max_cmd_len.saturating_sub(cmd.len()) + 4;
            for _ in 0..spaces {
                out.push(' ');
            }
            out.push_str(desc);
            out.push('\n');
        }
        out.push('\n');
    }

    if !params.is_empty() {
        out.push_str("Options:\n");
        let mut left_cols = Vec::new();
        for p in params {
            let mut left = String::new();
            if let Some(s) = p.names.short {
                left.push('-');
                left.push(s as char);
                if p.names.long.is_some() {
                    left.push_str(", ");
                }
            } else {
                left.push_str("    ");
            }
            if let Some(l) = p.names.long {
                left.push_str("--");
                left.push_str(std::str::from_utf8(l).unwrap_or(""));
            }
            if !p.id.value.is_empty() {
                left.push(' ');
                left.push('<');
                left.push_str(std::str::from_utf8(p.id.value).unwrap_or(""));
                left.push('>');
            }
            left_cols.push(left);
        }

        let max_left_len = left_cols.iter().map(|s| s.len()).max().unwrap_or(20);
        for (i, p) in params.iter().enumerate() {
            out.push_str("  ");
            let left = &left_cols[i];
            out.push_str(left);
            let spaces = max_left_len.saturating_sub(left.len()) + 4;
            for _ in 0..spaces {
                out.push(' ');
            }
            out.push_str(std::str::from_utf8(p.id.msg_plain).unwrap_or(""));
            out.push('\n');
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PARAMS: &[Param<Help>] = concat_params!(params!(
        "-v, --version           Print version",
        "-h, --help              Print help",
        "-p, --port <port>       Specify port",
        "-t, --target <target>   Build target",
        "-f, --force             Force overwrite",
        "-d, --define <def>...   Definitions",
    ));

    static TEST_TABLE: &ConvertedTable = comptime_table!(TEST_PARAMS);

    #[test]
    fn test_table_generation() {
        assert_eq!(TEST_TABLE.n_flags, 3);
        assert_eq!(TEST_TABLE.n_single, 2);
        assert_eq!(TEST_TABLE.n_multi, 1);

        let p_ver = TEST_TABLE.find_short(b'v').unwrap();
        assert_eq!(p_ver.names.short, Some(b'v'));
        assert_eq!(p_ver.names.long, Some(b"version" as &'static [u8]));

        let p_port = TEST_TABLE.find_long(b"port").unwrap();
        assert_eq!(p_port.names.short, Some(b'p'));
        assert_eq!(p_port.takes_value, Values::One);
    }

    #[test]
    fn test_streaming_parse() {
        let args = [
            b"-vf".as_slice(),
            b"--port".as_slice(),
            b"8080".as_slice(),
            b"--target=linux".as_slice(),
            b"-d".as_slice(),
            b"A=1".as_slice(),
            b"-d".as_slice(),
            b"B=2".as_slice(),
            b"main.er".as_slice(),
        ];
        let iter = SliceIterator::new(&args);
        let parsed = parse_with_table(TEST_TABLE, iter).expect("parse failed");

        assert!(parsed.flag("-v"));
        assert!(parsed.flag("--version"));
        assert!(parsed.flag("-f"));
        assert!(parsed.flag("--force"));
        assert!(!parsed.flag("-h"));

        assert_eq!(parsed.option("-p"), Some("8080"));
        assert_eq!(parsed.option("--port"), Some("8080"));
        assert_eq!(parsed.option("--target"), Some("linux"));

        let defs = parsed.options_multi_by_long(b"define");
        assert_eq!(defs, vec!["A=1", "B=2"]);

        assert_eq!(parsed.positionals_str(), vec!["main.er"]);
    }

    #[test]
    fn test_help_formatting() {
        let help = format_help(
            "eronom [OPTIONS] <FILE>",
            TEST_PARAMS,
            &[("init", "Initialize a new project"), ("build", "Build project")],
        );
        assert!(help.contains("Usage: eronom [OPTIONS] <FILE>"));
        assert!(help.contains("Commands:"));
        assert!(help.contains("init"));
        assert!(help.contains("-v, --version"));
    }
}
