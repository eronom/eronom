use crate::vm::value::{Value, push_positive_integer, ADD_SCRATCH};
use crate::vm::bytecode::{Instruction, OpCode};
use crate::vm::gc::{gc_allocate, gc_alloc_string, get_or_create_string, GcData};

pub unsafe fn execute_math_and_cmp_op(
    instruction: &Instruction,
    frame_slots: *mut Value,
) -> Result<bool, String> {
    match instruction.op {
        OpCode::Negate => {
            let dest = instruction.ra as usize;
            let src = instruction.rb as usize;
            let val = *frame_slots.add(src);
            if val.is_number() {
                *frame_slots.add(dest) = Value::number_unchecked(-val.as_number());
            } else {
                return Err("Operand must be a number".into());
            }
            Ok(true)
        }
        OpCode::Not => {
            let dest = instruction.ra as usize;
            let src = instruction.rb as usize;
            let val = *frame_slots.add(src);
            let res = if val.is_boolean() {
                !val.as_boolean()
            } else if val.is_null() {
                true
            } else {
                false
            };
            *frame_slots.add(dest) = Value::boolean(res);
            Ok(true)
        }
        OpCode::Add => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                *frame_slots.add(dest) = Value::number_unchecked(a.as_number() + b.as_number());
            } else {
                use std::fmt::Write;
                if a.is_string() {
                    let sa_str = a.as_str().unwrap_or("");
                    let val = ADD_SCRATCH.with(|scratch| {
                        let mut s_ref = scratch.borrow_mut();
                        s_ref.clear();
                        s_ref.push_str(sa_str);
                        if b.is_string() {
                            if let Some(sb_str) = b.as_str() {
                                s_ref.push_str(sb_str);
                            }
                        } else if b.is_number() {
                            let val = b.as_number();
                            if val >= 0.0 && val == val.trunc() && val < 1.8446744073709552e19 {
                                push_positive_integer(&mut s_ref, val as u64);
                            } else {
                                let _ = write!(&mut s_ref, "{}", val);
                            }
                        } else {
                            let _ = write!(&mut s_ref, "{}", b);
                        }
                        if let Some(inline) = Value::inline_string(s_ref.as_str()) {
                            inline
                        } else {
                            let new_ptr = gc_alloc_string(s_ref.as_str());
                            Value::string(new_ptr)
                        }
                    });
                    *frame_slots.add(dest) = val;
                } else if b.is_string() {
                    let sb_str = b.as_str().unwrap_or("");
                    let val = ADD_SCRATCH.with(|scratch| {
                        let mut s_ref = scratch.borrow_mut();
                        s_ref.clear();
                        if a.is_number() {
                            let val = a.as_number();
                            if val >= 0.0 && val == val.trunc() && val < 1.8446744073709552e19 {
                                push_positive_integer(&mut s_ref, val as u64);
                            } else {
                                let _ = write!(&mut s_ref, "{}", val);
                            }
                        } else {
                            let _ = write!(&mut s_ref, "{}", a);
                        }
                        s_ref.push_str(sb_str);
                        if let Some(inline) = Value::inline_string(s_ref.as_str()) {
                            inline
                        } else {
                            let new_ptr = gc_alloc_string(s_ref.as_str());
                            Value::string(new_ptr)
                        }
                    });
                    *frame_slots.add(dest) = val;
                } else {
                    return Err("Operands must be numbers or strings".into());
                }
            }
            Ok(true)
        }
        OpCode::Sub => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                *frame_slots.add(dest) = Value::number_unchecked(a.as_number() - b.as_number());
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::Mul => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                *frame_slots.add(dest) = Value::number_unchecked(a.as_number() * b.as_number());
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::Div => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                *frame_slots.add(dest) = Value::number_unchecked(a.as_number() / b.as_number());
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::Mod => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                *frame_slots.add(dest) = Value::number_unchecked(a.as_number() % b.as_number());
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::BitAnd => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                let res = ((a.as_number() as i64) & (b.as_number() as i64)) as f64;
                *frame_slots.add(dest) = Value::number_unchecked(res);
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::BitOr => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                let res = ((a.as_number() as i64) | (b.as_number() as i64)) as f64;
                *frame_slots.add(dest) = Value::number_unchecked(res);
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::BitXor => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                let res = ((a.as_number() as i64) ^ (b.as_number() as i64)) as f64;
                *frame_slots.add(dest) = Value::number_unchecked(res);
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::BitNot => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            if a.is_number() {
                let res = (!(a.as_number() as i64)) as f64;
                *frame_slots.add(dest) = Value::number_unchecked(res);
            } else {
                return Err("Operand must be a number".into());
            }
            Ok(true)
        }
        OpCode::ShiftLeft => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                let shift = (b.as_number() as u32) & 63;
                let res = ((a.as_number() as i64).wrapping_shl(shift)) as f64;
                *frame_slots.add(dest) = Value::number_unchecked(res);
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::ShiftRight => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                let shift = (b.as_number() as u32) & 63;
                let res = ((a.as_number() as i64).wrapping_shr(shift)) as f64;
                *frame_slots.add(dest) = Value::number_unchecked(res);
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::TypeOf => {
            let dest = instruction.ra as usize;
            let val = *frame_slots.add(instruction.rb as usize);
            let type_str = if val.is_number() {
                "number"
            } else if val.is_string() {
                "string"
            } else if val.is_boolean() {
                "boolean"
            } else if val.is_null() {
                "null"
            } else if val.is_array() {
                "array"
            } else if val.is_object() {
                "object"
            } else if val.is_function() || val.is_native_function() {
                "function"
            } else {
                "object"
            };
            let ptr = get_or_create_string(type_str);
            *frame_slots.add(dest) = Value::string(ptr);
            Ok(true)
        }
        OpCode::ArrayLen => {
            let dest = instruction.ra as usize;
            let src = instruction.rb as usize;
            let val = *frame_slots.add(src);
            if val.is_array() {
                let arr_ptr = val.as_gc_ptr();
                let len = match &(*arr_ptr).data {
                    GcData::Array(arr) => arr.len(),
                    _ => 0,
                };
                *frame_slots.add(dest) = Value::number(len as f64);
            } else {
                return Err("Expected array for length".into());
            }
            Ok(true)
        }
        OpCode::Equal => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            *frame_slots.add(dest) = Value::boolean(a == b);
            Ok(true)
        }
        OpCode::Greater => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                *frame_slots.add(dest) = Value::boolean(a.as_number() > b.as_number());
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        OpCode::Less => {
            let dest = instruction.ra as usize;
            let a = *frame_slots.add(instruction.rb as usize);
            let b = *frame_slots.add(instruction.rc as usize);
            if a.is_number() && b.is_number() {
                *frame_slots.add(dest) = Value::boolean(a.as_number() < b.as_number());
            } else {
                return Err("Operands must be numbers".into());
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}
