use eronom::vm as backend;
use eronom::frontend;
use eronom::jit;
use backend::{Compiler, VM, Value};
use super::natives::*;
use super::http_detect::{find_listen_port, has_http_import};

pub struct GcGuard;
impl Drop for GcGuard {
    fn drop(&mut self) {
        backend::gc_free_all();
        jit::reset_jit_state();
    }
}

pub fn run_file(path: &str) -> anyhow::Result<()> {
    let _guard = GcGuard;
    let path_buf = std::path::PathBuf::from(path);
    let in_vfs = backend::embedded::has_vfs_file(path);
    if !path_buf.exists() && !in_vfs {
        anyhow::bail!("File not found: {}", path);
    }

    let stmts = match frontend::parse_and_resolve_imports(&path_buf) {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Compile/Import error: {}", e),
    };

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
            if port.is_none() {
                if let Some(toml_content) = backend::embedded::get_vfs_text("eronom.toml") {
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
    backend::er_http::register_eronom_file_api(&mut vm).unwrap();
    backend::std_fs::register_fs_natives(&mut vm);
    backend::std_path::register_path_natives(&mut vm);
    backend::std_crypto::register_crypto_natives(&mut vm);
    backend::std_json::register_json_natives(&mut vm);
    backend::std_system::register_system_natives(&mut vm);
    backend::er_http::set_target_script_path(path);
    if let Some(css_text) = backend::embedded::get_vfs_text("css/global.css") {
        eronom::compiler::set_global_ermcss(css_text);
    }
    let main_path = std::path::Path::new(path);
    let mut config_loaded = false;
    if let Some(parent_dir) = main_path.parent() {
        let toml_path = parent_dir.join("eronom.toml");
        if toml_path.exists() {
            if let Ok(toml_content) = std::fs::read_to_string(&toml_path) {
                if let Ok(toml_val) = toml::from_str::<toml::Value>(&toml_content) {
                    if let Ok(json_val) = serde_json::to_value(toml_val) {
                        let config_val = backend::gc::json_to_value(json_val);
                        vm.register_global("config", config_val);
                        config_loaded = true;
                    }
                }
            }
        }
    }
    if !config_loaded {
        if let Some(toml_content) = backend::embedded::get_vfs_text("eronom.toml") {
            if let Ok(toml_val) = toml::from_str::<toml::Value>(&toml_content) {
                if let Ok(json_val) = serde_json::to_value(toml_val) {
                    let config_val = backend::gc::json_to_value(json_val);
                    vm.register_global("config", config_val);
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
