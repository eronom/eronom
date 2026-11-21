pub use eronom::vm as backend;
pub use eronom::frontend;
pub use eronom::jit;

pub use eronom::runner;

#[global_allocator]
static GLOBAL: eronom_core::alloc::MiMalloc = eronom_core::alloc::MiMalloc;

use runner::{run_file, run_test_command};

fn main() {
    // 1. Fast-path: Inspect raw OS argv directly via eronom-clap
    let os_iter = eronom_clap::OsIterator::init();
    if os_iter.remain.is_empty() {
        // If running as a standalone self-contained bundle with no arguments
        if let Some(res) = runner::check_and_run_embedded_bundle() {
            if let Err(e) = res {
                eprintln!("Error running standalone application: {}", e);
                std::process::exit(1);
            }
            return;
        }
        eronom::cli::print_root_help();
        std::process::exit(1);
    }

    let first_arg = os_iter.remain[0];
    if first_arg == b"-v" || first_arg == b"--version" {
        const VER_STR: &str = concat!("eronom ", env!("CARGO_PKG_VERSION"), "\n");
        #[cfg(unix)]
        unsafe {
            unsafe extern "C" {
                fn write(fd: i32, buf: *const u8, count: usize) -> isize;
            }
            let _ = write(1, VER_STR.as_ptr(), VER_STR.len());
        }
        #[cfg(not(unix))]
        {
            use std::io::Write;
            let _ = std::io::stdout().write_all(VER_STR.as_bytes());
        }
        return;
    }

    if first_arg == b"-h" || first_arg == b"--help" {
        eronom::cli::print_root_help();
        return;
    }

    eronom::jit::init_jit_hooks();

    // Direct script execution fast-path (sub-millisecond boot)
    if !first_arg.starts_with(b"-") {
        let first_str = std::str::from_utf8(first_arg).unwrap_or("");
        let is_subcommand = matches!(
            first_str,
            "init" | "build" | "start" | "dev" | "test" | "check" | "transpile" | "toolchain"
        );
        if !is_subcommand {
            let file_path = std::path::Path::new(first_str);
            if first_str.ends_with(".exe") && file_path.exists() {
                #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                {
                    let bytes = match std::fs::read(&file_path) {
                        Ok(b) => b,
                        Err(e) => {
                            eprintln!("Error reading {}: {}", file_path.display(), e);
                            std::process::exit(1);
                        }
                    };
                    match eronom::jit::aot::pe_writer::run_pe64_standalone(&bytes) {
                        Ok((code, output)) => {
                            print!("{}", output);
                            std::process::exit(code);
                        }
                        Err(e) => {
                            eprintln!("Error executing PE binary {}: {}", file_path.display(), e);
                            std::process::exit(1);
                        }
                    }
                }
                #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
                {
                    eprintln!("Running .exe directly is only supported on x86_64 Linux or natively on Windows.");
                    std::process::exit(1);
                }
            }
            if !first_str.ends_with(".er") && !file_path.exists() {
                if let Some(res) = runner::check_and_run_embedded_bundle() {
                    if let Err(e) = res {
                        eprintln!("Error running standalone application: {}", e);
                        std::process::exit(1);
                    }
                    return;
                }
                eprintln!("Error: Unknown command or file: {}", file_path.display());
                std::process::exit(1);
            }
            if let Err(e) = run_file(first_str) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            return;
        }
    }

    let cli = eronom::cli::Cli::parse();

    if let Some(file_path) = cli.file {
        if file_path.to_string_lossy().ends_with(".exe") && file_path.exists() {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            {
                let bytes = match std::fs::read(&file_path) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("Error reading {}: {}", file_path.display(), e);
                        std::process::exit(1);
                    }
                };
                match eronom::jit::aot::pe_writer::run_pe64_standalone(&bytes) {
                    Ok((code, output)) => {
                        print!("{}", output);
                        std::process::exit(code);
                    }
                    Err(e) => {
                        eprintln!("Error executing PE binary {}: {}", file_path.display(), e);
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
            {
                eprintln!("Running .exe directly is only supported on x86_64 Linux or natively on Windows.");
                std::process::exit(1);
            }
        }
        if !file_path.to_string_lossy().ends_with(".er") && !file_path.exists() {
            if let Some(res) = runner::check_and_run_embedded_bundle() {
                if let Err(e) = res {
                    eprintln!("Error running standalone application: {}", e);
                    std::process::exit(1);
                }
                return;
            }
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
            eronom::cli::Commands::Check { file, json } => {
                let path_buf = file.clone();
                match eronom::frontend::parse_and_resolve_imports(&path_buf) {
                    Ok(stmts) => {
                        match eronom::frontend::check_program(&stmts) {
                            Ok(()) => {
                                if json {
                                    let mut file_esc = String::new();
                                    eronom::frontend::ast::escape_json_string(&mut file_esc, &file.to_string_lossy());
                                    println!("{{\n  \"status\": \"ok\",\n  \"file\": \"{}\",\n  \"errors\": []\n}}", file_esc);
                                } else {
                                    println!("✓ Type check passed: {}", file.display());
                                }
                            }
                            Err(errs) => {
                                if json {
                                    let mut file_esc = String::new();
                                    eronom::frontend::ast::escape_json_string(&mut file_esc, &file.to_string_lossy());
                                    let err_strs: Vec<String> = errs.iter().map(|e| e.to_json()).collect();
                                    println!("{{\n  \"status\": \"error\",\n  \"file\": \"{}\",\n  \"errors\": [\n    {}\n  ]\n}}",
                                        file_esc, err_strs.join(",\n    "));
                                } else {
                                    eprintln!("✗ Type check failed with {} error(s):", errs.len());
                                    for err in errs {
                                        eprintln!("  {}", err);
                                    }
                                }
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        if json {
                            let mut file_esc = String::new();
                            eronom::frontend::ast::escape_json_string(&mut file_esc, &file.to_string_lossy());
                            let mut msg_esc = String::new();
                            eronom::frontend::ast::escape_json_string(&mut msg_esc, &format!("Parse/Import error: {}", e));
                            println!("{{\n  \"status\": \"error\",\n  \"file\": \"{}\",\n  \"errors\": [\n    {{\n      \"code\": \"E0001\",\n      \"message\": \"{}\",\n      \"loc\": {{\n        \"file_path\": \"{}\",\n        \"line\": 0,\n        \"col\": 0\n      }}\n    }}\n  ]\n}}",
                                file_esc, msg_esc, file_esc);
                        } else {
                            eprintln!("Parse/Import error: {}", e);
                        }
                        std::process::exit(1);
                    }
                }
            }
            eronom::cli::Commands::Transpile { file, out, check } => {
                let path_buf = file.clone();
                match eronom::frontend::parse_and_resolve_imports(&path_buf) {
                    Ok(stmts) => {
                        if check {
                            if let Err(errs) = eronom::frontend::check_program(&stmts) {
                                eprintln!("✗ Type check failed with {} error(s):", errs.len());
                                for err in &errs {
                                    eprintln!("  {}", err);
                                }
                                eprintln!("Tip: run with --check=false to transpile despite type errors.");
                                std::process::exit(1);
                            }
                        }
                        let js_code = eronom::frontend::transpile_to_js(&stmts);
                        let out_path = out.unwrap_or_else(|| {
                            let mut p = file.clone();
                            p.set_extension("js");
                            p
                        });
                        if let Err(e) = std::fs::write(&out_path, &js_code) {
                            eprintln!("Error writing output file {}: {}", out_path.display(), e);
                            std::process::exit(1);
                        }
                        println!("✓ Transpiled {} -> {}", file.display(), out_path.display());
                    }
                    Err(e) => {
                        eprintln!("Parse error: {}", e);
                        std::process::exit(1);
                    }
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
        if let Some(res) = runner::check_and_run_embedded_bundle() {
            if let Err(e) = res {
                eprintln!("Error running standalone application: {}", e);
                std::process::exit(1);
            }
            return;
        }
        eronom::cli::print_root_help();
        std::process::exit(1);
    }
}
