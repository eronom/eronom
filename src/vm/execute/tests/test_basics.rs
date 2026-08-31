use crate::vm::execute::types::VM;
use crate::vm::value::Value;
use crate::vm::gc::{gc_free_all, gc_allocate, gc_with_state, GC_NEEDS_STEP, GcPhase, GcData};
use crate::vm::compiler::Compiler;
use std::sync::atomic::Ordering;

pub fn run_code(source: &str) -> Result<VM, String> {
    gc_free_all();
    let tokens = crate::frontend::lex(source);
    let mut parser = crate::frontend::Parser::new(tokens);
    let stmts = parser.parse().map_err(|e| e.to_string())?;
    let compiler = Compiler::new();
    let function = compiler.compile(&stmts)?;
    
    let mut vm = VM::new();
    vm.use_jit = true;
    vm.run(function)?;

    let mut jit_globals = std::collections::HashMap::new();
    for (k, v) in &vm.globals {
        jit_globals.insert(k.clone(), v.to_string());
    }

    // Clean up JIT allocations before running Interpreter
    gc_free_all();

    // Recompile to get fresh constants for the Interpreter run
    let tokens = crate::frontend::lex(source);
    let mut parser = crate::frontend::Parser::new(tokens);
    let stmts = parser.parse().map_err(|e| e.to_string())?;
    let compiler = Compiler::new();
    let function_interp = compiler.compile(&stmts)?;

    let mut vm_interp = VM::new();
    vm_interp.use_jit = false;
    vm_interp.run(function_interp)?;

    for (k, v_interp) in &vm_interp.globals {
        let v_jit_str = jit_globals.get(k).expect("Missing global in JIT");
        assert_eq!(v_jit_str, &v_interp.to_string(), "Global mismatch for '{}': JIT={}, Interpreter={}", k, v_jit_str, v_interp);
    }

    Ok(vm_interp)
}

#[test]
fn test_arithmetic() {
    let vm = run_code("let res = 5 + 10 * 2").unwrap();
    assert_eq!(vm.get_global("res").unwrap().as_number(), 25.0);
}

#[test]
fn test_logical() {
    let vm = run_code("let res1 = true and false\nlet res2 = false or true").unwrap();
    assert_eq!(vm.get_global("res1").unwrap().as_boolean(), false);
    assert_eq!(vm.get_global("res2").unwrap().as_boolean(), true);
}

#[test]
fn test_for_loop() {
    let vm = run_code("let sum = 0\nfor i in 1..5 {\n  sum = sum + i\n}").unwrap();
    assert_eq!(vm.get_global("sum").unwrap().as_number(), 10.0);
}

#[test]
fn test_if_else() {
    let vm = run_code("let res = 0\nif (1 < 2) {\n  res = 10\n} else {\n  res = 20\n}").unwrap();
    assert_eq!(vm.get_global("res").unwrap().as_number(), 10.0);
}

#[test]
fn test_function_call() {
    let vm = run_code("let add = (a, b) => {\n  return a + b\n}\nlet res = add(3, 4)").unwrap();
    assert_eq!(vm.get_global("res").unwrap().as_number(), 7.0);
}

#[test]
fn test_function_declaration() {
    let vm = run_code("fn add(a, b) {\n  return a + b\n}\nlet res = add(3, 4)\nlet res2 = (fn(x) { return x * 2 })(5)").unwrap();
    assert_eq!(vm.get_global("res").unwrap().as_number(), 7.0);
    assert_eq!(vm.get_global("res2").unwrap().as_number(), 10.0);
}

#[test]
fn test_recursion() {
    let vm = run_code("let fib = (n) => {\n  if (n < 2) { return n }\n  return fib(n - 1) + fib(n - 2)\n}\nlet res = fib(5)").unwrap();
    assert_eq!(vm.get_global("res").unwrap().as_number(), 5.0);
}

#[test]
fn test_array() {
    let vm = run_code("let arr = [10, 20]\narr.push(30)\nlet l = arr.length\nlet val = arr[1]").unwrap();
    assert_eq!(vm.get_global("l").unwrap().as_number(), 3.0);
    assert_eq!(vm.get_global("val").unwrap().as_number(), 20.0);
}

#[test]
fn test_object() {
    let vm = run_code("let obj = { x: 100 }\nobj.x = 200\nlet val = obj.x").unwrap();
    assert_eq!(vm.get_global("val").unwrap().as_number(), 200.0);
}

#[test]
fn test_struct() {
    let vm = run_code("struct Player {\n  name: string,\n  age: int,\n}\nlet p : Player = {\n  name: \"Vishnu\",\n  age: 25,\n}\nlet val = p.name").unwrap();
    assert_eq!(vm.get_global("val").unwrap().as_str().unwrap(), "Vishnu");
}

#[test]
fn test_struct_type_safety() {
    let code = "struct Player {\n  name: string,\n  age: int,\n}\nlet p : Player = {\n  name: 67,\n  age: 25,\n}";
    let res = run_code(code);
    match res {
        Err(err) => {
            assert!(err.contains("Expected type \"string\" but got 67"));
        }
        Ok(_) => {
            panic!("Expected type error but code compiled successfully");
        }
    }
}

#[test]
fn test_struct_mutation() {
    let vm = run_code("struct Player {\n  name: string,\n  age: int,\n}\nlet p : Player = {\n  name: \"Vishnu\",\n  age: 25,\n}\np.age = 26\nlet val = p.age").unwrap();
    assert_eq!(vm.get_global("val").unwrap().as_number(), 26.0);
}

#[test]
fn test_struct_methods() {
    let vm = run_code("struct Player {\n  name: string,\n  age: int,\n  fn printPlayer() {\n    return this.name\n  }\n}\nlet p : Player = {\n  name: \"Vishnu\",\n  age: 25,\n}\nlet val = p.printPlayer()").unwrap();
    assert_eq!(vm.get_global("val").unwrap().as_str().unwrap(), "Vishnu");
}

#[test]
fn test_struct_nested_typecheck() {
    let code = "struct Position {\n  x: int,\n  y: int,\n}\nstruct Player {\n  pos: Position,\n  name: string,\n}\nlet position : Position = {\n  x: 10,\n  y: 20,\n}\nlet p : Player = {\n  pos: position,\n  name: \"Vishnu\",\n}\nlet val = p.pos.x";
    let vm = run_code(code).unwrap();
    assert_eq!(vm.get_global("val").unwrap().as_number(), 10.0);
}

#[test]
fn test_struct_composition() {
    let code = "struct Position {\n  x: int,\n  y: int,\n  fn printPos() {\n    return this.x\n  }\n}\nstruct Parent {\n  fn getVal() {\n    return 100\n  }\n}\nstruct Player embed Position, Parent {\n  name: string,\n  fn printPlayer() {\n    return this.name\n  }\n  fn getVal() {\n    return super.getVal() + 5\n  }\n}\nlet p : Player = {\n  x: 10,\n  y: 20,\n  name: \"Vishnu\",\n}\nlet val_x = p.printPos()\nlet val_name = p.printPlayer()\nlet val_super = p.getVal()";
    let vm = run_code(code).unwrap();
    assert_eq!(vm.get_global("val_x").unwrap().as_number(), 10.0);
    assert_eq!(vm.get_global("val_name").unwrap().as_str().unwrap(), "Vishnu");
    assert_eq!(vm.get_global("val_super").unwrap().as_number(), 105.0);
}

#[test]
fn test_interfaces() {
    let code = "interface Barker {\n  name: string,\n  fn bark()\n}\nstruct Dog {\n  name: string,\n  age: int,\n  fn bark() {\n    return \"Woof! \" + this.name\n  }\n}\nlet pet: Barker = Dog({\n  name: \"Rex\",\n  age: 3,\n})\nlet message = pet.bark()";
    let vm = run_code(code).unwrap();
    assert_eq!(vm.get_global("message").unwrap().as_str().unwrap(), "Woof! Rex");
}

#[test]
fn test_interfaces_invalid() {
    let code = "interface Barker {\n  name: string,\n  fn bark()\n}\nstruct Cat {\n  name: string,\n  age: int\n}\nlet pet: Barker = Cat({\n  name: \"Whiskers\",\n  age: 2,\n})";
    let result = run_code(code);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("does not implement interface"));
}

#[test]
fn test_struct_new_constructor_syntax() {
    let code = r#"
        struct Dog {
            name: string,
            age: int
        }

        const d = Dog()
        let val_d_name = d.name

        user1 = Dog("Vishnu")
        let val_u1_name = user1.name
        let val_u1_age = user1.age

        const user2 = Dog({ name: "vishnu" })
        let val_u2_name = user2.name

        let user3 : Dog = []
        let val_u3_name = user3.name

        let user4 : Dog = [{}]
        let val_u4_name = user4[0].name

        user5 = Dog([ { name: "A" }, { name: "B" } ])
        let val_u5_name0 = user5[0].name
        let val_u5_name1 = user5[1].name
    "#;
    let vm = run_code(code).unwrap();
    assert!(vm.get_global("val_d_name").unwrap().is_null());
    assert_eq!(vm.get_global("val_u1_name").unwrap().as_str().unwrap(), "Vishnu");
    assert!(vm.get_global("val_u1_age").unwrap().is_null());
    assert_eq!(vm.get_global("val_u2_name").unwrap().as_str().unwrap(), "vishnu");
    assert!(vm.get_global("val_u3_name").unwrap().is_null());
    assert!(vm.get_global("val_u4_name").unwrap().is_null());
    assert_eq!(vm.get_global("val_u5_name0").unwrap().as_str().unwrap(), "A");
    assert_eq!(vm.get_global("val_u5_name1").unwrap().as_str().unwrap(), "B");
}

#[test]
fn test_incremental_garbage_collector() {
    gc_free_all();

    let parent_ptr = gc_allocate(GcData::Array(vec![]));
    let parent = Value::array(parent_ptr);

    let garbage_ptr = gc_allocate(GcData::Array(vec![]));
    let _garbage = Value::array(garbage_ptr);

    let mut vm = VM::new();
    vm.stack.push(parent);

    assert_eq!(gc_with_state(|s| s.phase), GcPhase::Pause);

    for _ in 0..10000 {
        gc_allocate(GcData::Array(vec![]));
    }

    vm.gc_step();
    assert_eq!(gc_with_state(|s| s.phase), GcPhase::Mark);

    while gc_with_state(|s| s.phase) != GcPhase::Sweep {
        vm.gc_step();
    }

    while gc_with_state(|s| s.phase) == GcPhase::Sweep {
        vm.gc_step();
    }

    assert_eq!(gc_with_state(|s| s.phase), GcPhase::Pause);

    let mut found_parent = false;
    let mut found_garbage = false;
    unsafe {
        let mut curr = gc_with_state(|s| s.head);
        while !curr.is_null() {
            if curr == parent_ptr {
                found_parent = true;
            }
            if curr == garbage_ptr {
                found_garbage = true;
            }
            curr = (*curr).next;
        }
    }
    assert!(found_parent, "Parent should be alive");
    assert!(!found_garbage, "Garbage should be collected");

    gc_free_all();
}

#[test]
fn test_gc_stack_roots_deep_no_truncation() {
    gc_free_all();

    let mut vm = VM::new();
    // Fill stack with 500 null values (> 256)
    for _ in 0..500 {
        vm.stack.push(Value::null());
    }

    // Place a live object at slot 450 (which would have been ignored by 256 truncation)
    let deep_ptr = gc_allocate(GcData::Array(vec![Value::number(42.0)]));
    vm.stack[450] = Value::array(deep_ptr);

    let garbage_ptr = gc_allocate(GcData::Array(vec![Value::number(999.0)]));
    let _garbage = Value::array(garbage_ptr);

    // Run full GC collection
    vm.collect_garbage();

    let mut found_deep = false;
    let mut found_garbage = false;
    unsafe {
        let mut curr = gc_with_state(|s| s.head);
        while !curr.is_null() {
            if curr == deep_ptr {
                found_deep = true;
            }
            if curr == garbage_ptr {
                found_garbage = true;
            }
            curr = (*curr).next;
        }
    }
    assert!(found_deep, "Deep stack object (>256 slots) MUST be kept alive");
    assert!(!found_garbage, "Unreferenced garbage object must be reclaimed");

    gc_free_all();
}

#[test]
fn test_gc_string_cache_sweep() {
    gc_free_all();

    let mut vm = VM::new();

    let live_ptr = crate::vm::gc::intern_string("live_constant_identifier");
    let _dead_ptr = crate::vm::gc::intern_string("transient_dead_identifier");

    // Reference live_ptr in global variables
    vm.globals.insert("live_id".into(), Value::string(live_ptr));

    // Ensure both are in cache initially
    crate::vm::gc::STRING_CACHE.with(|cache| {
        let c = cache.borrow();
        assert!(c.contains_key("live_constant_identifier"));
        assert!(c.contains_key("transient_dead_identifier"));
    });

    // Run GC collection
    vm.collect_garbage();

    // Verify cache sweeping: live retained, dead evicted
    crate::vm::gc::STRING_CACHE.with(|cache| {
        let c = cache.borrow();
        assert!(c.contains_key("live_constant_identifier"), "Live interned string must be retained");
        assert!(!c.contains_key("transient_dead_identifier"), "Dead interned string must be evicted");
    });

    // Verify that re-interning live string returns identical pointer
    let live_ptr_2 = crate::vm::gc::intern_string("live_constant_identifier");
    assert_eq!(live_ptr, live_ptr_2);

    gc_free_all();
}

#[test]
fn test_gc_atomic_flag() {
    gc_free_all();

    assert_eq!(GC_NEEDS_STEP.load(Ordering::Relaxed), false);

    let threshold = gc_with_state(|s| s.alloc_threshold);
    for _ in 0..threshold {
        gc_allocate(GcData::Array(vec![]));
    }

    assert_eq!(GC_NEEDS_STEP.load(Ordering::Relaxed), true);

    let mut vm = VM::new();
    vm.collect_garbage();

    assert_eq!(GC_NEEDS_STEP.load(Ordering::Relaxed), false);

    gc_free_all();
}
