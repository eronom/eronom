use crate::vm::execute::types::VM;
use crate::vm::value::Value;
use crate::vm::gc::{gc_free_all, GcData};
use crate::vm::compiler::Compiler;

fn run_concurrency_script(source: &str) -> Result<VM, String> {
    let temp_dir = std::env::current_dir().unwrap().join("target").join(format!("test_concurrency_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let main_path = temp_dir.join("main.er");
    std::fs::write(&main_path, source).unwrap();

    let stmts = crate::frontend::parse_and_resolve_imports(&main_path)?;
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts)?;

    let mut vm = VM::new();
    crate::vm::execute::fiber::register_fiber_natives(&mut vm);
    vm.register_global("futureAwait", Value::native_function(crate::vm::er_http::native_future_await));
    vm.register_global("createPromisePair", Value::native_function(crate::vm::er_http::native_create_promise_pair));
    vm.register_global("arrayPush", Value::native_function(crate::vm::er_http::native_array_push));
    vm.register_global("arrayLen", Value::native_function(crate::vm::er_http::native_array_len));
    vm.register_global("sleep", Value::native_function(crate::vm::er_http::native_sleep));
    vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
    vm.register_global("clearTimeout", Value::native_function(crate::vm::er_http::native_clear_timeout));
    vm.register_global("now", Value::native_function(|_| {
        let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        Value::number(millis as f64)
    }));

    let func_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Function(Box::new(function)));
    vm.run_function_ptr(func_ptr)?;
    vm.run_event_loop()?;
    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(vm)
}

#[test]
fn test_task_spawn_and_join() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        import { Task } from "std/task"
        let result = 0
        const worker = fn() {
            return 42
        }
        let handle = Task.spawn(worker)
        result = Task.join(handle)
    "#;
    let vm = run_concurrency_script(src).unwrap();
    assert_eq!(vm.get_global("result").unwrap().as_number(), 42.0);
}

#[test]
fn test_task_all_concurrent_execution() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        import { Task } from "std/task"
        let t1 = fn() { return 10 }
        let t2 = fn() { return 20 }
        let t3 = fn() { return 30 }
        let results = Task.all([t1, t2, t3])
    "#;
    let vm = run_concurrency_script(src).unwrap();
    let results_val = vm.get_global("results").unwrap();
    unsafe {
        match &(*results_val.as_gc_ptr()).data {
            GcData::Array(arr) => {
                assert_eq!(arr.len(), 3);
                assert_eq!(arr[0].as_number(), 10.0);
                assert_eq!(arr[1].as_number(), 20.0);
                assert_eq!(arr[2].as_number(), 30.0);
            }
            _ => panic!("Expected array"),
        }
    }
}

#[test]
fn test_task_race_first_wins_cancels_rest() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        import { Task } from "std/task"
        let loserCleaned = 0
        let slowTask = fn() {
            Task.addFinalizer(fn() {
                loserCleaned = 1
            })
            while (true) {
                Task.yield()
            }
            return "slow"
        }
        let fastTask = fn() {
            return "fast"
        }
        let winner = Task.race([slowTask, fastTask])
    "#;
    let vm = run_concurrency_script(src).unwrap();
    assert_eq!(vm.get_global("winner").unwrap().as_str().unwrap(), "fast");
    assert_eq!(vm.get_global("loserCleaned").unwrap().as_number(), 1.0);
}

#[test]
fn test_task_foreach_bounded_concurrency() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        import { Task } from "std/task"
        let items = [1, 2, 3, 4, 5, 6, 7, 8]
        let doubled = Task.forEach(items, 2, fn(item, idx) {
            return item * 2
        })
    "#;
    let vm = run_concurrency_script(src).unwrap();
    let doubled_val = vm.get_global("doubled").unwrap();
    unsafe {
        match &(*doubled_val.as_gc_ptr()).data {
            GcData::Array(arr) => {
                assert_eq!(arr.len(), 8);
                assert_eq!(arr[0].as_number(), 2.0);
                assert_eq!(arr[7].as_number(), 16.0);
            }
            _ => panic!("Expected array"),
        }
    }
}

#[test]
fn test_task_scope_cancellation() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        import { Task } from "std/task"
        let cleaned = 0
        try {
            Task.scope(fn(scope) {
                scope.spawn(fn() {
                    Task.addFinalizer(fn() {
                        cleaned = 1
                    })
                    for i in 0..1000000 {
                        if (i == 1) {
                            Task.yield()
                        }
                    }
                })
                Task.yield()
                throw "EarlyScopeExit"
            })
        } catch (err) {
            // caught
        }
    "#;
    let vm = run_concurrency_script(src).unwrap();
    assert_eq!(vm.get_global("cleaned").unwrap().as_number(), 1.0);
}

#[test]
fn test_schedule_exponential_retry() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        import { Schedule } from "std/schedule"
        let attempts = 0
        let flaky = fn() {
            attempts = attempts + 1
            if (attempts < 3) {
                throw "TemporaryFailure"
            }
            return 999
        }
        let res = Schedule.exponential(1, 2.0).maxRetries(5).retry(flaky)
    "#;
    let vm = run_concurrency_script(src).unwrap();
    assert_eq!(vm.get_global("res").unwrap().as_number(), 999.0);
    assert_eq!(vm.get_global("attempts").unwrap().as_number(), 3.0);
}

#[test]
fn test_clock_virtual_advance() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        import { Clock } from "std/clock"
        let executed = 0
        Clock.run(fn() {
            Clock.schedule(100, fn() {
                executed = 1
            })
            Clock.schedule(500, fn() {
                executed = 2
            })
            Clock.advance(150)
        })
    "#;
    let vm = run_concurrency_script(src).unwrap();
    assert_eq!(vm.get_global("executed").unwrap().as_number(), 1.0);
}
