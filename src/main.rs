pub use eronom::vm as backend;
pub use eronom::frontend;
pub use eronom::jit;

mod runner;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use runner::{run_file, run_test_command};

fn main() {
    // 1. Check if the currently executing binary is a self-contained embedded executable
    if let Ok(true) = backend::embedded::check_and_mount_embedded() {
        let entrypoint = backend::embedded::get_vfs_entrypoint().unwrap_or_else(|| "server.er".to_string());
        if let Err(e) = run_file(&entrypoint) {
            eprintln!("Runtime error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    use clap::Parser;
    let cli = eronom::cli::Cli::parse();

    if let Some(file_path) = cli.file {
        if !file_path.to_string_lossy().ends_with(".er") && !file_path.exists() {
            eprintln!("Error: Unknown command or file: {}", file_path.display());
            std::process::exit(1);
        }
        if let Err(e) = run_file(file_path.to_str().unwrap_or("")) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    } else if let Some(cmd) = cli.command {
        match cmd {
            eronom::cli::Commands::Test { file } => {
                if let Err(e) = run_test_command(file) {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
            _ => {
                if let Err(e) = eronom::cli::run_command(cmd) {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    } else {
        use clap::CommandFactory;
        let mut cmd = eronom::cli::Cli::command();
        let _ = cmd.print_help();
        println!();
        std::process::exit(1);
    }
}
