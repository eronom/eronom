pub use eronom::vm as backend;
pub use eronom::frontend;

mod http_detect;
mod micro_runner;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    // 1. If running as a standalone self-contained bundle, execute the embedded app
    if let Some(res) = micro_runner::check_and_run_micro_bundle() {
        if let Err(e) = res {
            eprintln!("Error running standalone application: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // 2. Direct script execution fallback: eronom-runner path/to/script.er
    let mut args = std::env::args().skip(1);
    if let Some(script_path) = args.next() {
        if let Err(e) = micro_runner::run_micro_file(&script_path) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    } else {
        eprintln!("Usage: eronom-runner <script.er>");
        std::process::exit(1);
    }
}
