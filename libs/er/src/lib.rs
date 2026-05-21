pub mod backend;
pub mod frontend;
pub mod legacy;

use std::collections::HashMap;

use backend::{Compiler, Value, VM};
use frontend::{lex, Parser};

struct GcGuard;
impl Drop for GcGuard {
    fn drop(&mut self) {
        backend::gc_free_all();
    }
}

thread_local! {
    pub static ROUTES: std::cell::RefCell<Vec<(String, String, Value)>> = std::cell::RefCell::new(Vec::new());
    pub static RESPONSE: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
}

fn route_fn(_args: Vec<Value>) -> Value {
    let mut obj = HashMap::new();
    obj.insert(std::rc::Rc::from("get"), Value::NativeFunction(app_get));
    obj.insert(std::rc::Rc::from("post"), Value::NativeFunction(app_post));
    let ptr = backend::gc_allocate(backend::GcData::Object(obj));
    Value::Object(ptr)
}

fn app_get(args: Vec<Value>) -> Value {
    if args.len() >= 2 {
        if let (Value::String(path), handler) = (&args[0], &args[1]) {
            ROUTES.with(|r| r.borrow_mut().push(("GET".to_string(), path.to_string(), handler.clone())));
        }
    }
    Value::Null
}

fn app_post(args: Vec<Value>) -> Value {
    if args.len() >= 2 {
        if let (Value::String(path), handler) = (&args[0], &args[1]) {
            ROUTES.with(|r| r.borrow_mut().push(("POST".to_string(), path.to_string(), handler.clone())));
        }
    }
    Value::Null
}

fn c_json(args: Vec<Value>) -> Value {
    if let Some(val) = args.first() {
        let json_str = value_to_json(val);
        RESPONSE.with(|r| *r.borrow_mut() = Some(json_str));
    }
    Value::Null
}

fn value_to_json(val: &Value) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        Value::Array(ptr) => unsafe {
            match &(**ptr).data {
                backend::GcData::Array(arr) => {
                    let items: Vec<String> = arr.iter().map(value_to_json).collect();
                    format!("[{}]", items.join(","))
                }
                _ => unreachable!(),
            }
        },
        Value::Object(ptr) => unsafe {
            match &(**ptr).data {
                backend::GcData::Object(obj) => {
                    let items: Vec<String> = obj
                        .iter()
                        .map(|(k, v)| format!("\"{}\":{}", k, value_to_json(v)))
                        .collect();
                    format!("{{{}}}", items.join(","))
                }
                _ => unreachable!(),
            }
        },
        _ => "\"<function>\"".to_string(),
    }
}

pub fn handle_api_request(
    request: &mut tiny_http::Request,
    api_file_path: &str,
    base_path: &str,
) -> anyhow::Result<Option<tiny_http::Response<std::io::Cursor<Vec<u8>>>>> {
    let _guard = GcGuard;

    // Register ROUTES as GC roots
    backend::GC_ROOTS.with(|roots| {
        roots.borrow_mut().clear();
        roots.borrow_mut().push(Box::new(|| {
            ROUTES.with(|r| {
                for (_, _, handler) in r.borrow().iter() {
                    backend::mark_value(handler);
                }
            });
        }));
    });

    let content = match std::fs::read_to_string(api_file_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    // Clear previous routes and response
    ROUTES.with(|r| r.borrow_mut().clear());
    RESPONSE.with(|r| *r.borrow_mut() = None);

    let tokens = lex(&content);
    let mut parser = Parser::new(tokens);
    let stmts = match parser.parse() {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Parse error: {}", e),
    };

    let compiler = Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => anyhow::bail!("Compile error: {}", e),
    };

    let mut vm = VM::new();
    vm.register_global("route", Value::NativeFunction(route_fn));

    // Handle POST body parsing (basic support for VM variables)
    if request.method() == &tiny_http::Method::Post {
        let mut body = String::new();
        request.as_reader().read_to_string(&mut body).ok();
        // In a real implementation we'd parse JSON and register it as a global 'body' variable.
        // For simplicity, we just inject it as a string for now.
        vm.register_global("body_raw", Value::String(std::rc::Rc::from(body)));
    }

    if let Err(e) = vm.run(std::rc::Rc::new(function)) {
        anyhow::bail!("VM Runtime error: {}", e);
    }

    let target = request.url();
    let mut clean_target = target;
    if let Some(idx) = clean_target.find('?') {
        clean_target = &clean_target[..idx];
    }
    if let Some(idx) = clean_target.find('#') {
        clean_target = &clean_target[..idx];
    }

    let mut clean_target_str = clean_target.to_string();
    if clean_target_str.len() > 1 && clean_target_str.ends_with('/') {
        clean_target_str.pop();
    }
    let clean_target = clean_target_str.as_str();

    let mut matched_handler = None;

    ROUTES.with(|r| {
        for (method, path, handler) in r.borrow().iter() {
            if method == &request.method().to_string() {
                let mut full_route_path = base_path.to_string();
                if !full_route_path.ends_with('/') && !path.starts_with('/') {
                    full_route_path.push('/');
                }
                if full_route_path.ends_with('/') && path.starts_with('/') {
                    full_route_path.push_str(&path[1..]);
                } else {
                    full_route_path.push_str(path);
                }

                if full_route_path.len() > 1 && full_route_path.ends_with('/') {
                    full_route_path.pop();
                }

                let mut match_route = full_route_path == clean_target;
                if !match_route {
                    if full_route_path == "/" && (clean_target == "" || clean_target == "/") {
                        match_route = true;
                    }
                }

                if match_route {
                    matched_handler = Some(handler.clone());
                    break;
                }
            }
        }
    });

    if let Some(handler) = matched_handler {
        let mut c_obj = HashMap::new();
        c_obj.insert(std::rc::Rc::from("json"), Value::NativeFunction(c_json));
        let c_val = Value::Object(backend::gc_allocate(backend::GcData::Object(c_obj)));

        // We need to call the handler in the VM.
        // We can do this by pushing the handler and args, and generating a dummy function, 
        // OR we can expose a `call_function` method on VM.
        // Let's just create a small bytecode chunk to call it, or run it directly.
        match handler {
            Value::Function(func) => {
                // To keep it simple, let's just clear the VM and run the function
                let mut call_vm = VM::new();
                call_vm.register_global("route", Value::NativeFunction(route_fn));
                
                // We need the handler on the stack, then the args, then OpCode::Call.
                // Let's create a new function that just calls our handler.
                let mut call_chunk = backend::Chunk::default();
                let f_idx = call_chunk.add_constant(Value::Function(func.clone()));
                call_chunk.write(backend::OpCode::Constant(f_idx));
                let arg_idx = call_chunk.add_constant(c_val);
                call_chunk.write(backend::OpCode::Constant(arg_idx));
                call_chunk.write(backend::OpCode::Call(1));
                call_chunk.write(backend::OpCode::Return);

                let wrapper = backend::Function {
                    name: None,
                    chunk: call_chunk,
                    arity: 0,
                };
                
                if let Err(e) = call_vm.run(std::rc::Rc::new(wrapper)) {
                    anyhow::bail!("VM Runtime error in handler: {}", e);
                }
                
                let response_str = RESPONSE.with(|r| r.borrow().clone());
                if let Some(json_data) = response_str {
                    let response = tiny_http::Response::from_string(json_data)
                        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
                        .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                    return Ok(Some(response));
                }
            }
            _ => anyhow::bail!("Handler is not a function"),
        }
    }

    Ok(None)
}

// Stubs for the old exported functions so eronom doesn't break if it uses them
#[derive(Clone, Debug)]
pub struct Variable {
    pub value: String,
    pub is_mutable: bool,
    pub decl_line: usize,
    pub decl_path: String,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub handler_lines: Vec<String>,
}

pub fn evaluate_file(
    _path: &str,
    _variables: &mut HashMap<String, Variable>,
    _routes: &mut Vec<Route>,
) -> anyhow::Result<()> {
    Ok(())
}

fn native_print(args: Vec<Value>) -> Value {
    let mut outputs = Vec::new();
    for arg in args {
        outputs.push(arg.to_string());
    }
    println!("{}", outputs.join(" "));
    Value::Null
}

pub fn run_file(path: &str) -> anyhow::Result<()> {
    let _guard = GcGuard;
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    let tokens = lex(&content);
    let mut parser = Parser::new(tokens);
    let stmts = match parser.parse() {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Parse error: {}", e),
    };

    let compiler = Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => anyhow::bail!("Compile error: {}", e),
    };

    let mut vm = VM::new();
    vm.register_global("print", Value::NativeFunction(native_print));

    if let Err(e) = vm.run(std::rc::Rc::new(function)) {
        anyhow::bail!("VM Runtime error: {}", e);
    }

    Ok(())
}
