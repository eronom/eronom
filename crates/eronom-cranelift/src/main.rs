use std::path::PathBuf;

const COMPILE_PARAMS: &[eronom_clap::Param<eronom_clap::Help>] = eronom_clap::concat_params!(eronom_clap::params!(
    "-o, --output <output>       Output binary path",
    "-t, --target <target>       Target platform (e.g. windows, macos, linux)",
    "-h, --help                  Print help",
));
static COMPILE_TABLE: &eronom_clap::ConvertedTable = eronom_clap::comptime_table!(COMPILE_PARAMS, cold);

const ROOT_PARAMS: &[eronom_clap::Param<eronom_clap::Help>] = eronom_clap::concat_params!(eronom_clap::params!(
    "-v, --version               Print toolchain version",
    "-h, --help                  Print help",
));

fn print_help() {
    let help = eronom_clap::format_help(
        "eronom-cranelift <COMMAND> [OPTIONS]",
        ROOT_PARAMS,
        &[
            ("compile <script> -o <out>", "Compile an Eronom script to native binary"),
            ("version", "Print toolchain version"),
        ],
    );
    print!("{}", help);
}

fn main() -> anyhow::Result<()> {
    let os_iter = eronom_clap::OsIterator::init();
    if os_iter.remain.is_empty() {
        print_help();
        std::process::exit(1);
    }

    let first = os_iter.remain[0];
    if first == b"-v" || first == b"--version" || first == b"version" {
        println!("eronom-cranelift v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if first == b"-h" || first == b"--help" {
        print_help();
        return Ok(());
    }

    if first == b"compile" {
        let iter = eronom_clap::SliceIterator::new(&os_iter.remain[1..]);
        let parsed = eronom_clap::parse_with_table(COMPILE_TABLE, iter)?;
        if parsed.flag("-h") || parsed.flag("--help") {
            let h = eronom_clap::format_help(
                "eronom-cranelift compile <SCRIPT.ER> -o <OUTPUT> [OPTIONS]",
                COMPILE_PARAMS,
                &[],
            );
            print!("{}", h);
            return Ok(());
        }

        let script = parsed
            .positionals_str()
            .first()
            .copied()
            .map(PathBuf::from)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Source script (.er) is required. Usage: eronom-cranelift compile <script.er> -o <output>"
                )
            })?;

        let output = parsed
            .option("--output")
            .or_else(|| parsed.option("-o"))
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Output binary path is required via -o or --output"))?;

        let target = parsed
            .option("--target")
            .or_else(|| parsed.option("-t"))
            .map(|s| s.to_string());

        eronom_cranelift::build_cranelift_binary(&script, &output, target.as_deref())?;
        return Ok(());
    }

    anyhow::bail!(
        "Unknown command: '{}'. Run 'eronom-cranelift --help' for usage.",
        String::from_utf8_lossy(first)
    );
}
