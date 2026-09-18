use std::collections::HashMap;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use anyhow::Result;

use crate::vm::execute::VM;
use crate::vm::gc::GcData;
use crate::vm::value::Value;

pub const EMBEDDED_EDS_SCRIPT: &str = include_str!("../../eds/compiler.er");

pub fn find_eds_path() -> Option<PathBuf> {
    // 1. Search upwards from current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = Some(cwd.as_path());
        while let Some(dir) = current {
            let p1 = dir.join("libs").join("eds").join("compiler.er");
            if p1.is_file() {
                return Some(p1);
            }
            let p2 = dir.join("eds").join("compiler.er");
            if p2.is_file() {
                return Some(p2);
            }
            current = dir.parent();
        }
    }

    // 2. Search relative to executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let mut current = Some(exe_dir);
            while let Some(dir) = current {
                let p1 = dir.join("libs").join("eds").join("compiler.er");
                if p1.is_file() {
                    return Some(p1);
                }
                let p2 = dir.join("eds").join("compiler.er");
                if p2.is_file() {
                    return Some(p2);
                }
                current = dir.parent();
            }
        }
    }

    None
}

struct EdsEngine {
    vm: VM,
    generate_css_fn: Value,
    validate_tag_fn: Value,
    transform_primitive_fn: Value,
    is_primitive_fn: Option<Value>,
    epoch: u64,
}

fn init_eds_engine(project_dir: Option<&Path>) -> Result<EdsEngine> {
    let script_source: Cow<str> = if let Some(path) = find_eds_path() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            Cow::Owned(content)
        } else {
            Cow::Borrowed(EMBEDDED_EDS_SCRIPT)
        }
    } else {
        Cow::Borrowed(EMBEDDED_EDS_SCRIPT)
    };

    let tokens = crate::frontend::lexer::lex(&script_source);
    let mut parser = crate::frontend::parser::Parser::new(tokens);
    let stmts = parser.parse().map_err(|e| anyhow::anyhow!("EDS parse error: {}", e))?;

    let compiler = crate::vm::compiler::Compiler::new();
    let function = compiler.compile(&stmts).map_err(|e| anyhow::anyhow!("EDS compile error: {}", e))?;

    let mut vm = VM::new();
    vm.use_jit = false;

    vm.register_global("print", crate::vm::value::Value::native_function(|args| {
        let mut outputs = Vec::new();
        for arg in args {
            outputs.push(arg.to_string());
        }
        println!("{}", outputs.join(" "));
        crate::vm::value::Value::null()
    }));

    let lbrace_ptr = crate::vm::gc::get_or_create_string("{");
    vm.register_global("LBRACE", crate::vm::value::Value::string(lbrace_ptr));
    let rbrace_ptr = crate::vm::gc::get_or_create_string("}");
    vm.register_global("RBRACE", crate::vm::value::Value::string(rbrace_ptr));

    let base_str = project_dir
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let base_ptr = crate::vm::gc::get_or_create_string(&base_str);
    vm.register_global("PROJECT_DIR", crate::vm::value::Value::string(base_ptr));

    crate::vm::er_http::register_eronom_file_api(&mut vm).map_err(|e| anyhow::anyhow!("{}", e))?;

    if let Err(e) = vm.run(function) {
        anyhow::bail!("VM Runtime error for EDS: {}", e);
    }

    let generate_css_fn = vm
        .globals
        .get("generateRootCss")
        .copied()
        .ok_or_else(|| anyhow::anyhow!("Global 'generateRootCss' not found in EDS script"))?;
    let validate_tag_fn = vm
        .globals
        .get("validateTag")
        .copied()
        .ok_or_else(|| anyhow::anyhow!("Global 'validateTag' not found in EDS script"))?;
    let transform_primitive_fn = vm
        .globals
        .get("transformPrimitive")
        .copied()
        .ok_or_else(|| anyhow::anyhow!("Global 'transformPrimitive' not found in EDS script"))?;
    let is_primitive_fn = vm.globals.get("isPrimitive").copied();

    Ok(EdsEngine {
        vm,
        generate_css_fn,
        validate_tag_fn,
        transform_primitive_fn,
        is_primitive_fn,
        epoch: crate::vm::gc::gc_epoch(),
    })
}

thread_local! {
    static THREAD_EDS_ENGINE: std::cell::RefCell<Option<EdsEngine>> = const { std::cell::RefCell::new(None) };
}

pub fn reset_eds_engine() {
    THREAD_EDS_ENGINE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

fn with_eds_engine<R>(f: impl FnOnce(&mut EdsEngine) -> Result<R>) -> Result<R> {
    THREAD_EDS_ENGINE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let current_epoch = crate::vm::gc::gc_epoch();
        if borrow.as_ref().map_or(true, |e| e.epoch != current_epoch) {
            *borrow = Some(init_eds_engine(None)?);
        }
        f(borrow.as_mut().unwrap())
    })
}

/// Checks if a tag name corresponds to an EDS primitive.
pub fn is_primitive(tag_name: &str) -> bool {
    with_eds_engine(|engine| {
        if let Some(is_prim_fn) = engine.is_primitive_fn {
            let tag_name_ptr = crate::vm::gc::get_or_create_string(tag_name);
            let tag_name_val = crate::vm::value::Value::string(tag_name_ptr);
            if let Ok(res) = engine.vm.call_function_reentrant(is_prim_fn, vec![tag_name_val]) {
                if res.is_boolean() {
                    return Ok(res.as_boolean());
                }
            }
        }
        Ok(matches!(
            tag_name,
            "Box" | "Text" | "Stack" | "Cluster" | "Card" | "Button" | "Badge"
        ))
    })
    .unwrap_or_else(|_| {
        matches!(
            tag_name,
            "Box" | "Text" | "Stack" | "Cluster" | "Card" | "Button" | "Badge"
        )
    })
}

/// Generate complete CSS with CSS light-dark() root variables and atomic EDS classes
/// via the native Eronom Design System script.
pub fn generate_root_css(project_dir: Option<&Path>) -> String {
    with_eds_engine(|engine| {
        let dir_str = project_dir
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let dir_ptr = crate::vm::gc::get_or_create_string(&dir_str);
        let dir_val = crate::vm::value::Value::string(dir_ptr);

        let res = engine
            .vm
            .call_function_reentrant(engine.generate_css_fn, vec![dir_val])
            .map_err(|e| anyhow::anyhow!("EDS generateRootCss VM error: {}", e))?;

        Ok(res.as_str().unwrap_or_default().to_string())
    })
    .unwrap_or_default()
}

/// Validates an AST element tag and its attributes against the EDS rules
/// via the native Eronom Design System script.
pub fn validate_tag(
    tag_name: &str,
    attrs: &HashMap<String, String>,
    file: &str,
    line: usize,
) -> Result<()> {
    with_eds_engine(|engine| {
        let tag_name_ptr = crate::vm::gc::get_or_create_string(tag_name);
        let tag_name_val = crate::vm::value::Value::string(tag_name_ptr);

        let json_map: serde_json::Map<String, serde_json::Value> = attrs
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let attrs_val = crate::vm::gc::json_to_value(serde_json::Value::Object(json_map));

        let file_ptr = crate::vm::gc::get_or_create_string(file);
        let file_val = crate::vm::value::Value::string(file_ptr);
        let line_val = crate::vm::value::Value::number(line as f64);

        let res = engine
            .vm
            .call_function_reentrant(
                engine.validate_tag_fn,
                vec![tag_name_val, attrs_val, file_val, line_val],
            )
            .map_err(|e| anyhow::anyhow!("EDS validateTag VM error: {}", e))?;

        if let Some(err_str) = res.as_str() {
            if !err_str.is_empty() {
                anyhow::bail!("{}", err_str);
            }
        }
        Ok(())
    })
}

/// Transforms an EDS primitive into a native semantic HTML element with atomic EDS classes
/// via the native Eronom Design System script.
pub fn transform_primitive(
    tag_name: &str,
    attrs: &HashMap<String, String>,
) -> Option<(String, String)> {
    with_eds_engine(|engine| {
        let tag_name_ptr = crate::vm::gc::get_or_create_string(tag_name);
        let tag_name_val = crate::vm::value::Value::string(tag_name_ptr);

        let json_map: serde_json::Map<String, serde_json::Value> = attrs
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let attrs_val = crate::vm::gc::json_to_value(serde_json::Value::Object(json_map));

        let res = engine
            .vm
            .call_function_reentrant(
                engine.transform_primitive_fn,
                vec![tag_name_val, attrs_val],
            )
            .map_err(|e| anyhow::anyhow!("EDS transformPrimitive VM error: {}", e))?;

        if res.is_null() {
            return Ok(None);
        }

        if res.is_array() {
            unsafe {
                if let GcData::Array(ref arr) = (*res.as_gc_ptr()).data {
                    if arr.len() >= 2 {
                        if let (Some(open_tag), Some(target_tag)) = (arr[0].as_str(), arr[1].as_str()) {
                            return Ok(Some((open_tag.to_string(), target_tag.to_string())));
                        }
                    }
                }
            }
        }

        Ok(None)
    })
    .unwrap_or(None)
}
