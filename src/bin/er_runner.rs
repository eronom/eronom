#[path = "../runner/http_detect.rs"]
mod http_detect;
#[path = "../runner/micro_runner.rs"]
mod micro_runner;

fn main() {
    // 1. If running as a standalone self-contained bundle, execute the embedded app
    if let Some(res) = micro_runner::check_and_run_micro_bundle() {
        if let Err(e) = res {
            eprintln!("Error running standalone application: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // 2. Direct script execution fallback: er_runner path/to/script.er
    let mut args = std::env::args().skip(1);
    if let Some(script_path) = args.next() {
        if let Err(e) = micro_runner::run_micro_file(&script_path) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    } else {
        eprintln!("Usage: er_runner <script.er>");
        std::process::exit(1);
    }
}
