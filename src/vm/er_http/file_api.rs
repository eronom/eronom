use crate::vm::value::Value;
use crate::vm::execute::VM;
use super::types::*;
pub use super::string_api::*;
pub use super::file_write::*;

pub fn native_eronom_is_file(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let val = args[0];
    if val.is_object() {
        let ptr = val.as_gc_ptr();
        unsafe {
            match &(*ptr).data {
                crate::vm::gc::GcData::Struct(s) => {
                    return Value::boolean(s.descriptor.name.as_ref() == "File");
                }
                crate::vm::gc::GcData::Object(map) => {
                    let is_file_key = crate::vm::gc::get_or_create_string("_isFile");
                    if let Some(is_file_val) = map.get(&crate::vm::value::MapKey(Value::string(is_file_key))) {
                        return Value::boolean(is_file_val.as_boolean());
                    }
                }
                _ => {}
            }
        }
    }
    Value::boolean(false)
}

pub fn native_eronom_get_mime_type(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_val = args[0];
    let path = match path_val.as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
        
    let mime = match ext.as_str() {
        "json" => "application/json;charset=utf-8",
        "html" | "htm" => "text/html;charset=utf-8",
        "js" | "mjs" => "text/javascript;charset=utf-8",
        "css" => "text/css;charset=utf-8",
        "txt" => "text/plain;charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => "text/plain;charset=utf-8",
    };
    
    let ptr = crate::vm::gc::get_or_create_string(mime);
    Value::string(ptr)
}

pub fn native_eronom_get_file_size(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::number(0.0);
    }
    let path_val = args[0];
    let path = match path_val.as_str() {
        Some(s) => s,
        None => return Value::number(0.0),
    };
    
    let size = std::fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(0);
        
    Value::number(size as f64)
}

pub fn native_eronom_file_exists(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let path_val = args[0];
    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::boolean(false),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::boolean(std::path::Path::new(&path_str).exists());
    }
    let vm = unsafe { &mut *vm_ptr };

    if vm.use_evented_io {
        let state = std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending));
        let prom = crate::vm::gc::GcPromise {
            state: state.clone(),
            suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));

        let suspended_stack = std::mem::take(&mut vm.stack);
        let suspended_frames = std::mem::take(&mut vm.frames);

        unsafe {
            match &mut (*promise_ptr).data {
                crate::vm::gc::GcData::Promise(p) => {
                    *p.suspended_stack.lock().unwrap() = suspended_stack;
                    *p.suspended_frames.lock().unwrap() = suspended_frames;
                }
                _ => unreachable!(),
            }
        }

        let active_counter = vm.active_async_tasks.clone();
        let queue = vm.event_loop_queue.clone();
        active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let promise_ptr_usize = promise_ptr as usize;

        std::thread::spawn(move || {
            let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
            let exists = std::path::Path::new(&path_str).exists();
            let res_val = Value::boolean(exists);

            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolvePromise(promise_ptr, res_val),
            });

            active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });

        Value::null()
    } else {
        Value::boolean(std::path::Path::new(&path_str).exists())
    }
}

pub fn native_eronom_file_text(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_val = args[0];
    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        let content = std::fs::read_to_string(&path_str).unwrap_or_default();
        let ptr = crate::vm::gc::get_or_create_string(&content);
        return Value::string(ptr);
    }
    let vm = unsafe { &mut *vm_ptr };

    if vm.use_evented_io {
        let state = std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending));
        let prom = crate::vm::gc::GcPromise {
            state: state.clone(),
            suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));

        let suspended_stack = std::mem::take(&mut vm.stack);
        let suspended_frames = std::mem::take(&mut vm.frames);

        unsafe {
            match &mut (*promise_ptr).data {
                crate::vm::gc::GcData::Promise(p) => {
                    *p.suspended_stack.lock().unwrap() = suspended_stack;
                    *p.suspended_frames.lock().unwrap() = suspended_frames;
                }
                _ => unreachable!(),
            }
        }

        let active_counter = vm.active_async_tasks.clone();
        let queue = vm.event_loop_queue.clone();
        active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let promise_ptr_usize = promise_ptr as usize;

        std::thread::spawn(move || {
            let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
            let content = std::fs::read_to_string(&path_str).map_err(|e| e.to_string());

            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolveTextPromise(promise_ptr, content),
            });

            active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });

        Value::null()
    } else {
        let content = std::fs::read_to_string(&path_str).unwrap_or_default();
        let ptr = crate::vm::gc::get_or_create_string(&content);
        Value::string(ptr)
    }
}

pub fn native_eronom_file_json(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_val = args[0];
    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        let content = std::fs::read_to_string(&path_str).unwrap_or_default();
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
            return crate::vm::gc::json_to_value(json_val);
        } else {
            return Value::null();
        }
    }
    let vm = unsafe { &mut *vm_ptr };

    if vm.use_evented_io {
        let state = std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending));
        let prom = crate::vm::gc::GcPromise {
            state: state.clone(),
            suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));

        let suspended_stack = std::mem::take(&mut vm.stack);
        let suspended_frames = std::mem::take(&mut vm.frames);

        unsafe {
            match &mut (*promise_ptr).data {
                crate::vm::gc::GcData::Promise(p) => {
                    *p.suspended_stack.lock().unwrap() = suspended_stack;
                    *p.suspended_frames.lock().unwrap() = suspended_frames;
                }
                _ => unreachable!(),
            }
        }

        let active_counter = vm.active_async_tasks.clone();
        let queue = vm.event_loop_queue.clone();
        active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let promise_ptr_usize = promise_ptr as usize;

        std::thread::spawn(move || {
            let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
            let content = std::fs::read_to_string(&path_str).map_err(|e| e.to_string());

            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolveJsonPromise(promise_ptr, content),
            });

            active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });

        Value::null()
    } else {
        let content = std::fs::read_to_string(&path_str).unwrap_or_default();
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
            crate::vm::gc::json_to_value(json_val)
        } else {
            Value::null()
        }
    }
}

pub fn native_file_global(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_val = args[0];
    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let ext = std::path::Path::new(&path_str)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
        
    let mime = match ext.as_str() {
        "json" => "application/json;charset=utf-8",
        "html" | "htm" => "text/html;charset=utf-8",
        "js" | "mjs" => "text/javascript;charset=utf-8",
        "css" => "text/css;charset=utf-8",
        "txt" => "text/plain;charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => "text/plain;charset=utf-8",
    };

    let size = std::fs::metadata(&path_str)
        .map(|m| m.len())
        .unwrap_or(0);

    super::multipart::construct_file_object(vm, &path_str, mime, size as usize)
}

pub fn register_eronom_file_api(vm: &mut VM) -> Result<(), String> {
    // 1. Register native functions for Eronom File API
    vm.register_global("Eronom_nativeFileExists", Value::native_function(native_eronom_file_exists));
    vm.register_global("Eronom_nativeFileText", Value::native_function(native_eronom_file_text));
    vm.register_global("Eronom_nativeFileJson", Value::native_function(native_eronom_file_json));
    vm.register_global("Eronom_nativeGetMimeType", Value::native_function(native_eronom_get_mime_type));
    vm.register_global("Eronom_nativeGetFileSize", Value::native_function(native_eronom_get_file_size));
    vm.register_global("Eronom_nativeIsFile", Value::native_function(native_eronom_is_file));
    vm.register_global("Eronom_nativeWriteFile", Value::native_function(native_eronom_write_file));

    // 2. Register global built-ins so they are available without imports
    vm.register_global("file", Value::native_function(native_file_global));
    vm.register_global("write", Value::native_function(native_write_global));

    // 3. Register native string helpers
    vm.register_global("stringSplit", Value::native_function(native_string_split));
    vm.register_global("stringIncludes", Value::native_function(native_string_includes));
    vm.register_global("stringStartsWith", Value::native_function(native_string_starts_with));
    vm.register_global("stringEndsWith", Value::native_function(native_string_ends_with));
    vm.register_global("stringSubstring", Value::native_function(native_string_substring));
    vm.register_global("stringReplace", Value::native_function(native_string_replace));
    vm.register_global("stringTrim", Value::native_function(native_string_trim));
    vm.register_global("stringLength", Value::native_function(native_string_length));
    vm.register_global("stringCharAt", Value::native_function(native_string_char_at));
    vm.register_global("stringIndexOf", Value::native_function(native_string_index_of));

    // 4. Register core structured concurrency runtime
    let preamble = r#"
        const callWithArity = (func, args) => {
            const length = arrayLen(args)
            if (length == 0) { return func() }
            if (length == 1) { return func(args[0]) }
            if (length == 2) { return func(args[0], args[1]) }
            if (length == 3) { return func(args[0], args[1], args[2]) }
            if (length == 4) { return func(args[0], args[1], args[2], args[3]) }
            if (length == 5) { return func(args[0], args[1], args[2], args[3], args[4]) }
            return func()
        }

        const spawnTask = (func, args) => {
            const pair = createPromisePair()
            setTimeout((f, a, resolve) => {
                const res = callWithArity(f, a)
                resolve(res)
            }, 0, func, args, pair.resolve)
            return pair.promise
        }

        const spawn = spawnTask
    "#;

    let tokens = crate::frontend::lex(preamble);
    let mut parser = crate::frontend::Parser::new(tokens);
    if let Ok(stmts) = parser.parse() {
        let compiler = crate::vm::compiler::Compiler::new();
        if let Ok(func) = compiler.compile(&stmts) {
            let _ = vm.run(func);
        }
    }

    Ok(())
}
