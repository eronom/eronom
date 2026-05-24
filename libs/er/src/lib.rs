pub mod vm;
pub use vm as backend;
pub mod frontend;

use backend::{Compiler, VM, Value};
use frontend::{Parser, lex};

struct GcGuard;
impl Drop for GcGuard {
    fn drop(&mut self) {
        backend::gc_free_all();
    }
}

fn native_print(args: Vec<Value>) -> Value {
    let mut outputs = Vec::new();
    for arg in args {
        outputs.push(arg.to_string());
    }
    println!("{}", outputs.join(" "));
    Value::null()
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
    if vm.use_jit {
        eprintln!("[VM] Running with JIT compiler enabled");
    } else {
        eprintln!("[VM] Running with bytecode interpreter (no JIT)");
    }
    vm.register_global("print", Value::native_function(native_print));

    if let Err(e) = vm.run(function) {
        anyhow::bail!("VM Runtime error: {}", e);
    }

    Ok(())
}
