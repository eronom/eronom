use std::ffi::{c_void, CString, c_char};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashMap;
use std::rc::Rc;
use super::value::{Value, TAG_FALSE, TAG_NULL, TAG_TRUE};
use super::bytecode::{OpCode, Instruction};
use super::execute::VM;
use super::gc::{gc_allocate, GcData, GcObject, gc_write_barrier};

static JIT_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn next_id() -> usize {
    JIT_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[repr(C)]
pub struct MirDlist {
    pub head: *mut c_void,
    pub tail: *mut c_void,
}

#[repr(C)]
pub struct MirModule {
    pub data: *mut c_void,
    pub name: *const c_char,
    pub head_item: *mut c_void,
    pub tail_item: *mut c_void,
}

unsafe extern "C" {
    fn _MIR_init(alloc: *mut c_void, code_alloc: *mut c_void) -> *mut c_void;
    fn MIR_finish(ctx: *mut c_void);
    fn MIR_scan_string(ctx: *mut c_void, str: *const c_char);
    fn MIR_load_module(ctx: *mut c_void, module: *mut c_void);
    fn MIR_load_external(ctx: *mut c_void, name: *const c_char, addr: *mut c_void);
    fn MIR_link(
        ctx: *mut c_void,
        set_interface: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
        import_resolver: Option<unsafe extern "C" fn(*const c_char) -> *mut c_void>,
    );
    fn MIR_gen_init(ctx: *mut c_void);
    fn MIR_gen(ctx: *mut c_void, func_item: *mut c_void) -> *mut c_void;
    fn MIR_gen_finish(ctx: *mut c_void);
    fn MIR_get_module_list(ctx: *mut c_void) -> *mut c_void;
    fn MIR_set_gen_interface(ctx: *mut c_void, func_item: *mut c_void);
    fn MIR_gen_set_optimize_level(ctx: *mut c_void, level: u32);
}

struct ThreadJitState {
    ctx: *mut c_void,
    cache: HashMap<Vec<Instruction>, *const c_void>,
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
    static JIT_STATE: std::cell::RefCell<Option<ThreadJitState>> = std::cell::RefCell::new(None);
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
                    cache: HashMap::new(),
                });
            }
        }
        borrow.as_ref().unwrap().ctx
    })
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

    let mut mir = String::new();
    mir.push_str(&format!("{}: module\n", module_name));
    mir.push_str(&format!("          export {}\n", func_name));
    mir.push_str("          import er_jit_negate, er_jit_not, er_jit_add, er_jit_sub, er_jit_mul, er_jit_div, er_jit_equal, er_jit_greater, er_jit_less, er_jit_define_global, er_jit_get_global, er_jit_set_global, er_jit_make_array, er_jit_make_object, er_jit_get_property, er_jit_set_property, er_jit_get_index, er_jit_set_index\n");

    // Signature: returns status code (i64), arguments are pointers to vm, frame_slots, constants, etc.
    mir.push_str(&format!(
        "{}: func i64, p:vm, p:frame_slots, p:constants_ptr, i64:start_ip, p:ip_out, p:dest_reg_out, p:func_reg_out, p:arg_count_out, p:ret_val_out\n",
        func_name
    ));

    mir.push_str("p_negate: proto i64, p:vm, p:dest, p:src\n");
    mir.push_str("p_not: proto i64, p:dest, p:src\n");
    mir.push_str("p_add: proto i64, p:vm, p:dest, p:b, p:c\n");
    mir.push_str("p_sub: proto i64, p:vm, p:dest, p:b, p:c\n");
    mir.push_str("p_mul: proto i64, p:vm, p:dest, p:b, p:c\n");
    mir.push_str("p_div: proto i64, p:vm, p:dest, p:b, p:c\n");
    mir.push_str("p_equal: proto i64, p:vm, p:dest, p:b, p:c\n");
    mir.push_str("p_greater: proto i64, p:vm, p:dest, p:b, p:c\n");
    mir.push_str("p_less: proto i64, p:vm, p:dest, p:b, p:c\n");
    mir.push_str("p_def_global: proto i64, p:vm, p:name, p:val\n");
    mir.push_str("p_get_global: proto i64, p:vm, p:dest, p:name\n");
    mir.push_str("p_set_global: proto i64, p:vm, p:val, p:name\n");
    mir.push_str("p_make_array: proto i64, p:vm, p:dest, p:start, i64:count\n");
    mir.push_str("p_make_object: proto i64, p:vm, p:dest, p:start, i64:count\n");
    mir.push_str("p_get_property: proto i64, p:vm, p:dest, p:obj, p:name\n");
    mir.push_str("p_set_property: proto i64, p:vm, p:obj, p:val, p:name\n");
    mir.push_str("p_get_index: proto i64, p:vm, p:dest, p:obj, p:idx\n");
    mir.push_str("p_set_index: proto i64, p:vm, p:obj, p:idx, p:val\n");

    mir.push_str("          local i64:tmp, i64:tmp1, i64:tmp2, i64:status, i64:res_bool, i64:res_val, i64:cast_ptr\n");
    mir.push_str("          local i64:ra_ptr, i64:rb_ptr, i64:rc_ptr, i64:name_ptr, i64:val_ptr, i64:start_ptr, i64:dest_ptr, i64:idx_ptr, i64:obj_ptr\n");
    mir.push_str("          local d:da, d:db, d:dres\n");

    let mut num_regs = func.arity;
    for inst in &func.chunk.code {
        let max_accessed = match inst.op {
            OpCode::MakeArray => inst.rb as usize + inst.operand as usize,
            OpCode::MakeObject => inst.rb as usize + 2 * (inst.operand as usize),
            OpCode::Call => inst.rb as usize + 1 + inst.operand as usize,
            _ => {
                let mut m = inst.ra as usize;
                if inst.rb as usize > m { m = inst.rb as usize; }
                if inst.rc as usize > m { m = inst.rc as usize; }
                m + 1
            }
        };
        if max_accessed > num_regs {
            num_regs = max_accessed;
        }
    }

    if num_regs > 0 {
        let mut local_regs = String::new();
        for i in 0..num_regs {
            if i > 0 {
                local_regs.push_str(", ");
            }
            local_regs.push_str(&format!("i64:r{}", i));
        }
        mir.push_str(&format!("          local {}\n", local_regs));
    }

    mir.push_str("          alloca cast_ptr, 192\n");

    for i in 0..num_regs {
        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", i, i * 8));
    }

    for ip_target in 0..func.chunk.code.len() {
        mir.push_str(&format!("          beq inst_{}, start_ip, {}\n", ip_target, ip_target));
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum RegType {
        Unknown,
        Double,
    }

    let mut types_at_inst = vec![vec![RegType::Double; num_regs]; func.chunk.code.len()];
    if num_regs > 0 {
        for r in 0..num_regs {
            types_at_inst[0][r] = RegType::Unknown;
        }
    }

    let mut worklist = vec![0];
    let mut in_worklist = vec![false; func.chunk.code.len()];
    if !func.chunk.code.is_empty() {
        in_worklist[0] = true;
    }

    while let Some(pc) = worklist.pop() {
        in_worklist[pc] = false;
        let current_types = &types_at_inst[pc];
        let inst = &func.chunk.code[pc];

        let mut next_types = current_types.clone();
        let ra = inst.ra as usize;
        let rb = inst.rb as usize;
        let rc = inst.rc as usize;

        match inst.op {
            OpCode::LoadConst => {
                let val = func.chunk.constants[inst.operand as usize];
                if ra < num_regs {
                    next_types[ra] = if val.is_number() { RegType::Double } else { RegType::Unknown };
                }
            }
            OpCode::Move => {
                if ra < num_regs && rb < num_regs {
                    next_types[ra] = current_types[rb];
                }
            }
            OpCode::Add => {
                if ra < num_regs && rb < num_regs && rc < num_regs {
                    next_types[ra] = if current_types[rb] == RegType::Double && current_types[rc] == RegType::Double {
                        RegType::Double
                    } else {
                        RegType::Unknown
                    };
                }
            }
            OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Negate => {
                if ra < num_regs {
                    next_types[ra] = RegType::Double;
                }
            }
            OpCode::Not | OpCode::Equal | OpCode::Greater | OpCode::Less |
            OpCode::LoadNull | OpCode::LoadBool |
            OpCode::GetGlobal | OpCode::GetProperty | OpCode::GetIndex |
            OpCode::MakeArray | OpCode::MakeObject => {
                if ra < num_regs {
                    next_types[ra] = RegType::Unknown;
                }
            }
            _ => {}
        }

        let mut successors = Vec::new();
        match inst.op {
            OpCode::Return => {}
            OpCode::Jump => {
                let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                successors.push(target);
            }
            OpCode::Loop => {
                let target = (pc as i32 + 1 - inst.operand as i32) as usize;
                successors.push(target);
            }
            OpCode::JumpIfFalse => {
                let target = (pc as i32 + 1 + inst.operand as i32) as usize;
                successors.push(pc + 1);
                successors.push(target);
            }
            _ => {
                successors.push(pc + 1);
            }
        }

        for succ in successors {
            if succ >= func.chunk.code.len() {
                continue;
            }
            let mut changed = false;
            for r in 0..num_regs {
                let old = types_at_inst[succ][r];
                let new_t = if old == RegType::Double && next_types[r] == RegType::Double {
                    RegType::Double
                } else {
                    RegType::Unknown
                };
                if new_t != old {
                    types_at_inst[succ][r] = new_t;
                    changed = true;
                }
            }
            if changed && !in_worklist[succ] {
                worklist.push(succ);
                in_worklist[succ] = true;
            }
        }
    }

    for (idx, instruction) in func.chunk.code.iter().enumerate() {
        mir.push_str(&format!("inst_{}:\n", idx));
        match instruction.op {
            OpCode::LoadConst => {
                let ra = instruction.ra;
                let c_idx = instruction.operand;
                mir.push_str(&format!("          mov tmp, i64:{}(constants_ptr)\n", c_idx * 8));
                mir.push_str(&format!("          mov r{}, tmp\n", ra));
            }
            OpCode::LoadNull => {
                let ra = instruction.ra;
                mir.push_str(&format!("          mov r{}, {}\n", ra, TAG_NULL));
            }
            OpCode::LoadBool => {
                let ra = instruction.ra;
                let val = instruction.rb;
                let tag = if val != 0 { Value::boolean(true).0 } else { Value::boolean(false).0 };
                mir.push_str(&format!("          mov r{}, {}\n", ra, tag));
            }
            OpCode::Move => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                mir.push_str(&format!("          mov r{}, r{}\n", ra, rb));
            }
            OpCode::Negate => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;

                let rb_is_double = rb < num_regs as u8 && types_at_inst[idx][rb as usize] == RegType::Double;

                if rb_is_double {
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str("          dneg dres, da\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset2));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset2));
                } else {
                    mir.push_str(&format!("          ubge fallback_neg_{}, r{}, 0xfff0000000000000\n", idx, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str("          dneg dres, da\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset2));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset2));
                    mir.push_str(&format!("          jmp done_neg_{}\n", idx));
                    mir.push_str(&format!("fallback_neg_{}:\n", idx));
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                    mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                    mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                    mir.push_str("          call p_negate, er_jit_negate, status, vm, ra_ptr, rb_ptr\n");
                    mir.push_str("          blt err_label, status, 0\n");
                    mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                    mir.push_str(&format!("done_neg_{}:\n", idx));
                }
            }
            OpCode::Not => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                mir.push_str(&format!("          mov r{}, {}\n", ra, TAG_FALSE));
                mir.push_str(&format!("          beq done_not_{}, r{}, {}\n", idx, rb, TAG_TRUE));
                mir.push_str(&format!("          mov tmp1, {}\n", TAG_TRUE));
                mir.push_str(&format!("          beq set_true_{}, r{}, {}\n", idx, rb, TAG_FALSE));
                mir.push_str(&format!("          beq set_true_{}, r{}, {}\n", idx, rb, TAG_NULL));
                mir.push_str(&format!("          jmp done_not_{}\n", idx));
                mir.push_str(&format!("set_true_{}:\n", idx));
                mir.push_str(&format!("          mov r{}, tmp1\n", ra));
                mir.push_str(&format!("done_not_{}:\n", idx));
            }
            OpCode::Add => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let rc = instruction.rc;
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;

                let rb_is_double = rb < num_regs as u8 && types_at_inst[idx][rb as usize] == RegType::Double;
                let rc_is_double = rc < num_regs as u8 && types_at_inst[idx][rc as usize] == RegType::Double;

                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                    mir.push_str("          dadd dres, da, db\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset3));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                } else {
                    if !rb_is_double {
                        mir.push_str(&format!("          ubge fallback_add_{}, r{}, 0xfff0000000000000\n", idx, rb));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          ubge fallback_add_{}, r{}, 0xfff0000000000000\n", idx, rc));
                    }
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                    mir.push_str("          dadd dres, da, db\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset3));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    mir.push_str(&format!("          jmp done_add_{}\n", idx));
                    mir.push_str(&format!("fallback_add_{}:\n", idx));
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                    mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                    mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                    mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                    mir.push_str("          call p_add, er_jit_add, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                    mir.push_str("          blt err_label, status, 0\n");
                    mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                    mir.push_str(&format!("done_add_{}:\n", idx));
                }
            }
            OpCode::Sub => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let rc = instruction.rc;
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;

                let rb_is_double = rb < num_regs as u8 && types_at_inst[idx][rb as usize] == RegType::Double;
                let rc_is_double = rc < num_regs as u8 && types_at_inst[idx][rc as usize] == RegType::Double;

                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                    mir.push_str("          dsub dres, da, db\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset3));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                } else {
                    if !rb_is_double {
                        mir.push_str(&format!("          ubge fallback_sub_{}, r{}, 0xfff0000000000000\n", idx, rb));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          ubge fallback_sub_{}, r{}, 0xfff0000000000000\n", idx, rc));
                    }
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                    mir.push_str("          dsub dres, da, db\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset3));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    mir.push_str(&format!("          jmp done_sub_{}\n", idx));
                    mir.push_str(&format!("fallback_sub_{}:\n", idx));
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                    mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                    mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                    mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                    mir.push_str("          call p_sub, er_jit_sub, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                    mir.push_str("          blt err_label, status, 0\n");
                    mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                    mir.push_str(&format!("done_sub_{}:\n", idx));
                }
            }
            OpCode::Mul => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let rc = instruction.rc;
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;

                let rb_is_double = rb < num_regs as u8 && types_at_inst[idx][rb as usize] == RegType::Double;
                let rc_is_double = rc < num_regs as u8 && types_at_inst[idx][rc as usize] == RegType::Double;

                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                    mir.push_str("          dmul dres, da, db\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset3));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                } else {
                    if !rb_is_double {
                        mir.push_str(&format!("          ubge fallback_mul_{}, r{}, 0xfff0000000000000\n", idx, rb));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          ubge fallback_mul_{}, r{}, 0xfff0000000000000\n", idx, rc));
                    }
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                    mir.push_str("          dmul dres, da, db\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset3));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    mir.push_str(&format!("          jmp done_mul_{}\n", idx));
                    mir.push_str(&format!("fallback_mul_{}:\n", idx));
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                    mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                    mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                    mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                    mir.push_str("          call p_mul, er_jit_mul, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                    mir.push_str("          blt err_label, status, 0\n");
                    mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                    mir.push_str(&format!("done_mul_{}:\n", idx));
                }
            }
            OpCode::Div => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let rc = instruction.rc;
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let offset3 = ((idx * 3 + 2) % 24) * 8;

                let rb_is_double = rb < num_regs as u8 && types_at_inst[idx][rb as usize] == RegType::Double;
                let rc_is_double = rc < num_regs as u8 && types_at_inst[idx][rc as usize] == RegType::Double;

                if rb_is_double && rc_is_double {
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                    mir.push_str("          ddiv dres, da, db\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset3));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                } else {
                    if !rb_is_double {
                        mir.push_str(&format!("          ubge fallback_div_{}, r{}, 0xfff0000000000000\n", idx, rb));
                    }
                    if !rc_is_double {
                        mir.push_str(&format!("          ubge fallback_div_{}, r{}, 0xfff0000000000000\n", idx, rc));
                    }
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                    mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                    mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                    mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                    mir.push_str("          ddiv dres, da, db\n");
                    mir.push_str(&format!("          dmov d:{}(cast_ptr), dres\n", offset3));
                    mir.push_str(&format!("          mov r{}, i64:{}(cast_ptr)\n", ra, offset3));
                    mir.push_str(&format!("          jmp done_div_{}\n", idx));
                    mir.push_str(&format!("fallback_div_{}:\n", idx));
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                    mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                    mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                    mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                    mir.push_str("          call p_div, er_jit_div, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                    mir.push_str("          blt err_label, status, 0\n");
                    mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                    mir.push_str(&format!("done_div_{}:\n", idx));
                }
            }
            OpCode::Equal => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let rc = instruction.rc;
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let next_is_jmp_if_false = if idx + 1 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[idx + 1];
                    next_inst.op == OpCode::JumpIfFalse && next_inst.ra == ra
                } else {
                    false
                };

                let rb_is_double = rb < num_regs as u8 && types_at_inst[idx][rb as usize] == RegType::Double;
                let rc_is_double = rc < num_regs as u8 && types_at_inst[idx][rc as usize] == RegType::Double;

                if next_is_jmp_if_false {
                    let next_inst = &func.chunk.code[idx + 1];
                    let target = (idx + 2 + next_inst.operand as usize) as usize;

                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str(&format!("          dbne inst_{}, da, db\n", target));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_eq_{}, r{}, 0xfff0000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_eq_{}, r{}, 0xfff0000000000000\n", idx, rc));
                        }
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str(&format!("          dbne inst_{}, da, db\n", target));
                        mir.push_str(&format!("          jmp done_eq_{}\n", idx));

                        mir.push_str(&format!("fallback_eq_{}:\n", idx));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                        mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                        mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                        mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                        mir.push_str("          call p_equal, er_jit_equal, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                        mir.push_str("          blt err_label, status, 0\n");
                        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                        mir.push_str(&format!("          beq inst_{}, r{}, {}\n", target, ra, TAG_FALSE));
                        mir.push_str(&format!("          beq inst_{}, r{}, {}\n", target, ra, TAG_NULL));
                        mir.push_str(&format!("done_eq_{}:\n", idx));
                    }
                } else {
                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str("          deq res_bool, da, db\n");
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_eq_{}, r{}, 0xfff0000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_eq_{}, r{}, 0xfff0000000000000\n", idx, rc));
                        }
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str("          deq res_bool, da, db\n");
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                        mir.push_str(&format!("          jmp done_eq_{}\n", idx));

                        mir.push_str(&format!("fallback_eq_{}:\n", idx));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                        mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                        mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                        mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                        mir.push_str("          call p_equal, er_jit_equal, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                        mir.push_str("          blt err_label, status, 0\n");
                        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                        mir.push_str(&format!("done_eq_{}:\n", idx));
                    }
                }
            }
            OpCode::Greater => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let rc = instruction.rc;
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let next_is_jmp_if_false = if idx + 1 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[idx + 1];
                    next_inst.op == OpCode::JumpIfFalse && next_inst.ra == ra
                } else {
                    false
                };

                let rb_is_double = rb < num_regs as u8 && types_at_inst[idx][rb as usize] == RegType::Double;
                let rc_is_double = rc < num_regs as u8 && types_at_inst[idx][rc as usize] == RegType::Double;

                if next_is_jmp_if_false {
                    let next_inst = &func.chunk.code[idx + 1];
                    let target = (idx + 2 + next_inst.operand as usize) as usize;

                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str(&format!("          dble inst_{}, da, db\n", target));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_gt_{}, r{}, 0xfff0000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_gt_{}, r{}, 0xfff0000000000000\n", idx, rc));
                        }
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str(&format!("          dble inst_{}, da, db\n", target));
                        mir.push_str(&format!("          jmp done_gt_{}\n", idx));

                        mir.push_str(&format!("fallback_gt_{}:\n", idx));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                        mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                        mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                        mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                        mir.push_str("          call p_greater, er_jit_greater, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                        mir.push_str("          blt err_label, status, 0\n");
                        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                        mir.push_str(&format!("          beq inst_{}, r{}, {}\n", target, ra, TAG_FALSE));
                        mir.push_str(&format!("          beq inst_{}, r{}, {}\n", target, ra, TAG_NULL));
                        mir.push_str(&format!("done_gt_{}:\n", idx));
                    }
                } else {
                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str("          dgt res_bool, da, db\n");
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_gt_{}, r{}, 0xfff0000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_gt_{}, r{}, 0xfff0000000000000\n", idx, rc));
                        }
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str("          dgt res_bool, da, db\n");
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                        mir.push_str(&format!("          jmp done_gt_{}\n", idx));

                        mir.push_str(&format!("fallback_gt_{}:\n", idx));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                        mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                        mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                        mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                        mir.push_str("          call p_greater, er_jit_greater, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                        mir.push_str("          blt err_label, status, 0\n");
                        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                        mir.push_str(&format!("done_gt_{}:\n", idx));
                    }
                }
            }
            OpCode::Less => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let rc = instruction.rc;
                let offset1 = ((idx * 3) % 24) * 8;
                let offset2 = ((idx * 3 + 1) % 24) * 8;
                let next_is_jmp_if_false = if idx + 1 < func.chunk.code.len() {
                    let next_inst = &func.chunk.code[idx + 1];
                    next_inst.op == OpCode::JumpIfFalse && next_inst.ra == ra
                } else {
                    false
                };

                let rb_is_double = rb < num_regs as u8 && types_at_inst[idx][rb as usize] == RegType::Double;
                let rc_is_double = rc < num_regs as u8 && types_at_inst[idx][rc as usize] == RegType::Double;

                if next_is_jmp_if_false {
                    let next_inst = &func.chunk.code[idx + 1];
                    let target = (idx + 2 + next_inst.operand as usize) as usize;

                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str(&format!("          dbge inst_{}, da, db\n", target));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_lt_{}, r{}, 0xfff0000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_lt_{}, r{}, 0xfff0000000000000\n", idx, rc));
                        }
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str(&format!("          dbge inst_{}, da, db\n", target));
                        mir.push_str(&format!("          jmp done_lt_{}\n", idx));

                        mir.push_str(&format!("fallback_lt_{}:\n", idx));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                        mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                        mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                        mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                        mir.push_str("          call p_less, er_jit_less, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                        mir.push_str("          blt err_label, status, 0\n");
                        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                        mir.push_str(&format!("          beq inst_{}, r{}, {}\n", target, ra, TAG_FALSE));
                        mir.push_str(&format!("          beq inst_{}, r{}, {}\n", target, ra, TAG_NULL));
                        mir.push_str(&format!("done_lt_{}:\n", idx));
                    }
                } else {
                    if rb_is_double && rc_is_double {
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str("          dlt res_bool, da, db\n");
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                    } else {
                        if !rb_is_double {
                            mir.push_str(&format!("          ubge fallback_lt_{}, r{}, 0xfff0000000000000\n", idx, rb));
                        }
                        if !rc_is_double {
                            mir.push_str(&format!("          ubge fallback_lt_{}, r{}, 0xfff0000000000000\n", idx, rc));
                        }
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset1, rb));
                        mir.push_str(&format!("          dmov da, d:{}(cast_ptr)\n", offset1));
                        mir.push_str(&format!("          mov i64:{}(cast_ptr), r{}\n", offset2, rc));
                        mir.push_str(&format!("          dmov db, d:{}(cast_ptr)\n", offset2));
                        mir.push_str("          dlt res_bool, da, db\n");
                        mir.push_str("          mul res_val, res_bool, 0x0001000000000000\n");
                        mir.push_str("          add res_val, res_val, 0xfff2000000000000\n");
                        mir.push_str(&format!("          mov r{}, res_val\n", ra));
                        mir.push_str(&format!("          jmp done_lt_{}\n", idx));

                        mir.push_str(&format!("fallback_lt_{}:\n", idx));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                        mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                        mir.push_str(&format!("          add ra_ptr, frame_slots, {}\n", ra * 8));
                        mir.push_str(&format!("          add rb_ptr, frame_slots, {}\n", rb * 8));
                        mir.push_str(&format!("          add rc_ptr, frame_slots, {}\n", rc * 8));
                        mir.push_str("          call p_less, er_jit_less, status, vm, ra_ptr, rb_ptr, rc_ptr\n");
                        mir.push_str("          blt err_label, status, 0\n");
                        mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
                        mir.push_str(&format!("done_lt_{}:\n", idx));
                    }
                }
            }
            OpCode::DefineGlobal => {
                let ra = instruction.ra;
                let c_idx = instruction.operand;
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", ra * 8, ra));
                mir.push_str(&format!("          add name_ptr, constants_ptr, {}\n", c_idx * 8));
                mir.push_str(&format!("          add val_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str("          call p_def_global, er_jit_define_global, status, vm, name_ptr, val_ptr\n");
            }
            OpCode::GetGlobal => {
                let ra = instruction.ra;
                let c_idx = instruction.operand;
                mir.push_str(&format!("          add dest_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          add name_ptr, constants_ptr, {}\n", c_idx * 8));
                mir.push_str("          call p_get_global, er_jit_get_global, status, vm, dest_ptr, name_ptr\n");
                mir.push_str("          blt err_label, status, 0\n");
                mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
            }
            OpCode::SetGlobal => {
                let ra = instruction.ra;
                let c_idx = instruction.operand;
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", ra * 8, ra));
                mir.push_str(&format!("          add val_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          add name_ptr, constants_ptr, {}\n", c_idx * 8));
                mir.push_str("          call p_set_global, er_jit_set_global, status, vm, val_ptr, name_ptr\n");
                mir.push_str("          blt err_label, status, 0\n");
            }
            OpCode::Jump => {
                let target = (idx as i32 + 1 + instruction.operand as i32) as usize;
                mir.push_str(&format!("          jmp inst_{}\n", target));
            }
            OpCode::Loop => {
                let target = (idx as i32 + 1 - instruction.operand as i32) as usize;
                mir.push_str(&format!("          jmp inst_{}\n", target));
            }
            OpCode::JumpIfFalse => {
                let prev_was_optimized_cmp = if idx > 0 {
                    let prev_inst = &func.chunk.code[idx - 1];
                    matches!(prev_inst.op, OpCode::Less | OpCode::Greater | OpCode::Equal) && instruction.ra == prev_inst.ra
                } else {
                    false
                };
                if !prev_was_optimized_cmp {
                    let ra = instruction.ra;
                    let target = (idx as i32 + 1 + instruction.operand as i32) as usize;
                    mir.push_str(&format!("          beq inst_{}, r{}, {}\n", target, ra, TAG_FALSE));
                    mir.push_str(&format!("          beq inst_{}, r{}, {}\n", target, ra, TAG_NULL));
                }
            }
            OpCode::Call => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let arg_count = instruction.operand;
                for i in 0..num_regs {
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", i * 8, i));
                }
                mir.push_str(&format!("          mov i64:(ip_out), {}\n", idx + 1));
                mir.push_str(&format!("          mov i64:(dest_reg_out), {}\n", ra));
                mir.push_str(&format!("          mov i64:(func_reg_out), {}\n", rb));
                mir.push_str(&format!("          mov i64:(arg_count_out), {}\n", arg_count));
                mir.push_str("          ret 0\n");
            }
            OpCode::MakeArray => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let count = instruction.operand;
                for i in 0..count {
                    let reg = rb as usize + i as usize;
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", reg * 8, reg));
                }
                mir.push_str(&format!("          add dest_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          add start_ptr, frame_slots, {}\n", rb * 8));
                mir.push_str(&format!("          call p_make_array, er_jit_make_array, status, vm, dest_ptr, start_ptr, {}\n", count));
                mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
            }
            OpCode::MakeObject => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let count = instruction.operand;
                for i in 0..(2 * count) {
                    let reg = rb as usize + i as usize;
                    mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", reg * 8, reg));
                }
                mir.push_str(&format!("          add dest_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          add start_ptr, frame_slots, {}\n", rb * 8));
                mir.push_str(&format!("          call p_make_object, er_jit_make_object, status, vm, dest_ptr, start_ptr, {}\n", count));
                mir.push_str("          blt err_label, status, 0\n");
                mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
            }
            OpCode::GetProperty => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let c_idx = instruction.operand;
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                mir.push_str(&format!("          add dest_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          add obj_ptr, frame_slots, {}\n", rb * 8));
                mir.push_str(&format!("          add name_ptr, constants_ptr, {}\n", c_idx * 8));
                mir.push_str("          call p_get_property, er_jit_get_property, status, vm, dest_ptr, obj_ptr, name_ptr\n");
                mir.push_str("          blt err_label, status, 0\n");
                mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
            }
            OpCode::SetProperty => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let c_idx = instruction.operand;
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", ra * 8, ra));
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                mir.push_str(&format!("          add obj_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          add val_ptr, frame_slots, {}\n", rb * 8));
                mir.push_str(&format!("          add name_ptr, constants_ptr, {}\n", c_idx * 8));
                mir.push_str("          call p_set_property, er_jit_set_property, status, vm, obj_ptr, val_ptr, name_ptr\n");
                mir.push_str("          blt err_label, status, 0\n");
            }
            OpCode::GetIndex => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let rc = instruction.rc;
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                mir.push_str(&format!("          add dest_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          add obj_ptr, frame_slots, {}\n", rb * 8));
                mir.push_str(&format!("          add idx_ptr, frame_slots, {}\n", rc * 8));
                mir.push_str("          call p_get_index, er_jit_get_index, status, vm, dest_ptr, obj_ptr, idx_ptr\n");
                mir.push_str("          blt err_label, status, 0\n");
                mir.push_str(&format!("          mov r{}, i64:{}(frame_slots)\n", ra, ra * 8));
            }
            OpCode::SetIndex => {
                let ra = instruction.ra;
                let rb = instruction.rb;
                let rc = instruction.rc;
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", ra * 8, ra));
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rb * 8, rb));
                mir.push_str(&format!("          mov i64:{}(frame_slots), r{}\n", rc * 8, rc));
                mir.push_str(&format!("          add obj_ptr, frame_slots, {}\n", ra * 8));
                mir.push_str(&format!("          add idx_ptr, frame_slots, {}\n", rb * 8));
                mir.push_str(&format!("          add val_ptr, frame_slots, {}\n", rc * 8));
                mir.push_str("          call p_set_index, er_jit_set_index, status, vm, obj_ptr, idx_ptr, val_ptr\n");
                mir.push_str("          blt err_label, status, 0\n");
            }
            OpCode::Return => {
                let ra = instruction.ra;
                mir.push_str(&format!("          mov i64:(ret_val_out), r{}\n", ra));
                mir.push_str("          ret 1\n");
            }
        }
    }

    mir.push_str("          ret 1\n");
    mir.push_str("err_label:\n");
    mir.push_str("          ret -1\n");
    mir.push_str("          endfunc\n");
    mir.push_str("          endmodule\n");

    let _ = std::fs::write("/home/vishnus/Downloads/eronom/temp_compiled.mir", &mir);
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

// Clean up MIR context when VM is dropped
pub fn cleanup_jit(mir_ctx: *mut c_void) {
    unsafe {
        MIR_gen_finish(mir_ctx);
        MIR_finish(mir_ctx);
    }
}

// Register FFI Helper Functions in the MIR JIT compiler
unsafe fn register_helpers(ctx: *mut c_void) {
    let helpers: &[(&str, *mut c_void)] = &[
        ("er_jit_negate", er_jit_negate as *mut c_void),
        ("er_jit_not", er_jit_not as *mut c_void),
        ("er_jit_add", er_jit_add as *mut c_void),
        ("er_jit_sub", er_jit_sub as *mut c_void),
        ("er_jit_mul", er_jit_mul as *mut c_void),
        ("er_jit_div", er_jit_div as *mut c_void),
        ("er_jit_equal", er_jit_equal as *mut c_void),
        ("er_jit_greater", er_jit_greater as *mut c_void),
        ("er_jit_less", er_jit_less as *mut c_void),
        ("er_jit_define_global", er_jit_define_global as *mut c_void),
        ("er_jit_get_global", er_jit_get_global as *mut c_void),
        ("er_jit_set_global", er_jit_set_global as *mut c_void),
        ("er_jit_make_array", er_jit_make_array as *mut c_void),
        ("er_jit_make_object", er_jit_make_object as *mut c_void),
        ("er_jit_get_property", er_jit_get_property as *mut c_void),
        ("er_jit_set_property", er_jit_set_property as *mut c_void),
        ("er_jit_get_index", er_jit_get_index as *mut c_void),
        ("er_jit_set_index", er_jit_set_index as *mut c_void),
    ];

    for &(name, ptr) in helpers {
        let cname = CString::new(name).unwrap();
        unsafe {
            MIR_load_external(ctx, cname.as_ptr(), ptr);
        }
    }
}

// FFI Helpers implementation

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_negate(vm: *mut VM, dest: *mut Value, src: *const Value) -> i64 {
    unsafe {
        let val = *src;
        if val.is_number() {
            *dest = Value::number_unchecked(-val.as_number());
            0
        } else {
            (*vm).error = Some("Operand must be a number".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_not(_vm: *mut VM, dest: *mut Value, src: *const Value) -> i64 {
    unsafe {
        let val = *src;
        let res = if val.is_boolean() {
            !val.as_boolean()
        } else if val.is_null() {
            true
        } else {
            false
        };
        *dest = Value::boolean(res);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_add(vm: *mut VM, dest: *mut Value, b: *const Value, c: *const Value) -> i64 {
    unsafe {
        let val_b = *b;
        let val_c = *c;
        if val_b.is_number() && val_c.is_number() {
            *dest = Value::number_unchecked(val_b.as_number() + val_c.as_number());
            0
        } else {
            if val_b.is_string() {
                let sa_str = match &(*val_b.as_gc_ptr()).data {
                    GcData::String(s) => s,
                    _ => unreachable!(),
                };
                let sb_str = val_c.to_string();
                let new_str = format!("{}{}", sa_str, sb_str);
                let new_ptr = gc_allocate(GcData::String(new_str));
                *dest = Value::string(new_ptr);
                0
            } else if val_c.is_string() {
                let sa_str = val_b.to_string();
                let sb_str = match &(*val_c.as_gc_ptr()).data {
                    GcData::String(s) => s,
                    _ => unreachable!(),
                };
                let new_str = format!("{}{}", sa_str, sb_str);
                let new_ptr = gc_allocate(GcData::String(new_str));
                *dest = Value::string(new_ptr);
                0
            } else {
                (*vm).error = Some("Operands must be numbers or strings".into());
                -1
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_sub(vm: *mut VM, dest: *mut Value, b: *const Value, c: *const Value) -> i64 {
    unsafe {
        let val_b = *b;
        let val_c = *c;
        if val_b.is_number() && val_c.is_number() {
            *dest = Value::number_unchecked(val_b.as_number() - val_c.as_number());
            0
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_mul(vm: *mut VM, dest: *mut Value, b: *const Value, c: *const Value) -> i64 {
    unsafe {
        let val_b = *b;
        let val_c = *c;
        if val_b.is_number() && val_c.is_number() {
            *dest = Value::number_unchecked(val_b.as_number() * val_c.as_number());
            0
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_div(vm: *mut VM, dest: *mut Value, b: *const Value, c: *const Value) -> i64 {
    unsafe {
        let val_b = *b;
        let val_c = *c;
        if val_b.is_number() && val_c.is_number() {
            *dest = Value::number_unchecked(val_b.as_number() / val_c.as_number());
            0
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_equal(_vm: *mut VM, dest: *mut Value, b: *const Value, c: *const Value) -> i64 {
    unsafe {
        *dest = Value::boolean(*b == *c);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_greater(vm: *mut VM, dest: *mut Value, b: *const Value, c: *const Value) -> i64 {
    unsafe {
        let val_b = *b;
        let val_c = *c;
        if val_b.is_number() && val_c.is_number() {
            *dest = Value::boolean(val_b.as_number() > val_c.as_number());
            0
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_less(vm: *mut VM, dest: *mut Value, b: *const Value, c: *const Value) -> i64 {
    unsafe {
        let val_b = *b;
        let val_c = *c;
        if val_b.is_number() && val_c.is_number() {
            *dest = Value::boolean(val_b.as_number() < val_c.as_number());
            0
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_define_global(vm: *mut VM, name_val: *const Value, val: *const Value) -> i64 {
    unsafe {
        let name_v = *name_val;
        let name = match &(*name_v.as_gc_ptr()).data {
            GcData::String(s) => Rc::from(s.as_str()),
            _ => unreachable!(),
        };
        (*vm).globals.insert(name, *val);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_global(vm: *mut VM, dest: *mut Value, name_val: *const Value) -> i64 {
    unsafe {
        let name_v = *name_val;
        let name = match &(*name_v.as_gc_ptr()).data {
            GcData::String(s) => s.as_str(),
            _ => unreachable!(),
        };
        if let Some(val) = (*vm).globals.get(name) {
            *dest = *val;
            0
        } else {
            (*vm).error = Some(format!("Undefined variable '{}'", name));
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_global(vm: *mut VM, val: *const Value, name_val: *const Value) -> i64 {
    unsafe {
        let name_v = *name_val;
        let name: Rc<str> = match &(*name_v.as_gc_ptr()).data {
            GcData::String(s) => Rc::from(s.as_str()),
            _ => unreachable!(),
        };
        match (*vm).globals.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.insert(*val);
                0
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                (*vm).error = Some(format!(
                    "Variable '{}' not declared. It needs to be declared with 'let' or 'const'.",
                    name
                ));
                -1
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_make_array(vm: *mut VM, dest: *mut Value, start_reg: *const Value, count: i64) -> i64 {
    unsafe {
        (*vm).gc_step();
        (*vm).gc_step();
        let mut elements = Vec::with_capacity(count as usize);
        for i in 0..count {
            elements.push(*start_reg.offset(i as isize));
        }
        let ptr = gc_allocate(GcData::Array(elements));
        *dest = Value::array(ptr);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_make_object(vm: *mut VM, dest: *mut Value, start_reg: *const Value, count: i64) -> i64 {
    unsafe {
        (*vm).gc_step();
        (*vm).gc_step();
        let mut obj = HashMap::new();
        for i in 0..count {
            let key_val = *start_reg.offset((i * 2) as isize);
            let val = *start_reg.offset((i * 2 + 1) as isize);
            if !key_val.is_string() {
                (*vm).error = Some("Object key must be string".into());
                return -1;
            }
            let key = match &(*key_val.as_gc_ptr()).data {
                GcData::String(s) => Rc::from(s.as_str()),
                _ => unreachable!(),
            };
            obj.insert(key, val);
        }
        let ptr = gc_allocate(GcData::Object(obj));
        *dest = Value::object(ptr);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_property(vm: *mut VM, dest: *mut Value, obj_val: *const Value, name_val: *const Value) -> i64 {
    unsafe {
        let obj = *obj_val;
        let name = match &(*(*name_val).as_gc_ptr()).data {
            GcData::String(s) => s.as_str(),
            _ => unreachable!(),
        };
        if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            match &(*ptr).data {
                GcData::Object(map) => {
                    let val = map.get(name).cloned().unwrap_or(Value::null());
                    *dest = val;
                    0
                }
                _ => unreachable!(),
            }
        } else if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            match &(*ptr).data {
                GcData::Array(arr) => {
                    if name == "push" {
                        *dest = Value::array_method_push(ptr);
                    } else if name == "pop" {
                        *dest = Value::array_method_pop(ptr);
                    } else if name == "length" {
                        *dest = Value::number(arr.len() as f64);
                    } else if let Ok(idx) = name.parse::<usize>() {
                        let val = arr.get(idx).cloned().unwrap_or(Value::null());
                        *dest = val;
                    } else {
                        *dest = Value::null();
                    }
                    0
                }
                _ => unreachable!(),
            }
        } else {
            (*vm).error = Some("Only objects and arrays have properties".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_property(vm: *mut VM, obj_val: *const Value, val_val: *const Value, name_val: *const Value) -> i64 {
    unsafe {
        let obj = *obj_val;
        let val = *val_val;
        let name = match &(*(*name_val).as_gc_ptr()).data {
            GcData::String(s) => s.as_str(),
            _ => unreachable!(),
        };
        if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            match &mut (*ptr).data {
                GcData::Object(map) => {
                    map.insert(Rc::from(name), val);
                    gc_write_barrier(ptr, &val);
                    0
                }
                _ => unreachable!(),
            }
        } else if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            match &mut (*ptr).data {
                GcData::Array(arr) => {
                    if let Ok(idx) = name.parse::<usize>() {
                        if idx < arr.len() {
                            arr[idx] = val;
                        } else if idx == arr.len() {
                            arr.push(val);
                        } else {
                            (*vm).error = Some(format!(
                                "Index {} out of bounds for array of length {}",
                                idx,
                                arr.len()
                            ));
                            return -1;
                        }
                        gc_write_barrier(ptr, &val);
                        0
                    } else {
                        (*vm).error = Some("Cannot set non-numeric property on array".into());
                        -1
                    }
                }
                _ => unreachable!(),
            }
        } else {
            (*vm).error = Some("Only objects and arrays have properties".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_index(vm: *mut VM, dest: *mut Value, obj_val: *const Value, idx_val: *const Value) -> i64 {
    unsafe {
        let obj = *obj_val;
        let index = *idx_val;
        if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            if index.is_number() {
                let idx = index.as_number() as usize;
                match &(*ptr).data {
                    GcData::Array(arr) => {
                        let val = arr.get(idx).cloned().unwrap_or(Value::null());
                        *dest = val;
                        0
                    }
                    _ => unreachable!(),
                }
            } else if index.is_string() {
                let s = match &(*index.as_gc_ptr()).data {
                    GcData::String(st) => st.as_str(),
                    _ => unreachable!(),
                };
                if let Ok(idx) = s.parse::<usize>() {
                    match &(*ptr).data {
                        GcData::Array(arr) => {
                            let val = arr.get(idx).cloned().unwrap_or(Value::null());
                            *dest = val;
                            0
                        }
                        _ => unreachable!(),
                    }
                } else {
                    *dest = Value::null();
                    0
                }
            } else {
                (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                -1
            }
        } else if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            if index.is_string() {
                let s = match &(*index.as_gc_ptr()).data {
                    GcData::String(st) => st.as_str(),
                    _ => unreachable!(),
                };
                match &(*ptr).data {
                    GcData::Object(map) => {
                        let val = map.get(s).cloned().unwrap_or(Value::null());
                        *dest = val;
                        0
                    }
                    _ => unreachable!(),
                }
            } else {
                (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                -1
            }
        } else {
            (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_index(vm: *mut VM, obj_val: *const Value, idx_val: *const Value, val_val: *const Value) -> i64 {
    unsafe {
        let obj = *obj_val;
        let index = *idx_val;
        let val = *val_val;
        if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            if index.is_number() {
                let idx = index.as_number() as usize;
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        if idx < arr.len() {
                            arr[idx] = val;
                        } else if idx == arr.len() {
                            arr.push(val);
                        } else {
                            (*vm).error = Some(format!(
                                "Index {} out of bounds for array of length {}",
                                idx,
                                arr.len()
                            ));
                            return -1;
                        }
                        gc_write_barrier(ptr, &val);
                        0
                    }
                    _ => unreachable!(),
                }
            } else if index.is_string() {
                let s = match &(*index.as_gc_ptr()).data {
                    GcData::String(st) => st.as_str(),
                    _ => unreachable!(),
                };
                if let Ok(idx) = s.parse::<usize>() {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            if idx < arr.len() {
                                arr[idx] = val;
                            } else if idx == arr.len() {
                                arr.push(val);
                            } else {
                                (*vm).error = Some(format!(
                                    "Index {} out of bounds for array of length {}",
                                    idx,
                                    arr.len()
                                ));
                                return -1;
                            }
                            gc_write_barrier(ptr, &val);
                            0
                        }
                        _ => unreachable!(),
                    }
                } else {
                    (*vm).error = Some("Cannot set non-numeric property on array".into());
                    -1
                }
            } else {
                (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                -1
            }
        } else if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            if index.is_string() {
                let s = match &(*index.as_gc_ptr()).data {
                    GcData::String(st) => Rc::from(st.as_str()),
                    _ => unreachable!(),
                };
                match &mut (*ptr).data {
                    GcData::Object(map) => {
                        map.insert(s, val);
                        gc_write_barrier(ptr, &val);
                        0
                    }
                    _ => unreachable!(),
                }
            } else {
                (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                -1
            }
        } else {
            (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
            -1
        }
    }
}
