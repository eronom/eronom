pub mod commands;
pub mod spinner;
pub mod init;
pub mod build;

use std::path::Path;
use crate::server::start_server;
use clap::Parser;

pub use commands::{Cli, Commands, BuildMode};
pub use init::init_project;
pub use build::build_project;

pub fn run_cli(args: Vec<String>) -> anyhow::Result<()> {
    let cli = Cli::try_parse_from(args)?;
    if let Some(cmd) = cli.command {
        run_command(cmd)
    } else if let Some(file_path) = cli.file {
        anyhow::bail!("Running files is handled via the main entrypoint: {}", file_path.display())
    } else {
        anyhow::bail!("No command or file specified")
    }
}

pub fn run_command(cmd: Commands) -> anyhow::Result<()> {
    match cmd {
        Commands::Init {
            dir,
            template,
            branch,
            force,
            git,
            no_commit,
            offline,
        } => {
            init_project(&dir, template, branch, force, git, no_commit, offline)?;
        }
        Commands::Build {
            mut dir,
            target,
            output,
            runner_stub: _,
            ssr: _,
            ssg,
            ppr,
            aot,
        } => {
            if dir.starts_with("target=") || dir.starts_with("target:") {
                let _val = dir.split_once('=').or_else(|| dir.split_once(':')).unwrap().1;
                dir = ".".to_string();
            }

            let path = Path::new(&dir);
            let is_script = dir.ends_with(".er") || path.is_file();
            let wants_binary = aot || output.is_some() || target.is_some() || is_script;

            if is_script {
                let out_path = output.clone().map(std::path::PathBuf::from).unwrap_or_else(|| {
                    let stem = path.file_stem().unwrap_or_default();
                    std::path::PathBuf::from(stem)
                });
                println!("Compiling {} into native standalone binary: {}", path.display(), out_path.display());
                crate::jit::aot::build_aot_binary(path, &out_path)?;
                println!("✓ Successfully built native executable: {}", out_path.display());
                return Ok(());
            }

            let mode = if ppr {
                BuildMode::Ppr
            } else if ssg {
                BuildMode::Ssg
            } else {
                BuildMode::Ssr
            };

            // Build web assets / templates
            build_project(&dir, mode)?;

            // If user requested a binary output from the project directory, compile the entrypoint
            if wants_binary {
                let build_server = path.join("build").join("server.er");
                let direct_server = path.join("server.er");
                let script_to_compile = if build_server.exists() {
                    build_server
                } else if direct_server.exists() {
                    direct_server
                } else {
                    path.join("main.er")
                };

                let out_path = output.clone().map(std::path::PathBuf::from).unwrap_or_else(|| {
                    let stem = path.file_name().unwrap_or_default();
                    if stem == "." || stem.is_empty() {
                        std::path::PathBuf::from("server_bin")
                    } else {
                        std::path::PathBuf::from(stem)
                    }
                });

                println!("Compiling {} into native standalone binary: {}", script_to_compile.display(), out_path.display());
                crate::jit::aot::build_aot_binary(&script_to_compile, &out_path)?;
                println!("✓ Successfully built native executable: {}", out_path.display());
            }
        }
        Commands::Start {
            dir_or_port,
            port_pos,
            port,
        } => {
            let (mut dir, port_val) = commands::parse_dir_and_port(dir_or_port, port_pos, port, "build")?;
            if Path::new(&dir).join("build").exists() {
                dir = Path::new(&dir).join("build").to_string_lossy().to_string();
            } else if dir == "build" && !Path::new("build").exists() && Path::new("app/build").exists() {
                dir = "app/build".to_string();
            }
            let resolved_port = commands::resolve_port(&dir, port_val, false)?;
            start_server(&dir, true, resolved_port)?;
        }
        Commands::Dev {
            dir_or_port,
            port_pos,
            port,
        } => {
            let (dir, port_val) = commands::parse_dir_and_port(dir_or_port, port_pos, port, ".")?;
            let resolved_port = commands::resolve_port(&dir, port_val, false)?;
            start_server(&dir, false, resolved_port)?;
        }
        Commands::Test { file: _ } => {
            // Handled via the main binary entrypoint
        }
        Commands::Check { file: _ } => {
            // Handled via the main binary entrypoint
        }
        Commands::Transpile { .. } => {
            // Handled via the main binary entrypoint
        }
    }
    Ok(())
}
