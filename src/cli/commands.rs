use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Cli {
    /// The script file to run (e.g. main.er)
    pub file: Option<PathBuf>,
    pub command: Option<Commands>,
}

impl Cli {
    pub fn parse() -> Self {
        let os_iter = eronom_clap::OsIterator::init();
        Self::parse_from_slice(os_iter.remain)
    }

    pub fn try_parse_from<I, T>(itr: I) -> anyhow::Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        let strings: Vec<String> = itr.into_iter().map(Into::into).collect();
        let bytes: Vec<&'static [u8]> = strings
            .into_iter()
            .map(|s| Box::leak(s.into_bytes().into_boxed_slice()) as &'static [u8])
            .collect();
        let slice: &'static [&'static [u8]] = Box::leak(bytes.into_boxed_slice());
        let remain = if !slice.is_empty()
            && (slice[0].ends_with(b"eronom") || slice[0].ends_with(b"eronom.exe"))
        {
            &slice[1..]
        } else {
            slice
        };
        Ok(Self::parse_from_slice(remain))
    }

    pub fn parse_from_slice(args: &[&[u8]]) -> Self {
        if args.is_empty() {
            return Self {
                file: None,
                command: None,
            };
        }
        let first_str = std::str::from_utf8(args[0]).unwrap_or("");
        let is_subcommand = matches!(
            first_str,
            "init" | "build" | "start" | "dev" | "test" | "check" | "transpile" | "toolchain"
        );
        if is_subcommand {
            match parse_command(first_str, &args[1..]) {
                Ok(cmd) => Self {
                    file: None,
                    command: Some(cmd),
                },
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        } else if !first_str.starts_with('-') {
            Self {
                file: Some(PathBuf::from(first_str)),
                command: None,
            }
        } else {
            Self {
                file: None,
                command: None,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Ssr,
    Ssg,
    Ppr,
}

#[derive(Debug, Clone)]
pub enum Commands {
    /// Initialize a fresh Eronom project
    Init {
        dir: String,
        template: Option<String>,
        branch: Option<String>,
        force: bool,
        git: bool,
        no_commit: bool,
        offline: bool,
    },
    /// Build the project or compile into a standalone binary
    Build {
        dir: String,
        target: Option<String>,
        output: Option<String>,
        runner_stub: Option<String>,
        ssr: bool,
        ssg: bool,
        ppr: bool,
        aot: bool,
        in_process: bool,
        cranelift: bool,
        mir: bool,
    },
    /// Start the server in production mode
    Start {
        dir_or_port: Option<String>,
        port_pos: Option<u16>,
        port: Option<u16>,
    },
    /// Start the server in development mode
    Dev {
        dir_or_port: Option<String>,
        port_pos: Option<u16>,
        port: Option<u16>,
    },
    /// Run Eronom test files (e.g. `eronom test` or `eronom test test_runner.er`)
    Test {
        file: Option<PathBuf>,
    },
    /// Type-check an Eronom script or project without running it (like tsc --noEmit)
    Check {
        file: PathBuf,
        json: bool,
    },
    /// Transpile an Eronom (.er) script to JavaScript (.js)
    Transpile {
        file: PathBuf,
        out: Option<PathBuf>,
        check: bool,
    },
    /// Manage optional Eronom toolchains (e.g. cranelift)
    Toolchain {
        action: ToolchainAction,
    },
}

#[derive(Debug, Clone)]
pub enum ToolchainAction {
    List,
    Add { name: String },
    Remove { name: String },
}

// ─────────────────────────────────────────────────────────────────────────────
// Subcommand Compile-Time Parameter Tables
// ─────────────────────────────────────────────────────────────────────────────

pub const INIT_PARAMS: &[eronom_clap::Param<eronom_clap::Help>] = eronom_clap::concat_params!(eronom_clap::params!(
    "-t, --template <template>   The template to start from",
    "-b, --branch <branch>       Branch argument for template",
    "--force                     Create project even if directory is not empty",
    "--git                       Initialize a git repository",
    "--no-commit                 Do not create an initial commit",
    "--offline                   Initialize using local template files without cloning",
    "-h, --help                  Print help",
));
pub static INIT_TABLE: &eronom_clap::ConvertedTable = eronom_clap::comptime_table!(INIT_PARAMS, cold);

pub const BUILD_PARAMS: &[eronom_clap::Param<eronom_clap::Help>] = eronom_clap::concat_params!(eronom_clap::params!(
    "-t, --target <target>       Target platform: host, windows, linux, macos",
    "-o, --output <output>       Output executable path",
    "--runner-stub <path>        Custom runner stub binary path",
    "--ssr                       Build for Server-Side Rendering (SSR)",
    "--ssg                       Build for Static Site Generation (SSG)",
    "--ppr                       Build for Partial Prerendering (PPR)",
    "--aot                       Compile ahead-of-time (AOT) to standalone native binary",
    "--in-process/--no-cc        Build standalone binary in-process without external cc",
    "--cranelift                 Use pure-Rust Cranelift backend for native AOT codegen",
    "--mir                       [Deprecated] Legacy alias for native code generation",
    "-h, --help                  Print help",
));
pub static BUILD_TABLE: &eronom_clap::ConvertedTable = eronom_clap::comptime_table!(BUILD_PARAMS, cold);

pub const SERVER_PARAMS: &[eronom_clap::Param<eronom_clap::Help>] = eronom_clap::concat_params!(eronom_clap::params!(
    "-p, --port <port>           Port number to bind",
    "-h, --help                  Print help",
));
pub static SERVER_TABLE: &eronom_clap::ConvertedTable = eronom_clap::comptime_table!(SERVER_PARAMS, cold);

pub const TEST_PARAMS: &[eronom_clap::Param<eronom_clap::Help>] = eronom_clap::concat_params!(eronom_clap::params!(
    "-h, --help                  Print help",
));
pub static TEST_TABLE: &eronom_clap::ConvertedTable = eronom_clap::comptime_table!(TEST_PARAMS, cold);

pub const CHECK_PARAMS: &[eronom_clap::Param<eronom_clap::Help>] = eronom_clap::concat_params!(eronom_clap::params!(
    "--json                      Output diagnostics as machine-readable JSON",
    "-h, --help                  Print help",
));
pub static CHECK_TABLE: &eronom_clap::ConvertedTable = eronom_clap::comptime_table!(CHECK_PARAMS, cold);

pub const TRANSPILE_PARAMS: &[eronom_clap::Param<eronom_clap::Help>] = eronom_clap::concat_params!(eronom_clap::params!(
    "-o, --out <out>             Output .js file path",
    "--check                     Type check before transpiling",
    "--no-check                  Disable type checking before transpiling",
    "-h, --help                  Print help",
));
pub static TRANSPILE_TABLE: &eronom_clap::ConvertedTable = eronom_clap::comptime_table!(TRANSPILE_PARAMS, cold);

pub fn parse_command(subcommand: &str, args: &[&[u8]]) -> anyhow::Result<Commands> {
    match subcommand {
        "init" => {
            let iter = eronom_clap::SliceIterator::new(args);
            let parsed = eronom_clap::parse_with_table(INIT_TABLE, iter)?;
            if parsed.flag("-h") || parsed.flag("--help") {
                let h = eronom_clap::format_help("eronom init [DIR] [OPTIONS]", INIT_PARAMS, &[]);
                print!("{}", h);
                std::process::exit(0);
            }
            let dir = parsed
                .positionals_str()
                .first()
                .copied()
                .unwrap_or(".")
                .to_string();
            let template = parsed
                .option("--template")
                .or_else(|| parsed.option("-t"))
                .map(|s| s.to_string());
            let branch = parsed
                .option("--branch")
                .or_else(|| parsed.option("-b"))
                .map(|s| s.to_string());
            let force = parsed.flag("--force");
            let git = parsed.flag("--git");
            let no_commit = parsed.flag("--no-commit");
            let offline = parsed.flag("--offline");
            Ok(Commands::Init {
                dir,
                template,
                branch,
                force,
                git,
                no_commit,
                offline,
            })
        }
        "build" => {
            let iter = eronom_clap::SliceIterator::new(args);
            let parsed = eronom_clap::parse_with_table(BUILD_TABLE, iter)?;
            if parsed.flag("-h") || parsed.flag("--help") {
                let h = eronom_clap::format_help("eronom build [DIR] [OPTIONS]", BUILD_PARAMS, &[]);
                print!("{}", h);
                std::process::exit(0);
            }
            let dir = parsed
                .positionals_str()
                .first()
                .copied()
                .unwrap_or(".")
                .to_string();
            let target = parsed
                .option("--target")
                .or_else(|| parsed.option("-t"))
                .map(|s| s.to_string());
            let output = parsed
                .option("--output")
                .or_else(|| parsed.option("-o"))
                .map(|s| s.to_string());
            let runner_stub = parsed.option("--runner-stub").map(|s| s.to_string());
            let ssr = parsed.flag("--ssr");
            let ssg = parsed.flag("--ssg");
            let ppr = parsed.flag("--ppr");
            let aot = parsed.flag("--aot");
            let in_process = parsed.flag("--in-process") || parsed.flag("--no-cc");
            let cranelift = parsed.flag("--cranelift");
            let mir = parsed.flag("--mir");
            Ok(Commands::Build {
                dir,
                target,
                output,
                runner_stub,
                ssr,
                ssg,
                ppr,
                aot,
                in_process,
                cranelift,
                mir,
            })
        }
        "start" => {
            let iter = eronom_clap::SliceIterator::new(args);
            let parsed = eronom_clap::parse_with_table(SERVER_TABLE, iter)?;
            if parsed.flag("-h") || parsed.flag("--help") {
                let h = eronom_clap::format_help(
                    "eronom start [DIR_OR_PORT] [PORT] [OPTIONS]",
                    SERVER_PARAMS,
                    &[],
                );
                print!("{}", h);
                std::process::exit(0);
            }
            let pos = parsed.positionals_str();
            let dir_or_port = pos.first().copied().map(|s| s.to_string());
            let port_pos = pos.get(1).and_then(|s| s.parse::<u16>().ok());
            let port = parsed
                .option("--port")
                .or_else(|| parsed.option("-p"))
                .and_then(|s| s.parse::<u16>().ok());
            Ok(Commands::Start {
                dir_or_port,
                port_pos,
                port,
            })
        }
        "dev" => {
            let iter = eronom_clap::SliceIterator::new(args);
            let parsed = eronom_clap::parse_with_table(SERVER_TABLE, iter)?;
            if parsed.flag("-h") || parsed.flag("--help") {
                let h = eronom_clap::format_help(
                    "eronom dev [DIR_OR_PORT] [PORT] [OPTIONS]",
                    SERVER_PARAMS,
                    &[],
                );
                print!("{}", h);
                std::process::exit(0);
            }
            let pos = parsed.positionals_str();
            let dir_or_port = pos.first().copied().map(|s| s.to_string());
            let port_pos = pos.get(1).and_then(|s| s.parse::<u16>().ok());
            let port = parsed
                .option("--port")
                .or_else(|| parsed.option("-p"))
                .and_then(|s| s.parse::<u16>().ok());
            Ok(Commands::Dev {
                dir_or_port,
                port_pos,
                port,
            })
        }
        "test" => {
            let iter = eronom_clap::SliceIterator::new(args);
            let parsed = eronom_clap::parse_with_table(TEST_TABLE, iter)?;
            if parsed.flag("-h") || parsed.flag("--help") {
                let h = eronom_clap::format_help("eronom test [FILE] [OPTIONS]", TEST_PARAMS, &[]);
                print!("{}", h);
                std::process::exit(0);
            }
            let file = parsed.positionals_str().first().copied().map(PathBuf::from);
            Ok(Commands::Test { file })
        }
        "check" => {
            let iter = eronom_clap::SliceIterator::new(args);
            let parsed = eronom_clap::parse_with_table(CHECK_TABLE, iter)?;
            if parsed.flag("-h") || parsed.flag("--help") {
                let h = eronom_clap::format_help("eronom check <FILE> [OPTIONS]", CHECK_PARAMS, &[]);
                print!("{}", h);
                std::process::exit(0);
            }
            let file = parsed
                .positionals_str()
                .first()
                .copied()
                .map(PathBuf::from)
                .ok_or_else(|| {
                    anyhow::anyhow!("File to type-check is required. Usage: eronom check <file.er>")
                })?;
            let json = parsed.flag("--json");
            Ok(Commands::Check { file, json })
        }
        "transpile" => {
            let iter = eronom_clap::SliceIterator::new(args);
            let parsed = eronom_clap::parse_with_table(TRANSPILE_TABLE, iter)?;
            if parsed.flag("-h") || parsed.flag("--help") {
                let h = eronom_clap::format_help(
                    "eronom transpile <FILE> [OPTIONS]",
                    TRANSPILE_PARAMS,
                    &[],
                );
                print!("{}", h);
                std::process::exit(0);
            }
            let file = parsed
                .positionals_str()
                .first()
                .copied()
                .map(PathBuf::from)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Input .er file to transpile is required. Usage: eronom transpile <file.er>"
                    )
                })?;
            let out = parsed
                .option("--out")
                .or_else(|| parsed.option("-o"))
                .map(PathBuf::from);
            let check = !parsed.flag("--no-check");
            Ok(Commands::Transpile { file, out, check })
        }
        "toolchain" => {
            if args.is_empty() || args[0] == b"list" {
                Ok(Commands::Toolchain {
                    action: ToolchainAction::List,
                })
            } else if args[0] == b"add" {
                let name = args
                    .get(1)
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_default();
                Ok(Commands::Toolchain {
                    action: ToolchainAction::Add { name },
                })
            } else if args[0] == b"remove" {
                let name = args
                    .get(1)
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_default();
                Ok(Commands::Toolchain {
                    action: ToolchainAction::Remove { name },
                })
            } else {
                anyhow::bail!("Unknown toolchain action. Available: list, add <name>, remove <name>");
            }
        }
        other => anyhow::bail!(
            "Unknown command: '{}'. Run 'eronom --help' for available commands.",
            other
        ),
    }
}

pub fn parse_dir_and_port(
    dir_or_port: Option<String>,
    port_pos: Option<u16>,
    port_flag: Option<u16>,
    default_dir: &str,
) -> anyhow::Result<(String, Option<u16>)> {
    let mut dir = default_dir.to_string();
    let mut port_val = port_flag;

    if let Some(arg) = dir_or_port {
        if let Ok(p) = arg.parse::<u16>() {
            if port_val.is_none() {
                port_val = Some(p);
            }
        } else {
            dir = arg;
            if port_val.is_none() {
                port_val = port_pos;
            }
        }
    }
    Ok((dir, port_val))
}

pub fn resolve_port(
    dir: &str,
    port_val: Option<u16>,
    is_build_or_init: bool,
) -> anyhow::Result<u16> {
    if let Some(p) = port_val {
        Ok(p)
    } else if let Ok(port_str) = std::env::var("PORT") {
        if let Ok(p) = port_str.parse::<u16>() {
            Ok(p)
        } else {
            anyhow::bail!("Invalid port in PORT environment variable: '{}'", port_str);
        }
    } else if let Some(config_port) = get_port_from_config_file(dir) {
        Ok(config_port)
    } else if is_build_or_init {
        Ok(0)
    } else {
        anyhow::bail!(
            "No port specified. Please provide a port in eronom.toml or PORT environment variable."
        );
    }
}

pub fn get_port_from_config_file(dir: &str) -> Option<u16> {
    let path = Path::new(dir);
    let mut current = if path.is_file() {
        path.parent()
    } else {
        Some(path)
    };

    let mut found_toml = None;

    while let Some(p) = current {
        let check_toml = p.join("eronom.toml");
        if check_toml.exists() {
            found_toml = Some(check_toml);
            break;
        }
        current = p.parent();
    }

    if let Some(toml_path) = found_toml {
        if let Ok(content) = fs::read_to_string(toml_path) {
            if let Some(port_i64) = eronom_toml::lookup_integer(&content, &["server", "port"]) {
                return Some(port_i64 as u16);
            }
        }
    }

    None
}
