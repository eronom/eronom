pub mod commands;
pub mod spinner;
pub mod init;
pub mod build;
pub mod standalone;

use std::path::Path;
use crate::server::start_server;
use clap::Parser;

pub use commands::{Cli, Commands, BuildMode};
pub use init::init_project;
pub use build::build_project;
pub use standalone::build_standalone;

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
            ermcss,
        } => {
            init_project(&dir, template, branch, force, git, no_commit, ermcss)?;
        }
        Commands::Build {
            mut dir,
            mut target,
            output,
            runner_stub,
            ssr: _,
            ssg,
            ppr,
        } => {
            if dir.starts_with("target=") || dir.starts_with("target:") {
                let val = dir.split_once('=').or_else(|| dir.split_once(':')).unwrap().1;
                target = Some(val.trim_matches('"').trim_matches('\'').to_string());
                dir = ".".to_string();
            }

            let mode = if ppr {
                BuildMode::Ppr
            } else if ssg {
                BuildMode::Ssg
            } else {
                BuildMode::Ssr
            };
            if let Some(target_str) = target {
                build_standalone(&dir, mode, &target_str, output, runner_stub)?;
            } else if output.is_some() {
                build_standalone(&dir, mode, "host", output, runner_stub)?;
            } else {
                build_project(&dir, mode)?;
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
    }
    Ok(())
}
