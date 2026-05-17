/// Benchmark: VM-based vs Legacy tree-walking interpreter
/// Run with: cargo run --release --bin bench -p er

use std::rc::Rc;
use std::time::Instant;

fn main() {
    // Pure compute — NO print calls (I/O dominates timing and floods the terminal)
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

    eprintln!("=== ER Language Benchmark ===");
    eprintln!("  Script: 10,000 loop iterations with arithmetic + conditionals");
    eprintln!("  Runs:   {} iterations each\n", iterations);

    // --- Legacy (tree-walking) benchmark ---
    er::legacy::run_source(source).ok(); // warmup

    let start = Instant::now();
    for _ in 0..iterations {
        er::legacy::run_source(source).ok();
    }
    let legacy_elapsed = start.elapsed();
    let legacy_avg = legacy_elapsed / iterations;

    // --- VM-based benchmark ---
    fn noop_print(args: Vec<er::backend::Value>) -> er::backend::Value {
        let _ = args;
        er::backend::Value::Null
    }

    // Warmup
    {
        let tokens = er::frontend::lex(source);
        let mut parser = er::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = er::backend::Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let mut vm = er::backend::VM::new();
        vm.register_global("print", er::backend::Value::NativeFunction(noop_print));
        vm.run(Rc::new(function)).ok();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let tokens = er::frontend::lex(source);
        let mut parser = er::frontend::Parser::new(tokens);
        let stmts = parser.parse().unwrap();
        let compiler = er::backend::Compiler::new();
        let function = compiler.compile(&stmts).unwrap();
        let mut vm = er::backend::VM::new();
        vm.register_global("print", er::backend::Value::NativeFunction(noop_print));
        vm.run(Rc::new(function)).ok();
    }
    let vm_elapsed = start.elapsed();
    let vm_avg = vm_elapsed / iterations;

    // --- Compile-only benchmark ---
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

    // --- Results ---
    eprintln!("┌──────────────────────────────────────────┐");
    eprintln!("│          ER BENCHMARK RESULTS            │");
    eprintln!("├──────────────────────────────────────────┤");
    eprintln!("│  Legacy (tree-walk)  │  avg {:>12?}  │", legacy_avg);
    eprintln!("│  VM (bytecode)       │  avg {:>12?}  │", vm_avg);
    eprintln!("│  Compile only        │  avg {:>12?}  │", compile_avg);
    eprintln!("├──────────────────────────────────────────┤");

    if vm_avg < legacy_avg {
        let speedup = legacy_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
        eprintln!("│  ✅ VM is {:.2}x FASTER than legacy       │", speedup);
    } else {
        let slowdown = vm_avg.as_nanos() as f64 / legacy_avg.as_nanos() as f64;
        eprintln!("│  ⚠️  VM is {:.2}x SLOWER than legacy       │", slowdown);
    }
    eprintln!("└──────────────────────────────────────────┘");
}
