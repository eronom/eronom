mod structs;
mod types;
mod await_handles;
mod expr_call;
mod expr_inc_dec;
mod expr;
mod stmt_control;
mod stmt_struct;
mod stmt;

pub use structs::{FlattenedStructInfo, RawStructInfo, InterfaceInfo, collect_structs, collect_interfaces, flatten_struct};
pub use types::{get_expr_type, is_type_compatible, check_type};

use crate::frontend::Stmt;
use crate::vm::bytecode::{Chunk, Function, OpCode, UpvalueDescriptor};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub(crate) struct LoopContext {
    pub(crate) break_jumps: Vec<usize>,
    pub(crate) continue_jumps: Vec<usize>,
    pub(crate) is_switch: bool,
}

#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub depth: usize,
    pub is_const: bool,
    pub loc: crate::frontend::SourceLocation,
    pub ty: Option<String>,
}

pub struct Compiler {
    pub(crate) parent: Option<*mut Compiler>,
    pub(crate) function: Function,
    pub(crate) locals: Vec<Local>,
    pub(crate) upvalues: Vec<UpvalueDescriptor>,
    pub(crate) upvalue_names: Vec<(String, bool, crate::frontend::SourceLocation, Option<String>)>,
    pub(crate) scope_depth: usize,
    pub(crate) next_reg: usize,
    pub(crate) const_globals: Rc<RefCell<HashMap<String, crate::frontend::SourceLocation>>>,
    pub(crate) structs: HashMap<String, FlattenedStructInfo>,
    pub(crate) interfaces: HashMap<String, InterfaceInfo>,
    pub(crate) global_types: HashMap<String, String>,
    pub(crate) current_return_type: Option<String>,
    pub(crate) current_struct: Option<String>,
    pub(crate) concurrent_scopes: Vec<usize>,
    pub(crate) loop_stack: Vec<LoopContext>,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            parent: None,
            function: Function {
                name: None,
                chunk: Chunk::default(),
                arity: 0,
                jit_ptr: std::cell::Cell::new(None),
                invocation_count: std::cell::Cell::new(0),
                is_async: false,
                has_loop: false,
                upvalues: Vec::new(),
            },
            locals: Vec::new(),
            upvalues: Vec::new(),
            upvalue_names: Vec::new(),
            scope_depth: 0,
            next_reg: 0,
            const_globals: Rc::new(RefCell::new(HashMap::new())),
            structs: HashMap::new(),
            interfaces: HashMap::new(),
            global_types: HashMap::new(),
            current_return_type: None,
            current_struct: None,
            concurrent_scopes: Vec::new(),
            loop_stack: Vec::new(),
        }
    }

    pub(crate) fn current_chunk(&mut self) -> &mut Chunk {
        &mut self.function.chunk
    }

    pub fn compile(mut self, stmts: &[Stmt]) -> Result<Function, String> {
        let mut raw_structs = HashMap::new();
        collect_structs(stmts, &mut raw_structs);

        let mut resolved = HashMap::new();
        for name in raw_structs.keys() {
            let mut visiting = std::collections::HashSet::new();
            flatten_struct(name, &raw_structs, &mut resolved, &mut visiting)?;
        }

        self.structs = resolved;
        collect_interfaces(stmts, &mut self.interfaces);

        for stmt in stmts {
            self.compile_stmt(stmt)?;
        }
        self.current_chunk().write_instruction(OpCode::LoadNull, 0, 0, 0, 0);
        self.current_chunk().write_instruction(OpCode::Return, 0, 0, 0, 0);
        Ok(self.function)
    }

    pub(crate) fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    pub(crate) fn end_scope(&mut self) {
        self.scope_depth -= 1;
        while let Some(local) = self.locals.last() {
            if local.depth > self.scope_depth {
                self.locals.pop();
            } else {
                break;
            }
        }
    }

    pub(crate) fn resolve_local(&self, name: &str) -> Option<usize> {
        self.locals
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, local)| if local.name == name { Some(i) } else { None })
    }

    pub(crate) fn add_upvalue(&mut self, is_local: bool, index: u8, name: &str, is_const: bool, loc: crate::frontend::SourceLocation, ty: Option<String>) -> usize {
        for (i, uv) in self.upvalues.iter().enumerate() {
            if uv.is_local == is_local && uv.index == index {
                return i;
            }
        }
        self.upvalues.push(UpvalueDescriptor { is_local, index });
        self.upvalue_names.push((name.to_string(), is_const, loc, ty));
        self.upvalues.len() - 1
    }

    pub(crate) fn resolve_upvalue(&mut self, name: &str) -> Option<usize> {
        if let Some(parent_ptr) = self.parent {
            unsafe {
                let parent = &mut *parent_ptr;
                if let Some(local_idx) = parent.resolve_local(name) {
                    let is_const = parent.locals[local_idx].is_const;
                    let loc = parent.locals[local_idx].loc.clone();
                    let ty = parent.locals[local_idx].ty.clone();
                    return Some(self.add_upvalue(true, local_idx as u8, name, is_const, loc, ty));
                }
                if let Some(parent_upval_idx) = parent.resolve_upvalue(name) {
                    let (ref u_name, is_const, ref loc, ref ty) = parent.upvalue_names[parent_upval_idx];
                    let loc_clone = loc.clone();
                    let ty_clone = ty.clone();
                    let u_name_clone = u_name.clone();
                    return Some(self.add_upvalue(false, parent_upval_idx as u8, &u_name_clone, is_const, loc_clone, ty_clone));
                }
            }
        }
        None
    }

    pub(crate) fn emit_jump(&mut self, op: OpCode, cond_reg: usize) -> usize {
        self.current_chunk().write_instruction(op, cond_reg as u8, 0, 0, 0);
        self.current_chunk().code.len() - 1
    }

    pub(crate) fn patch_jump(&mut self, offset: usize) {
        let jump = self.current_chunk().code.len() - 1 - offset;
        let inst = &mut self.current_chunk().code[offset];
        match inst.op {
            OpCode::JumpIfFalse | OpCode::Jump => inst.operand = jump as u32,
            _ => unreachable!(),
        }
    }

    pub(crate) fn patch_jump_to(&mut self, offset: usize, target_ip: usize) {
        let jump = target_ip - 1 - offset;
        let inst = &mut self.current_chunk().code[offset];
        match inst.op {
            OpCode::JumpIfFalse | OpCode::Jump => inst.operand = jump as u32,
            _ => unreachable!(),
        }
    }

    pub(crate) fn emit_loop(&mut self, loop_start: usize) {
        self.function.has_loop = true;
        let offset = self.current_chunk().code.len() - loop_start + 1;
        self.current_chunk().write_instruction(OpCode::Loop, 0, 0, 0, offset as u32);
    }

    pub(crate) fn format_const_assign_error(&self, name: &str, assign_loc: &crate::frontend::SourceLocation, decl_loc: &crate::frontend::SourceLocation) -> String {
        fn get_file_line(path: &str, line_num: usize) -> Option<String> {
            if let Ok(content) = std::fs::read_to_string(path) {
                content.lines().nth(line_num - 1).map(|s| s.to_string())
            } else {
                None
            }
        }
        
        fn format_snippet(loc: &crate::frontend::SourceLocation) -> String {
            let line_str = loc.line.to_string();
            let prefix = format!("{} | ", line_str);
            let mut result = String::new();
            if let Some(content) = get_file_line(&loc.file_path, loc.line) {
                result.push_str(&format!("{}{}\n", prefix, content));
                let spaces = " ".repeat(line_str.len() + 3 + loc.col - 1);
                result.push_str(&format!("{}^\n", spaces));
            }
            result
        }

        format!(
            "{}\
            error: This assignment will throw because \"{}\" is a constant\n\
            \x20\x20\x20\x20at {}:{}:{}\n\n\
            {}\
            note: The symbol \"{}\" was declared a constant here:\n\
            \x20\x20\x20at {}:{}:{}",
            format_snippet(assign_loc),
            name,
            assign_loc.file_path, assign_loc.line, assign_loc.col,
            format_snippet(decl_loc),
            name,
            decl_loc.file_path, decl_loc.line, decl_loc.col
        )
    }
}
