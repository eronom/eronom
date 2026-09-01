use crate::vm::value::Value;
use super::types::*;

pub fn write_file_helper(path: &str, data: &str, is_src_file: bool) -> Result<usize, String> {
    if is_src_file {
        std::fs::copy(data, path)
            .map(|bytes| bytes as usize)
            .map_err(|e| e.to_string())
    } else {
        std::fs::write(path, data)
            .map(|_| data.len())
            .map_err(|e| e.to_string())
    }
}

pub fn native_eronom_write_file(args: Vec<Value>) -> Value {
    if args.len() < 3 {
        return Value::number(0.0);
    }
    let path_val = args[0];
    let data_val = args[1];
    let is_src_file = args[2].as_boolean();

    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::number(0.0),
    };

    let data_str = match data_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::number(0.0),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        let res = write_file_helper(&path_str, &data_str, is_src_file);
        return Value::number(res.unwrap_or(0) as f64);
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
            let res = write_file_helper(&path_str, &data_str, is_src_file);

            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolveWritePromise(promise_ptr, res),
            });

            active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });

        Value::null()
    } else {
        let res = write_file_helper(&path_str, &data_str, is_src_file);
        Value::number(res.unwrap_or(0) as f64)
    }
}

pub fn native_write_global(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::number(0.0);
    }
    let path_val = args[0];
    let data_val = args[1];

    if path_val.as_str().is_none() {
        return Value::number(0.0);
    }

    let mut is_src_file = false;
    let mut data_str = String::new();

    if data_val.is_object() {
        let ptr = data_val.as_gc_ptr();
        unsafe {
            match &(*ptr).data {
                crate::vm::gc::GcData::Struct(s) => {
                    if s.descriptor.name.as_ref() == "File" {
                        is_src_file = true;
                        let name_key = crate::vm::gc::get_or_create_string("name");
                        if let Some(name_val) = s.get_field(Value::string(name_key)) {
                            if let Some(s) = name_val.as_str() {
                                data_str = s.to_string();
                            }
                        }
                    }
                }
                crate::vm::gc::GcData::Object(map) => {
                    let is_file_key = crate::vm::gc::get_or_create_string("_isFile");
                    let is_file_val = map.get(&crate::vm::value::MapKey(Value::string(is_file_key)))
                        .map(|v| v.as_boolean())
                        .unwrap_or(false);
                    if is_file_val {
                        is_src_file = true;
                        let name_key = crate::vm::gc::get_or_create_string("name");
                        let name_val = map.get(&crate::vm::value::MapKey(Value::string(name_key))).cloned().unwrap_or(Value::null());
                        if let Some(s) = name_val.as_str() {
                            data_str = s.to_string();
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if !is_src_file {
        data_str = match data_val.as_str() {
            Some(s) => s.to_string(),
            None => return Value::number(0.0),
        };
    }

    let args_to_pass = vec![
        path_val,
        Value::string(crate::vm::gc::get_or_create_string(&data_str)),
        Value::boolean(is_src_file)
    ];
    native_eronom_write_file(args_to_pass)
}
