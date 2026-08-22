pub use eronom::vm as backend;
pub use eronom::frontend;
pub use eronom::jit;

use backend::{Compiler, VM, Value};
use frontend::{Expr, LiteralValue, Stmt};

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

#[repr(C)]
struct Tm {
    tm_sec: std::ffi::c_int,
    tm_min: std::ffi::c_int,
    tm_hour: std::ffi::c_int,
    tm_mday: std::ffi::c_int,
    tm_mon: std::ffi::c_int,
    tm_year: std::ffi::c_int,
    tm_wday: std::ffi::c_int,
    tm_yday: std::ffi::c_int,
    tm_isdst: std::ffi::c_int,
    #[cfg(unix)]
    tm_gmtoff: std::ffi::c_long,
    #[cfg(unix)]
    tm_zone: *const std::ffi::c_char,
}

#[cfg(unix)]
unsafe extern "C" {
    fn time(time: *mut std::ffi::c_long) -> std::ffi::c_long;
    fn localtime_r(timep: *const std::ffi::c_long, result: *mut Tm) -> *mut Tm;
}

#[cfg(windows)]
unsafe extern "C" {
    fn _time64(time: *mut i64) -> i64;
    fn _localtime64_s(result: *mut Tm, timep: *const i64) -> std::ffi::c_int;
}

fn get_local_time_string() -> String {
    unsafe {
        let mut tm_val = std::mem::zeroed::<Tm>();
        #[cfg(unix)]
        {
            let mut t: std::ffi::c_long = 0;
            time(&mut t);
            localtime_r(&t, &mut tm_val);
        }
        #[cfg(windows)]
        {
            let mut t: i64 = 0;
            _time64(&mut t);
            _localtime64_s(&mut tm_val, &t);
        }
        let hour = tm_val.tm_hour;
        let min = tm_val.tm_min;
        let sec = tm_val.tm_sec;
        let am_pm = if hour >= 12 { "PM" } else { "AM" };
        let display_hour = if hour == 0 {
            12
        } else if hour > 12 {
            hour - 12
        } else {
            hour
        };
        format!("{:02}:{:02}:{:02} {}", display_hour, min, sec, am_pm)
    }
}

fn native_now(_args: Vec<Value>) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    Value::number(now as f64)
}

fn native_local_time_string(_args: Vec<Value>) -> Value {
    let time_str = get_local_time_string();
    let ptr = backend::gc::get_or_create_string(&time_str);
    Value::string(ptr)
}

fn value_to_json(val: Value) -> String {
    if val.is_null() {
        "null".to_string()
    } else if val.is_boolean() {
        val.as_boolean().to_string()
    } else if val.is_number() {
        val.as_number().to_string()
    } else if val.is_string() {
        format!("\"{}\"", val.as_str().unwrap_or("").replace("\"", "\\\""))
    } else if val.is_array() {
        unsafe {
            match &(*val.as_gc_ptr()).data {
                backend::GcData::Array(arr) => {
                    let items: Vec<String> = arr.iter().map(|&v| value_to_json(v)).collect();
                    format!("[{}]", items.join(","))
                }
                _ => "[]".to_string(),
            }
        }
    } else if val.is_object() {
        unsafe {
            match &(*val.as_gc_ptr()).data {
                backend::GcData::Object(obj) => {
                    let mut items = Vec::new();
                    for (k, &v) in obj {
                        let s = match &(*k.0.as_gc_ptr()).data {
                            backend::GcData::String(s) => s.as_ref(),
                            _ => continue,
                        };
                        items.push(format!("\"{}\":{}", s, value_to_json(v)));
                    }
                    format!("{{{}}}", items.join(","))
                }
                backend::GcData::Struct(s) => {
                    let mut items = Vec::new();
                    for (map_key, &idx) in &s.descriptor.field_indices {
                        let name = map_key.0.as_str().unwrap_or("");
                        items.push(format!("\"{}\":{}", name, value_to_json(s.fields[idx])));
                    }
                    format!("{{{}}}", items.join(","))
                }
                _ => "{}".to_string(),
            }
        }
    } else {
        "null".to_string()
    }
}

fn native_render(args: Vec<Value>) -> Value {
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
            match &(*params_val.as_gc_ptr()).data {
                backend::GcData::Object(map) => {
                    for (k, v) in map {
                        if let Some(key_str) = k.0.as_str() {
                            let val_str = if v.is_string() {
                                v.as_str().unwrap_or("").to_string()
                            } else if v.is_array() || v.is_object() {
                                value_to_json(*v)
                            } else {
                                v.to_string()
                            };
                            params_map.insert(key_str.to_string(), val_str);
                        }
                    }
                }
                backend::GcData::Struct(s) => {
                    for (map_key, &idx) in &s.descriptor.field_indices {
                        let name = map_key.0.as_str().unwrap_or("");
                        let v = s.fields[idx];
                        let val_str = if v.is_string() {
                            v.as_str().unwrap_or("").to_string()
                        } else if v.is_array() || v.is_object() {
                            value_to_json(v)
                        } else {
                            v.to_string()
                        };
                        params_map.insert(name.to_string(), val_str);
                    }
                }
                _ => {}
            }
        }
    }
    
    let path = std::path::Path::new(file_path);
    let mut resolved_path = if path.is_relative() {
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
        if let Some(first_part) = path.iter().next().and_then(|p| p.to_str()) {
            if let Ok(stripped) = path.strip_prefix(first_part) {
                let fallback = if stripped.is_relative() {
                    if let Some(script_path) = backend::er_http::get_target_script_path() {
                        if let Some(parent) = std::path::Path::new(&script_path).parent() {
                            parent.join(stripped)
                        } else {
                            stripped.to_path_buf()
                        }
                    } else {
                        stripped.to_path_buf()
                    }
                } else {
                    stripped.to_path_buf()
                };
                if fallback.exists() {
                    resolved_path = fallback;
                }
            }
        }
    }
    
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

    if resolved_path.extension().map_or(false, |ext| ext == "html") {
        let mut final_content = content;
        if !params_map.is_empty() {
            let mut params_js = String::from("window.__erm_params = {");
            for (k, v) in &params_map {
                params_js.push_str(&format!("{}: \"{}\",", k, v.replace("\"", "\\\"")));
            }
            params_js.push_str("};");
            final_content = final_content.replace("window.__erm_params = {};", &params_js);
        }
        let ptr = backend::gc::get_or_create_string(&final_content);
        return Value::string(ptr);
    }
    
    match eronom::compiler::process_erm_component(resolved_path.to_str().unwrap_or(&base_dir), &content, true, &params_map) {
        Ok(html) => {
            let ptr = backend::gc::get_or_create_string(&html);
            Value::string(ptr)
        }
        Err(e) => {
            eprintln!("[render] Compiler error: {:?}", e);
            Value::null()
        }
    }
}


fn has_http_import(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        if has_http_import_in_stmt(stmt) {
            return true;
        }
    }
    false
}

fn has_http_import_in_stmt(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::VarDecl(_, _, _, _, loc) => {
            if loc.file_path.ends_with("std/http.er") {
                return true;
            }
        }
        Stmt::Struct(_, _, _, _, loc) => {
            if loc.file_path.ends_with("std/http.er") {
                return true;
            }
        }
        Stmt::Interface(_, _, _, loc) => {
            if loc.file_path.ends_with("std/http.er") {
                return true;
            }
        }
        Stmt::Block(inner) => {
            if has_http_import(inner) {
                return true;
            }
        }
        Stmt::If(_, then_stmt, else_stmt) => {
            if has_http_import_in_stmt(then_stmt) {
                return true;
            }
            if let Some(e) = else_stmt {
                if has_http_import_in_stmt(e) {
                    return true;
                }
            }
        }
        Stmt::While(_, body) => {
            if has_http_import_in_stmt(body) {
                return true;
            }
        }
        Stmt::Try(try_body, catch_clause, finally_body) => {
            if has_http_import_in_stmt(try_body) {
                return true;
            }
            if let Some((_, catch_b)) = catch_clause {
                if has_http_import_in_stmt(catch_b) {
                    return true;
                }
            }
            if let Some(finally_b) = finally_body {
                if has_http_import_in_stmt(finally_b) {
                    return true;
                }
            }
        }
        Stmt::Switch(_, cases, default_body) => {
            for c in cases {
                if has_http_import_in_stmt(&c.body) {
                    return true;
                }
            }
            if let Some(def_b) = default_body {
                if has_http_import_in_stmt(def_b) {
                    return true;
                }
            }
        }
        Stmt::Export(inner) => {
            if has_http_import_in_stmt(inner) {
                return true;
            }
        }
        _ => {}
    }
    false
}

fn find_listen_port_in_expr(expr: &Expr) -> Option<i32> {
    match expr {
        Expr::Call(callee, args) => {
            if let Expr::Get(_, method) = callee.as_ref() {
                if method == "listen" && !args.is_empty() {
                    if let Expr::Literal(LiteralValue::Number(n)) = &args[0] {
                        return Some(*n as i32);
                    }
                }
            }
            for arg in args {
                if let Some(port) = find_listen_port_in_expr(arg) {
                    return Some(port);
                }
            }
            None
        }
        Expr::Assign(_, val, _) => find_listen_port_in_expr(val),
        Expr::Binary(left, _, right) => {
            find_listen_port_in_expr(left).or_else(|| find_listen_port_in_expr(right))
        }
        Expr::Logical(left, _, right) => {
            find_listen_port_in_expr(left).or_else(|| find_listen_port_in_expr(right))
        }
        Expr::Get(obj, _) => return find_listen_port_in_expr(obj),
        Expr::Set(target, _, val) => {
            find_listen_port_in_expr(target).or_else(|| find_listen_port_in_expr(val))
        }
        Expr::Array(elements) => {
            for el in elements {
                if let Some(port) = find_listen_port_in_expr(el) {
                    return Some(port);
                }
            }
            None
        }
        Expr::Object(entries) => {
            for (_, val) in entries {
                if let Some(port) = find_listen_port_in_expr(val) {
                    return Some(port);
                }
            }
            None
        }
        Expr::Function(_, body) => find_listen_port_in_stmt(body),
        Expr::GetIndex(target, index) => {
            find_listen_port_in_expr(target).or_else(|| find_listen_port_in_expr(index))
        }
        Expr::SetIndex(target, index, val) => find_listen_port_in_expr(target)
            .or_else(|| find_listen_port_in_expr(index))
            .or_else(|| find_listen_port_in_expr(val)),
        Expr::StructInst(_, fields, _) => {
            for (_, val) in fields {
                if let Some(port) = find_listen_port_in_expr(val) {
                    return Some(port);
                }
            }
            None
        }
        Expr::Spawn(inner) => find_listen_port_in_expr(inner),
        _ => None,
    }
}

fn find_listen_port_in_stmt(stmt: &Stmt) -> Option<i32> {
    match stmt {
        Stmt::Expr(expr) => find_listen_port_in_expr(expr),
        Stmt::Print(expr) => find_listen_port_in_expr(expr),
        Stmt::VarDecl(_, _, _, init, _) => find_listen_port_in_expr(init),
        Stmt::Block(stmts) => {
            for s in stmts {
                if let Some(port) = find_listen_port_in_stmt(s) {
                    return Some(port);
                }
            }
            None
        }
        Stmt::If(cond, then_stmt, else_stmt) => {
            if let Some(p) = find_listen_port_in_expr(cond) {
                return Some(p);
            }
            if let Some(p) = find_listen_port_in_stmt(then_stmt) {
                return Some(p);
            }
            if let Some(e) = else_stmt {
                if let Some(p) = find_listen_port_in_stmt(e) {
                    return Some(p);
                }
            }
            None
        }
        Stmt::While(cond, body) => {
            if let Some(p) = find_listen_port_in_expr(cond) {
                return Some(p);
            }
            if let Some(p) = find_listen_port_in_stmt(body) {
                return Some(p);
            }
            None
        }
        Stmt::For(_, start, end, body) => {
            if let Some(p) = find_listen_port_in_expr(start) {
                return Some(p);
            }
            if let Some(p) = find_listen_port_in_expr(end) {
                return Some(p);
            }
            if let Some(p) = find_listen_port_in_stmt(body) {
                return Some(p);
            }
            None
        }
        Stmt::Throw(expr) => find_listen_port_in_expr(expr),
        Stmt::Try(try_body, catch_clause, finally_body) => {
            if let Some(p) = find_listen_port_in_stmt(try_body) {
                return Some(p);
            }
            if let Some((_, catch_b)) = catch_clause {
                if let Some(p) = find_listen_port_in_stmt(catch_b) {
                    return Some(p);
                }
            }
            if let Some(finally_b) = finally_body {
                if let Some(p) = find_listen_port_in_stmt(finally_b) {
                    return Some(p);
                }
            }
            None
        }
        Stmt::Switch(target, cases, default_body) => {
            if let Some(p) = find_listen_port_in_expr(target) {
                return Some(p);
            }
            for c in cases {
                for v in &c.values {
                    if let Some(p) = find_listen_port_in_expr(v) {
                        return Some(p);
                    }
                }
                if let Some(p) = find_listen_port_in_stmt(&c.body) {
                    return Some(p);
                }
            }
            if let Some(def_b) = default_body {
                if let Some(p) = find_listen_port_in_stmt(def_b) {
                    return Some(p);
                }
            }
            None
        }
        Stmt::Return(expr_opt) => {
            if let Some(expr) = expr_opt {
                find_listen_port_in_expr(expr)
            } else {
                None
            }
        }
        Stmt::Export(inner) => find_listen_port_in_stmt(inner),
        _ => None,
    }
}

fn find_listen_port(stmts: &[Stmt]) -> Option<i32> {
    for s in stmts {
        if let Some(port) = find_listen_port_in_stmt(s) {
            return Some(port);
        }
    }
    None
}

pub fn run_file(path: &str) -> anyhow::Result<()> {
    let _guard = GcGuard;
    let path_buf = std::path::PathBuf::from(path);
    if !path_buf.exists() {
        anyhow::bail!("File not found: {}", path);
    }

    let stmts = match frontend::parse_and_resolve_imports(&path_buf) {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Compile/Import error: {}", e),
    };

    if has_http_import(&stmts) {
        let port = find_listen_port(&stmts).unwrap_or(3000);
        backend::er_http::LISTEN_PORT.with(|p| p.set(Some(port)));
    }

    let compiler = Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let mut vm = VM::new();
    vm.register_global("print", Value::native_function(native_print));
    vm.register_global("router", Value::native_function(backend::er_http::native_route));
    vm.register_global("render", Value::native_function(native_render));
    vm.register_global("fetch", Value::native_function(backend::er_http::native_fetch));
    vm.register_global("setTimeout", Value::native_function(backend::er_http::native_set_timeout));
    vm.register_global("fetchSync", Value::native_function(backend::er_http::native_fetch_sync));
    vm.register_global("fetchEvented", Value::native_function(backend::er_http::native_fetch_evented));
    vm.register_global("futureAwait", Value::native_function(backend::er_http::native_future_await));
    vm.register_global("arrayLen", Value::native_function(backend::er_http::native_array_len));
    vm.register_global("arrayPush", Value::native_function(backend::er_http::native_array_push));
    vm.register_global("sleep", Value::native_function(backend::er_http::native_sleep));
    vm.register_global("createPromisePair", Value::native_function(backend::er_http::native_create_promise_pair));
    vm.register_global("setIoMode", Value::native_function(backend::er_http::native_set_io_mode));
    vm.register_global("getIoMode", Value::native_function(backend::er_http::native_get_io_mode));
    vm.register_global("now", Value::native_function(native_now));
    vm.register_global("localTimeString", Value::native_function(native_local_time_string));
    backend::er_http::register_eronom_file_api(&mut vm).unwrap();
    backend::er_http::set_target_script_path(path);
    let main_path = std::path::Path::new(path);
    if let Some(parent_dir) = main_path.parent() {
        let toml_path = parent_dir.join("eronom.toml");
        if toml_path.exists() {
            if let Ok(toml_content) = std::fs::read_to_string(&toml_path) {
                if let Ok(toml_val) = toml::from_str::<toml::Value>(&toml_content) {
                    if let Ok(json_val) = serde_json::to_value(toml_val) {
                        let config_val = backend::gc::json_to_value(json_val);
                        vm.register_global("config", config_val);
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
    use clap::Parser;
    let cli = eronom::cli::Cli::parse();

    if let Some(file_path) = cli.file {
        if !file_path.to_string_lossy().ends_with(".er") && !file_path.exists() {
            eprintln!("Error: Unknown command or file: {}", file_path.display());
            std::process::exit(1);
        }
        if let Err(e) = run_file(file_path.to_str().unwrap_or("")) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    } else if let Some(cmd) = cli.command {
        if let Err(e) = eronom::cli::run_command(cmd) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    } else {
        use clap::CommandFactory;
        let mut cmd = eronom::cli::Cli::command();
        let _ = cmd.print_help();
        println!();
        std::process::exit(1);
    }
}
