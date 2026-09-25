use std::collections::HashMap;
use std::path::PathBuf;
use anyhow::Result;

use crate::vm::execute::VM;
use crate::vm::value::Value;

pub const EMBEDDED_ERM_COMPILER_SCRIPT: &str = include_str!("compiler.er");

pub fn find_erm_compiler_path() -> Option<PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = Some(cwd.as_path());
        while let Some(dir) = current {
            let p1 = dir.join("libs").join("erm").join("compiler.er");
            if p1.is_file() {
                return Some(p1);
            }
            let p2 = dir.join("erm").join("compiler.er");
            if p2.is_file() {
                return Some(p2);
            }
            current = dir.parent();
        }
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let mut current = Some(exe_dir);
            while let Some(dir) = current {
                let p1 = dir.join("libs").join("erm").join("compiler.er");
                if p1.is_file() {
                    return Some(p1);
                }
                current = dir.parent();
            }
        }
    }
    None
}

struct ErmEngine {
    vm: VM,
    process_fn: Value,
    epoch: u64,
}

fn init_erm_engine() -> Result<ErmEngine> {
    let compiler_path = find_erm_compiler_path().unwrap_or_else(|| PathBuf::from("libs/erm/compiler.er"));
    let stmts = crate::frontend::parse_and_resolve_imports(&compiler_path)
        .map_err(|e| anyhow::anyhow!("ERM resolve imports error: {}", e))?;

    let compiler = crate::vm::compiler::Compiler::new();
    let function = compiler.compile(&stmts).map_err(|e| anyhow::anyhow!("ERM compile error: {}", e))?;

    let mut vm = VM::new();
    vm.use_jit = false;

    vm.register_global("print", Value::native_function(|args| {
        let mut outputs = Vec::new();
        for arg in args {
            outputs.push(arg.to_string());
        }
        println!("{}", outputs.join(" "));
        Value::null()
    }));

    let lbrace_ptr = crate::vm::gc::get_or_create_string("{");
    vm.register_global("LBRACE", Value::string(lbrace_ptr));
    let rbrace_ptr = crate::vm::gc::get_or_create_string("}");
    vm.register_global("RBRACE", Value::string(rbrace_ptr));

    crate::vm::er_http::register_eronom_file_api(&mut vm).map_err(|e| anyhow::anyhow!("{}", e))?;

    vm.run(function).map_err(|e| anyhow::anyhow!("VM Runtime error for ERM compiler: {}", e))?;

    let process_fn = vm
        .globals
        .get("processErmComponent")
        .copied()
        .ok_or_else(|| anyhow::anyhow!("Global 'processErmComponent' not found in compiler.er"))?;

    Ok(ErmEngine {
        vm,
        process_fn,
        epoch: crate::vm::gc::gc_epoch(),
    })
}

thread_local! {
    static THREAD_ERM_ENGINE: std::cell::RefCell<Option<ErmEngine>> = const { std::cell::RefCell::new(None) };
}

pub fn reset_erm_engine() {
    THREAD_ERM_ENGINE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

fn with_erm_engine<R>(f: impl FnOnce(&mut ErmEngine) -> Result<R>) -> Result<R> {
    THREAD_ERM_ENGINE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let current_epoch = crate::vm::gc::gc_epoch();
        if borrow.as_ref().map_or(true, |e| e.epoch != current_epoch) {
            *borrow = Some(init_erm_engine()?);
        }
        f(borrow.as_mut().unwrap())
    })
}

pub fn process_erm_component(
    file_path: &str,
    content: &str,
    is_prod: bool,
    params: &HashMap<String, String>,
) -> Result<String> {
    with_erm_engine(|engine| {
        let fp_ptr = crate::vm::gc::get_or_create_string(file_path);
        let fp_val = Value::string(fp_ptr);

        let content_ptr = crate::vm::gc::get_or_create_string(content);
        let content_val = Value::string(content_ptr);

        let is_prod_val = Value::boolean(is_prod);

        let mut params_map = crate::vm::gc::get_pooled_map(params.len());
        for (k, v) in params {
            let k_ptr = crate::vm::gc::get_or_create_string(k);
            let v_ptr = crate::vm::gc::get_or_create_string(v);
            params_map.insert(
                crate::vm::value::MapKey(Value::string(k_ptr)),
                Value::string(v_ptr),
            );
        }
        let params_obj_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Object(params_map));
        let params_val = Value::object(params_obj_ptr);

        let result_val = engine.vm.call_function_reentrant(
            engine.process_fn,
            vec![fp_val, content_val, is_prod_val, params_val],
        ).map_err(|e| anyhow::anyhow!("processErmComponent execution failed: {}", e))?;

        if let Some(s) = result_val.as_str() {
            Ok(s.to_string())
        } else {
            Ok(result_val.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_process_erm_component() {
        let template = "<main><h1>Hello from Bridge</h1></main>";
        let res = process_erm_component("test.erm", template, true, &HashMap::new());
        assert!(res.is_ok(), "Failed to process ERM component: {:?}", res.err());
        let html = res.unwrap();
        assert!(html.contains("Hello from Bridge"));
    }
}
