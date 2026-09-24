pub mod commands;
pub mod spinner;
pub mod init;
pub mod build;
pub mod standalone;

use std::path::{Path, PathBuf};
use clap::Parser;

pub use commands::{Cli, Commands, BuildMode};
pub use init::init_project;
pub use build::build_project;
pub use standalone::build_standalone;

pub fn find_erm_server_path() -> Option<PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = Some(cwd.as_path());
        while let Some(dir) = current {
            let p1 = dir.join("libs").join("erm").join("server.er");
            if p1.is_file() {
                return Some(p1);
            }
            let p2 = dir.join("erm").join("server.er");
            if p2.is_file() {
                return Some(p2);
            }
            current = dir.parent();
        }
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let mut current = Some(exe_dir);
            while let Some(dir) = current {
                let p1 = dir.join("libs").join("erm").join("server.er");
                if p1.is_file() {
                    return Some(p1);
                }
                let p2 = dir.join("erm").join("server.er");
                if p2.is_file() {
                    return Some(p2);
                }
                current = dir.parent();
            }
        }
    }
    Some(PathBuf::from("erm/server.er"))
}

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
            unsafe {
                std::env::set_var("PORT", resolved_port.to_string());
                std::env::set_var("ERONOM_DIR", &dir);
                std::env::set_var("ERONOM_MODE", "prod");
            }
            let server_er = Path::new(&dir).join("server.er");
            if server_er.exists() {
                crate::runner::run_file(server_er.to_str().unwrap())?;
            } else {
                let server_script = find_erm_server_path().unwrap_or_else(|| PathBuf::from("libs/erm/server.er"));
                crate::runner::run_file(server_script.to_str().unwrap())?;
            }
        }
        Commands::Dev {
            dir_or_port,
            port_pos,
            port,
        } => {
            let (dir, port_val) = commands::parse_dir_and_port(dir_or_port, port_pos, port, ".")?;
            let resolved_port = commands::resolve_port(&dir, port_val, false)?;
            unsafe {
                std::env::set_var("PORT", resolved_port.to_string());
                std::env::set_var("ERONOM_DIR", &dir);
                std::env::set_var("ERONOM_MODE", "dev");
            }
            let server_script = find_erm_server_path().unwrap_or_else(|| PathBuf::from("libs/erm/server.er"));
            crate::runner::run_file(server_script.to_str().unwrap())?;
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
