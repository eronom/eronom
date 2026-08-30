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

use std::time::Instant;
use std::alloc::{GlobalAlloc, Layout};
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
        let ptr = unsafe { mimalloc::MiMalloc.alloc(layout) };
        if !ptr.is_null() {
            COUNTER.add(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { mimalloc::MiMalloc.dealloc(ptr, layout) };
        COUNTER.sub(layout.size());
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { mimalloc::MiMalloc.alloc_zeroed(layout) };
        if !ptr.is_null() {
            COUNTER.add(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { mimalloc::MiMalloc.realloc(ptr, layout, new_size) };
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
        None => "-".to_string(),
    }
}

fn run_command_with_metrics(
    cmd: &str,
    args: &[&str],
) -> (Option<String>, Option<usize>) {
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

    if let Ok(output) = std::process::Command::new(cmd).args(args).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            return (Some(stdout), None);
        }
    }

    (None, None)
}

struct BenchmarkDef {
    id: &'static str,
    aliases: &'static [&'static str],
    name: &'static str,
    description: &'static str,
    default_iterations: usize,
    er_source: &'static str,
    js_source: &'static str,
}

fn get_benchmark_suite() -> Vec<BenchmarkDef> {
    vec![
        BenchmarkDef {
            id: "object_alloc",
            aliases: &["alloc", "objects", "object"],
            name: "Object & Array Allocation Lifecycle",
            description: "50,000 loop iterations allocating objects, arrays, resizing & string templating",
            default_iterations: 30,
            er_source: r#"
for i in 1..50000 {
    let arr = [i, i + 1, i + 2]
    arr.push(i + 3)
    let pop_val = arr.pop()
    let obj = { a: arr, b: i }
    let s = "num: {i}"
    let dummy = obj.a
}
"#,
            js_source: r#"
for (let i = 1; i < 50000; i++) {
    let arr = [i, i + 1, i + 2];
    arr.push(i + 3);
    let pop_val = arr.pop();
    let obj = { a: arr, b: i };
    let s = `num: ${i}`;
    let dummy = obj.a;
}
"#,
        },
        BenchmarkDef {
            id: "fibonacci",
            aliases: &["fib"],
            name: "Recursive Fibonacci (Call Frame & Stack Dispatch)",
            description: "Deep recursive fib(28) testing function call dispatch, call frames & integer math",
            default_iterations: 20,
            er_source: r#"
fn fib(n) {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
let res = fib(28);
"#,
            js_source: r#"
function fib(n) {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
let res = fib(28);
"#,
        },
        BenchmarkDef {
            id: "binary_trees",
            aliases: &["trees", "tree", "binary_tree"],
            name: "Binary Trees (GC Object Graph & Tree Traversal)",
            description: "Bottom-up binary tree creation (depth=12) & recursive item checksums",
            default_iterations: 20,
            er_source: r#"
fn bottomUpTree(depth) {
    if (depth <= 0) {
        return { left: null, right: null };
    }
    return {
        left: bottomUpTree(depth - 1),
        right: bottomUpTree(depth - 1)
    };
}

fn itemCheck(node) {
    if (node.left == null) {
        return 1;
    }
    return 1 + itemCheck(node.left) + itemCheck(node.right);
}

let maxDepth = 12;
let stretchDepth = maxDepth + 1;
let check = itemCheck(bottomUpTree(stretchDepth));
let longLivedTree = bottomUpTree(maxDepth);
let longLivedCheck = itemCheck(longLivedTree);
"#,
            js_source: r#"
function bottomUpTree(depth) {
    if (depth <= 0) {
        return { left: null, right: null };
    }
    return {
        left: bottomUpTree(depth - 1),
        right: bottomUpTree(depth - 1)
    };
}

function itemCheck(node) {
    if (node.left === null) {
        return 1;
    }
    return 1 + itemCheck(node.left) + itemCheck(node.right);
}

let maxDepth = 12;
let stretchDepth = maxDepth + 1;
let check = itemCheck(bottomUpTree(stretchDepth));
let longLivedTree = bottomUpTree(maxDepth);
let longLivedCheck = itemCheck(longLivedTree);
"#,
        },
        BenchmarkDef {
            id: "sieve_primes",
            aliases: &["sieve", "primes", "prime"],
            name: "Sieve of Eratosthenes (Array Mutation & Indexing)",
            description: "Prime sieve computing all primes up to 30,000 with inner loop cross-off",
            default_iterations: 20,
            er_source: r#"
fn sieve(limit) {
    let is_prime = [];
    for i in 0..limit {
        is_prime.push(1);
    }
    let p = 2;
    while (p * p < limit) {
        if (is_prime[p] == 1) {
            let k = p * p;
            while (k < limit) {
                is_prime[k] = 0;
                k = k + p;
            }
        }
        p = p + 1;
    }
    let count = 0;
    for i in 2..limit {
        if (is_prime[i] == 1) {
            count = count + 1;
        }
    }
    return count;
}
let primes = sieve(30000);
"#,
            js_source: r#"
function sieve(limit) {
    let is_prime = [];
    for (let i = 0; i < limit; i++) {
        is_prime.push(1);
    }
    let p = 2;
    while (p * p < limit) {
        if (is_prime[p] === 1) {
            let k = p * p;
            while (k < limit) {
                is_prime[k] = 0;
                k = k + p;
            }
        }
        p = p + 1;
    }
    let count = 0;
    for (let i = 2; i < limit; i++) {
        if (is_prime[i] === 1) {
            count = count + 1;
        }
    }
    return count;
}
let primes = sieve(30000);
"#,
        },
        BenchmarkDef {
            id: "matrix_mult",
            aliases: &["matrix", "matmul"],
            name: "Matrix Multiplication (2D Array Math & 3-Level Loops)",
            description: "45x45 2D matrix multiplication with O(N^3) nested loop arithmetic",
            default_iterations: 20,
            er_source: r#"
fn makeMatrix(rows, cols, initial) {
    let mat = [];
    for r in 0..rows {
        let row = [];
        for c in 0..cols {
            row.push(initial + r + c);
        }
        mat.push(row);
    }
    return mat;
}

fn matrixMultiply(a, b, n) {
    let res = [];
    for i in 0..n {
        let row = [];
        for j in 0..n {
            let sum = 0;
            for k in 0..n {
                sum = sum + a[i][k] * b[k][j];
            }
            row.push(sum);
        }
        res.push(row);
    }
    return res;
}

let n = 45;
let a = makeMatrix(n, n, 1);
let b = makeMatrix(n, n, 2);
let c = matrixMultiply(a, b, n);
"#,
            js_source: r#"
function makeMatrix(rows, cols, initial) {
    let mat = [];
    for (let r = 0; r < rows; r++) {
        let row = [];
        for (let c = 0; c < cols; c++) {
            row.push(initial + r + c);
        }
        mat.push(row);
    }
    return mat;
}

function matrixMultiply(a, b, n) {
    let res = [];
    for (let i = 0; i < n; i++) {
        let row = [];
        for (let j = 0; j < n; j++) {
            let sum = 0;
            for (let k = 0; k < n; k++) {
                sum = sum + a[i][k] * b[k][j];
            }
            row.push(sum);
        }
        res.push(row);
    }
    return res;
}

let n = 45;
let a = makeMatrix(n, n, 1);
let b = makeMatrix(n, n, 2);
let c = matrixMultiply(a, b, n);
"#,
        },
        BenchmarkDef {
            id: "mandelbrot",
            aliases: &["mandel", "fractal"],
            name: "Mandelbrot Computation (Hot Floating-Point Loops)",
            description: "100x100 grid Mandelbrot escape-time calculation with 100 max iterations",
            default_iterations: 20,
            er_source: r#"
fn mandelbrot(width, height, max_iter) {
    let checksum = 0;
    for y in 0..height {
        let ci = (y * 2.0 / height) - 1.0;
        for x in 0..width {
            let cr = (x * 3.0 / width) - 2.0;
            let zr = 0.0;
            let zi = 0.0;
            let iter = 0;
            while (zr * zr + zi * zi <= 4.0 and iter < max_iter) {
                let temp = zr * zr - zi * zi + cr;
                zi = 2.0 * zr * zi + ci;
                zr = temp;
                iter = iter + 1;
            }
            checksum = checksum + iter;
        }
    }
    return checksum;
}
let sum = mandelbrot(100, 100, 100);
"#,
            js_source: r#"
function mandelbrot(width, height, max_iter) {
    let checksum = 0;
    for (let y = 0; y < height; y++) {
        let ci = (y * 2.0 / height) - 1.0;
        for (let x = 0; x < width; x++) {
            let cr = (x * 3.0 / width) - 2.0;
            let zr = 0.0;
            let zi = 0.0;
            let iter = 0;
            while (zr * zr + zi * zi <= 4.0 && iter < max_iter) {
                let temp = zr * zr - zi * zi + cr;
                zi = 2.0 * zr * zi + ci;
                zr = temp;
                iter = iter + 1;
            }
            checksum += iter;
        }
    }
    return checksum;
}
let sum = mandelbrot(100, 100, 100);
"#,
        },
        BenchmarkDef {
            id: "data_pipeline",
            aliases: &["pipeline", "data", "records"],
            name: "Data Pipeline & Aggregation (Realistic Backend Workload)",
            description: "Generating 5,000 user records, filtering, transforming fields & computing aggregates",
            default_iterations: 20,
            er_source: r#"
fn runPipeline(count) {
    let users = [];
    for i in 0..count {
        let active = 0;
        if (i % 2 == 0) {
            active = 1;
        }
        let user = {
            id: i,
            name: "user_{i}",
            score: (i * 17) % 100,
            active: active
        };
        users.push(user);
    }
    
    let totalScore = 0;
    let activeCount = 0;
    let highScorers = [];
    for i in 0..count {
        let u = users[i];
        if (u.active == 1) {
            totalScore = totalScore + u.score;
            activeCount = activeCount + 1;
            if (u.score > 50) {
                highScorers.push(u);
            }
        }
    }
    let hs_len = highScorers.length;
    let res = {
        total: count,
        active: activeCount,
        sumScore: totalScore,
        highScoreCount: hs_len
    };
    return res;
}
let stats = runPipeline(5000);
"#,
            js_source: r#"
function runPipeline(count) {
    let users = [];
    for (let i = 0; i < count; i++) {
        let active = 0;
        if (i % 2 === 0) {
            active = 1;
        }
        let user = {
            id: i,
            name: `user_${i}`,
            score: (i * 17) % 100,
            active: active
        };
        users.push(user);
    }
    
    let totalScore = 0;
    let activeCount = 0;
    let highScorers = [];
    for (let i = 0; i < count; i++) {
        let u = users[i];
        if (u.active === 1) {
            totalScore = totalScore + u.score;
            activeCount = activeCount + 1;
            if (u.score > 50) {
                highScorers.push(u);
            }
        }
    }
    let hs_len = highScorers.length;
    let res = {
        total: count,
        active: activeCount,
        sumScore: totalScore,
        highScoreCount: hs_len
    };
    return res;
}
let stats = runPipeline(5000);
"#,
        },
    ]
}

#[allow(dead_code)]
#[derive(Default)]
struct BenchResultRow {
    bench_id: String,
    bench_name: String,
    vm_jit_avg: std::time::Duration,
    vm_interp_avg: std::time::Duration,
    bun_pure_avg: std::time::Duration,
    node_pure_avg: std::time::Duration,
    deno_pure_avg: std::time::Duration,
}

fn run_single_benchmark(bench: &BenchmarkDef, iterations: usize) -> BenchResultRow {
    let source = bench.er_source;
    let run_jit = std::env::var("ER_NO_JIT").is_err();

    fn noop_print(args: Vec<eronom::backend::Value>) -> eronom::backend::Value {
        let _ = args;
        eronom::backend::Value::null()
    }

    eprintln!("================================================================================");
    eprintln!("  Benchmark: ⚡ {} ({})", bench.name, bench.id);
    eprintln!("  Details:   {}", bench.description);
    eprintln!("  Runs:      {} iterations each", iterations);
    eprintln!("================================================================================");

    // Warmup (JIT)
    if run_jit {
        let tokens = eronom::frontend::lex(source);
        let mut parser = eronom::frontend::Parser::new(tokens);
        if let Ok(stmts) = parser.parse() {
            let compiler = eronom::backend::Compiler::new();
            if let Ok(function) = compiler.compile(&stmts) {
                let mut vm = eronom::backend::VM::new();
                vm.use_jit = true;
                vm.register_global("print", eronom::backend::Value::native_function(noop_print));
                vm.run(function).ok();
                eronom::backend::gc_free_all();
            }
        }
    }

    // Warmup (Interpreter)
    {
        let tokens = eronom::frontend::lex(source);
        let mut parser = eronom::frontend::Parser::new(tokens);
        if let Ok(stmts) = parser.parse() {
            let compiler = eronom::backend::Compiler::new();
            if let Ok(function) = compiler.compile(&stmts) {
                let mut vm = eronom::backend::VM::new();
                vm.use_jit = false;
                vm.register_global("print", eronom::backend::Value::native_function(noop_print));
                vm.run(function).ok();
                eronom::backend::gc_free_all();
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
        let tokens = eronom::frontend::lex(source);
        let mut parser = eronom::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = eronom::backend::Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let mut vm = eronom::backend::VM::new();
        vm.use_jit = false;
        vm.register_global("print", eronom::backend::Value::native_function(noop_print));
        vm.run(function).ok();
        eronom::backend::gc_free_all();
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
            let tokens = eronom::frontend::lex(source);
            let mut parser = eronom::frontend::Parser::new(tokens);
            let stmts = parser.parse().unwrap();
            let compiler = eronom::backend::Compiler::new();
            let function = compiler.compile(&stmts).unwrap();
            let mut vm = eronom::backend::VM::new();
            vm.use_jit = true;
            vm.register_global("print", eronom::backend::Value::native_function(noop_print));
            vm.run(function).ok();
            eronom::backend::gc_free_all();
        }
        let vm_jit_elapsed = start.elapsed();
        vm_jit_peak_heap = COUNTER.peak.load(Ordering::SeqCst).saturating_sub(baseline);
        vm_jit_elapsed / (iterations as u32)
    } else {
        std::time::Duration::from_nanos(0)
    };

    // --- Compile-only benchmark ---
    let baseline = COUNTER.allocated.load(Ordering::SeqCst);
    COUNTER.reset_peak();
    let start = Instant::now();
    for _ in 0..iterations {
        let tokens = eronom::frontend::lex(source);
        let mut parser = eronom::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = eronom::backend::Compiler::new();
        let _function = compiler.compile(&stmts).unwrap();
        eronom::backend::gc_free_all();
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

    // --- Bun benchmark ---
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

    // --- Node.js benchmark ---
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

    // --- Deno benchmark ---
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

    if bun_pure_avg.as_nanos() > 0 {
        print_row("Bun (pure run)", bun_pure_avg, bun_pure_rss, None);
    }
    if bun_cli_avg.as_nanos() > 0 {
        print_row("Bun (CLI exec)", bun_cli_avg, bun_cli_rss, None);
    }
    if node_pure_avg.as_nanos() > 0 {
        print_row("Node (pure run)", node_pure_avg, node_pure_rss, None);
    }
    if node_cli_avg.as_nanos() > 0 {
        print_row("Node (CLI exec)", node_cli_avg, node_cli_rss, None);
    }
    if deno_pure_avg.as_nanos() > 0 {
        print_row("Deno (pure run)", deno_pure_avg, deno_pure_rss, None);
    }
    if deno_cli_avg.as_nanos() > 0 {
        print_row("Deno (CLI exec)", deno_cli_avg, deno_cli_rss, None);
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

    let engine_name = if vm_jit_avg.as_nanos() > 0 { "JIT" } else { "Interpreter" };

    if bun_pure_avg.as_nanos() > 0 {
        if vm_avg < bun_pure_avg {
            let speedup = bun_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Bun (pure)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / bun_pure_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Bun (pure)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if bun_cli_avg.as_nanos() > 0 {
        if vm_avg < bun_cli_avg {
            let speedup = bun_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Bun (CLI)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / bun_cli_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Bun (CLI)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if node_pure_avg.as_nanos() > 0 {
        if vm_avg < node_pure_avg {
            let speedup = node_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Node (pure)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / node_pure_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Node (pure)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if node_cli_avg.as_nanos() > 0 {
        if vm_avg < node_cli_avg {
            let speedup = node_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Node (CLI)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / node_cli_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Node (CLI)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if deno_pure_avg.as_nanos() > 0 {
        if vm_avg < deno_pure_avg {
            let speedup = deno_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Deno (pure)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / deno_pure_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Deno (pure)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if deno_cli_avg.as_nanos() > 0 {
        if vm_avg < deno_cli_avg {
            let speedup = deno_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Deno (CLI)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / deno_cli_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Deno (CLI)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if has_footer {
        let footer_bottom = "─".repeat(59);
        eprintln!("└{}┘", footer_bottom);
    } else {
        eprintln!("└────────────────────┴────────────┴────────────┴────────────┘");
    }
    eprintln!();

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

fn print_consolidated_summary(results: &[BenchResultRow]) {
    eprintln!("╔═══════════════════════════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║                                  SUMMARY COMPARISON MATRIX                                        ║");
    eprintln!("╠═══════════════════╦══════════════╦══════════════╦══════════════╦══════════════╦══════════════╦════╣");
    eprintln!("║ Benchmark         ║ Eronom JIT   ║ Eronom Interp║ Node.js Pure ║ Bun Pure     ║ Deno Pure    ║ JIT║");
    eprintln!("╠═══════════════════╬══════════════╬══════════════╬══════════════╬══════════════╬══════════════╬════╣");

    for r in results {
        let jit_str = if r.vm_jit_avg.as_nanos() > 0 {
            format!("{:?}", r.vm_jit_avg)
        } else {
            "-".to_string()
        };
        let interp_str = format!("{:?}", r.vm_interp_avg);
        let node_str = if r.node_pure_avg.as_nanos() > 0 { format!("{:?}", r.node_pure_avg) } else { "-".to_string() };
        let bun_str = if r.bun_pure_avg.as_nanos() > 0 { format!("{:?}", r.bun_pure_avg) } else { "-".to_string() };
        let deno_str = if r.deno_pure_avg.as_nanos() > 0 { format!("{:?}", r.deno_pure_avg) } else { "-".to_string() };
        
        let speedup_str = if r.vm_jit_avg.as_nanos() > 0 {
            let s = r.vm_interp_avg.as_nanos() as f64 / r.vm_jit_avg.as_nanos() as f64;
            format!("{:.1}x", s)
        } else {
            "-".to_string()
        };

        eprintln!(
            "║ {:<17} ║ {:>12} ║ {:>12} ║ {:>12} ║ {:>12} ║ {:>12} ║ {:>2} ║",
            r.bench_id, jit_str, interp_str, node_str, bun_str, deno_str, speedup_str
        );
    }
    eprintln!("╚═══════════════════╩══════════════╩══════════════╩══════════════╩══════════════╩══════════════╩════╝");
    eprintln!();
}

fn main() {
    let suite = get_benchmark_suite();

    fn noop_print(args: Vec<eronom::backend::Value>) -> eronom::backend::Value {
        let _ = args;
        eronom::backend::Value::null()
    }

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
                        let tokens = eronom::frontend::lex(source);
                        let mut parser = eronom::frontend::Parser::new(tokens);
                        if let Ok(stmts) = parser.parse() {
                            let compiler = eronom::backend::Compiler::new();
                            if let Ok(function) = compiler.compile(&stmts) {
                                let mut vm = eronom::backend::VM::new();
                                vm.use_jit = false;
                                vm.register_global("print", eronom::backend::Value::native_function(noop_print));
                                vm.run(function).ok();
                                eronom::backend::gc_free_all();
                            }
                        }
                    }
                    return;
                }
                "--run-vm-jit" => {
                    for _ in 0..iters {
                        let tokens = eronom::frontend::lex(source);
                        let mut parser = eronom::frontend::Parser::new(tokens);
                        if let Ok(stmts) = parser.parse() {
                            let compiler = eronom::backend::Compiler::new();
                            if let Ok(function) = compiler.compile(&stmts) {
                                let mut vm = eronom::backend::VM::new();
                                vm.use_jit = true;
                                vm.register_global("print", eronom::backend::Value::native_function(noop_print));
                                vm.run(function).ok();
                                eronom::backend::gc_free_all();
                            }
                        }
                    }
                    return;
                }
                "--run-compiler" => {
                    for _ in 0..iters {
                        let tokens = eronom::frontend::lex(source);
                        let mut parser = eronom::frontend::Parser::new(tokens);
                        if let Ok(stmts) = parser.parse() {
                            let compiler = eronom::backend::Compiler::new();
                            let _ = compiler.compile(&stmts);
                            eronom::backend::gc_free_all();
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

    println!("Size of Value: {} bytes", std::mem::size_of::<eronom::backend::Value>());
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
