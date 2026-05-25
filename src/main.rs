use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.er>", args[0]);
        std::process::exit(1);
    }
    if let Err(e) = er::run_file(&args[1]) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
