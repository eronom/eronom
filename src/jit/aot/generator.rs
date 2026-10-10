use std::path::Path;
use std::process::Command;
use super::ir::{EirBinaryOp, EirFunction, EirInst, EirModule, EirType, EirUnaryOp};

/// Generates valid MIR textual representation from an E-IR module.
pub fn generate_mir(module: &EirModule) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}: module\n", module.name));
    out.push_str("    export main\n");
    out.push_str("    import er_runtime_init, er_runtime_cleanup, er_print_string, er_println_string, er_println_i64, er_println_f64, er_println_bool, er_print_value, er_println_value, er_alloc_string, er_alloc_array, er_alloc_object, er_alloc_struct, er_aot_add_f64, er_aot_sub_f64, er_aot_mul_f64, er_aot_div_f64, er_aot_add_i64, er_aot_sub_i64, er_aot_mul_i64, er_aot_array_push, er_aot_array_len, er_aot_array_get, er_aot_array_set\n\n");

    out.push_str("    p_init: proto\n");
    out.push_str("    p_cleanup: proto\n");
    out.push_str("    p_println_i64: proto, i64:val\n");
    out.push_str("    p_println_f64: proto, d:val\n");
    out.push_str("    p_println_bool: proto, i64:val\n");
    out.push_str("    p_alloc_string: proto i64, p:ptr, i64:len\n");
    out.push_str("    p_alloc_array: proto i64, i64:cap\n");
    out.push_str("    p_alloc_object: proto i64\n");
    out.push_str("    p_alloc_struct: proto i64, p:name, i64:fields\n");
    out.push_str("    p_array_push: proto, i64:arr, i64:val\n");
    out.push_str("    p_array_get: proto i64, i64:arr, i64:idx\n");
    out.push_str("    p_array_set: proto, i64:arr, i64:idx, i64:val\n\n");

    for func in &module.functions {
        emit_mir_function(&mut out, func);
    }

    out.push_str("endmodule\n");
    out
}

fn emit_mir_function(out: &mut String, func: &EirFunction) {
    let ret_type = match func.return_type {
        EirType::F64 => "d",
        EirType::Void => "",
        _ => "i64",
    };

    if ret_type.is_empty() {
        out.push_str(&format!("    {}: func\n", func.name));
    } else {
        out.push_str(&format!("    {}: func {}\n", func.name, ret_type));
    }

    // Local registers
    let num_regs = func.reg_count.max(16);
    let mut reg_decls = Vec::new();
    for r in 0..num_regs {
        reg_decls.push(format!("i64:r{}", r));
        reg_decls.push(format!("d:d{}", r));
    }
    out.push_str(&format!("        local {}\n", reg_decls.join(", ")));

    if func.name == "main" {
        out.push_str("        call p_init, er_runtime_init\n");
    }

    for (idx, inst) in func.instructions.iter().enumerate() {
        out.push_str(&format!("    lbl_{}:\n", idx));
        match inst {
            EirInst::ConstI64 { dest, val } => {
                out.push_str(&format!("        mov r{}, {}\n", dest, val));
            }
            EirInst::ConstF64 { dest, val } => {
                out.push_str(&format!("        dmov d{}, {}\n", dest, val));
            }
            EirInst::ConstBool { dest, val } => {
                out.push_str(&format!("        mov r{}, {}\n", dest, if *val { 1 } else { 0 }));
            }
            EirInst::ConstString { dest, val } => {
                out.push_str(&format!("        # string literal: {}\n", val));
                out.push_str(&format!("        mov r{}, 0\n", dest));
            }
            EirInst::Move { dest, src, ty } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("        dmov d{}, d{}\n", dest, src));
                } else {
                    out.push_str(&format!("        mov r{}, r{}\n", dest, src));
                }
            }
            EirInst::Binary { dest, op, ty, left, right } => {
                if *ty == EirType::F64 {
                    match op {
                        EirBinaryOp::Add => out.push_str(&format!("        dadd d{}, d{}, d{}\n", dest, left, right)),
                        EirBinaryOp::Sub => out.push_str(&format!("        dsub d{}, d{}, d{}\n", dest, left, right)),
                        EirBinaryOp::Mul => out.push_str(&format!("        dmul d{}, d{}, d{}\n", dest, left, right)),
                        EirBinaryOp::Div => out.push_str(&format!("        ddiv d{}, d{}, d{}\n", dest, left, right)),
                        _ => out.push_str(&format!("        dadd d{}, d{}, d{}\n", dest, left, right)),
                    }
                } else {
                    match op {
                        EirBinaryOp::Add => out.push_str(&format!("        add r{}, r{}, r{}\n", dest, left, right)),
                        EirBinaryOp::Sub => out.push_str(&format!("        sub r{}, r{}, r{}\n", dest, left, right)),
                        EirBinaryOp::Mul => out.push_str(&format!("        mul r{}, r{}, r{}\n", dest, left, right)),
                        EirBinaryOp::Div => out.push_str(&format!("        div r{}, r{}, r{}\n", dest, left, right)),
                        EirBinaryOp::Eq => out.push_str(&format!("        eq r{}, r{}, r{}\n", dest, left, right)),
                        EirBinaryOp::Lt => out.push_str(&format!("        lt r{}, r{}, r{}\n", dest, left, right)),
                        EirBinaryOp::Gt => out.push_str(&format!("        gt r{}, r{}, r{}\n", dest, left, right)),
                        _ => out.push_str(&format!("        add r{}, r{}, r{}\n", dest, left, right)),
                    }
                }
            }
            EirInst::Unary { dest, op, ty, src } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("        dneg d{}, d{}\n", dest, src));
                } else {
                    match op {
                        EirUnaryOp::Neg => out.push_str(&format!("        neg r{}, r{}\n", dest, src)),
                        EirUnaryOp::Not => out.push_str(&format!("        eq r{}, r{}, 0\n", dest, src)),
                        EirUnaryOp::BitNot => out.push_str(&format!("        xor r{}, r{}, -1\n", dest, src)),
                    }
                }
            }
            EirInst::Jump { target } => {
                out.push_str(&format!("        jmp lbl_{}\n", target));
            }
            EirInst::BranchIfFalse { cond, target } => {
                out.push_str(&format!("        beq lbl_{}, r{}, 0\n", target, cond));
            }
            EirInst::Return { val } => {
                if func.name == "main" {
                    out.push_str("        call p_cleanup, er_runtime_cleanup\n");
                    out.push_str("        ret 0\n");
                } else if let Some((r, ty)) = val {
                    if *ty == EirType::F64 {
                        out.push_str(&format!("        ret d{}\n", r));
                    } else {
                        out.push_str(&format!("        ret r{}\n", r));
                    }
                } else {
                    out.push_str("        ret\n");
                }
            }
            EirInst::Println { reg, ty } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("        call p_println_f64, er_println_f64, d{}\n", reg));
                } else {
                    out.push_str(&format!("        call p_println_i64, er_println_i64, r{}\n", reg));
                }
            }
            _ => {
                out.push_str("        # nop\n");
            }
        }
    }

    if func.name == "main" {
        out.push_str("        call p_cleanup, er_runtime_cleanup\n");
        out.push_str("        ret 0\n");
    }
    out.push_str("    endfunc\n\n");
}

/// Generates optimized native C representation for ahead-of-time compilation.
pub fn generate_native_c(module: &EirModule) -> String {
    let mut out = String::new();
    out.push_str("#include <stdint.h>\n");
    out.push_str("#include <stdbool.h>\n");
    out.push_str("#include <stdio.h>\n");
    out.push_str("#include <stdlib.h>\n");
    out.push_str("#include <string.h>\n\n");

    out.push_str("// Eronom standalone runtime helpers\n");
    out.push_str("static inline void er_runtime_init(void) {}\n");
    out.push_str("static inline void er_runtime_cleanup(void) {}\n");
    out.push_str("static inline void er_println_i64(int64_t val) { printf(\"%lld\\n\", (long long)val); }\n");
    out.push_str("static inline void er_println_f64(double val) { printf(\"%f\\n\", val); }\n");
    out.push_str("static inline void er_println_bool(bool val) { printf(\"%s\\n\", val ? \"true\" : \"false\"); }\n");
    out.push_str("static inline void er_print_string(const uint8_t *ptr, size_t len) { if (ptr && len > 0) fwrite(ptr, 1, len, stdout); }\n");
    out.push_str("static inline void er_println_string(const uint8_t *ptr, size_t len) { if (ptr && len > 0) fwrite(ptr, 1, len, stdout); putchar('\\n'); }\n");
    out.push_str("static inline void* er_alloc_string(const uint8_t *ptr, size_t len) {\n");
    out.push_str("    char *s = (char*)malloc(len + 1);\n");
    out.push_str("    if (ptr && len > 0) memcpy(s, ptr, len);\n");
    out.push_str("    s[len] = '\\0';\n");
    out.push_str("    return s;\n");
    out.push_str("}\n");
    out.push_str("static inline void* er_alloc_array(size_t cap) { return calloc(cap > 0 ? cap : 4, sizeof(uint64_t)); }\n");
    out.push_str("static inline void* er_alloc_object(void) { return malloc(64); }\n");
    out.push_str("static inline void* er_alloc_struct(const char *name, size_t fields) { (void)name; return calloc(fields > 0 ? fields : 1, sizeof(uint64_t)); }\n");
    out.push_str("static inline void er_aot_array_push(void *arr, uint64_t val) { (void)arr; (void)val; }\n");
    out.push_str("static inline size_t er_aot_array_len(void *arr) { (void)arr; return 0; }\n");
    out.push_str("static inline uint64_t er_aot_array_get(void *arr, size_t index) { (void)arr; (void)index; return 0; }\n");
    out.push_str("static inline void er_aot_array_set(void *arr, size_t index, uint64_t val) { (void)arr; (void)index; (void)val; }\n\n");


    for func in &module.functions {
        emit_c_function(&mut out, func);
    }

    out
}

fn emit_c_function(out: &mut String, func: &EirFunction) {
    let is_main = func.name == "main";
    let ret_type = if is_main {
        "int"
    } else {
        match func.return_type {
            EirType::F64 => "double",
            EirType::Void => "void",
            _ => "int64_t",
        }
    };

    out.push_str(&format!("{} {}(void) {{\n", ret_type, func.name));

    // Registers
    let num_regs = func.reg_count.max(32);
    out.push_str(&format!("    int64_t r[{}] = {{0}};\n", num_regs));
    out.push_str(&format!("    double d[{}] = {{0.0}};\n\n", num_regs));

    if is_main {
        out.push_str("    er_runtime_init();\n\n");
    }

    for (idx, inst) in func.instructions.iter().enumerate() {
        out.push_str(&format!("lbl_{}:\n", idx));
        match inst {
            EirInst::ConstI64 { dest, val } => {
                out.push_str(&format!("    r[{}] = {}LL;\n", dest, val));
            }
            EirInst::ConstF64 { dest, val } => {
                out.push_str(&format!("    d[{}] = {};\n", dest, val));
            }
            EirInst::ConstBool { dest, val } => {
                out.push_str(&format!("    r[{}] = {};\n", dest, if *val { 1 } else { 0 }));
            }
            EirInst::ConstString { dest, val } => {
                out.push_str(&format!("    r[{}] = (int64_t)(uintptr_t)er_alloc_string((const uint8_t*)\"{}\", {});\n", dest, val, val.len()));
            }
            EirInst::Move { dest, src, ty } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("    d[{}] = d[{}];\n", dest, src));
                } else {
                    out.push_str(&format!("    r[{}] = r[{}];\n", dest, src));
                }
            }
            EirInst::Binary { dest, op, ty, left, right } => {
                if *ty == EirType::F64 {
                    let op_str = match op {
                        EirBinaryOp::Add => "+",
                        EirBinaryOp::Sub => "-",
                        EirBinaryOp::Mul => "*",
                        EirBinaryOp::Div => "/",
                        _ => "+",
                    };
                    out.push_str(&format!("    d[{}] = d[{}] {} d[{}];\n", dest, left, op_str, right));
                } else {
                    let op_str = match op {
                        EirBinaryOp::Add => "+",
                        EirBinaryOp::Sub => "-",
                        EirBinaryOp::Mul => "*",
                        EirBinaryOp::Div => "/",
                        EirBinaryOp::Eq => "==",
                        EirBinaryOp::Ne => "!=",
                        EirBinaryOp::Lt => "<",
                        EirBinaryOp::Le => "<=",
                        EirBinaryOp::Gt => ">",
                        EirBinaryOp::Ge => ">=",
                        _ => "+",
                    };
                    out.push_str(&format!("    r[{}] = r[{}] {} r[{}];\n", dest, left, op_str, right));
                }
            }
            EirInst::Unary { dest, op, ty, src } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("    d[{}] = -d[{}];\n", dest, src));
                } else {
                    match op {
                        EirUnaryOp::Neg => out.push_str(&format!("    r[{}] = -r[{}];\n", dest, src)),
                        EirUnaryOp::Not => out.push_str(&format!("    r[{}] = !r[{}];\n", dest, src)),
                        EirUnaryOp::BitNot => out.push_str(&format!("    r[{}] = ~r[{}];\n", dest, src)),
                    }
                }
            }
            EirInst::Jump { target } => {
                out.push_str(&format!("    goto lbl_{};\n", target));
            }
            EirInst::BranchIfFalse { cond, target } => {
                out.push_str(&format!("    if (!r[{}]) goto lbl_{};\n", cond, target));
            }
            EirInst::Return { val } => {
                if is_main {
                    out.push_str("    er_runtime_cleanup();\n");
                    out.push_str("    return 0;\n");
                } else if let Some((r, ty)) = val {
                    if *ty == EirType::F64 {
                        out.push_str(&format!("    return d[{}];\n", r));
                    } else {
                        out.push_str(&format!("    return r[{}];\n", r));
                    }
                } else {
                    out.push_str("    return;\n");
                }
            }
            EirInst::Println { reg, ty } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("    er_println_f64(d[{}]);\n", reg));
                } else {
                    out.push_str(&format!("    er_println_i64(r[{}]);\n", reg));
                }
            }
            _ => {}
        }
    }

    if is_main {
        out.push_str("    er_runtime_cleanup();\n");
        out.push_str("    return 0;\n");
    }
    out.push_str("}\n\n");
}

/// Emit native relocatable object file (.o) from an E-IR module.
pub fn emit_native_object(module: &EirModule, obj_path: &Path) -> anyhow::Result<()> {
    let c_code = generate_native_c(module);
    let temp_c = obj_path.with_extension("tmp.c");
    std::fs::write(&temp_c, c_code)?;

    let status = Command::new("cc")
        .arg("-O2")
        .arg("-c")
        .arg(&temp_c)
        .arg("-o")
        .arg(obj_path)
        .status()?;

    let _ = std::fs::remove_file(temp_c);

    if !status.success() {
        anyhow::bail!("Failed to compile native object file with cc: exit code {:?}", status.code());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::aot::ir::{EirFunction, EirInst, EirModule, EirType};

    #[test]
    fn test_generate_mir_and_c() {
        let mut module = EirModule::new("test_mod");
        let func = EirFunction {
            name: "main".to_string(),
            arity: 0,
            param_types: vec![],
            return_type: EirType::I64,
            reg_count: 8,
            instructions: vec![
                EirInst::ConstI64 { dest: 0, val: 42 },
                EirInst::Println { reg: 0, ty: EirType::I64 },
                EirInst::Return { val: Some((0, EirType::I64)) },
            ],
        };
        module.functions.push(func);

        let mir = generate_mir(&module);
        assert!(mir.contains("test_mod: module"));
        assert!(mir.contains("export main"));
        assert!(mir.contains("mov r0, 42"));

        let c_code = generate_native_c(&module);
        assert!(c_code.contains("int main(void)"));
        assert!(c_code.contains("r[0] = 42LL;"));
        assert!(c_code.contains("er_runtime_init();"));
        assert!(c_code.contains("er_runtime_cleanup();"));
    }
}

