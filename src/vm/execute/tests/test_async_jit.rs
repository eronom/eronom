use crate::vm::execute::types::VM;
use crate::vm::value::Value;
use crate::vm::gc::{gc_free_all, GcData};
use crate::vm::compiler::Compiler;
use super::test_basics::run_code;

#[test]
fn test_imports_exports() {
    use std::fs;
    let dir = std::env::current_dir().unwrap().join("target").join("test_imports_exports");
    fs::create_dir_all(&dir).unwrap();
    
    let lib_path = dir.join("lib.er");
    fs::write(&lib_path, "export const value = 42\nexport const other = 100").unwrap();
    
    let main_path = dir.join("main.er");
    fs::write(&main_path, "import { value } from \"./lib.er\"\nlet res = value + 10").unwrap();

    let stmts = crate::frontend::parse_and_resolve_imports(&main_path).unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();
    
    let mut vm = VM::new();
    vm.use_jit = true;
    vm.run(function.clone()).unwrap();
    assert_eq!(vm.get_global("res").unwrap().as_number(), 52.0);

    // Test failing when name is not exported
    let main_bad_path = dir.join("main_bad.er");
    fs::write(&main_bad_path, "import { not_exist } from \"./lib.er\"\n").unwrap();
    assert!(crate::frontend::parse_and_resolve_imports(&main_bad_path).is_err());
    
    // Clean up
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn test_async_await_event_loop() {
    gc_free_all();
    let source = "
        let result = 0
        const get_val = () => {
            return 42
        }
        const main = () => {
            const pair = createPromisePair()
            setTimeout((resolve) => {
                const x = get_val()
                resolve(x)
            }, 0, pair.resolve)
            let x = futureAwait(pair.promise)
            result = x + 10
        }
        main()
    ";
    let tokens = crate::frontend::lex(source);
    let mut parser = crate::frontend::Parser::new(tokens);
    let stmts = parser.parse().unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();
    let mut vm = VM::new();
    vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
    vm.register_global("futureAwait", Value::native_function(crate::vm::er_http::native_future_await));
    vm.register_global("createPromisePair", Value::native_function(crate::vm::er_http::native_create_promise_pair));
    vm.use_jit = true;
    vm.run(function).unwrap();
    vm.run_event_loop().unwrap();
    
    assert_eq!(vm.get_global("result").unwrap().as_number(), 52.0);
}

#[test]
fn test_concurrent_structured_syntax() {
    gc_free_all();
    let source = "
        let counter = 0
        const taskA = () => {
            counter = counter + 10
        }
        const taskB = () => {
            counter = counter + 20
        }
        concurrent {
            taskA()
            taskB()
        }
    ";
    let tokens = crate::frontend::lex(source);
    let mut parser = crate::frontend::Parser::new(tokens);
    let stmts = parser.parse().unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();

    let mut vm = VM::new();
    vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
    vm.register_global("futureAwait", Value::native_function(crate::vm::er_http::native_future_await));
    vm.register_global("createPromisePair", Value::native_function(crate::vm::er_http::native_create_promise_pair));
    vm.register_global("arrayPush", Value::native_function(crate::vm::er_http::native_array_push));
    vm.register_global("arrayLen", Value::native_function(crate::vm::er_http::native_array_len));
    vm.register_global("setIoMode", Value::native_function(crate::vm::er_http::native_set_io_mode));
    vm.register_global("getIoMode", Value::native_function(crate::vm::er_http::native_get_io_mode));
    crate::vm::er_http::register_eronom_file_api(&mut vm).unwrap();
    vm.use_jit = true;
    vm.run(function).unwrap();
    vm.run_event_loop().unwrap();

    assert_eq!(vm.get_global("counter").unwrap().as_number(), 30.0);
}

#[test]
fn test_set_timeout_scale_and_ordering() {
    gc_free_all();
    let source = "
        let order = []
        setTimeout(() => {
            arrayPush(order, 3)
        }, 30)
        setTimeout(() => {
            arrayPush(order, 1)
        }, 10)
        setTimeout(() => {
            arrayPush(order, 2)
        }, 20)
    ";
    let tokens = crate::frontend::lex(source);
    let mut parser = crate::frontend::Parser::new(tokens);
    let stmts = parser.parse().unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();

    let mut vm = VM::new();
    vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
    vm.register_global("clearTimeout", Value::native_function(crate::vm::er_http::native_clear_timeout));
    vm.register_global("arrayPush", Value::native_function(crate::vm::er_http::native_array_push));
    vm.use_jit = true;
    vm.run(function).unwrap();
    vm.run_event_loop().unwrap();

    let order_val = vm.get_global("order").unwrap();
    assert!(order_val.is_array());
    unsafe {
        if let GcData::Array(arr) = &(*order_val.as_gc_ptr()).data {
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0].as_number(), 1.0);
            assert_eq!(arr[1].as_number(), 2.0);
            assert_eq!(arr[2].as_number(), 3.0);
        } else {
            panic!("Expected array");
        }
    }
}

#[test]
fn test_clear_timeout_functionality() {
    gc_free_all();
    let source = "
        let fired = []
        let t1 = setTimeout(() => {
            arrayPush(fired, 1)
        }, 10)
        let t2 = setTimeout(() => {
            arrayPush(fired, 2)
        }, 20)
        clearTimeout(t2)
    ";
    let tokens = crate::frontend::lex(source);
    let mut parser = crate::frontend::Parser::new(tokens);
    let stmts = parser.parse().unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();

    let mut vm = VM::new();
    vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
    vm.register_global("clearTimeout", Value::native_function(crate::vm::er_http::native_clear_timeout));
    vm.register_global("arrayPush", Value::native_function(crate::vm::er_http::native_array_push));
    vm.use_jit = true;
    vm.run(function).unwrap();
    vm.run_event_loop().unwrap();

    let fired_val = vm.get_global("fired").unwrap();
    assert!(fired_val.is_array());
    unsafe {
        if let GcData::Array(arr) = &(*fired_val.as_gc_ptr()).data {
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0].as_number(), 1.0);
        } else {
            panic!("Expected array");
        }
    }
}

#[test]
fn test_http_response_aborted_safety() {
    crate::vm::er_http::end_http_response_json(std::ptr::null_mut(), "{}");
    let null_args = vec![Value::null()];
    crate::vm::er_http::native_context_json(null_args.clone());
    crate::vm::er_http::native_context_html(null_args);
}

#[test]
fn test_websocket_pubsub_api() {
    gc_free_all();
    let source = "
        let app = router()
        let open_called = false
        let msg_received = \"\"
        let is_binary_received = false
        
        app.ws(\"/chat\", {
            open: (ws) => {
                open_called = true
            },
            message: (ws, msg, is_binary) => {
                msg_received = msg
                is_binary_received = is_binary
            },
            close: (ws, code, reason) => {
            }
        })

        let pub_res = app.publish(\"global_room\", \"Hello Everyone\")
        let subs_count = app.numSubscribers(\"global_room\")
    ";
    let tokens = crate::frontend::lex(source);
    let mut parser = crate::frontend::Parser::new(tokens);
    let stmts = parser.parse().unwrap();
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts).unwrap();

    let mut vm = VM::new();
    vm.register_global("router", Value::native_function(crate::vm::er_http::native_route));
    vm.use_jit = true;
    vm.run(function).unwrap();

    // Verify WebSocket route registered
    let has_ws_routes = crate::vm::er_http::WS_ROUTES.with(|routes| {
        routes.borrow().iter().any(|r| r.path == "/chat")
    });
    assert!(has_ws_routes);

    // Test Simulated open event
    let dummy_ws = 0x12345 as *mut std::ffi::c_void;
    let path_c = std::ffi::CString::new("/chat").unwrap();
    crate::vm::er_http::ACTIVE_VM.with(|active| active.set(&mut vm as *mut VM));
    crate::vm::er_http::er_ws_on_open(dummy_ws, path_c.as_ptr(), path_c.as_bytes().len());
    assert_eq!(vm.get_global("open_called").unwrap().as_boolean(), true);

    // Test Simulated text message event
    let text_msg = "Hello Eronom";
    let text_c = std::ffi::CString::new(text_msg).unwrap();
    crate::vm::er_http::er_ws_on_message(
        dummy_ws,
        path_c.as_ptr(),
        path_c.as_bytes().len(),
        text_c.as_ptr(),
        text_c.as_bytes().len(),
        0,
    );
    let received_val = vm.get_global("msg_received").unwrap();
    assert_eq!(received_val.as_str().unwrap(), "Hello Eronom");
    assert_eq!(vm.get_global("is_binary_received").unwrap().as_boolean(), false);

    // Test Simulated binary message event
    let binary_bytes: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];
    crate::vm::er_http::er_ws_on_message(
        dummy_ws,
        path_c.as_ptr(),
        path_c.as_bytes().len(),
        binary_bytes.as_ptr() as *const std::ffi::c_char,
        binary_bytes.len(),
        1,
    );
    let bin_msg_val = vm.get_global("msg_received").unwrap();
    assert!(bin_msg_val.is_array());
    unsafe {
        if let GcData::Array(arr) = &(*bin_msg_val.as_gc_ptr()).data {
            assert_eq!(arr.len(), 4);
            assert_eq!(arr[0].as_number(), 0xde as f64);
            assert_eq!(arr[1].as_number(), 0xad as f64);
            assert_eq!(arr[2].as_number(), 0xbe as f64);
            assert_eq!(arr[3].as_number(), 0xef as f64);
        } else {
            panic!("Expected Array for binary frame");
        }
    }
    assert_eq!(vm.get_global("is_binary_received").unwrap().as_boolean(), true);

    // Clean up
    crate::vm::er_http::ACTIVE_VM.with(|active| active.set(std::ptr::null_mut()));
}

#[test]
fn test_extract_bytes_from_value() {
    gc_free_all();
    // 1. Test string extraction
    let s_ptr = crate::vm::gc::gc_alloc_string("hello");
    let (bytes, is_bin) = crate::vm::er_http::extract_bytes_from_value(Value::string(s_ptr), false);
    assert_eq!(bytes, b"hello");
    assert_eq!(is_bin, false);

    let (bytes_forced, is_bin_forced) = crate::vm::er_http::extract_bytes_from_value(Value::string(s_ptr), true);
    assert_eq!(bytes_forced, b"hello");
    assert_eq!(is_bin_forced, true);

    // 2. Test array of numbers extraction
    let mut arr_elems = Vec::new();
    arr_elems.push(Value::number(1.0));
    arr_elems.push(Value::number(2.0));
    arr_elems.push(Value::number(255.0));
    let arr_ptr = crate::vm::gc::gc_allocate(GcData::Array(arr_elems));
    let (arr_bytes, arr_is_bin) = crate::vm::er_http::extract_bytes_from_value(Value::array(arr_ptr), false);
    assert_eq!(arr_bytes, vec![1, 2, 255]);
    assert_eq!(arr_is_bin, true);
}

#[test]
fn test_jit_closures_and_upvalues() {
    let code = "
        fn makeCounter(initial) {
            let count = initial
            fn inc(step) {
                count = count + step
                return count
            }
            return inc
        }

        let c1 = makeCounter(10)
        let res1 = c1(5)
        let res2 = c1(3)
    ";
    let vm = run_code(code).unwrap();
    assert_eq!(vm.get_global("res1").unwrap().as_number(), 15.0);
    assert_eq!(vm.get_global("res2").unwrap().as_number(), 18.0);
}

#[test]
fn test_jit_struct_field_access_and_methods() {
    let code = "
        struct Point {
            x: int,
            y: int,
            fn sum() {
                return this.x + this.y
            }
        }

        let pt : Point = { x: 10, y: 25 }
        let sum_val = pt.sum()
        pt.x = 40
        let sum_val2 = pt.sum()
    ";
    let vm = run_code(code).unwrap();
    assert_eq!(vm.get_global("sum_val").unwrap().as_number(), 35.0);
    assert_eq!(vm.get_global("sum_val2").unwrap().as_number(), 65.0);
}

#[test]
fn test_jit_object_literals_and_array_methods() {
    let code = "
        let obj = { a: 1, b: \"hello\", c: [10, 20] }
        obj.c.push(30)
        let popped = obj.c.pop()
        let len = obj.c.length
        let val_a = obj.a
        let val_b = obj.b
    ";
    let vm = run_code(code).unwrap();
    assert_eq!(vm.get_global("popped").unwrap().as_number(), 30.0);
    assert_eq!(vm.get_global("len").unwrap().as_number(), 2.0);
    assert_eq!(vm.get_global("val_a").unwrap().as_number(), 1.0);
    assert_eq!(vm.get_global("val_b").unwrap().as_str().unwrap(), "hello");
}

#[test]
fn test_jit_dynamic_type_bailout() {
    let code = "
        fn dynAdd(a, b) {
            return a + b
        }

        let r1 = dynAdd(10, 20)
        let r2 = dynAdd(\"Hello \", \"World\")
        let r3 = dynAdd(\"Number: \", 42)
    ";
    let vm = run_code(code).unwrap();
    assert_eq!(vm.get_global("r1").unwrap().as_number(), 30.0);
    assert_eq!(vm.get_global("r2").unwrap().as_str().unwrap(), "Hello World");
    assert_eq!(vm.get_global("r3").unwrap().as_str().unwrap(), "Number: 42");
}

#[test]
fn test_jit_lifecycle_reset() {
    for i in 0..5 {
        let code = format!("let val = {} * 10 + 5", i);
        let vm = run_code(&code).unwrap();
        assert_eq!(vm.get_global("val").unwrap().as_number(), (i * 10 + 5) as f64);
        crate::jit::reset_jit_state();
    }
}
