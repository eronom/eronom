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

fn native_render_erm(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let file_path_val = args[0];
    let params_val = args[1];
    
    let file_path = match file_path_val.as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    
    let mut params_map = std::collections::HashMap::new();
    if params_val.is_object() {
        unsafe {
            if let backend::GcData::Object(map) = &(*params_val.as_gc_ptr()).data {
                for (k, v) in map {
                    if let Some(key_str) = k.0.as_str() {
                        let val_str = if let Some(s) = v.as_str() {
                            s.to_string()
                        } else {
                            v.to_string()
                        };
                        params_map.insert(key_str.to_string(), val_str);
                    }
                }
            }
        }
    }
    
    let path = std::path::Path::new(file_path);
    let resolved_path = if path.is_relative() {
        if let Some(script_path) = backend::er_http::get_target_script_path() {
            if let Some(parent) = std::path::Path::new(&script_path).parent() {
                parent.join(path)
            } else {
                path.to_path_buf()
            }
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };
    
    if !resolved_path.exists() {
        return Value::null();
    }
    
    let base_dir = match resolved_path.parent() {
        Some(p) => p.to_string_lossy().to_string(),
        None => "".to_string(),
    };
    
    let content = match std::fs::read_to_string(&resolved_path) {
        Ok(c) => c,
        Err(_) => return Value::null(),
    };
    
    match eronom::compiler::process_erm_component(&base_dir, &content, true, &params_map) {
        Ok(html) => {
            let ptr = backend::gc::get_or_create_string(&html);
            Value::string(ptr)
        }
        Err(e) => {
            eprintln!("[renderErm] Compiler error: {:?}", e);
            Value::null()
        }
    }
}


pub fn run_file(path: &str) -> anyhow::Result<()> {
    let _guard = GcGuard;
    let path_buf = std::path::PathBuf::from(path);
    if !path_buf.exists() {
        return Ok(());
    }

    let stmts = match frontend::parse_and_resolve_imports(&path_buf) {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Compile/Import error: {}", e),
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
    vm.register_global("renderErm", Value::native_function(native_render_erm));
    vm.register_global("fetch", Value::native_function(backend::er_http::native_fetch));
    vm.register_global("setTimeout", Value::native_function(backend::er_http::native_set_timeout));
    vm.register_global("fetchAsync", Value::native_function(backend::er_http::native_fetch_async));
    vm.register_global("fetchSync", Value::native_function(backend::er_http::native_fetch_sync));
    vm.register_global("fetchEvented", Value::native_function(backend::er_http::native_fetch_evented));
    vm.register_global("futureAwait", Value::native_function(backend::er_http::native_future_await));
    vm.register_global("arrayLen", Value::native_function(backend::er_http::native_array_len));
    vm.register_global("sleep", Value::native_function(backend::er_http::native_sleep));
    vm.register_global("createPromisePair", Value::native_function(backend::er_http::native_create_promise_pair));
    vm.register_global("setIoMode", Value::native_function(backend::er_http::native_set_io_mode));
    vm.register_global("getIoMode", Value::native_function(backend::er_http::native_get_io_mode));
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

    if let Err(e) = vm.run_event_loop() {
        anyhow::bail!("VM Event loop error: {}", e);
    }

    backend::er_http::start_http_server_if_needed(&mut vm);

    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.er> OR {} [dev|build|start|init] [options]", args[0], args[0]);
        std::process::exit(1);
    }
    let first_arg = &args[1];
    if matches!(first_arg.as_str(), "build" | "dev" | "start" | "init") {
        if let Err(e) = eronom::cli::run_cli(args) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    } else {
        if let Err(e) = run_file(first_arg) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
