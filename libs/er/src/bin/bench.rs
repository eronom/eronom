/// Benchmark: VM-based vs Legacy tree-walking interpreter
/// Run with: cargo run --release --bin bench -p er
use std::time::Instant;

fn main() {
    println!("Size of Value: {}", std::mem::size_of::<er::backend::Value>());
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

    // --- VM-based benchmark ---
    fn noop_print(args: Vec<er::backend::Value>) -> er::backend::Value {
        let _ = args;
        er::backend::Value::null()
    }

    // Warmup (JIT)
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

    // Benchmark VM (JIT)
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
    let vm_jit_avg = vm_jit_elapsed / iterations;

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
    if let Ok(output) = std::process::Command::new("lua")
        .arg("-e")
        .arg(&lua_pure_source)
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Ok(secs) = stdout.trim().parse::<f64>() {
            lua_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
        }
    }

    let mut lua_cli_avg = std::time::Duration::from_secs(0);
    if let Ok(_) = std::process::Command::new("lua").arg("-v").output() {
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
        }
    }

    // --- Luau benchmark ---
    let run_luau_file =
        |args: &[&str], source: &str| -> Result<String, Box<dyn std::error::Error>> {
            let temp_filename = "temp_bench_luau.lua";
            std::fs::write(temp_filename, source)?;
            let output = std::process::Command::new("luau")
                .args(args)
                .arg(temp_filename)
                .output();
            let _ = std::fs::remove_file(temp_filename);
            let output = output?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            } else {
                Err("Command failed".into())
            }
        };

    let mut luau_pure_avg = std::time::Duration::from_secs(0);
    if let Ok(output) = run_luau_file(&[], &lua_pure_source) {
        if let Ok(secs) = output.trim().parse::<f64>() {
            luau_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
        }
    }

    let mut luau_codegen_pure_avg = std::time::Duration::from_secs(0);
    if let Ok(output) = run_luau_file(&["--codegen"], &lua_pure_source) {
        if let Ok(secs) = output.trim().parse::<f64>() {
            luau_codegen_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
        }
    }

    let mut luau_cli_avg = std::time::Duration::from_secs(0);
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
            let _ = std::fs::remove_file(temp_filename);
            if success {
                luau_cli_avg = start.elapsed() / iterations;
            }
        }
    }

    let mut luau_codegen_cli_avg = std::time::Duration::from_secs(0);
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
            let _ = std::fs::remove_file(temp_filename);
            if success {
                luau_codegen_cli_avg = start.elapsed() / iterations;
            }
        }
    }

    // --- Node benchmark ---
    let run_node_file =
        |args: &[&str], source: &str| -> Result<String, Box<dyn std::error::Error>> {
            let temp_filename = "temp_bench_node.js";
            std::fs::write(temp_filename, source)?;
            let output = std::process::Command::new("node")
                .args(args)
                .arg(temp_filename)
                .output();
            let _ = std::fs::remove_file(temp_filename);
            let output = output?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
    if let Ok(output) = run_node_file(&[], &node_pure_source) {
        if let Ok(secs) = output.trim().parse::<f64>() {
            node_pure_avg = std::time::Duration::from_secs_f64(secs / iterations as f64);
        }
    }

    let mut node_cli_avg = std::time::Duration::from_secs(0);
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
            let _ = std::fs::remove_file(temp_filename);
            if success {
                node_cli_avg = start.elapsed() / iterations;
            }
        }
    }

    // --- Results ---
    eprintln!("┌──────────────────────────────────────────┐");
    eprintln!("│          ER BENCHMARK RESULTS            │");
    eprintln!("├──────────────────────────────────────────┤");
    eprintln!("│  VM (Interpreter)    │  avg {:>12?}  │", vm_interpreter_avg);
    eprintln!("│  VM (JIT)            │  avg {:>12?}  │", vm_jit_avg);
    eprintln!("│  Compile only        │  avg {:>12?}  │", compile_avg);
    if lua_pure_avg.as_nanos() > 0 {
        eprintln!("│  Lua (pure run)      │  avg {:>12?}  │", lua_pure_avg);
    }
    if lua_cli_avg.as_nanos() > 0 {
        eprintln!("│  Lua (external CLI)  │  avg {:>12?}  │", lua_cli_avg);
    }
    if luau_pure_avg.as_nanos() > 0 {
        eprintln!("│  Luau (pure run)     │  avg {:>12?}  │", luau_pure_avg);
    }
    if luau_codegen_pure_avg.as_nanos() > 0 {
        eprintln!(
            "│  Luau+Codegen (pure) │  avg {:>12?}  │",
            luau_codegen_pure_avg
        );
    }
    if luau_cli_avg.as_nanos() > 0 {
        eprintln!("│  Luau (external CLI) │  avg {:>12?}  │", luau_cli_avg);
    }
    if luau_codegen_cli_avg.as_nanos() > 0 {
        eprintln!(
            "│  Luau+Codegen (CLI)  │  avg {:>12?}  │",
            luau_codegen_cli_avg
        );
    }
    if node_pure_avg.as_nanos() > 0 {
        eprintln!("│  Node (pure run)     │  avg {:>12?}  │", node_pure_avg);
    }
    if node_cli_avg.as_nanos() > 0 {
        eprintln!("│  Node (external CLI) │  avg {:>12?}  │", node_cli_avg);
    }
    eprintln!("├──────────────────────────────────────────┤");

    let jit_speedup = vm_interpreter_avg.as_nanos() as f64 / vm_jit_avg.as_nanos() as f64;
    let jit_text = format!("JIT is {:.2}x FASTER than Interpreter", jit_speedup);
    eprintln!("│  🚀 {:<37}│", jit_text);
    eprintln!("├──────────────────────────────────────────┤");

    let vm_avg = vm_jit_avg; // compare JIT VM against others

    if lua_pure_avg.as_nanos() > 0 {
        if vm_avg < lua_pure_avg {
            let speedup = lua_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Lua (pure)", speedup);
            eprintln!("│  ✅ {:<37}│", text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / lua_pure_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Lua (pure)", slowdown);
            eprintln!("│  ⚠️  {:<36}│", text);
        }
    }

    if lua_cli_avg.as_nanos() > 0 {
        if vm_avg < lua_cli_avg {
            let speedup = lua_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Lua (CLI)", speedup);
            eprintln!("│  ✅ {:<37}│", text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / lua_cli_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Lua (CLI)", slowdown);
            eprintln!("│  ⚠️  {:<36}│", text);
        }
    }

    if luau_pure_avg.as_nanos() > 0 {
        if vm_avg < luau_pure_avg {
            let speedup = luau_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Luau (pure)", speedup);
            eprintln!("│  ✅ {:<37}│", text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / luau_pure_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Luau (pure)", slowdown);
            eprintln!("│  ⚠️  {:<36}│", text);
        }
    }

    if luau_codegen_pure_avg.as_nanos() > 0 {
        if vm_avg < luau_codegen_pure_avg {
            let speedup = luau_codegen_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Luau (native)", speedup);
            eprintln!("│  ✅ {:<37}│", text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / luau_codegen_pure_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Luau (native)", slowdown);
            eprintln!("│  ⚠️  {:<36}│", text);
        }
    }

    if luau_cli_avg.as_nanos() > 0 {
        if vm_avg < luau_cli_avg {
            let speedup = luau_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Luau (CLI)", speedup);
            eprintln!("│  ✅ {:<37}│", text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / luau_cli_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Luau (CLI)", slowdown);
            eprintln!("│  ⚠️  {:<36}│", text);
        }
    }

    if luau_codegen_cli_avg.as_nanos() > 0 {
        if vm_avg < luau_codegen_cli_avg {
            let speedup = luau_codegen_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Luau (cg CLI)", speedup);
            eprintln!("│  ✅ {:<37}│", text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / luau_codegen_cli_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Luau (cg CLI)", slowdown);
            eprintln!("│  ⚠️  {:<36}│", text);
        }
    }

    if node_pure_avg.as_nanos() > 0 {
        if vm_avg < node_pure_avg {
            let speedup = node_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Node (pure)", speedup);
            eprintln!("│  ✅ {:<37}│", text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / node_pure_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Node (pure)", slowdown);
            eprintln!("│  ⚠️  {:<36}│", text);
        }
    }

    if node_cli_avg.as_nanos() > 0 {
        if vm_avg < node_cli_avg {
            let speedup = node_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x FASTER than Node (CLI)", speedup);
            eprintln!("│  ✅ {:<37}│", text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / node_cli_avg.as_nanos() as f64;
            let text = format!("VM is {:.2}x SLOWER than Node (CLI)", slowdown);
            eprintln!("│  ⚠️  {:<36}│", text);
        }
    }

    eprintln!("└──────────────────────────────────────────┘");
}
