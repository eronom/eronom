/// Benchmark: VM-based vs Legacy tree-walking interpreter
/// Run with: cargo run --release --bin bench -p er
use std::time::Instant;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counter {
    allocated: AtomicUsize,
    peak: AtomicUsize,
}

impl Counter {
    const fn new() -> Self {
        Self {
            allocated: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }
    }

    fn add(&self, size: usize) {
        let prev = self.allocated.fetch_add(size, Ordering::SeqCst);
        let current = prev + size;
        let mut peak = self.peak.load(Ordering::SeqCst);
        while current > peak {
            match self.peak.compare_exchange_weak(peak, current, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(actual) => peak = actual,
            }
        }
    }

    fn sub(&self, size: usize) {
        self.allocated.fetch_sub(size, Ordering::SeqCst);
    }

    fn reset_peak(&self) {
        self.peak.store(self.allocated.load(Ordering::SeqCst), Ordering::SeqCst);
    }
}

static COUNTER: Counter = Counter::new();

struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            COUNTER.add(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        COUNTER.sub(layout.size());
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            COUNTER.add(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            COUNTER.sub(layout.size());
            COUNTER.add(new_size);
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn format_rss(kb: Option<usize>) -> String {
    match kb {
        Some(k) => {
            if k >= 1024 {
                format!("{:.2} MB", k as f64 / 1024.0)
            } else {
                format!("{} KB", k)
            }
        }
        None => "N/A".to_string(),
    }
}

fn run_command_with_metrics(
    cmd: &str,
    args: &[&str],
) -> (Option<String>, Option<usize>) {
    // Try to run with /usr/bin/time first to measure RSS
    let mut time_cmd = std::process::Command::new("/usr/bin/time");
    time_cmd.arg("-f").arg("%M");
    time_cmd.arg(cmd).args(args);
    if let Ok(output) = time_cmd.output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut rss = None;
            if let Some(last_line) = stderr.lines().last() {
                if let Ok(kb) = last_line.trim().parse::<usize>() {
                    rss = Some(kb);
                }
            }
            return (Some(stdout), rss);
        }
    }

    // Fallback: run directly without /usr/bin/time
    if let Ok(output) = std::process::Command::new(cmd).args(args).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            return (Some(stdout), None);
        }
    }

    (None, None)
}

fn main() {
    let source = r#"
let x = 0

for i in 1..10000 {
    let val = i + i
    if (val > 5000) {
        let dummy = val + 1
    } else {
        let dummy = val + 2
    }
}
"#;

    let iterations = 50;

    fn noop_print(args: Vec<er::backend::Value>) -> er::backend::Value {
        let _ = args;
        er::backend::Value::null()
    }

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let mode = &args[1];
        match mode.as_str() {
            "--run-vm-interpreter" => {
                // Warmup
                {
                    let tokens = er::frontend::lex(source);
                    let mut parser = er::frontend::Parser::new(tokens);
                    let stmts = parser.parse().unwrap();
                    let compiler = er::backend::Compiler::new();
                    let function = compiler.compile(&stmts).unwrap();
                    let mut vm = er::backend::VM::new();
                    vm.use_jit = false;
                    vm.register_global("print", er::backend::Value::native_function(noop_print));
                    vm.run(function).ok();
                }
                // Loop
                for _ in 0..iterations {
                    let tokens = er::frontend::lex(source);
                    let mut parser = er::frontend::Parser::new(tokens);
                    let stmts = parser.parse().unwrap();
                    let compiler = er::backend::Compiler::new();
                    let function = compiler.compile(&stmts).unwrap();
                    let mut vm = er::backend::VM::new();
                    vm.use_jit = false;
                    vm.register_global("print", er::backend::Value::native_function(noop_print));
                    vm.run(function).ok();
                }
                return;
            }
            "--run-vm-jit" => {
                // Warmup
                {
                    let tokens = er::frontend::lex(source);
                    let mut parser = er::frontend::Parser::new(tokens);
                    let stmts = parser.parse().unwrap();
                    let compiler = er::backend::Compiler::new();
                    let function = compiler.compile(&stmts).unwrap();
                    let mut vm = er::backend::VM::new();
                    vm.use_jit = true;
                    vm.register_global("print", er::backend::Value::native_function(noop_print));
                    vm.run(function).ok();
                }
                // Loop
                for _ in 0..iterations {
                    let tokens = er::frontend::lex(source);
                    let mut parser = er::frontend::Parser::new(tokens);
                    let stmts = parser.parse().unwrap();
                    let compiler = er::backend::Compiler::new();
                    let function = compiler.compile(&stmts).unwrap();
                    let mut vm = er::backend::VM::new();
                    vm.use_jit = true;
                    vm.register_global("print", er::backend::Value::native_function(noop_print));
                    vm.run(function).ok();
                }
                return;
            }
            "--run-compiler" => {
                for _ in 0..iterations {
                    let tokens = er::frontend::lex(source);
                    let mut parser = er::frontend::Parser::new(tokens);
                    let stmts = parser.parse().unwrap();
                    let compiler = er::backend::Compiler::new();
                    let _function = compiler.compile(&stmts).unwrap();
                }
                return;
            }
            _ => {}
        }
    }

    println!("Size of Value: {}", std::mem::size_of::<er::backend::Value>());

    eprintln!("=== ER Language Benchmark ===");
    eprintln!("  Script: 10,000 loop iterations with arithmetic + conditionals");
    eprintln!("  Runs:   {} iterations each", iterations);
    if let Ok(cwd) = std::env::current_dir() {
        eprintln!("CWD: {}", cwd.display());
    }
    eprintln!();

    let run_jit = std::env::var("ER_NO_JIT").is_err();

    // Warmup (JIT)
    if run_jit {
        let tokens = er::frontend::lex(source);
        let mut parser = er::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = er::backend::Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let mut vm = er::backend::VM::new();
        vm.use_jit = true;
        vm.register_global("print", er::backend::Value::native_function(noop_print));
        vm.run(function).ok();
    }

    // Warmup (Interpreter)
    {
        let tokens = er::frontend::lex(source);
        let mut parser = er::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = er::backend::Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let mut vm = er::backend::VM::new();
        vm.use_jit = false;
        vm.register_global("print", er::backend::Value::native_function(noop_print));
        vm.run(function).ok();
    }

    // Benchmark VM (Interpreter)
    let baseline = COUNTER.allocated.load(Ordering::SeqCst);
    COUNTER.reset_peak();
    let start = Instant::now();
    for _ in 0..iterations {
        let tokens = er::frontend::lex(source);
        let mut parser = er::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = er::backend::Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let mut vm = er::backend::VM::new();
        vm.use_jit = false;
        vm.register_global("print", er::backend::Value::native_function(noop_print));
        vm.run(function).ok();
    }
    let vm_interpreter_elapsed = start.elapsed();
    let vm_interpreter_avg = vm_interpreter_elapsed / iterations;
    let vm_interpreter_peak_heap = COUNTER.peak.load(Ordering::SeqCst).saturating_sub(baseline);

    // Benchmark VM (JIT)
    let baseline = COUNTER.allocated.load(Ordering::SeqCst);
    COUNTER.reset_peak();
    let mut vm_jit_peak_heap = 0;
    let vm_jit_avg = if run_jit {
        let start = Instant::now();
        for _ in 0..iterations {
            let tokens = er::frontend::lex(source);
            let mut parser = er::frontend::Parser::new(tokens);
            let stmts = parser.parse().unwrap();
            let compiler = er::backend::Compiler::new();
            let function = compiler.compile(&stmts).unwrap();
            let mut vm = er::backend::VM::new();
            vm.use_jit = true;
            vm.register_global("print", er::backend::Value::native_function(noop_print));
            vm.run(function).ok();
        }
        let vm_jit_elapsed = start.elapsed();
        vm_jit_peak_heap = COUNTER.peak.load(Ordering::SeqCst).saturating_sub(baseline);
        vm_jit_elapsed / iterations
    } else {
        std::time::Duration::from_nanos(0)
    };

    // --- Compile-only benchmark ---
    let baseline = COUNTER.allocated.load(Ordering::SeqCst);
    COUNTER.reset_peak();
    let start = Instant::now();
    for _ in 0..iterations {
        let tokens = er::frontend::lex(source);
        let mut parser = er::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = er::backend::Compiler::new();
        let _function = compiler.compile(&stmts).unwrap();
    }
    let compile_elapsed = start.elapsed();
    let compile_avg = compile_elapsed / iterations;
    let compile_peak_heap = COUNTER.peak.load(Ordering::SeqCst).saturating_sub(baseline);

    // --- Measure RSS of Rust runs via subprocesses ---
    let self_exe = std::env::current_exe().ok();
    let run_self_subprocess = |mode: &str| -> Option<usize> {
        let exe = self_exe.as_ref()?;
        let mut time_cmd = std::process::Command::new("/usr/bin/time");
        time_cmd.arg("-f").arg("%M");
        time_cmd.arg(exe).arg(mode);
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

    // --- Lua benchmark ---
    let lua_source = r#"
local x = 0
for i = 1, 9999 do
    local val = i + i
    if val > 5000 then
        local dummy = val + 1
    else
        local dummy = val + 2
    end
end
"#;

    let lua_pure_source = format!(
        r#"
local start = os.clock()
for _ = 1, {} do
    local x = 0
    for i = 1, 9999 do
        local val = i + i
        if val > 5000 then
            local dummy = val + 1
        else
            local dummy = val + 2
        end
    end
end
print(os.clock() - start)
"#,
        iterations
    );

    let mut lua_pure_avg = std::time::Duration::from_secs(0);
    let mut lua_pure_rss = None;
    if let (Some(stdout), rss) = run_command_with_metrics("lua", &["-e", &lua_pure_source]) {
        if let Ok(secs) = stdout.trim().parse::<f64>() {
            lua_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
            lua_pure_rss = rss;
        }
    }

    let mut lua_cli_avg = std::time::Duration::from_secs(0);
    let mut lua_cli_rss = None;
    if std::process::Command::new("lua").arg("-v").output().is_ok() {
        let start = Instant::now();
        let mut success = true;
        for _ in 0..iterations {
            if std::process::Command::new("lua")
                .arg("-e")
                .arg(lua_source)
                .output()
                .is_err()
            {
                success = false;
                break;
            }
        }
        if success {
            lua_cli_avg = start.elapsed() / iterations;
            let (_, rss) = run_command_with_metrics("lua", &["-e", lua_source]);
            lua_cli_rss = rss;
        }
    }

    // --- Luau benchmark ---
    let run_luau_file =
        |args: &[&str], source: &str| -> Result<(String, Option<usize>), Box<dyn std::error::Error>> {
            let temp_filename = "temp_bench_luau.lua";
            std::fs::write(temp_filename, source)?;
            
            let mut full_args = args.to_vec();
            full_args.push(temp_filename);
            
            let result = run_command_with_metrics("luau", &full_args);
            let _ = std::fs::remove_file(temp_filename);
            
            if let (Some(stdout), rss) = result {
                Ok((stdout, rss))
            } else {
                Err("Command failed".into())
            }
        };

    let mut luau_pure_avg = std::time::Duration::from_secs(0);
    let mut luau_pure_rss = None;
    if let Ok((output, rss)) = run_luau_file(&[], &lua_pure_source) {
        if let Ok(secs) = output.trim().parse::<f64>() {
            luau_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
            luau_pure_rss = rss;
        }
    }

    let mut luau_codegen_pure_avg = std::time::Duration::from_secs(0);
    let mut luau_codegen_pure_rss = None;
    if let Ok((output, rss)) = run_luau_file(&["--codegen"], &lua_pure_source) {
        if let Ok(secs) = output.trim().parse::<f64>() {
            luau_codegen_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
            luau_codegen_pure_rss = rss;
        }
    }

    let mut luau_cli_avg = std::time::Duration::from_secs(0);
    let mut luau_cli_rss = None;
    if std::process::Command::new("luau")
        .arg("-h")
        .output()
        .is_ok()
    {
        let temp_filename = "temp_bench_luau_cli.lua";
        if std::fs::write(temp_filename, lua_source).is_ok() {
            let start = Instant::now();
            let mut success = true;
            for _ in 0..iterations {
                if !std::process::Command::new("luau")
                    .arg(temp_filename)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
                {
                    success = false;
                    break;
                }
            }
            if success {
                luau_cli_avg = start.elapsed() / iterations;
                let (_, rss) = run_command_with_metrics("luau", &[temp_filename]);
                luau_cli_rss = rss;
            }
            let _ = std::fs::remove_file(temp_filename);
        }
    }

    let mut luau_codegen_cli_avg = std::time::Duration::from_secs(0);
    let mut luau_codegen_cli_rss = None;
    if std::process::Command::new("luau")
        .arg("-h")
        .output()
        .is_ok()
    {
        let temp_filename = "temp_bench_luau_cg_cli.lua";
        if std::fs::write(temp_filename, lua_source).is_ok() {
            let start = Instant::now();
            let mut success = true;
            for _ in 0..iterations {
                if !std::process::Command::new("luau")
                    .arg("--codegen")
                    .arg(temp_filename)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
                {
                    success = false;
                    break;
                }
            }
            if success {
                luau_codegen_cli_avg = start.elapsed() / iterations;
                let (_, rss) = run_command_with_metrics("luau", &["--codegen", temp_filename]);
                luau_codegen_cli_rss = rss;
            }
            let _ = std::fs::remove_file(temp_filename);
        }
    }

    // --- Node benchmark ---
    let run_node_file =
        |args: &[&str], source: &str| -> Result<(String, Option<usize>), Box<dyn std::error::Error>> {
            let temp_filename = "temp_bench_node.js";
            std::fs::write(temp_filename, source)?;
            
            let mut full_args = args.to_vec();
            full_args.push(temp_filename);
            
            let result = run_command_with_metrics("node", &full_args);
            let _ = std::fs::remove_file(temp_filename);
            
            if let (Some(stdout), rss) = result {
                Ok((stdout, rss))
            } else {
                Err("Command failed".into())
            }
        };

    let node_source = r#"
let x = 0
for (let i = 1; i < 10000; i++) {
    let val = i + i
    if (val > 5000) {
        let dummy = val + 1
    } else {
        let dummy = val + 2
    }
}
"#;

    let node_pure_source = format!(
        r#"
const start = performance.now();
for (let r = 0; r < {}; r++) {{
    let x = 0;
    for (let i = 1; i < 10000; i++) {{
        let val = i + i;
        if (val > 5000) {{
            let dummy = val + 1;
        }} else {{
            let dummy = val + 2;
        }}
    }}
}}
console.log((performance.now() - start) / 1000);
"#,
        iterations
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
    if std::process::Command::new("node")
        .arg("-v")
        .output()
        .is_ok()
    {
        let temp_filename = "temp_bench_node_cli.js";
        if std::fs::write(temp_filename, node_source).is_ok() {
            let start = Instant::now();
            let mut success = true;
            for _ in 0..iterations {
                if !std::process::Command::new("node")
                    .arg(temp_filename)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
                {
                    success = false;
                    break;
                }
            }
            if success {
                node_cli_avg = start.elapsed() / iterations;
                let (_, rss) = run_command_with_metrics("node", &[temp_filename]);
                node_cli_rss = rss;
            }
            let _ = std::fs::remove_file(temp_filename);
        }
    }

    // --- Results Table ---
    eprintln!("┌────────────────────┬────────────┬────────────┬────────────┐");
    eprintln!("│ Runner             │ Avg Time   │ Peak RSS   │ Peak Heap  │");
    eprintln!("├────────────────────┼────────────┼────────────┼────────────┤");

    let print_row = |name: &str, avg_time: std::time::Duration, rss: Option<usize>, heap: Option<usize>| {
        let time_str = format!("{:?}", avg_time);
        let rss_str = format_rss(rss);
        let heap_str = match heap {
            Some(h) => format_bytes(h),
            None => "-".to_string(),
        };
        eprintln!(
            "│ {:<18} │ {:>10} │ {:>10} │ {:>10} │",
            name, time_str, rss_str, heap_str
        );
    };

    print_row("VM (Interpreter)", vm_interpreter_avg, vm_interpreter_rss, Some(vm_interpreter_peak_heap));
    if vm_jit_avg.as_nanos() > 0 {
        print_row("VM (JIT)", vm_jit_avg, vm_jit_rss, Some(vm_jit_peak_heap));
    }
    print_row("Compile only", compile_avg, compile_rss, Some(compile_peak_heap));
    if lua_pure_avg.as_nanos() > 0 {
        print_row("Lua (pure run)", lua_pure_avg, lua_pure_rss, None);
    }
    if lua_cli_avg.as_nanos() > 0 {
        print_row("Lua (external CLI)", lua_cli_avg, lua_cli_rss, None);
    }
    if luau_pure_avg.as_nanos() > 0 {
        print_row("Luau (pure run)", luau_pure_avg, luau_pure_rss, None);
    }
    if luau_codegen_pure_avg.as_nanos() > 0 {
        print_row("Luau+Codegen (pure)", luau_codegen_pure_avg, luau_codegen_pure_rss, None);
    }
    if luau_cli_avg.as_nanos() > 0 {
        print_row("Luau (external CLI)", luau_cli_avg, luau_cli_rss, None);
    }
    if luau_codegen_cli_avg.as_nanos() > 0 {
        print_row("Luau+Codegen (CLI)", luau_codegen_cli_avg, luau_codegen_cli_rss, None);
    }
    if node_pure_avg.as_nanos() > 0 {
        print_row("Node (pure run)", node_pure_avg, node_pure_rss, None);
    }
    if node_cli_avg.as_nanos() > 0 {
        print_row("Node (external CLI)", node_cli_avg, node_cli_rss, None);
    }

    // Footers / Speedups comparisons
    let mut has_footer = false;

    let mut print_footer = |icon: &str, text: &str| {
        if !has_footer {
            eprintln!("├────────────────────┴────────────┴────────────┴────────────┤");
            has_footer = true;
        }
        let text_len = text.chars().count();
        let padding_len = 52_usize.saturating_sub(text_len);
        let padding = " ".repeat(padding_len);
        eprintln!("│  {} {} {} │", icon, text, padding);
    };

    if vm_jit_avg.as_nanos() > 0 {
        let jit_speedup = vm_interpreter_avg.as_nanos() as f64 / vm_jit_avg.as_nanos() as f64;
        let jit_text = format!("JIT is {:.2}x FASTER than Interpreter", jit_speedup);
        print_footer("🚀", &jit_text);
    }

    let vm_avg = if vm_jit_avg.as_nanos() > 0 { vm_jit_avg } else { vm_interpreter_avg };

    if lua_pure_avg.as_nanos() > 0 {
        if vm_avg < lua_pure_avg {
            let speedup = lua_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Lua (pure)", speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / lua_pure_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Lua (pure)", slowdown);
            print_footer("⚠️", &text);
        }
    }

    if lua_cli_avg.as_nanos() > 0 {
        if vm_avg < lua_cli_avg {
            let speedup = lua_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Lua (CLI)", speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / lua_cli_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Lua (CLI)", slowdown);
            print_footer("⚠️", &text);
        }
    }

    if luau_pure_avg.as_nanos() > 0 {
        if vm_avg < luau_pure_avg {
            let speedup = luau_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Luau (pure)", speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / luau_pure_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Luau (pure)", slowdown);
            print_footer("⚠️", &text);
        }
    }

    if luau_codegen_pure_avg.as_nanos() > 0 {
        if vm_avg < luau_codegen_pure_avg {
            let speedup = luau_codegen_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Luau (native)", speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / luau_codegen_pure_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Luau (native)", slowdown);
            print_footer("⚠️", &text);
        }
    }

    if luau_cli_avg.as_nanos() > 0 {
        if vm_avg < luau_cli_avg {
            let speedup = luau_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Luau (CLI)", speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / luau_cli_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Luau (CLI)", slowdown);
            print_footer("⚠️", &text);
        }
    }

    if luau_codegen_cli_avg.as_nanos() > 0 {
        if vm_avg < luau_codegen_cli_avg {
            let speedup = luau_codegen_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Luau (cg CLI)", speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / luau_codegen_cli_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Luau (cg CLI)", slowdown);
            print_footer("⚠️", &text);
        }
    }

    if node_pure_avg.as_nanos() > 0 {
        if vm_avg < node_pure_avg {
            let speedup = node_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Node (pure)", speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / node_pure_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Node (pure)", slowdown);
            print_footer("⚠️", &text);
        }
    }

    if node_cli_avg.as_nanos() > 0 {
        if vm_avg < node_cli_avg {
            let speedup = node_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Node (CLI)", speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / node_cli_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Node (CLI)", slowdown);
            print_footer("⚠️", &text);
        }
    }

    if has_footer {
        let footer_bottom = "─".repeat(59);
        eprintln!("└{}┘", footer_bottom);
    } else {
        eprintln!("└────────────────────┴────────────┴────────────┴────────────┘");
    }
}
