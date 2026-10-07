use crate::vm::execute::types::VM;
use crate::vm::value::Value;
use crate::vm::gc::{gc_free_all, GcData};
use crate::frontend::parse_and_resolve_imports;
use crate::vm::compiler::Compiler;

fn run_jit_script(source: &str) -> Result<VM, String> {
    let temp_dir = std::env::current_dir().unwrap().join("target").join(format!("test_jit_spec_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let main_path = temp_dir.join("main.er");
    std::fs::write(&main_path, source).unwrap();

    let stmts = parse_and_resolve_imports(&main_path)?;
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts)?;

    let mut vm = VM::new();
    vm.use_jit = true;
    vm.jit_threshold = 0;
    crate::vm::execute::fiber::register_fiber_natives(&mut vm);
    vm.register_global("print", Value::native_function(|_| Value::null()));
    vm.register_global("sleep", Value::native_function(crate::vm::er_http::native_sleep));

    let func_ptr = crate::vm::gc::gc_allocate(GcData::Function(Box::new(function)));
    vm.run_function_ptr(func_ptr)?;
    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(vm)
}

#[test]
fn test_pure_function_annotated_in_bytecode() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let temp_file = std::env::temp_dir().join(format!("test_pure_{}.er", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::write(&temp_file, r#"
        const fib = fn(n) {
            if (n <= 1) {
                return n
            }
            return fib(n - 1) + fib(n - 2)
        }
    "#).unwrap();
    let stmts = parse_and_resolve_imports(&temp_file).unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();
    let _ = std::fs::remove_file(&temp_file);

    // Find the fib function constant
    let fib_fn = function.chunk.constants.iter().find_map(|val| {
        if val.is_function() {
            let ptr = val.as_gc_ptr();
            match unsafe { &(*ptr).data } {
                GcData::Function(f) => Some(f.clone()),
                _ => None,
            }
        } else {
            None
        }
    }).expect("fib function constant not found");

    assert!(fib_fn.is_pure, "fib should be marked is_pure = true");
    assert!(!fib_fn.can_suspend, "fib should be marked can_suspend = false");
}

#[test]
fn test_impure_function_annotated_in_bytecode() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let temp_file = std::env::temp_dir().join(format!("test_impure_{}.er", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::write(&temp_file, r#"
        const log_and_add = fn(x) {
            print(x)
            return x + 1
        }
    "#).unwrap();
    let stmts = parse_and_resolve_imports(&temp_file).unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();
    let _ = std::fs::remove_file(&temp_file);

    let fn_obj = function.chunk.constants.iter().find_map(|val| {
        if val.is_function() {
            let ptr = val.as_gc_ptr();
            match unsafe { &(*ptr).data } {
                GcData::Function(f) => Some(f.clone()),
                _ => None,
            }
        } else {
            None
        }
    }).expect("log_and_add function constant not found");

    assert!(!fn_obj.is_pure, "log_and_add should be marked is_pure = false");
    assert!(!fn_obj.can_suspend, "log_and_add should be marked can_suspend = false");
}

#[test]
fn test_suspending_function_annotated_in_bytecode() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let temp_file = std::env::temp_dir().join(format!("test_suspend_{}.er", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::write(&temp_file, r#"
        const do_sleep = fn() {
            sleep(10)
            return 1
        }
    "#).unwrap();
    let stmts = parse_and_resolve_imports(&temp_file).unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();
    let _ = std::fs::remove_file(&temp_file);

    let fn_obj = function.chunk.constants.iter().find_map(|val| {
        if val.is_function() {
            let ptr = val.as_gc_ptr();
            match unsafe { &(*ptr).data } {
                GcData::Function(f) => Some(f.clone()),
                _ => None,
            }
        } else {
            None
        }
    }).expect("do_sleep function constant not found");

    assert!(!fn_obj.is_pure, "do_sleep should be marked is_pure = false");
    assert!(fn_obj.can_suspend, "do_sleep should be marked can_suspend = true");
}

#[test]
fn test_jit_execution_pure_fibonacci() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        const fib = fn(n) {
            if (n <= 1) {
                return n
            }
            return fib(n - 1) + fib(n - 2)
        }
        let res = fib(20)
    "#;
    let vm = run_jit_script(src).unwrap();
    assert_eq!(vm.get_global("res").unwrap().as_number(), 6765.0);
}

#[test]
fn test_jit_execution_pure_tight_loop() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        let sum = 0
        for i in 1..10001 {
            sum = sum + i
        }
    "#;
    let vm = run_jit_script(src).unwrap();
    assert_eq!(vm.get_global("sum").unwrap().as_number(), 50005000.0);
}

#[test]
fn test_jit_execution_sync_attribute_purity() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let temp_file = std::env::temp_dir().join(format!("test_sync_attr_{}.er", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::write(&temp_file, r#"
        @sync
        fn compute(a, b) {
            return (a * b) + 42
        }
    "#).unwrap();
    let stmts = parse_and_resolve_imports(&temp_file).unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();
    let _ = std::fs::remove_file(&temp_file);

    let fn_obj = function.chunk.constants.iter().find_map(|val| {
        if val.is_function() {
            let ptr = val.as_gc_ptr();
            match unsafe { &(*ptr).data } {
                GcData::Function(f) => Some(f.clone()),
                _ => None,
            }
        } else {
            None
        }
    }).expect("compute function constant not found");

    assert!(fn_obj.is_pure, "@sync compute should be is_pure = true");
    assert!(!fn_obj.can_suspend, "@sync compute should be can_suspend = false");
}
