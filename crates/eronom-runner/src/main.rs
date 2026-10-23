pub use eronom::vm as backend;
pub use eronom::frontend;
pub use eronom::jit;

mod http_detect;
mod natives;
mod embedded_bundle;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    eronom::jit::init_jit_hooks();
    // 1. If running as a standalone self-contained bundle, execute the embedded app
    if let Some(res) = embedded_bundle::check_and_run_embedded_bundle() {
        if let Err(e) = res {
            eprintln!("Error running standalone application: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // 2. Direct script execution fallback: eronom-runner path/to/script.er
    let mut args = std::env::args().skip(1);
    if let Some(script_path) = args.next() {
        if let Err(e) = embedded_bundle::run_embedded_file(&script_path) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    } else {
        eprintln!("Usage: eronom-runner <script.er>");
        std::process::exit(1);
    }
}
