pub mod vm;
pub use vm as backend;
pub mod frontend;
pub mod jit;

use backend::{Compiler, VM, Value};
use frontend::{Parser, lex};

struct GcGuard;
impl Drop for GcGuard {
    fn drop(&mut self) {
        backend::gc_free_all();
    }
}

fn native_print(args: Vec<Value>) -> Value {
    let mut outputs = Vec::new();
    for arg in args {
        outputs.push(arg.to_string());
    }
    println!("{}", outputs.join(" "));
    Value::null()
}

pub fn run_file(path: &str) -> anyhow::Result<()> {
    let _guard = GcGuard;
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    let tokens = lex(&content);
    let mut parser = Parser::new(tokens);
    let stmts = match parser.parse() {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Parse error: {}", e),
    };

    let compiler = Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => anyhow::bail!("Compile error: {}", e),
    };

    let mut vm = VM::new();
    if vm.use_jit {
        eprintln!("[VM] Running with JIT compiler enabled");
    } else {
        eprintln!("[VM] Running with bytecode interpreter (no JIT)");
    }
    vm.register_global("print", Value::native_function(native_print));
    vm.register_global("route", Value::native_function(backend::er_http::native_route));
    backend::er_http::set_target_script_path(path);

    let main_path = std::path::Path::new(path);
    if let Some(parent_dir) = main_path.parent() {
        let config_path = parent_dir.join("config.er");
        if config_path.exists() {
            if let Ok(config_content) = std::fs::read_to_string(&config_path) {
                let config_tokens = lex(&config_content);
                let mut config_parser = Parser::new(config_tokens);
                if let Ok(config_stmts) = config_parser.parse() {
                    let config_compiler = Compiler::new();
                    if let Ok(config_func) = config_compiler.compile(&config_stmts) {
                        if let Err(e) = vm.run(config_func) {
                            eprintln!("[Warning] Failed to run config.er: {}", e);
                        }
                    }
                }
            }
        }
    }

    if let Err(e) = vm.run(function) {
        anyhow::bail!("VM Runtime error: {}", e);
    }

    backend::er_http::start_http_server_if_needed(&mut vm);

    Ok(())
}
