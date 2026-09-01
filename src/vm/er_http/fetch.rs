use crate::vm::value::Value;
use crate::vm::gc::GcData;
use super::types::*;

pub fn native_fetch(args: Vec<Value>) -> Value {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        eprintln!("[Fetch] Error: ACTIVE_VM is null");
        return Value::null();
    }
    let vm = unsafe { &*vm_ptr };
    if vm.use_evented_io {
        native_fetch_evented(args)
    } else {
        native_fetch_sync(args)
    }
}

pub fn native_set_timeout(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        eprintln!("[setTimeout] Error: callback and delay are required");
        return Value::null();
    }
    let callback = args[0];
    let delay_val = args[1];
    if !callback.is_function() && !callback.is_native_function() {
        eprintln!("[setTimeout] Error: first argument must be a function");
        return Value::null();
    }
    if !delay_val.is_number() {
        eprintln!("[setTimeout] Error: second argument must be a number");
        return Value::null();
    }
    let delay_ms = delay_val.as_number() as u64;

    let mut cb_args = Vec::new();
    if args.len() > 2 {
        cb_args.extend_from_slice(&args[2..]);
    }

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        eprintln!("[setTimeout] Error: ACTIVE_VM is null");
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let timer_id = vm.next_timer_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let due_time = std::time::Instant::now() + std::time::Duration::from_millis(delay_ms);

    vm.timers.lock().unwrap().push(crate::vm::execute::VmTimer {
        id: timer_id,
        due_time,
        action: crate::vm::execute::VmTimerAction::Callback {
            callback,
            args: cb_args,
        },
    });
    vm.active_async_tasks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    vm.event_loop_condvar.notify_all();

    Value::number(timer_id as f64)
}

pub fn native_clear_timeout(args: Vec<Value>) -> Value {
    if args.is_empty() || !args[0].is_number() {
        return Value::null();
    }
    let timer_id = args[0].as_number() as u64;

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let mut timers = vm.timers.lock().unwrap();
    let mut remaining: Vec<crate::vm::execute::VmTimer> = timers.drain().collect();
    if let Some(pos) = remaining.iter().position(|t| t.id == timer_id) {
        remaining.swap_remove(pos);
        vm.active_async_tasks.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
    for t in remaining {
        timers.push(t);
    }

    Value::null()
}

fn get_ureq_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .user_agent("Eronom/0.9.2")
            .max_idle_connections(100)
            .max_idle_connections_per_host(100)
            .build();
        config.new_agent()
    })
}

fn perform_native_fetch(url: &str) -> Result<String, String> {
    let agent = get_ureq_agent();
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

pub fn native_fetch_sync(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let url_val = args[0];
    if !url_val.is_string() {
        eprintln!("[FetchSync] Error: URL must be a string");
        return Value::null();
    }
    let url_str = match url_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };

    match perform_native_fetch(&url_str) {
        Ok(body_str) => {
            let mut map = crate::vm::gc::get_pooled_map(2);
            let body_key = crate::vm::gc::get_or_create_string("_body");
            let body_val = crate::vm::gc::get_or_create_string(&body_str);
            map.insert(crate::vm::value::MapKey(Value::string(body_key)), Value::string(body_val));
            let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Object(map));
            Value::object(ptr)
        }
        Err(e) => {
            eprintln!("[FetchSync] Error: {}", e);
            Value::null()
        }
    }
}

pub fn native_fetch_evented(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let url_val = args[0];
    if !url_val.is_string() {
        eprintln!("[FetchEvented] Error: URL must be a string");
        return Value::null();
    }
    let url_str = match url_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        eprintln!("[FetchEvented] Error: ACTIVE_VM is null");
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    // 1. Create a promise
    let state = std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending));
    let prom = crate::vm::gc::GcPromise {
        state: state.clone(),
        suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));

    // 2. Take the stack and frames to suspend the VM
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

    // 3. Increment active async tasks counter
    let active_counter = vm.active_async_tasks.clone();
    let queue = vm.event_loop_queue.clone();
    let condvar = vm.event_loop_condvar.clone();
    active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let promise_ptr_usize = promise_ptr as usize;

    // 4. Spawn background thread to fetch URL
    std::thread::spawn(move || {
        let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
        let res = perform_native_fetch(&url_str);

        // Post ResolveFetchPromise back to event loop
        {
            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolveFetchPromise(promise_ptr, res),
            });
            condvar.notify_one();
        }

        active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    });

    Value::null()
}

pub fn native_future_await(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let future_val = args[0];
    let promise_ptr = if future_val.is_promise() {
        future_val.as_gc_ptr()
    } else if future_val.is_object() {
        unsafe {
            match &(*future_val.as_gc_ptr()).data {
                GcData::Object(map) => {
                    let key = crate::vm::gc::get_or_create_string("_promise");
                    if let Some(val) = map.get(&crate::vm::value::MapKey(Value::string(key))) {
                        if val.is_promise() {
                            val.as_gc_ptr()
                        } else {
                            return Value::null();
                        }
                    } else {
                        return Value::null();
                    }
                }
                GcData::Struct(s) => {
                    if let Some(val) = s.get_field_by_name("_promise") {
                        if val.is_promise() {
                            val.as_gc_ptr()
                        } else {
                            return Value::null();
                        }
                    } else {
                        return Value::null();
                    }
                }
                _ => return Value::null(),
            }
        }
    } else {
        return Value::null();
    };

    unsafe {
        match &(*promise_ptr).data {
            GcData::Promise(prom) => {
                let state = prom.state.lock().unwrap();
                match &*state {
                    crate::vm::gc::PromiseState::Fulfilled(val) => {
                        return *val;
                    }
                    crate::vm::gc::PromiseState::Rejected(err) => {
                        eprintln!("[Future Await] Error: {}", err);
                        return Value::null();
                    }
                    crate::vm::gc::PromiseState::Pending => {}
                }
            }
            _ => return Value::null(),
        }
    }

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };
    vm.close_upvalues(0);

    let suspended_stack = std::mem::take(&mut vm.stack);
    let suspended_frames = std::mem::take(&mut vm.frames);

    unsafe {
        match &mut (*promise_ptr).data {
            GcData::Promise(p) => {
                *p.suspended_stack.lock().unwrap() = suspended_stack;
                *p.suspended_frames.lock().unwrap() = suspended_frames;
            }
            _ => unreachable!(),
        }
    }

    Value::null()
}

pub fn native_set_io_mode(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let mode_val = args[0];
    if !mode_val.is_string() {
        return Value::null();
    }
    let mode_str = match mode_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &mut *vm_ptr };
        if mode_str == "evented" {
            vm.use_evented_io = true;
        } else {
            vm.use_evented_io = false;
        }
    }
    Value::null()
}

pub fn native_get_io_mode(_args: Vec<Value>) -> Value {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &*vm_ptr };
        let mode = if vm.use_evented_io { "evented" } else { "threaded" };
        let ptr = crate::vm::gc::get_or_create_string(mode);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_array_len(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::number(0.0);
    }
    let arr_val = args[0];
    if !arr_val.is_array() {
        return Value::number(0.0);
    }
    let arr_ptr = arr_val.as_gc_ptr();
    unsafe {
        match &(*arr_ptr).data {
            GcData::Array(arr) => Value::number(arr.len() as f64),
            _ => Value::number(0.0),
        }
    }
}

pub fn native_array_push(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let arr_val = args[0];
    let elem = args[1];
    if !arr_val.is_array() {
        return Value::null();
    }
    let arr_ptr = arr_val.as_gc_ptr();
    unsafe {
        match &mut (*arr_ptr).data {
            GcData::Array(arr) => {
                arr.push(elem);
            }
            _ => {}
        }
    }
    Value::null()
}

pub fn native_sleep(args: Vec<Value>) -> Value {
    let delay_ms = if args.is_empty() {
        0
    } else {
        args[0].as_number() as u64
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let prom = crate::vm::gc::GcPromise {
        state: std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending)),
        suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));
    let promise_val = Value::promise(promise_ptr);

    let timer_id = vm.next_timer_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let due_time = std::time::Instant::now() + std::time::Duration::from_millis(delay_ms);

    vm.timers.lock().unwrap().push(crate::vm::execute::VmTimer {
        id: timer_id,
        due_time,
        action: crate::vm::execute::VmTimerAction::ResolvePromise {
            promise_ptr,
            value: Value::null(),
        },
    });
    vm.active_async_tasks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    vm.event_loop_condvar.notify_all();

    promise_val
}

pub fn native_create_promise_pair(_args: Vec<Value>) -> Value {
    let prom = crate::vm::gc::GcPromise {
        state: std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending)),
        suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));
    let promise_val = Value::promise(promise_ptr);

    let resolver = Value(crate::vm::value::TAG_METHOD_RESOLVE | (promise_ptr as u64 & crate::vm::value::PTR_MASK));

    let mut map = crate::vm::gc::get_pooled_map(2);
    let promise_key = crate::vm::gc::get_or_create_string("promise");
    let resolve_key = crate::vm::gc::get_or_create_string("resolve");
    map.insert(crate::vm::value::MapKey(Value::string(promise_key)), promise_val);
    map.insert(crate::vm::value::MapKey(Value::string(resolve_key)), resolver);

    let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Object(map));
    Value::object(ptr)
}
