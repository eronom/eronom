use std::sync::atomic::Ordering;
use std::time::Instant;
use ::eronom::vm as backend;
use ::eronom::frontend;
use super::metrics::{run_command_with_metrics, COUNTER};
use super::reporter::{print_comparative_footer, print_table_row, BenchResultRow};
use super::suite::BenchmarkDef;

pub fn noop_print(args: Vec<backend::Value>) -> backend::Value {
    let _ = args;
    backend::Value::null()
}

pub fn run_single_benchmark(bench: &BenchmarkDef, iterations: usize) -> BenchResultRow {
    let source = bench.er_source;
    let run_jit = std::env::var("ER_NO_JIT").is_err();

    eprintln!("================================================================================");
    eprintln!("  Benchmark: ⚡ {} ({})", bench.name, bench.id);
    eprintln!("  Details:   {}", bench.description);
    eprintln!("  Runs:      {} iterations each", iterations);
    eprintln!("================================================================================");

    // Warmup (JIT)
    if run_jit {
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

    // Warmup (Interpreter)
    {
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

    // Benchmark VM (Interpreter)
    let baseline = COUNTER.allocated.load(Ordering::SeqCst);
    COUNTER.reset_peak();
    unsafe extern "C" {
        fn er_gc_reset_stats();
    }
    unsafe { er_gc_reset_stats(); }
    let start = Instant::now();
    for _ in 0..iterations {
        let tokens = frontend::lex(source);
        let mut parser = frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = backend::Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let mut vm = backend::VM::new();
        vm.use_jit = false;
        vm.register_global("print", backend::Value::native_function(noop_print));
        vm.run(function).ok();
        backend::gc_free_all();
    }
    let vm_interpreter_elapsed = start.elapsed();
    let vm_interpreter_avg = vm_interpreter_elapsed / (iterations as u32);
    let vm_interpreter_peak_heap = COUNTER.peak.load(Ordering::SeqCst).saturating_sub(baseline);

    // Benchmark VM (JIT)
    let baseline = COUNTER.allocated.load(Ordering::SeqCst);
    COUNTER.reset_peak();
    let mut vm_jit_peak_heap = 0;
    let vm_jit_avg = if run_jit {
        unsafe extern "C" {
            fn er_jit_reset_profiler();
            fn er_gc_reset_stats();
        }
        unsafe {
            er_jit_reset_profiler();
            er_gc_reset_stats();
        }
        let start = Instant::now();
        for _ in 0..iterations {
            let tokens = frontend::lex(source);
            let mut parser = frontend::Parser::new(tokens);
            let stmts = parser.parse().unwrap();
            let compiler = backend::Compiler::new();
            let function = compiler.compile(&stmts).unwrap();
            let mut vm = backend::VM::new();
            vm.use_jit = true;
            vm.register_global("print", backend::Value::native_function(noop_print));
            vm.run(function).ok();
            backend::gc_free_all();
        }
        let vm_jit_elapsed = start.elapsed();
        vm_jit_peak_heap = COUNTER.peak.load(Ordering::SeqCst).saturating_sub(baseline);
        vm_jit_elapsed / (iterations as u32)
    } else {
        std::time::Duration::from_nanos(0)
    };

    // Compile-only benchmark
    let baseline = COUNTER.allocated.load(Ordering::SeqCst);
    COUNTER.reset_peak();
    let start = Instant::now();
    for _ in 0..iterations {
        let tokens = frontend::lex(source);
        let mut parser = frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = backend::Compiler::new();
        let _function = compiler.compile(&stmts).unwrap();
        backend::gc_free_all();
    }
    let compile_elapsed = start.elapsed();
    let compile_avg = compile_elapsed / (iterations as u32);
    let compile_peak_heap = COUNTER.peak.load(Ordering::SeqCst).saturating_sub(baseline);

    // Subprocess RSS measurement
    let self_exe = std::env::current_exe().ok();
    let run_self_subprocess = |mode: &str| -> Option<usize> {
        let exe = self_exe.as_ref()?;
        let mut time_cmd = std::process::Command::new("/usr/bin/time");
        time_cmd.arg("-f").arg("%M");
        time_cmd.arg(exe).arg(mode).arg(bench.id).arg(iterations.to_string());
        let output = time_cmd.output().ok()?;
        if output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if let Some(last_line) = stderr.lines().last() {
                if let Ok(kb) = last_line.trim().parse::<usize>() {
                    return Some(kb);
                }
            }
        }
        None
    };

    let vm_interpreter_rss = run_self_subprocess("--run-vm-interpreter");
    let vm_jit_rss = if run_jit {
        run_self_subprocess("--run-vm-jit")
    } else {
        None
    };
    let compile_rss = run_self_subprocess("--run-compiler");

    // Bun benchmark
    let run_bun_file =
        |args: &[&str], source: &str| -> Result<(String, Option<usize>), Box<dyn std::error::Error>> {
            let temp_filename = format!("temp_bench_bun_{}.js", bench.id);
            std::fs::write(&temp_filename, source)?;
            let mut full_args = args.to_vec();
            full_args.push(&temp_filename);
            let result = run_command_with_metrics("bun", &full_args);
            let _ = std::fs::remove_file(&temp_filename);
            if let (Some(stdout), rss) = result {
                Ok((stdout, rss))
            } else {
                Err("Command failed".into())
            }
        };

    let bun_pure_source = format!(
        r#"
const start = performance.now();
for (let r = 0; r < {}; r++) {{
    {}
}}
console.log((performance.now() - start) / 1000);
"#,
        iterations,
        bench.js_source
    );

    let mut bun_pure_avg = std::time::Duration::from_secs(0);
    let mut bun_pure_rss = None;
    if let Ok((output, rss)) = run_bun_file(&[], &bun_pure_source) {
        if let Ok(secs) = output.trim().parse::<f64>() {
            bun_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
            bun_pure_rss = rss;
        }
    }

    let mut bun_cli_avg = std::time::Duration::from_secs(0);
    let mut bun_cli_rss = None;
    if std::process::Command::new("bun").arg("-v").output().is_ok() {
        let temp_filename = format!("temp_bench_bun_cli_{}.js", bench.id);
        if std::fs::write(&temp_filename, bench.js_source).is_ok() {
            let start = Instant::now();
            let mut success = true;
            for _ in 0..iterations {
                if !std::process::Command::new("bun")
                    .arg(&temp_filename)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
                {
                    success = false;
                    break;
                }
            }
            if success {
                bun_cli_avg = start.elapsed() / (iterations as u32);
                let (_, rss) = run_command_with_metrics("bun", &[&temp_filename]);
                bun_cli_rss = rss;
            }
            let _ = std::fs::remove_file(&temp_filename);
        }
    }

    // Node.js benchmark
    let run_node_file =
        |args: &[&str], source: &str| -> Result<(String, Option<usize>), Box<dyn std::error::Error>> {
            let temp_filename = format!("temp_bench_node_{}.js", bench.id);
            std::fs::write(&temp_filename, source)?;
            let mut full_args = args.to_vec();
            full_args.push(&temp_filename);
            let result = run_command_with_metrics("node", &full_args);
            let _ = std::fs::remove_file(&temp_filename);
            if let (Some(stdout), rss) = result {
                Ok((stdout, rss))
            } else {
                Err("Command failed".into())
            }
        };

    let node_pure_source = format!(
        r#"
const perf = typeof performance !== 'undefined' ? performance : require('perf_hooks').performance;
const start = perf.now();
for (let r = 0; r < {}; r++) {{
    {}
}}
console.log((perf.now() - start) / 1000);
"#,
        iterations,
        bench.js_source
    );

    let mut node_pure_avg = std::time::Duration::from_secs(0);
    let mut node_pure_rss = None;
    if let Ok((output, rss)) = run_node_file(&[], &node_pure_source) {
        if let Ok(secs) = output.trim().parse::<f64>() {
            node_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
            node_pure_rss = rss;
        }
    }

    let mut node_cli_avg = std::time::Duration::from_secs(0);
    let mut node_cli_rss = None;
    if std::process::Command::new("node").arg("-v").output().is_ok() {
        let temp_filename = format!("temp_bench_node_cli_{}.js", bench.id);
        if std::fs::write(&temp_filename, bench.js_source).is_ok() {
            let start = Instant::now();
            let mut success = true;
            for _ in 0..iterations {
                if !std::process::Command::new("node")
                    .arg(&temp_filename)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
                {
                    success = false;
                    break;
                }
            }
            if success {
                node_cli_avg = start.elapsed() / (iterations as u32);
                let (_, rss) = run_command_with_metrics("node", &[&temp_filename]);
                node_cli_rss = rss;
            }
            let _ = std::fs::remove_file(&temp_filename);
        }
    }

    // Deno benchmark
    let run_deno_file =
        |args: &[&str], source: &str| -> Result<(String, Option<usize>), Box<dyn std::error::Error>> {
            let temp_filename = format!("temp_bench_deno_{}.js", bench.id);
            std::fs::write(&temp_filename, source)?;
            let mut full_args = args.to_vec();
            full_args.push("run");
            full_args.push(&temp_filename);
            let result = run_command_with_metrics("deno", &full_args);
            let _ = std::fs::remove_file(&temp_filename);
            if let (Some(stdout), rss) = result {
                Ok((stdout, rss))
            } else {
                Err("Command failed".into())
            }
        };

    let deno_pure_source = format!(
        r#"
const start = performance.now();
for (let r = 0; r < {}; r++) {{
    {}
}}
console.log((performance.now() - start) / 1000);
"#,
        iterations,
        bench.js_source
    );

    let mut deno_pure_avg = std::time::Duration::from_secs(0);
    let mut deno_pure_rss = None;
    if let Ok((output, rss)) = run_deno_file(&[], &deno_pure_source) {
        if let Ok(secs) = output.trim().parse::<f64>() {
            deno_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
            deno_pure_rss = rss;
        }
    }

    let mut deno_cli_avg = std::time::Duration::from_secs(0);
    let mut deno_cli_rss = None;
    if std::process::Command::new("deno").arg("--version").output().is_ok() {
        let temp_filename = format!("temp_bench_deno_cli_{}.js", bench.id);
        if std::fs::write(&temp_filename, bench.js_source).is_ok() {
            let start = Instant::now();
            let mut success = true;
            for _ in 0..iterations {
                if !std::process::Command::new("deno")
                    .arg("run")
                    .arg(&temp_filename)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
                {
                    success = false;
                    break;
                }
            }
            if success {
                deno_cli_avg = start.elapsed() / (iterations as u32);
                let (_, rss) = run_command_with_metrics("deno", &["run", &temp_filename]);
                deno_cli_rss = rss;
            }
            let _ = std::fs::remove_file(&temp_filename);
        }
    }

    // Results Table
    eprintln!("┌────────────────────┬────────────┬────────────┬────────────┐");
    eprintln!("│ Runner             │ Avg Time   │ Peak RSS   │ Peak Heap  │");
    eprintln!("├────────────────────┼────────────┼────────────┼────────────┤");

    print_table_row("VM (Interpreter)", vm_interpreter_avg, vm_interpreter_rss, Some(vm_interpreter_peak_heap));
    if vm_jit_avg.as_nanos() > 0 {
        print_table_row("VM (JIT)", vm_jit_avg, vm_jit_rss, Some(vm_jit_peak_heap));
    }
    print_table_row("Compile only", compile_avg, compile_rss, Some(compile_peak_heap));

    if bun_pure_avg.as_nanos() > 0 {
        print_table_row("Bun (pure run)", bun_pure_avg, bun_pure_rss, None);
    }
    if bun_cli_avg.as_nanos() > 0 {
        print_table_row("Bun (CLI exec)", bun_cli_avg, bun_cli_rss, None);
    }
    if node_pure_avg.as_nanos() > 0 {
        print_table_row("Node (pure run)", node_pure_avg, node_pure_rss, None);
    }
    if node_cli_avg.as_nanos() > 0 {
        print_table_row("Node (CLI exec)", node_cli_avg, node_cli_rss, None);
    }
    if deno_pure_avg.as_nanos() > 0 {
        print_table_row("Deno (pure run)", deno_pure_avg, deno_pure_rss, None);
    }
    if deno_cli_avg.as_nanos() > 0 {
        print_table_row("Deno (CLI exec)", deno_cli_avg, deno_cli_rss, None);
    }

    print_comparative_footer(
        vm_jit_avg,
        vm_interpreter_avg,
        bun_pure_avg,
        bun_cli_avg,
        node_pure_avg,
        node_cli_avg,
        deno_pure_avg,
        deno_cli_avg,
    );

    BenchResultRow {
        bench_id: bench.id.to_string(),
        bench_name: bench.name.to_string(),
        vm_jit_avg,
        vm_interp_avg: vm_interpreter_avg,
        bun_pure_avg,
        node_pure_avg,
        deno_pure_avg,
    }
}
