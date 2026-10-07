use crate::frontend::{lex, Parser};
use crate::vm::compiler::Compiler;
use crate::vm::execute::VM;
use crate::vm::value::Value;
use crate::vm::gc::gc_free_all;

fn run_script(source: &str) -> VM {
    let tokens = lex(source);
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse().unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();

    let mut vm = VM::new();
    crate::vm::execute::fiber::register_fiber_natives(&mut vm);
    vm.register_global("futureAwait", Value::native_function(crate::vm::er_http::native_future_await));
    vm.register_global("createPromisePair", Value::native_function(crate::vm::er_http::native_create_promise_pair));
    vm.register_global("arrayPush", Value::native_function(crate::vm::er_http::native_array_push));
    vm.register_global("arrayLen", Value::native_function(crate::vm::er_http::native_array_len));

    let func_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Function(Box::new(function)));
    let _ = vm.run_function_ptr(func_ptr);
    let _ = vm.run_event_loop();
    vm
}

#[test]
fn test_basic_fiber_spawn() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        let output = 0
        const worker = fn() {
            output = 42
            return 42
        }
        let f = Eronom_fiberSpawn(worker)
    "#;
    let vm = run_script(src);
    assert_eq!(vm.get_global("output").unwrap().as_number(), 42.0);
}

#[test]
fn test_fiber_yield_interleaving() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        let trace = []
        const f1 = fn() {
            trace.push("f1-1")
            Eronom_fiberYield()
            trace.push("f1-2")
        }
        const f2 = fn() {
            trace.push("f2-1")
            Eronom_fiberYield()
            trace.push("f2-2")
        }
        Eronom_fiberSpawn(f1)
        Eronom_fiberSpawn(f2)
    "#;
    let vm = run_script(src);
    let trace_val = vm.get_global("trace").unwrap();
    unsafe {
        match &(*trace_val.as_gc_ptr()).data {
            crate::vm::gc::GcData::Array(arr) => {
                assert_eq!(arr.len(), 4);
                assert_eq!(arr[0].as_str().unwrap(), "f1-1");
                assert_eq!(arr[1].as_str().unwrap(), "f2-1");
                assert_eq!(arr[2].as_str().unwrap(), "f1-2");
                assert_eq!(arr[3].as_str().unwrap(), "f2-2");
            }
            _ => panic!("Expected trace to be array"),
        }
    }
}

#[test]
fn test_1000_interleaved_fibers() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        let total = 0
        const task = fn() {
            total = total + 1
            Eronom_fiberYield()
            total = total + 1
        }
        for i in 0..1000 {
            Eronom_fiberSpawn(task)
        }
    "#;
    let vm = run_script(src);
    assert_eq!(vm.get_global("total").unwrap().as_number(), 2000.0);
}

#[test]
fn test_fiber_id_and_status() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        let child_id = 0
        let status_before = ""
        const worker = fn() {
            child_id = Eronom_fiberCurrentId()
        }
        let f = Eronom_fiberSpawn(worker)
        status_before = Eronom_fiberStatus(f)
    "#;
    let vm = run_script(src);
    assert_eq!(vm.get_global("status_before").unwrap().as_str().unwrap(), "ready");
    assert!(vm.get_global("child_id").unwrap().as_number() > 1.0);
}

#[test]
fn test_fiber_join_and_result() {
    let _lock = crate::vm::gc::TEST_GC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gc_free_all();
    let src = r#"
        let result = 0
        const calc = fn() {
            return 99
        }
        let f = Eronom_fiberSpawn(calc)
        let res = futureAwait(f._promise)
        result = res + 1
    "#;
    let vm = run_script(src);
    assert_eq!(vm.get_global("result").unwrap().as_number(), 100.0);
}
