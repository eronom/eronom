#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use eronom::frontend;
use eronom::vm as backend;
use eronom::jit;
use backend::{Compiler, VM, Value};
pub const MAGIC_BYTES: &[u8; 16] = b"ERONOM_STUB_v1\0\0";

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct AppBundle {
    pub entrypoint: String,
    pub files: Vec<(String, Vec<u8>)>,
}

use backend::gc::{self, GcData};
use super::http_detect::{find_listen_port, has_http_import};

fn native_print(args: Vec<Value>) -> Value {
    let mut outputs = Vec::new();
    for arg in args {
        outputs.push(arg.to_string());
    }
    println!("{}", outputs.join(" "));
    Value::null()
}

fn native_now(_args: Vec<Value>) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    Value::number(now as f64)
}

fn native_local_time_string(_args: Vec<Value>) -> Value {
    let now = std::time::SystemTime::now();
    let ptr = backend::gc::gc_alloc_string(&format!("{:?}", now));
    Value::string(ptr)
}

fn extract_params_map(params_val: Value) -> std::collections::HashMap<String, String> {
    let mut params_map = std::collections::HashMap::new();
    if params_val.is_object() {
        unsafe {
            match &(*params_val.as_gc_ptr()).data {
                backend::GcData::Object(map) => {
                    for (k, v) in map {
                        if let Some(key_str) = k.0.as_str() {
                            let val_str = if v.is_string() {
                                v.as_str().unwrap_or("").to_string()
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
    params_map
}

pub fn native_render(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let file_path_val = args[0];
    let file_path = match file_path_val.as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let params_val = if args.len() > 1 { args[1] } else { Value::null() };
    let params_map = extract_params_map(params_val);

    let path = std::path::Path::new(file_path);
    let mut resolved_path = if path.exists() {
        path.to_path_buf()
    } else if path.with_extension("html").exists() {
        path.with_extension("html")
    } else {
        path.to_path_buf()
    };

    if !resolved_path.exists() {
        if let Some(target) = backend::er_http::get_target_script_path() {
            if let Some(parent) = std::path::Path::new(&target).parent() {
                let candidate = parent.join(path);
                if candidate.exists() {
                    resolved_path = candidate;
                } else if candidate.with_extension("html").exists() {
                    resolved_path = candidate.with_extension("html");
                }
            }
        }
    }

    if let Ok(content) = std::fs::read_to_string(&resolved_path) {
        let mut final_content = content;
        if !params_map.is_empty() {
            let mut params_js = String::from("window.__erm_params = {");
            for (k, v) in &params_map {
                params_js.push_str(&format!("{}: \"{}\",", k, v.replace("\"", "\\\"")));
            }
            params_js.push_str("};");
            final_content = final_content.replace("window.__erm_params = {};", &params_js);
        }
        let ptr = backend::gc::gc_alloc_string(&final_content);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_render_string(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let content = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let params_val = if args.len() > 1 { args[1] } else { Value::null() };
    let params_map = extract_params_map(params_val);

    let mut final_content = content.to_string();
    if !params_map.is_empty() {
        for (k, v) in &params_map {
            final_content = final_content.replace(&format!("{{{}}}", k), v);
        }
    }
    let ptr = backend::gc::gc_alloc_string(&final_content);
    Value::string(ptr)
}

pub fn register_micro_natives(vm: &mut VM) {
    vm.register_global("print", Value::native_function(native_print));
    vm.register_global("router", Value::native_function(backend::er_http::native_route));
    vm.register_global("Eronom_nativeRoute", Value::native_function(backend::er_http::native_route));
    vm.register_global("serve", Value::native_function(backend::er_http::native_serve));
    vm.register_global("render", Value::native_function(native_render));
    vm.register_global("renderString", Value::native_function(native_render_string));
    vm.register_global("Eronom_nativeRender", Value::native_function(native_render));
    vm.register_global("Eronom_nativeRenderString", Value::native_function(native_render_string));
    vm.register_global("fetch", Value::native_function(backend::er_http::native_fetch));
    vm.register_global("setTimeout", Value::native_function(backend::er_http::native_set_timeout));
    vm.register_global("clearTimeout", Value::native_function(backend::er_http::native_clear_timeout));
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

    backend::execute::fiber::register_fiber_natives(vm);
    let _ = backend::er_http::register_eronom_file_api(vm);
    backend::std_fs::register_fs_natives(vm);
    backend::std_path::register_path_natives(vm);
    backend::std_crypto::register_crypto_natives(vm);
    backend::std_json::register_json_natives(vm);
    backend::std_system::register_system_natives(vm);
}

pub fn run_micro_file(path: &str) -> anyhow::Result<()> {
    jit::init_jit_hooks();
    let path_buf = PathBuf::from(path);
    if !path_buf.exists() {
        anyhow::bail!("File not found: {}", path);
    }

    let stmts = match frontend::parse_and_resolve_imports(&path_buf) {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Compile/Import error: {}", e),
    };

    let fe = frontend::infer_effects(&stmts);
    if let Err(effect_errors) = frontend::check_sync_constraints(&stmts, &fe) {
        let mut err_msg = String::from("Compile error: synchronous constraint violation:\n");
        for err in effect_errors {
            err_msg.push_str(&format!("  - {}\n", err));
        }
        anyhow::bail!("{}", err_msg);
    }

    if has_http_import(&stmts) {
        let mut port = find_listen_port(&stmts);
        if port.is_none() {
            let main_path = std::path::Path::new(path);
            if let Some(parent_dir) = main_path.parent() {
                let toml_path = parent_dir.join("eronom.toml");
                if let Ok(toml_content) = std::fs::read_to_string(&toml_path) {
                    if let Ok(toml_val) = toml::from_str::<toml::Value>(&toml_content) {
                        if let Some(server) = toml_val.get("server") {
                            if let Some(p) = server.get("port").and_then(|p| p.as_integer()) {
                                port = Some(p as i32);
                            }
                        }
                    }
                }
            }
        }
        let final_port = port.unwrap_or(3000);
        backend::er_http::LISTEN_PORT.with(|p| p.set(Some(final_port)));
    }

    let mut vm = VM::new();
    register_micro_natives(&mut vm);
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

    let compiler = Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => anyhow::bail!("Compiler error: {}", e),
    };

    let func_ptr = gc::gc_allocate(GcData::Function(Box::new(function)));
    let func_val = Value::function(func_ptr);
    gc::gc_push_temp_slice(&func_val, 1);
    let run_res = vm.run_function_ptr(func_ptr);
    gc::gc_pop_temp_slice();

    if let Err(e) = run_res {
        anyhow::bail!("VM Runtime error: {}", e);
    }

    if let Err(e) = vm.run_event_loop() {
        anyhow::bail!("VM Event loop error: {}", e);
    }

    backend::er_http::start_http_server_if_needed(&mut vm);

    gc::gc_free_all();
    jit::reset_jit_state();
    Ok(())
}

pub fn check_and_run_micro_bundle() -> Option<anyhow::Result<()>> {
    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return None,
    };

    let mut file = match fs::File::open(&current_exe) {
        Ok(f) => f,
        Err(_) => return None,
    };

    use std::io::{Read, Seek, SeekFrom};
    let file_len = match file.metadata().map(|m| m.len()) {
        Ok(l) => l,
        Err(_) => return None,
    };

    if file_len < 24 {
        return None;
    }

    // 1. Check magic bytes
    if file.seek(SeekFrom::Start(file_len - 16)).is_err() {
        return None;
    }
    let mut magic_buf = [0u8; 16];
    if file.read_exact(&mut magic_buf).is_err() {
        return None;
    }
    if &magic_buf != MAGIC_BYTES {
        return None;
    }

    // 2. Read bundle length
    if file.seek(SeekFrom::Start(file_len - 24)).is_err() {
        return None;
    }
    let mut len_buf = [0u8; 8];
    if file.read_exact(&mut len_buf).is_err() {
        return None;
    }
    let payload_len = u64::from_le_bytes(len_buf);
    if payload_len + 24 > file_len {
        return Some(Err(anyhow::anyhow!("Corrupted embedded bundle payload length")));
    }

    // 3. Read bundle payload
    if file.seek(SeekFrom::Start(file_len - 24 - payload_len)).is_err() {
        return Some(Err(anyhow::anyhow!("Failed to seek to embedded bundle payload")));
    }
    let mut payload = vec![0u8; payload_len as usize];
    if let Err(e) = file.read_exact(&mut payload) {
        return Some(Err(anyhow::anyhow!("Failed to read embedded bundle: {}", e)));
    }

    let bundle: AppBundle = match serde_json::from_slice(&payload) {
        Ok(b) => b,
        Err(e) => return Some(Err(anyhow::anyhow!("Failed to deserialize embedded bundle: {}", e))),
    };

    // 4. Unpack bundle to isolated temp folder
    let temp_app_dir = std::env::temp_dir().join(format!(
        ".er_app_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    if let Err(e) = fs::create_dir_all(&temp_app_dir) {
        return Some(Err(anyhow::anyhow!("Failed to create bundle runtime dir: {}", e)));
    }

    for (rel_path, content) in &bundle.files {
        let dest = temp_app_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&dest, content);
    }

    // 5. Execute the unpacked application
    let entry_path = temp_app_dir.join(&bundle.entrypoint);
    let orig_dir = std::env::current_dir().ok();
    let _ = std::env::set_current_dir(&temp_app_dir);

    let res = run_micro_file(entry_path.to_str().unwrap_or(&bundle.entrypoint));
    if let Some(orig) = orig_dir {
        let _ = std::env::set_current_dir(orig);
    }
    let _ = fs::remove_dir_all(&temp_app_dir);
    Some(res)
}
