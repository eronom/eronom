/// Benchmark: VM-based vs Legacy tree-walking interpreter
/// Run with: cargo run --bin bench

use std::rc::Rc;
use std::time::Instant;

fn main() {
    let source = r#"
let x = 0

for i in 1..1000 {
    let val = i + i
    if (val > 500) {
        let dummy = val + 1
    } else {
        let dummy = val + 2
    }
}

for j in 1..100 {
    print("{j}")
}
"#;

    let iterations = 10;

    println!("=== ER Language Benchmark ===");
    println!("Running {} iterations each\n", iterations);

    // --- Legacy (tree-walking) benchmark ---
    println!("--- Legacy Tree-Walking Interpreter ---");
    // Warmup
    er::legacy::run_source(source).ok();

    let start = Instant::now();
    for _ in 0..iterations {
        er::legacy::run_source(source).ok();
    }
    let legacy_elapsed = start.elapsed();
    let legacy_avg = legacy_elapsed / iterations;
    println!("  Total:   {:?}", legacy_elapsed);
    println!("  Average: {:?}\n", legacy_avg);

    // --- VM-based benchmark ---
    println!("--- VM-Based (Bytecode) Interpreter ---");

    fn native_print(args: Vec<er::backend::Value>) -> er::backend::Value {
        let mut outputs = Vec::new();
        for arg in args {
            outputs.push(arg.to_string());
        }
        println!("{}", outputs.join(" "));
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
        vm.register_global("print", er::backend::Value::NativeFunction(native_print));
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
        vm.register_global("print", er::backend::Value::NativeFunction(native_print));
        vm.run(Rc::new(function)).ok();
    }
    let vm_elapsed = start.elapsed();
    let vm_avg = vm_elapsed / iterations;
    println!("  Total:   {:?}", vm_elapsed);
    println!("  Average: {:?}\n", vm_avg);

    // --- Compile-only benchmark ---
    println!("--- VM Compile Only (no execution) ---");
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
    println!("  Total:   {:?}", compile_elapsed);
    println!("  Average: {:?}\n", compile_avg);

    // --- Summary ---
    println!("========== RESULTS ==========");
    println!("  Legacy avg:       {:?}", legacy_avg);
    println!("  VM avg:           {:?}", vm_avg);
    println!("  Compile-only avg: {:?}", compile_avg);

    if vm_avg < legacy_avg {
        let speedup = legacy_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
        println!("\n  ✅ VM is {:.2}x FASTER than legacy", speedup);
    } else {
        let slowdown = vm_avg.as_nanos() as f64 / legacy_avg.as_nanos() as f64;
        println!("\n  ⚠️  VM is {:.2}x SLOWER than legacy", slowdown);
    }
    println!("=============================");
}
