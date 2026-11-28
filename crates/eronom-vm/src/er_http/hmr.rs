use std::fs;
use std::path::Path;
use std::time::SystemTime;
use crate::vm::execute::VM;
use super::types::*;

pub fn toml_to_value(val: toml::Value) -> crate::vm::value::Value {
    match val {
        toml::Value::String(s) => {
            let ptr = crate::vm::gc::gc_alloc_string(&s);
            crate::vm::value::Value::string(ptr)
        }
        toml::Value::Integer(i) => crate::vm::value::Value::number(i as f64),
        toml::Value::Float(f) => crate::vm::value::Value::number(f),
        toml::Value::Boolean(b) => crate::vm::value::Value::boolean(b),
        toml::Value::Datetime(d) => {
            let ptr = crate::vm::gc::gc_alloc_string(&d.to_string());
            crate::vm::value::Value::string(ptr)
        }
        toml::Value::Array(arr) => {
            let mut elements = crate::vm::gc::gc_vec_with_capacity(arr.len());
            for v in arr {
                elements.push(toml_to_value(v));
            }
            let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Array(elements));
            crate::vm::value::Value::array(ptr)
        }
        toml::Value::Table(tbl) => {
            let mut keys = Vec::with_capacity(tbl.len());
            let mut values = Vec::with_capacity(tbl.len());
            for (k, v) in tbl {
                let key_ptr = crate::vm::gc::intern_string(&k);
                keys.push(crate::vm::value::Value::string(key_ptr));
                values.push(toml_to_value(v));
            }
            let (desc, offsets) = crate::vm::shape::get_or_create_anonymous_shape(&keys);
            let mut fields = crate::vm::gc::StructFields::with_len_null(keys.len());
            for i in 0..keys.len() {
                fields[offsets[i]] = values[i];
            }
            let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Struct(crate::vm::gc::GcStruct {
                descriptor: desc,
                fields,
            }));
            crate::vm::value::Value::object(ptr)
        }
    }
}

pub fn set_target_script_path(path: &str) {
    TARGET_SCRIPT_PATH.with(|p| {
        *p.borrow_mut() = Some(path.to_string());
    });
    let mtime = get_max_mtime_for_reload(path);
    LAST_MTIME.with(|m| {
        m.set(mtime);
    });
}

pub fn get_target_script_path() -> Option<String> {
    TARGET_SCRIPT_PATH.with(|p| p.borrow().clone())
}

pub fn get_max_mtime_for_reload(path: &str) -> Option<SystemTime> {
    let mut max_mtime = fs::metadata(path).ok()?.modified().ok()?;
    
    let path_obj = Path::new(path);
    if let Some(parent) = path_obj.parent() {
        let toml_path = parent.join("eronom.toml");
        if toml_path.exists() {
            if let Ok(meta) = fs::metadata(&toml_path) {
                if let Ok(mtime) = meta.modified() {
                    if mtime > max_mtime {
                        max_mtime = mtime;
                    }
                }
            }
        }
    }
    
    Some(max_mtime)
}

pub fn check_and_reload_script_if_needed(vm: &mut VM) {
    let now = SystemTime::now();
    let should_check = LAST_CHECK_TIME.with(|last_check| {
        if let Some(last) = last_check.get() {
            if let Ok(elapsed) = now.duration_since(last) {
                if elapsed.as_millis() < 500 {
                    return false;
                }
            }
        }
        last_check.set(Some(now));
        true
    });
    
    if !should_check {
        return;
    }

    let script_path = TARGET_SCRIPT_PATH.with(|p| p.borrow().clone());
    let Some(path) = script_path else {
        return;
    };
    
    let current_mtime = match get_max_mtime_for_reload(&path) {
        Some(mtime) => mtime,
        None => return,
    };
    
    let last_mtime = LAST_MTIME.with(|m| m.get());
    if Some(current_mtime) == last_mtime {
        return;
    }
    
    println!("[HTTP] File change detected, reloading script: {}...", path);
    
    // Safely free old MIR JIT buffers and clear code caches on reload
    crate::vm::execute::types::trigger_jit_reset();

    let old_routes = ROUTES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_ws_routes = WS_ROUTES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_mws = MIDDLEWARES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_mounts = STATIC_MOUNTS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_listen_port = LISTEN_PORT.with(|p| p.replace(None));
    let old_listen_callback = LISTEN_CALLBACK.with(|cb| cb.replace(None));
    ROUTER.with(|r| r.borrow_mut().clear());
    
    let path_buf = Path::new(&path);
    let stmts = match crate::frontend::parse_and_resolve_imports(path_buf) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Parsing/Import resolution failed: {}", e);
            ROUTES.with(|r| *r.borrow_mut() = old_routes.clone());
            WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
            MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
            STATIC_MOUNTS.with(|r| *r.borrow_mut() = old_mounts);
            LISTEN_PORT.with(|p| p.set(old_listen_port));
            LISTEN_CALLBACK.with(|cb| *cb.borrow_mut() = old_listen_callback);
            ROUTER.with(|r| {
                let mut router = r.borrow_mut();
                router.clear();
                for route in &old_routes {
                    router.insert(&route.method, &route.path, route.callback);
                }
            });
            return;
        }
    };
    
    let compiler = crate::vm::compiler::Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Compilation failed: {}", e);
            ROUTES.with(|r| *r.borrow_mut() = old_routes.clone());
            WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
            MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
            STATIC_MOUNTS.with(|r| *r.borrow_mut() = old_mounts);
            LISTEN_PORT.with(|p| p.set(old_listen_port));
            LISTEN_CALLBACK.with(|cb| *cb.borrow_mut() = old_listen_callback);
            ROUTER.with(|r| {
                let mut router = r.borrow_mut();
                router.clear();
                for route in &old_routes {
                    router.insert(&route.method, &route.path, route.callback);
                }
            });
            return;
        }
    };
    
    // Reload eronom.toml if it exists
    let parent_dir = Path::new(&path).parent();
    if let Some(parent) = parent_dir {
        let toml_path = parent.join("eronom.toml");
        if toml_path.exists() {
            if let Ok(toml_content) = fs::read_to_string(&toml_path) {
                if let Ok(toml_val) = toml::from_str::<toml::Value>(&toml_content) {
                    let config_val = toml_to_value(toml_val);
                    vm.register_global("config", config_val);
                }
            }
        }
    }
    
    if let Err(e) = vm.run(function) {
        eprintln!("[HTTP] Reload error: Execution failed: {}", e);
        ROUTES.with(|r| *r.borrow_mut() = old_routes.clone());
        WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
        MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
        STATIC_MOUNTS.with(|r| *r.borrow_mut() = old_mounts);
        LISTEN_PORT.with(|p| p.set(old_listen_port));
        LISTEN_CALLBACK.with(|cb| *cb.borrow_mut() = old_listen_callback);
        ROUTER.with(|r| {
            let mut router = r.borrow_mut();
            router.clear();
            for route in &old_routes {
                router.insert(&route.method, &route.path, route.callback);
            }
        });
        return;
    }
    
    LAST_MTIME.with(|m| m.set(Some(current_mtime)));
    println!("[HTTP] Reload successful. VM state and routes updated.");
}
