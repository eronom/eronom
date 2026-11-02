use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "eronom-cranelift", about = "Cranelift AOT Toolchain for Eronom")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compiles an Eronom script ahead-of-time into a native standalone binary
    Compile {
        /// Source Eronom script (.er)
        script: PathBuf,

        /// Output binary path
        #[arg(short, long)]
        output: PathBuf,

        /// Target platform (e.g. windows, macos, linux, x86_64-pc-windows-msvc)
        #[arg(short, long)]
        target: Option<String>,
    },
    /// Print toolchain version
    Version,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Compile { script, output, target } => {
            eronom_cranelift::build_cranelift_binary(&script, &output, target.as_deref())?;
            Ok(())
        }
        Commands::Version => {
            println!("eronom-cranelift v{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
