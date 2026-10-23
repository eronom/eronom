use std::process::Command;
use super::gc::{gc_alloc_string, gc_allocate, get_or_create_string, get_pooled_map, get_pooled_vec, GcData};
use super::value::{MapKey, Value};
use super::execute::VM;

pub fn native_env_get(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let key = match args[0].as_str() {
        Some(k) => k,
        None => return Value::null(),
    };

    match std::env::var(key) {
        Ok(val) => {
            let ptr = gc_alloc_string(&val);
            Value::string(ptr)
        }
        Err(_) => {
            if args.len() > 1 {
                args[1]
            } else {
                Value::null()
            }
        }
    }
}

pub fn native_env_set(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let key = match args[0].as_str() {
        Some(k) => k,
        None => return Value::boolean(false),
    };
    let val = match args[1].as_str() {
        Some(v) => v.to_string(),
        None => args[1].to_string(),
    };

    unsafe {
        std::env::set_var(key, val);
    }
    Value::boolean(true)
}

pub fn native_env_has(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let key = match args[0].as_str() {
        Some(k) => k,
        None => return Value::boolean(false),
    };
    Value::boolean(std::env::var(key).is_ok())
}

pub fn native_env_remove(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let key = match args[0].as_str() {
        Some(k) => k,
        None => return Value::boolean(false),
    };
    unsafe {
        std::env::remove_var(key);
    }
    Value::boolean(true)
}

pub fn native_env_all(_args: Vec<Value>) -> Value {
    let vars: Vec<(String, String)> = std::env::vars().collect();
    let mut map = get_pooled_map(vars.len());
    for (k, v) in vars {
        let key_ptr = get_or_create_string(&k);
        let val_ptr = gc_alloc_string(&v);
        map.insert(MapKey(Value::string(key_ptr)), Value::string(val_ptr));
    }
    let ptr = gc_allocate(GcData::Object(map));
    Value::object(ptr)
}

pub fn native_process_args(_args: Vec<Value>) -> Value {
    let args: Vec<String> = std::env::args().collect();
    let mut arr = get_pooled_vec(args.len());
    for a in args {
        let ptr = gc_alloc_string(&a);
        arr.push(Value::string(ptr));
    }
    let ptr = gc_allocate(GcData::Array(arr));
    Value::array(ptr)
}

pub fn native_process_pid(_args: Vec<Value>) -> Value {
    Value::number(std::process::id() as f64)
}

pub fn native_process_cwd(_args: Vec<Value>) -> Value {
    match std::env::current_dir() {
        Ok(p) => {
            let s = p.to_string_lossy();
            let ptr = gc_alloc_string(&s);
            Value::string(ptr)
        }
        Err(_) => Value::null(),
    }
}

pub fn native_process_chdir(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let dir = match args[0].as_str() {
        Some(d) => d,
        None => return Value::boolean(false),
    };
    Value::boolean(std::env::set_current_dir(dir).is_ok())
}

pub fn native_process_exit(args: Vec<Value>) -> Value {
    let code = if !args.is_empty() && args[0].is_number() {
        args[0].as_number() as i32
    } else {
        0
    };
    std::process::exit(code);
}

pub fn native_process_exec(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let cmd_str = match args[0].as_str() {
        Some(c) => c,
        None => return Value::null(),
    };

    let mut command = if args.len() > 1 && args[1].is_array() {
        let mut cmd = Command::new(cmd_str);
        unsafe {
            if let GcData::Array(ref arr) = (*args[1].as_gc_ptr()).data {
                for arg in arr {
                    if let Some(s) = arg.as_str() {
                        cmd.arg(s);
                    } else {
                        cmd.arg(arg.to_string());
                    }
                }
            }
        }
        cmd
    } else {
        #[cfg(target_os = "windows")]
        {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", cmd_str]);
            cmd
        }
        #[cfg(not(target_os = "windows"))]
        {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", cmd_str]);
            cmd
        }
    };

    let output = match command.output() {
        Ok(out) => out,
        Err(e) => {
            let mut map = get_pooled_map(5);
            map.insert(MapKey(Value::string(get_or_create_string("stdout"))), Value::string(gc_alloc_string("")));
            map.insert(MapKey(Value::string(get_or_create_string("stderr"))), Value::string(gc_alloc_string(&e.to_string())));
            map.insert(MapKey(Value::string(get_or_create_string("exitCode"))), Value::number(-1.0));
            map.insert(MapKey(Value::string(get_or_create_string("status"))), Value::number(-1.0));
            map.insert(MapKey(Value::string(get_or_create_string("success"))), Value::boolean(false));
            let ptr = gc_allocate(GcData::Object(map));
            return Value::object(ptr);
        }
    };

    let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1) as f64;
    let success = output.status.success();

    let mut map = get_pooled_map(5);
    map.insert(MapKey(Value::string(get_or_create_string("stdout"))), Value::string(gc_alloc_string(&stdout_str)));
    map.insert(MapKey(Value::string(get_or_create_string("stderr"))), Value::string(gc_alloc_string(&stderr_str)));
    map.insert(MapKey(Value::string(get_or_create_string("exitCode"))), Value::number(exit_code));
    map.insert(MapKey(Value::string(get_or_create_string("status"))), Value::number(exit_code));
    map.insert(MapKey(Value::string(get_or_create_string("success"))), Value::boolean(success));

    let ptr = gc_allocate(GcData::Object(map));
    Value::object(ptr)
}

pub fn native_process_platform(_args: Vec<Value>) -> Value {
    #[cfg(target_os = "linux")]
    let p = "linux";
    #[cfg(target_os = "macos")]
    let p = "macos";
    #[cfg(target_os = "windows")]
    let p = "windows";
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let p = "unknown";

    let ptr = gc_alloc_string(p);
    Value::string(ptr)
}

pub fn native_process_arch(_args: Vec<Value>) -> Value {
    #[cfg(target_arch = "x86_64")]
    let a = "x86_64";
    #[cfg(target_arch = "aarch64")]
    let a = "aarch64";
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    let a = "unknown";

    let ptr = gc_alloc_string(a);
    Value::string(ptr)
}

pub fn create_env_object() -> Value {
    let mut map = get_pooled_map(5);
    map.insert(MapKey(Value::string(get_or_create_string("get"))), Value::native_function(native_env_get));
    map.insert(MapKey(Value::string(get_or_create_string("set"))), Value::native_function(native_env_set));
    map.insert(MapKey(Value::string(get_or_create_string("has"))), Value::native_function(native_env_has));
    map.insert(MapKey(Value::string(get_or_create_string("remove"))), Value::native_function(native_env_remove));
    map.insert(MapKey(Value::string(get_or_create_string("delete"))), Value::native_function(native_env_remove));
    map.insert(MapKey(Value::string(get_or_create_string("all"))), Value::native_function(native_env_all));

    let ptr = gc_allocate(GcData::Object(map));
    Value::object(ptr)
}

pub fn create_process_object() -> Value {
    let mut map = get_pooled_map(9);
    map.insert(MapKey(Value::string(get_or_create_string("args"))), native_process_args(Vec::new()));
    map.insert(MapKey(Value::string(get_or_create_string("pid"))), native_process_pid(Vec::new()));
    map.insert(MapKey(Value::string(get_or_create_string("cwd"))), Value::native_function(native_process_cwd));
    map.insert(MapKey(Value::string(get_or_create_string("chdir"))), Value::native_function(native_process_chdir));
    map.insert(MapKey(Value::string(get_or_create_string("exit"))), Value::native_function(native_process_exit));
    map.insert(MapKey(Value::string(get_or_create_string("exec"))), Value::native_function(native_process_exec));
    map.insert(MapKey(Value::string(get_or_create_string("platform"))), native_process_platform(Vec::new()));
    map.insert(MapKey(Value::string(get_or_create_string("arch"))), native_process_arch(Vec::new()));
    map.insert(MapKey(Value::string(get_or_create_string("env"))), create_env_object());

    let ptr = gc_allocate(GcData::Object(map));
    Value::object(ptr)
}

pub fn register_system_natives(vm: &mut VM) {
    vm.register_global("Eronom_nativeEnvGet", Value::native_function(native_env_get));
    vm.register_global("Eronom_nativeEnvSet", Value::native_function(native_env_set));
    vm.register_global("Eronom_nativeEnvHas", Value::native_function(native_env_has));
    vm.register_global("Eronom_nativeEnvRemove", Value::native_function(native_env_remove));
    vm.register_global("Eronom_nativeEnvAll", Value::native_function(native_env_all));

    vm.register_global("Eronom_nativeProcessArgs", Value::native_function(native_process_args));
    vm.register_global("Eronom_nativeProcessPid", Value::native_function(native_process_pid));
    vm.register_global("Eronom_nativeProcessCwd", Value::native_function(native_process_cwd));
    vm.register_global("Eronom_nativeProcessChdir", Value::native_function(native_process_chdir));
    vm.register_global("Eronom_nativeProcessExit", Value::native_function(native_process_exit));
    vm.register_global("Eronom_nativeProcessExec", Value::native_function(native_process_exec));
    vm.register_global("Eronom_nativeProcessPlatform", Value::native_function(native_process_platform));
    vm.register_global("Eronom_nativeProcessArch", Value::native_function(native_process_arch));

    vm.register_global("env", create_env_object());
    vm.register_global("process", create_process_object());
}
