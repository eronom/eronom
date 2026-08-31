pub mod opt;
pub mod type_flow;
pub mod emit_header;
pub mod emit_ops;
pub mod emit_math;
pub mod emit_cmp;
pub mod emit_cmp_equal;
pub mod emit_cmp_rel;
pub mod emit_obj;
pub mod emit_call;

use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicUsize, Ordering};
use fnv::FnvHashMap;
use crate::vm::bytecode::{Instruction, OpCode};
use crate::vm::execute::VM;
use crate::vm::gc::{GcData, GcObject};
use super::bindings::{
    _MIR_init, MIR_finish, MIR_scan_string, MIR_load_module, MIR_load_external,
    MIR_link, MIR_gen_init, MIR_gen, MIR_gen_finish, MIR_get_module_list,
    MIR_set_gen_interface, MIR_gen_set_optimize_level, MirDlist, MirModule,
};
use super::helpers;

use self::opt::eliminate_dead_instructions;
use self::type_flow::{analyze_types, RegType};
use self::emit_header::{calculate_max_regs, calculate_param_doubles, calculate_resume_targets, emit_prologue};
use self::emit_ops::emit_op;
use self::emit_obj::emit_obj;
use self::emit_call::emit_call_and_control;

static JIT_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn next_id() -> usize {
    JIT_COUNTER.fetch_add(1, Ordering::Relaxed)
}

struct ThreadJitState {
    ctx: *mut c_void,
    cache: FnvHashMap<Vec<Instruction>, *const c_void>,
}

impl Drop for ThreadJitState {
    fn drop(&mut self) {
        unsafe {
            MIR_gen_finish(self.ctx);
            MIR_finish(self.ctx);
        }
    }
}

thread_local! {
    static JIT_STATE: std::cell::RefCell<Option<ThreadJitState>> = const { std::cell::RefCell::new(None) };
}

// Ensure the JIT is initialized for the VM context
pub fn get_or_init_jit_ctx(_vm: &mut VM) -> *mut c_void {
    JIT_STATE.with(|state| {
        let mut borrow = state.borrow_mut();
        if borrow.is_none() {
            unsafe {
                let ctx = _MIR_init(std::ptr::null_mut(), std::ptr::null_mut());
                MIR_gen_init(ctx);
                MIR_gen_set_optimize_level(ctx, 3);
                register_helpers(ctx);
                *borrow = Some(ThreadJitState {
                    ctx,
                    cache: FnvHashMap::default(),
                });
            }
        }
        borrow.as_ref().unwrap().ctx
    })
}

// Safely free all MIR native code buffers and clear the JIT cache on hot-reloads
pub fn reset_jit_state() {
    JIT_STATE.with(|state| {
        *state.borrow_mut() = None;
    });
    crate::jit::helpers::reset_global_ic();
}

pub fn compile_function(vm: &mut VM, func_obj: *mut GcObject) -> *const c_void {
    let func = match unsafe { &(*func_obj).data } {
        GcData::Function(f) => f,
        _ => panic!("Expected function object"),
    };

    if let Some(ptr) = func.jit_ptr.get() {
        return ptr;
    }

    let instructions = &func.chunk.code;
    let cached_ptr = JIT_STATE.with(|state| {
        let borrow = state.borrow();
        if let Some(s) = borrow.as_ref() {
            s.cache.get(instructions).copied()
        } else {
            None
        }
    });

    if let Some(ptr) = cached_ptr {
        func.jit_ptr.set(Some(ptr));
        return ptr;
    }

    let ctx = get_or_init_jit_ctx(vm);
    let id = next_id();
    let module_name = format!("m_{}", id);
    let func_name = format!("jit_fn_{}", id);

    let num_regs = calculate_max_regs(func);
    let param_is_double = calculate_param_doubles(func);
    let is_resume_target = calculate_resume_targets(func);

    let (is_dead, live_in, _live_out) = eliminate_dead_instructions(func, num_regs, &is_resume_target);
    let (types_at_inst, is_init) = analyze_types(func, num_regs, &param_is_double);

    let mut mir = String::new();
    emit_prologue(
        &mut mir,
        &module_name,
        &func_name,
        func,
        num_regs,
        &param_is_double,
        &is_resume_target,
    );

    let sync_edge = |mir: &mut String, pc: usize, target: usize| {
        if target < types_at_inst.len() {
            for r in 0..num_regs {
                if live_in[target][r] {
                    if types_at_inst[pc][r] == RegType::Double && types_at_inst[target][r] == RegType::Unknown {
                        let offset = (r % 24) * 8;
                        mir.push_str(&format!("          dmov d:{}(cast_ptr), d{}\n", offset, r));
                        mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", r, offset));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", r * 8, r));
                    }
                }
            }
        }
    };

    // Entry points conversion: from r{} to d{}
    for ip_target in 0..func.chunk.code.len() {
        if !is_resume_target[ip_target] {
            continue;
        }
        mir.push_str(&format!("entry_{}:\n", ip_target));
        for r in 0..num_regs {
            if live_in[ip_target][r] && types_at_inst[ip_target][r] == RegType::Double {
                let offset = (r % 24) * 8;
                mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset, r));
                mir.push_str(&format!("          dmov d{}, d:{}(cast_ptr)\n", r, offset));
            }
        }
        mir.push_str(&format!("          jmp inst_{}\n", ip_target));
    }

    let has_closures = func.chunk.code.iter().any(|inst| inst.op == OpCode::Closure);
    let save_all_registers = |mir: &mut String, idx: usize| {
        for r in 0..num_regs {
            if is_init[idx][r] && (has_closures || live_in[idx][r]) {
                if types_at_inst[idx][r] == RegType::Double {
                    mir.push_str(&format!("          dmov d:{}(frame_slots), d{}\n", r * 8, r));
                } else {
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", r * 8, r));
                }
            }
        }
    };

    for (idx, instruction) in func.chunk.code.iter().enumerate() {
        mir.push_str(&format!("inst_{}:\n", idx));

        let mut next_types = types_at_inst[idx].clone();
        let ra = instruction.ra as usize;
        let rb = instruction.rb as usize;
        let rc = instruction.rc as usize;
        match instruction.op {
            OpCode::LoadConst => {
                let val = func.chunk.constants[instruction.operand as usize];
                if ra < num_regs {
                    next_types[ra] = if val.is_number() { RegType::Double } else { RegType::Unknown };
                }
            }
            OpCode::Move => {
                if ra < num_regs && rb < num_regs {
                    next_types[ra] = types_at_inst[idx][rb];
                }
            }
            OpCode::Add => {
                if ra < num_regs && rb < num_regs && rc < num_regs {
                    next_types[ra] = if types_at_inst[idx][rb] == RegType::Double && types_at_inst[idx][rc] == RegType::Double {
                        RegType::Double
                    } else {
                        RegType::Unknown
                    };
                }
            }
            OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Negate |
            OpCode::Mod | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor | OpCode::BitNot | OpCode::ShiftLeft | OpCode::ShiftRight | OpCode::ArrayLen => {
                if ra < num_regs {
                    next_types[ra] = RegType::Double;
                }
            }
            OpCode::Equal | OpCode::Greater | OpCode::Less => {
                let is_fused_3 = if idx + 2 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[idx + 1];
                    let jmp_inst = &func.chunk.code[idx + 2];
                    next_inst.op == OpCode::Not && next_inst.ra == instruction.ra && next_inst.rb == instruction.ra
                        && jmp_inst.op == OpCode::JumpIfFalse && jmp_inst.ra == instruction.ra
                } else {
                    false
                };
                let is_fused_2 = if !is_fused_3 && idx + 1 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[idx + 1];
                    next_inst.op == OpCode::JumpIfFalse && next_inst.ra == instruction.ra
                } else {
                    false
                };
                if !is_fused_3 && !is_fused_2 && ra < num_regs {
                    next_types[ra] = RegType::Unknown;
                }
            }
            OpCode::Not |
            OpCode::LoadNull | OpCode::LoadBool |
            OpCode::GetGlobal | OpCode::GetProperty | OpCode::GetIndex |
            OpCode::MakeArray | OpCode::MakeObject | OpCode::TypeOf | OpCode::ToIter |
            OpCode::GetUpvalue | OpCode::Closure | OpCode::Await | OpCode::Call => {
                if ra < num_regs {
                    next_types[ra] = RegType::Unknown;
                }
            }
            _ => {}
        }

        if is_dead[idx] {
            continue;
        }

        match instruction.op {
            OpCode::LoadConst | OpCode::LoadNull | OpCode::LoadBool | OpCode::Move |
            OpCode::Negate | OpCode::Not | OpCode::Add | OpCode::Sub | OpCode::Mul |
            OpCode::Div | OpCode::Mod | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor |
            OpCode::BitNot | OpCode::ShiftLeft | OpCode::ShiftRight | OpCode::TypeOf |
            OpCode::ArrayLen | OpCode::Equal | OpCode::Greater | OpCode::Less => {
                emit_op(
                    &mut mir,
                    idx,
                    instruction,
                    func,
                    ra,
                    rb,
                    rc,
                    num_regs,
                    &types_at_inst,
                    &next_types,
                    &sync_edge,
                );
            }
            OpCode::DefineGlobal | OpCode::DefineStruct | OpCode::GetGlobal | OpCode::SetGlobal |
            OpCode::MakeArray | OpCode::MakeObject | OpCode::GetProperty | OpCode::SetProperty |
            OpCode::GetIndex | OpCode::SetIndex => {
                emit_obj(
                    &mut mir,
                    idx,
                    instruction,
                    func,
                    ra,
                    rb,
                    rc,
                    num_regs,
                    &types_at_inst,
                    &next_types,
                );
            }
            OpCode::Jump | OpCode::Loop | OpCode::JumpIfFalse | OpCode::Call |
            OpCode::Return | OpCode::Throw | OpCode::GetUpvalue | OpCode::SetUpvalue |
            OpCode::Closure | OpCode::CloseUpvalue | OpCode::Await => {
                emit_call_and_control(
                    &mut mir,
                    idx,
                    instruction,
                    func,
                    func_obj,
                    &func_name,
                    ra,
                    rb,
                    rc,
                    num_regs,
                    &types_at_inst,
                    &next_types,
                    &is_init,
                    &live_in,
                    &save_all_registers,
                    &sync_edge,
                );
            }
            _ => {}
        }
    }

    mir.push_str("          ret 1\n");
    mir.push_str("err_label:\n");
    mir.push_str("          ret -1\n");
    mir.push_str("          endfunc\n");
    mir.push_str("          endmodule\n");

    let debug_jit = std::env::var("ER_DEBUG_JIT").is_ok();
    if debug_jit {
        static MIR_COUNT: AtomicUsize = AtomicUsize::new(0);
        let id = MIR_COUNT.fetch_add(1, Ordering::SeqCst);
        let name_str = func.name.as_deref().unwrap_or("anon");
        let path = format!("/home/vishnus/Downloads/eronom/temp_compiled_{}_{}.mir", id, name_str);
        let _ = std::fs::write(&path, &mir);
    }
    let c_mir = CString::new(mir).unwrap();

    unsafe {
        MIR_scan_string(ctx, c_mir.as_ptr());
        let list_ptr = MIR_get_module_list(ctx) as *mut MirDlist;
        let module = (*list_ptr).tail as *mut MirModule;
        MIR_load_module(ctx, module as *mut c_void);
        MIR_link(ctx, Some(MIR_set_gen_interface), None);
        
        // Find the function item (type 0) dynamically
        let mut func_item = std::ptr::null_mut();
        let mut curr = (*module).head_item as *mut *mut c_void;
        while !curr.is_null() {
            let item_type = *(curr.offset(4) as *const u32);
            if item_type == 0 {
                func_item = curr as *mut c_void;
                break;
            }
            let next = *(curr.offset(3) as *const *mut c_void);
            curr = next as *mut *mut c_void;
        }

        assert!(!func_item.is_null(), "Could not find MIR_func_item in JIT module");
        let native_ptr = MIR_gen(ctx, func_item);
        
        JIT_STATE.with(|state| {
            if let Some(s) = &mut *state.borrow_mut() {
                s.cache.insert(instructions.clone(), native_ptr);
            }
        });

        func.jit_ptr.set(Some(native_ptr));
        native_ptr
    }
}

// Register FFI Helper Functions in the MIR JIT compiler
unsafe fn register_helpers(ctx: *mut c_void) {
    let helpers: &[(&str, *mut c_void)] = &[
        ("er_jit_negate", helpers::er_jit_negate as *mut c_void),
        ("er_jit_not", helpers::er_jit_not as *mut c_void),
        ("er_jit_add", helpers::er_jit_add as *mut c_void),
        ("er_jit_sub", helpers::er_jit_sub as *mut c_void),
        ("er_jit_mul", helpers::er_jit_mul as *mut c_void),
        ("er_jit_div", helpers::er_jit_div as *mut c_void),
        ("er_jit_mod", helpers::er_jit_mod as *mut c_void),
        ("er_jit_bit_and", helpers::er_jit_bit_and as *mut c_void),
        ("er_jit_bit_or", helpers::er_jit_bit_or as *mut c_void),
        ("er_jit_bit_xor", helpers::er_jit_bit_xor as *mut c_void),
        ("er_jit_bit_not", helpers::er_jit_bit_not as *mut c_void),
        ("er_jit_shift_left", helpers::er_jit_shift_left as *mut c_void),
        ("er_jit_shift_right", helpers::er_jit_shift_right as *mut c_void),
        ("er_jit_typeof", helpers::er_jit_typeof as *mut c_void),
        ("er_jit_to_iter", helpers::er_jit_to_iter as *mut c_void),
        ("er_jit_array_len_op", helpers::er_jit_array_len_op as *mut c_void),
        ("er_jit_equal", helpers::er_jit_equal as *mut c_void),
        ("er_jit_greater", helpers::er_jit_greater as *mut c_void),
        ("er_jit_less", helpers::er_jit_less as *mut c_void),
        ("er_jit_define_global", helpers::er_jit_define_global as *mut c_void),
        ("er_jit_get_global", helpers::er_jit_get_global as *mut c_void),
        ("er_jit_set_global", helpers::er_jit_set_global as *mut c_void),
        ("er_jit_make_array", helpers::er_jit_make_array as *mut c_void),
        ("er_jit_make_object", helpers::er_jit_make_object as *mut c_void),
        ("er_jit_get_property", helpers::er_jit_get_property as *mut c_void),
        ("er_jit_set_property", helpers::er_jit_set_property as *mut c_void),
        ("er_jit_get_index", helpers::er_jit_get_index as *mut c_void),
        ("er_jit_set_index", helpers::er_jit_set_index as *mut c_void),
        ("er_jit_call_fast", helpers::er_jit_call_fast as *mut c_void),
        ("er_jit_call_non_vm", helpers::er_jit_call_non_vm as *mut c_void),
        ("er_jit_array_push", helpers::er_jit_array_push as *mut c_void),
        ("er_jit_array_pop", helpers::er_jit_array_pop as *mut c_void),
        ("er_jit_has_error", helpers::er_jit_has_error as *mut c_void),
        ("er_jit_needs_gc", helpers::er_jit_needs_gc as *mut c_void),
        ("er_gc_needs_step", &crate::vm::gc::GC_NEEDS_STEP as *const _ as *mut c_void),
        ("er_jit_define_struct", helpers::er_jit_define_struct as *mut c_void),
        ("er_jit_get_upvalue", helpers::er_jit_get_upvalue as *mut c_void),
        ("er_jit_set_upvalue", helpers::er_jit_set_upvalue as *mut c_void),
        ("er_jit_make_closure", helpers::er_jit_make_closure as *mut c_void),
        ("er_jit_close_upvalues", helpers::er_jit_close_upvalues as *mut c_void),
        ("er_jit_await", helpers::er_jit_await as *mut c_void),
        ("er_jit_write_barrier", helpers::er_jit_write_barrier as *mut c_void),
    ];

    for &(name, ptr) in helpers {
        let cname = CString::new(name).unwrap();
        unsafe {
            MIR_load_external(ctx, cname.as_ptr(), ptr);
        }
    }
}
