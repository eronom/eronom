/// Eronom Language & VM Benchmarking Suite (vs JavaScript: Node.js, Bun, Deno)
/// Run with: cargo run --release --bin bench -p er
/// Options:
///   --bench <name> / -b <name>   Select benchmark (alloc, fib, trees, sieve, matrix, mandelbrot, pipeline, all)
///   --iterations <n> / -n <n>    Override iteration count
///   --all                        Run all benchmarks and show comprehensive comparison matrix
pub use ::eronom::vm as backend;
pub use ::eronom::frontend;
pub use ::eronom::jit;

use crate as eronom;

mod metrics;
mod suite;
mod reporter;
mod runners;

use suite::{get_benchmark_suite, BenchmarkDef};
use reporter::print_consolidated_summary;
use runners::{noop_print, run_single_benchmark};

#[global_allocator]
static GLOBAL_TRACKER: metrics::TrackingAllocator = metrics::TrackingAllocator;

fn main() {
    backend::alloc::init_allocator_options();
    let suite = get_benchmark_suite();

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let mode = &args[1];
        if mode == "--run-vm-interpreter" || mode == "--run-vm-jit" || mode == "--run-compiler" {
            let target_id = args.get(2).map(|s| s.as_str()).unwrap_or("object_alloc");
            let iters = args.get(3).and_then(|s| s.parse::<usize>().ok()).unwrap_or(20);
            
            let bench = suite.iter().find(|b| b.id == target_id || b.aliases.contains(&target_id)).unwrap_or(&suite[0]);
            let source = bench.er_source;

            match mode.as_str() {
                "--run-vm-interpreter" => {
                    for _ in 0..iters {
                        let tokens = frontend::lex(source);
                        let mut parser = frontend::Parser::new(tokens);
                        if let Ok(stmts) = parser.parse() {
                            let compiler = backend::Compiler::new();
                            if let Ok(function) = compiler.compile(&stmts) {
                                let mut vm = backend::VM::new();
                                vm.use_jit = false;
                                vm.register_global("print", backend::Value::native_function(noop_print));
                                vm.run(function).ok();
                                backend::gc_free_all();
                            }
                        }
                    }
                    return;
                }
                "--run-vm-jit" => {
                    for _ in 0..iters {
                        let tokens = frontend::lex(source);
                        let mut parser = frontend::Parser::new(tokens);
                        if let Ok(stmts) = parser.parse() {
                            let compiler = backend::Compiler::new();
                            if let Ok(function) = compiler.compile(&stmts) {
                                let mut vm = backend::VM::new();
                                vm.use_jit = true;
                                vm.register_global("print", backend::Value::native_function(noop_print));
                                vm.run(function).ok();
                                backend::gc_free_all();
                            }
                        }
                    }
                    return;
                }
                "--run-compiler" => {
                    for _ in 0..iters {
                        let tokens = frontend::lex(source);
                        let mut parser = frontend::Parser::new(tokens);
                        if let Ok(stmts) = parser.parse() {
                            let compiler = backend::Compiler::new();
                            let _ = compiler.compile(&stmts);
                            backend::gc_free_all();
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
    }

    // CLI Option Parsing
    let mut selected_bench: Option<String> = None;
    let mut custom_iterations: Option<usize> = None;
    let mut run_all = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--bench" || arg == "-b" {
            if i + 1 < args.len() {
                selected_bench = Some(args[i + 1].clone());
                i += 2;
                continue;
            }
        } else if arg.starts_with("--bench=") {
            selected_bench = Some(arg.trim_start_matches("--bench=").to_string());
        } else if arg == "--iterations" || arg == "-n" {
            if i + 1 < args.len() {
                custom_iterations = args[i + 1].parse::<usize>().ok();
                i += 2;
                continue;
            }
        } else if arg.starts_with("--iterations=") {
            custom_iterations = arg.trim_start_matches("--iterations=").parse::<usize>().ok();
        } else if arg == "--all" || arg == "-a" {
            run_all = true;
        } else if !arg.starts_with('-') && selected_bench.is_none() {
            selected_bench = Some(arg.clone());
        }
        i += 1;
    }

    println!("Size of Value: {} bytes", std::mem::size_of::<backend::Value>());
    if let Ok(cwd) = std::env::current_dir() {
        eprintln!("CWD: {}", cwd.display());
    }
    eprintln!();

    let benchmarks_to_run: Vec<&BenchmarkDef> = if run_all || selected_bench.as_deref() == Some("all") {
        suite.iter().collect()
    } else if let Some(ref target) = selected_bench {
        let found = suite.iter().find(|b| b.id == target.as_str() || b.aliases.contains(&target.as_str()));
        if let Some(b) = found {
            vec![b]
        } else {
            eprintln!("Unknown benchmark '{}'. Available options:", target);
            for b in &suite {
                eprintln!("  - {} (aliases: {:?})", b.id, b.aliases);
            }
            eprintln!("  - all");
            return;
        }
    } else {
        suite.iter().collect()
    };

    let mut results = Vec::new();
    for bench in &benchmarks_to_run {
        let iters = custom_iterations.unwrap_or(bench.default_iterations);
        let res = run_single_benchmark(bench, iters);
        results.push(res);
    }

    if results.len() > 1 {
        print_consolidated_summary(&results);
    }
}
