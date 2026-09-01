use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "eronom", version = "0.1.0")]
#[command(about = "Eronom: A web framework and runtime", long_about = None)]
pub struct Cli {
    /// The script file to run (e.g. main.er)
    pub file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Ssr,
    Ssg,
    Ppr,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Initialize a fresh Eronom project
    Init {
        /// The root directory of the new project
        #[arg(default_value = ".")]
        dir: String,

        /// The template to start from
        #[arg(long, short)]
        template: Option<String>,

        /// Branch argument that can only be used with template option
        #[arg(long, short, requires = "template")]
        branch: Option<String>,

        /// Create the project even if the specified directory is not empty
        #[arg(long)]
        force: bool,

        /// Initialize a git repository
        #[arg(long)]
        git: bool,

        /// Do not create an initial commit
        #[arg(long, requires = "git")]
        no_commit: bool,

        /// Add ermcss capability for the project
        #[arg(long)]
        ermcss: bool,
    },
    /// Build the project or compile into a standalone binary
    Build {
        /// Source directory or script file (e.g. . or app or main.er)
        #[arg(default_value = ".")]
        dir: String,

        /// Target platform: host, windows, linux, macos
        #[arg(long, short = 't')]
        target: Option<String>,

        /// Output executable path (e.g. dist/my-app or dist/my-app.exe)
        #[arg(long, short = 'o')]
        output: Option<String>,

        /// Custom runner stub binary path for cross-compilation
        #[arg(long)]
        runner_stub: Option<String>,

        /// Build for Server-Side Rendering (SSR) (default)
        #[arg(long, conflicts_with = "ssg", conflicts_with = "ppr")]
        ssr: bool,

        /// Build for Static Site Generation (SSG)
        #[arg(long, conflicts_with = "ppr")]
        ssg: bool,

        /// Build for Partial Prerendering (PPR)
        #[arg(long)]
        ppr: bool,
    },
    /// Start the server in production mode
    Start {
        /// Build directory or port
        dir_or_port: Option<String>,

        /// Port number if directory was also specified
        port_pos: Option<u16>,

        /// Port number
        #[arg(short, long = "port")]
        port: Option<u16>,
    },
    /// Start the server in development mode
    Dev {
        /// Source directory or port
        dir_or_port: Option<String>,

        /// Port number if directory was also specified
        port_pos: Option<u16>,

        /// Port number
        #[arg(short, long = "port")]
        port: Option<u16>,
    },
    /// Run Eronom test files (e.g. `eronom test` or `eronom test test_runner.er`)
    Test {
        /// Test file or directory to run (e.g. test_runner.er, tests/)
        file: Option<PathBuf>,
    },
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

pub fn resolve_port(dir: &str, port_val: Option<u16>, is_build_or_init: bool) -> anyhow::Result<u16> {
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
        anyhow::bail!("No port specified. Please provide a port in eronom.toml or PORT environment variable.");
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
            if let Ok(toml_val) = toml::from_str::<toml::Value>(&content) {
                if let Some(server) = toml_val.get("server") {
                    if let Some(port) = server.get("port") {
                        if let Some(port_u64) = port.as_integer() {
                            return Some(port_u64 as u16);
                        }
                    }
                }
            }
        }
    }

    None
}
