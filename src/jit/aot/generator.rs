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
            EirInst::ConstNull { dest } => {
                out.push_str(&format!("        mov r{}, 0\n", dest));
            }
            EirInst::DefineGlobal { name, src } => {
                out.push_str(&format!("        # define global {}\n", name));
                out.push_str(&format!("        mov r{}, r{}\n", src, src));
            }
            EirInst::GetGlobal { dest, name } => {
                out.push_str(&format!("        # get global {}\n", name));
                out.push_str(&format!("        mov r{}, 0\n", dest));
            }
            EirInst::ConstFuncPtr { dest, symbol } => {
                out.push_str(&format!("        # func ptr {}\n        mov r{}, 0\n", symbol, dest));
            }
            EirInst::MakeClosure { dest, symbol, .. } => {
                out.push_str(&format!("        # closure {}\n        mov r{}, 0\n", symbol, dest));
            }
            EirInst::GetUpvalue { dest, .. } => {
                out.push_str(&format!("        mov r{}, 0\n", dest));
            }
            EirInst::SetUpvalue { .. } => {
                out.push_str("        # set upvalue\n");
            }
            EirInst::MakeObject { dest, .. } => {
                out.push_str(&format!("        # make object\n        mov r{}, 0\n", dest));
            }
            EirInst::GetProperty { dest, .. } => {
                out.push_str(&format!("        # get prop\n        mov r{}, 0\n", dest));
            }
            EirInst::SetProperty { .. } => {
                out.push_str("        # set prop\n");
            }
            EirInst::MakeArray { dest, .. } => {
                out.push_str(&format!("        # make array\n        mov r{}, 0\n", dest));
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

    out.push_str("// Eronom standalone runtime tags and helpers\n");
    out.push_str("#define ER_TAG_NULL           0xfff1000000000000ULL\n");
    out.push_str("#define ER_TAG_FALSE          0xfff2000000000000ULL\n");
    out.push_str("#define ER_TAG_TRUE           0xfff3000000000000ULL\n");
    out.push_str("#define ER_TAG_STRING         0xfff4000000000000ULL\n");
    out.push_str("#define ER_TAG_ARRAY          0xfff5000000000000ULL\n");
    out.push_str("#define ER_TAG_OBJECT         0xfff6000000000000ULL\n");
    out.push_str("#define ER_TAG_CLOSURE        0xfff7000000000000ULL\n");
    out.push_str("#define ER_TAG_FUNCTION       0xfff7000000000000ULL\n");
    out.push_str("#define ER_TAG_BUILTIN_PRINT  0xfff8000000000001ULL\n");
    out.push_str("#define ER_PTR_MASK           0x0000ffffffffffffULL\n\n");

    out.push_str("typedef struct {\n");
    out.push_str("    char *data;\n");
    out.push_str("    size_t len;\n");
    out.push_str("} ErStringObj;\n\n");

    out.push_str("typedef struct {\n");
    out.push_str("    char key[64];\n");
    out.push_str("    int64_t value;\n");
    out.push_str("} ErGlobalEntry;\n\n");

    out.push_str("static ErGlobalEntry er_globals[512];\n");
    out.push_str("static size_t er_globals_count = 0;\n\n");

    out.push_str("static inline void er_runtime_init(void) {\n");
    out.push_str("    er_globals_count = 0;\n");
    out.push_str("}\n\n");

    out.push_str("static inline void er_runtime_cleanup(void) {\n");
    out.push_str("    er_globals_count = 0;\n");
    out.push_str("}\n\n");

    out.push_str("static inline void er_println_i64(int64_t val) { printf(\"%lld\\n\", (long long)val); fflush(stdout); }\n");
    out.push_str("static inline void er_println_f64(double val) { printf(\"%f\\n\", val); fflush(stdout); }\n");
    out.push_str("static inline void er_println_bool(bool val) { printf(\"%s\\n\", val ? \"true\" : \"false\"); fflush(stdout); }\n");
    out.push_str("static inline void er_print_string(const uint8_t *ptr, size_t len) { if (ptr && len > 0) fwrite(ptr, 1, len, stdout); fflush(stdout); }\n");
    out.push_str("static inline void er_println_string(const uint8_t *ptr, size_t len) { if (ptr && len > 0) fwrite(ptr, 1, len, stdout); putchar('\\n'); fflush(stdout); }\n\n");

    out.push_str("static inline int64_t er_alloc_string(const uint8_t *ptr, size_t len) {\n");
    out.push_str("    ErStringObj *s = (ErStringObj*)malloc(sizeof(ErStringObj));\n");
    out.push_str("    s->len = len;\n");
    out.push_str("    s->data = (char*)malloc(len + 1);\n");
    out.push_str("    if (ptr && len > 0) memcpy(s->data, ptr, len);\n");
    out.push_str("    s->data[len] = '\\0';\n");
    out.push_str("    return (int64_t)(ER_TAG_STRING | ((uintptr_t)s & ER_PTR_MASK));\n");
    out.push_str("}\n\n");

    out.push_str("static inline bool er_is_string(int64_t val) {\n");
    out.push_str("    return ((uint64_t)val & ~ER_PTR_MASK) == ER_TAG_STRING;\n");
    out.push_str("}\n\n");

    out.push_str("static inline ErStringObj* er_as_string_obj(int64_t val) {\n");
    out.push_str("    return (ErStringObj*)(uintptr_t)((uint64_t)val & ER_PTR_MASK);\n");
    out.push_str("}\n\n");

    out.push_str("static inline void er_set_global(const char *name, int64_t val) {\n");
    out.push_str("    for (size_t i = 0; i < er_globals_count; ++i) {\n");
    out.push_str("        if (strcmp(er_globals[i].key, name) == 0) {\n");
    out.push_str("            er_globals[i].value = val;\n");
    out.push_str("            return;\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    if (er_globals_count < 512) {\n");
    out.push_str("        strncpy(er_globals[er_globals_count].key, name, 63);\n");
    out.push_str("        er_globals[er_globals_count].key[63] = '\\0';\n");
    out.push_str("        er_globals[er_globals_count].value = val;\n");
    out.push_str("        er_globals_count++;\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    out.push_str("static inline int64_t er_get_global(const char *name) {\n");
    out.push_str("    if (strcmp(name, \"print\") == 0) {\n");
    out.push_str("        return (int64_t)ER_TAG_BUILTIN_PRINT;\n");
    out.push_str("    }\n");
    out.push_str("    for (size_t i = 0; i < er_globals_count; ++i) {\n");
    out.push_str("        if (strcmp(er_globals[i].key, name) == 0) {\n");
    out.push_str("            return er_globals[i].value;\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    return (int64_t)ER_TAG_NULL;\n");
    out.push_str("}\n\n");

    out.push_str("typedef struct {\n");
    out.push_str("    char key[64];\n");
    out.push_str("    int64_t value;\n");
    out.push_str("} ErProperty;\n\n");

    out.push_str("typedef struct {\n");
    out.push_str("    ErProperty properties[64];\n");
    out.push_str("    size_t count;\n");
    out.push_str("} ErObjectObj;\n\n");

    out.push_str("static inline int64_t er_alloc_object(size_t cap) {\n");
    out.push_str("    (void)cap;\n");
    out.push_str("    ErObjectObj *obj = (ErObjectObj*)malloc(sizeof(ErObjectObj));\n");
    out.push_str("    obj->count = 0;\n");
    out.push_str("    return (int64_t)(ER_TAG_OBJECT | ((uintptr_t)obj & ER_PTR_MASK));\n");
    out.push_str("}\n\n");

    out.push_str("static inline bool er_is_object(int64_t val) {\n");
    out.push_str("    return ((uint64_t)val & ~ER_PTR_MASK) == ER_TAG_OBJECT;\n");
    out.push_str("}\n\n");

    out.push_str("static inline ErObjectObj* er_as_object_obj(int64_t val) {\n");
    out.push_str("    return (ErObjectObj*)(uintptr_t)((uint64_t)val & ER_PTR_MASK);\n");
    out.push_str("}\n\n");

    out.push_str("static inline void er_object_set(int64_t obj_val, const char *key, int64_t val) {\n");
    out.push_str("    if (!er_is_object(obj_val)) return;\n");
    out.push_str("    ErObjectObj *obj = er_as_object_obj(obj_val);\n");
    out.push_str("    if (!obj) return;\n");
    out.push_str("    for (size_t i = 0; i < obj->count; ++i) {\n");
    out.push_str("        if (strcmp(obj->properties[i].key, key) == 0) {\n");
    out.push_str("            obj->properties[i].value = val;\n");
    out.push_str("            return;\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    if (obj->count < 64) {\n");
    out.push_str("        strncpy(obj->properties[obj->count].key, key, 63);\n");
    out.push_str("        obj->properties[obj->count].key[63] = '\\0';\n");
    out.push_str("        obj->properties[obj->count].value = val;\n");
    out.push_str("        obj->count++;\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    out.push_str("static inline void er_object_set_reg(int64_t obj_val, int64_t key_val, int64_t val) {\n");
    out.push_str("    if (er_is_string(key_val)) {\n");
    out.push_str("        ErStringObj *s = er_as_string_obj(key_val);\n");
    out.push_str("        if (s && s->data) { er_object_set(obj_val, s->data, val); return; }\n");
    out.push_str("    }\n");
    out.push_str("    char buf[64];\n");
    out.push_str("    snprintf(buf, sizeof(buf), \"%lld\", (long long)key_val);\n");
    out.push_str("    er_object_set(obj_val, buf, val);\n");
    out.push_str("}\n\n");

    out.push_str("static inline int64_t er_object_get(int64_t obj_val, const char *key) {\n");
    out.push_str("    if (!er_is_object(obj_val)) return (int64_t)ER_TAG_NULL;\n");
    out.push_str("    ErObjectObj *obj = er_as_object_obj(obj_val);\n");
    out.push_str("    if (!obj) return (int64_t)ER_TAG_NULL;\n");
    out.push_str("    for (size_t i = 0; i < obj->count; ++i) {\n");
    out.push_str("        if (strcmp(obj->properties[i].key, key) == 0) {\n");
    out.push_str("            return obj->properties[i].value;\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    return (int64_t)ER_TAG_NULL;\n");
    out.push_str("}\n\n");

    out.push_str("typedef struct {\n");
    out.push_str("    int64_t *items;\n");
    out.push_str("    size_t len;\n");
    out.push_str("    size_t cap;\n");
    out.push_str("} ErArrayObj;\n\n");

    out.push_str("static inline int64_t er_alloc_array(size_t cap) {\n");
    out.push_str("    ErArrayObj *arr = (ErArrayObj*)malloc(sizeof(ErArrayObj));\n");
    out.push_str("    arr->len = 0;\n");
    out.push_str("    arr->cap = cap > 0 ? cap : 8;\n");
    out.push_str("    arr->items = (int64_t*)malloc(arr->cap * sizeof(int64_t));\n");
    out.push_str("    return (int64_t)(ER_TAG_ARRAY | ((uintptr_t)arr & ER_PTR_MASK));\n");
    out.push_str("}\n\n");

    out.push_str("static inline bool er_is_array(int64_t val) {\n");
    out.push_str("    return ((uint64_t)val & ~ER_PTR_MASK) == ER_TAG_ARRAY;\n");
    out.push_str("}\n\n");

    out.push_str("static inline ErArrayObj* er_as_array_obj(int64_t val) {\n");
    out.push_str("    return (ErArrayObj*)(uintptr_t)((uint64_t)val & ER_PTR_MASK);\n");
    out.push_str("}\n\n");

    out.push_str("static inline void er_array_push(int64_t arr_val, int64_t val) {\n");
    out.push_str("    if (!er_is_array(arr_val)) return;\n");
    out.push_str("    ErArrayObj *arr = er_as_array_obj(arr_val);\n");
    out.push_str("    if (!arr) return;\n");
    out.push_str("    if (arr->len >= arr->cap) {\n");
    out.push_str("        arr->cap *= 2;\n");
    out.push_str("        arr->items = (int64_t*)realloc(arr->items, arr->cap * sizeof(int64_t));\n");
    out.push_str("    }\n");
    out.push_str("    arr->items[arr->len++] = val;\n");
    out.push_str("}\n\n");

    out.push_str("static inline int64_t er_array_get(int64_t arr_val, int64_t index) {\n");
    out.push_str("    if (!er_is_array(arr_val)) return (int64_t)ER_TAG_NULL;\n");
    out.push_str("    ErArrayObj *arr = er_as_array_obj(arr_val);\n");
    out.push_str("    if (!arr || index < 0 || (size_t)index >= arr->len) return (int64_t)ER_TAG_NULL;\n");
    out.push_str("    return arr->items[(size_t)index];\n");
    out.push_str("}\n\n");

    out.push_str("static inline void er_array_set(int64_t arr_val, int64_t index, int64_t val) {\n");
    out.push_str("    if (!er_is_array(arr_val)) return;\n");
    out.push_str("    ErArrayObj *arr = er_as_array_obj(arr_val);\n");
    out.push_str("    if (!arr || index < 0) return;\n");
    out.push_str("    if ((size_t)index >= arr->cap) {\n");
    out.push_str("        arr->cap = (size_t)index + 8;\n");
    out.push_str("        arr->items = (int64_t*)realloc(arr->items, arr->cap * sizeof(int64_t));\n");
    out.push_str("    }\n");
    out.push_str("    while (arr->len <= (size_t)index) {\n");
    out.push_str("        arr->items[arr->len++] = (int64_t)ER_TAG_NULL;\n");
    out.push_str("    }\n");
    out.push_str("    arr->items[(size_t)index] = val;\n");
    out.push_str("}\n\n");

    out.push_str("typedef struct {\n");
    out.push_str("    void *fn_ptr;\n");
    out.push_str("    size_t upvalue_count;\n");
    out.push_str("    int64_t *upvalues[64];\n");
    out.push_str("} ErClosureObj;\n\n");

    out.push_str("static inline int64_t er_alloc_closure(void *fn_ptr, size_t upval_count) {\n");
    out.push_str("    ErClosureObj *c = (ErClosureObj*)malloc(sizeof(ErClosureObj));\n");
    out.push_str("    c->fn_ptr = fn_ptr;\n");
    out.push_str("    c->upvalue_count = upval_count;\n");
    out.push_str("    for (size_t i = 0; i < 64; ++i) c->upvalues[i] = NULL;\n");
    out.push_str("    return (int64_t)(ER_TAG_CLOSURE | ((uintptr_t)c & ER_PTR_MASK));\n");
    out.push_str("}\n\n");

    out.push_str("static inline int64_t* er_new_upval_cell(int64_t initial_val) {\n");
    out.push_str("    int64_t *cell = (int64_t*)malloc(sizeof(int64_t));\n");
    out.push_str("    *cell = initial_val;\n");
    out.push_str("    return cell;\n");
    out.push_str("}\n\n");

    out.push_str("static inline bool er_is_closure(int64_t val) {\n");
    out.push_str("    return ((uint64_t)val & ~ER_PTR_MASK) == ER_TAG_CLOSURE;\n");
    out.push_str("}\n\n");

    out.push_str("static inline ErClosureObj* er_as_closure_obj(int64_t val) {\n");
    out.push_str("    return (ErClosureObj*)(uintptr_t)((uint64_t)val & ER_PTR_MASK);\n");
    out.push_str("}\n\n");

    out.push_str("static __thread ErClosureObj *er_current_closure = NULL;\n\n");

    out.push_str("static inline void er_print_single_val(int64_t val) {\n");
    out.push_str("    if (er_is_string(val)) {\n");
    out.push_str("        ErStringObj *s = er_as_string_obj(val);\n");
    out.push_str("        if (s && s->data) { fputs(s->data, stdout); }\n");
    out.push_str("    } else if (er_is_array(val)) {\n");
    out.push_str("        ErArrayObj *arr = er_as_array_obj(val);\n");
    out.push_str("        putchar('[');\n");
    out.push_str("        if (arr) {\n");
    out.push_str("            for (size_t i = 0; i < arr->len; ++i) {\n");
    out.push_str("                if (i > 0) fputs(\", \", stdout);\n");
    out.push_str("                er_print_single_val(arr->items[i]);\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("        putchar(']');\n");
    out.push_str("    } else if (er_is_object(val)) {\n");
    out.push_str("        ErObjectObj *obj = er_as_object_obj(val);\n");
    out.push_str("        putchar('{');\n");
    out.push_str("        if (obj) {\n");
    out.push_str("            for (size_t i = 0; i < obj->count; ++i) {\n");
    out.push_str("                if (i > 0) fputs(\", \", stdout);\n");
    out.push_str("                printf(\"%s: \", obj->properties[i].key);\n");
    out.push_str("                er_print_single_val(obj->properties[i].value);\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("        putchar('}');\n");
    out.push_str("    } else if (er_is_closure(val)) {\n");
    out.push_str("        fputs(\"<function>\", stdout);\n");
    out.push_str("    } else if ((uint64_t)val == ER_TAG_NULL) {\n");
    out.push_str("        fputs(\"null\", stdout);\n");
    out.push_str("    } else if ((uint64_t)val == ER_TAG_TRUE) {\n");
    out.push_str("        fputs(\"true\", stdout);\n");
    out.push_str("    } else if ((uint64_t)val == ER_TAG_FALSE) {\n");
    out.push_str("        fputs(\"false\", stdout);\n");
    out.push_str("    } else {\n");
    out.push_str("        printf(\"%lld\", (long long)val);\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    out.push_str("static inline int64_t er_call(int64_t callee, const int64_t *args, size_t argc) {\n");
    out.push_str("    if ((uint64_t)callee == ER_TAG_BUILTIN_PRINT) {\n");
    out.push_str("        for (size_t i = 0; i < argc; ++i) {\n");
    out.push_str("            if (i > 0) putchar(' ');\n");
    out.push_str("            er_print_single_val(args[i]);\n");
    out.push_str("        }\n");
    out.push_str("        putchar('\\n');\n");
    out.push_str("        fflush(stdout);\n");
    out.push_str("        return (int64_t)ER_TAG_NULL;\n");
    out.push_str("    }\n");
    out.push_str("    void *fn_to_call = NULL;\n");
    out.push_str("    ErClosureObj *closure = NULL;\n");
    out.push_str("    if (er_is_closure(callee)) {\n");
    out.push_str("        closure = er_as_closure_obj(callee);\n");
    out.push_str("        if (closure) { fn_to_call = closure->fn_ptr; }\n");
    out.push_str("    } else if (callee != 0 && ((uint64_t)callee & 0xffff000000000000ULL) == 0) {\n");
    out.push_str("        fn_to_call = (void*)(uintptr_t)callee;\n");
    out.push_str("    }\n");
    out.push_str("    if (fn_to_call) {\n");
    out.push_str("        ErClosureObj *prev = er_current_closure;\n");
    out.push_str("        if (closure) er_current_closure = closure;\n");
    out.push_str("        int64_t ret = (int64_t)ER_TAG_NULL;\n");
    out.push_str("        typedef int64_t (*Func0)(void);\n");
    out.push_str("        typedef int64_t (*Func1)(int64_t);\n");
    out.push_str("        typedef int64_t (*Func2)(int64_t, int64_t);\n");
    out.push_str("        typedef int64_t (*Func3)(int64_t, int64_t, int64_t);\n");
    out.push_str("        typedef int64_t (*Func4)(int64_t, int64_t, int64_t, int64_t);\n");
    out.push_str("        typedef int64_t (*Func5)(int64_t, int64_t, int64_t, int64_t, int64_t);\n");
    out.push_str("        typedef int64_t (*Func6)(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);\n");
    out.push_str("        typedef int64_t (*Func7)(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);\n");
    out.push_str("        typedef int64_t (*Func8)(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);\n");
    out.push_str("        switch (argc) {\n");
    out.push_str("            case 0: ret = ((Func0)fn_to_call)(); break;\n");
    out.push_str("            case 1: ret = ((Func1)fn_to_call)(args[0]); break;\n");
    out.push_str("            case 2: ret = ((Func2)fn_to_call)(args[0], args[1]); break;\n");
    out.push_str("            case 3: ret = ((Func3)fn_to_call)(args[0], args[1], args[2]); break;\n");
    out.push_str("            case 4: ret = ((Func4)fn_to_call)(args[0], args[1], args[2], args[3]); break;\n");
    out.push_str("            case 5: ret = ((Func5)fn_to_call)(args[0], args[1], args[2], args[3], args[4]); break;\n");
    out.push_str("            case 6: ret = ((Func6)fn_to_call)(args[0], args[1], args[2], args[3], args[4], args[5]); break;\n");
    out.push_str("            case 7: ret = ((Func7)fn_to_call)(args[0], args[1], args[2], args[3], args[4], args[5], args[6]); break;\n");
    out.push_str("            default: ret = ((Func8)fn_to_call)(args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7]); break;\n");
    out.push_str("        }\n");
    out.push_str("        if (closure) er_current_closure = prev;\n");
    out.push_str("        return ret;\n");
    out.push_str("    }\n");
    out.push_str("    return (int64_t)ER_TAG_NULL;\n");
    out.push_str("}\n\n");

    out.push_str("static inline int64_t er_val_add(int64_t a, int64_t b) {\n");
    out.push_str("    if (er_is_string(a) || er_is_string(b)) {\n");
    out.push_str("        char buf_a[64];\n");
    out.push_str("        char buf_b[64];\n");
    out.push_str("        const char *sa = \"\";\n");
    out.push_str("        size_t la = 0;\n");
    out.push_str("        const char *sb = \"\";\n");
    out.push_str("        size_t lb = 0;\n");
    out.push_str("        if (er_is_string(a)) {\n");
    out.push_str("            ErStringObj *obj = er_as_string_obj(a);\n");
    out.push_str("            if (obj && obj->data) { sa = obj->data; la = obj->len; }\n");
    out.push_str("        } else if ((uint64_t)a == ER_TAG_NULL) {\n");
    out.push_str("            sa = \"null\"; la = 4;\n");
    out.push_str("        } else if ((uint64_t)a == ER_TAG_TRUE) {\n");
    out.push_str("            sa = \"true\"; la = 4;\n");
    out.push_str("        } else if ((uint64_t)a == ER_TAG_FALSE) {\n");
    out.push_str("            sa = \"false\"; la = 5;\n");
    out.push_str("        } else {\n");
    out.push_str("            la = snprintf(buf_a, sizeof(buf_a), \"%lld\", (long long)a);\n");
    out.push_str("            sa = buf_a;\n");
    out.push_str("        }\n");
    out.push_str("        if (er_is_string(b)) {\n");
    out.push_str("            ErStringObj *obj = er_as_string_obj(b);\n");
    out.push_str("            if (obj && obj->data) { sb = obj->data; lb = obj->len; }\n");
    out.push_str("        } else if ((uint64_t)b == ER_TAG_NULL) {\n");
    out.push_str("            sb = \"null\"; lb = 4;\n");
    out.push_str("        } else if ((uint64_t)b == ER_TAG_TRUE) {\n");
    out.push_str("            sb = \"true\"; lb = 4;\n");
    out.push_str("        } else if ((uint64_t)b == ER_TAG_FALSE) {\n");
    out.push_str("            sb = \"false\"; lb = 5;\n");
    out.push_str("        } else {\n");
    out.push_str("            lb = snprintf(buf_b, sizeof(buf_b), \"%lld\", (long long)b);\n");
    out.push_str("            sb = buf_b;\n");
    out.push_str("        }\n");
    out.push_str("        size_t total_len = la + lb;\n");
    out.push_str("        char *concat = (char*)malloc(total_len + 1);\n");
    out.push_str("        if (la > 0) memcpy(concat, sa, la);\n");
    out.push_str("        if (lb > 0) memcpy(concat + la, sb, lb);\n");
    out.push_str("        concat[total_len] = '\\0';\n");
    out.push_str("        ErStringObj *res = (ErStringObj*)malloc(sizeof(ErStringObj));\n");
    out.push_str("        res->data = concat;\n");
    out.push_str("        res->len = total_len;\n");
    out.push_str("        return (int64_t)(ER_TAG_STRING | ((uintptr_t)res & ER_PTR_MASK));\n");
    out.push_str("    }\n");
    out.push_str("    return a + b;\n");
    out.push_str("}\n\n");

    // Forward declarations for child functions
    for func in &module.functions {
        if func.name != "main" {
            let mut p = Vec::new();
            for i in 0..func.arity {
                p.push(format!("int64_t arg{}", i));
            }
            let params_str = if p.is_empty() { "void".to_string() } else { p.join(", ") };
            out.push_str(&format!("static int64_t {}({});\n", func.name, params_str));
        }
    }
    out.push('\n');

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

    if is_main {
        out.push_str("int main(void) {\n");
    } else {
        let mut p = Vec::new();
        for i in 0..func.arity {
            p.push(format!("int64_t arg{}", i));
        }
        let params_str = if p.is_empty() { "void".to_string() } else { p.join(", ") };
        out.push_str(&format!("static {} {}({}) {{\n", ret_type, func.name, params_str));
    }

    // Registers
    let num_regs = func.reg_count.max(64);
    out.push_str(&format!("    int64_t r[{}] = {{0}};\n", num_regs));
    out.push_str(&format!("    double d[{}] = {{0.0}};\n", num_regs));
    out.push_str("    int64_t *upval_cells[64] = {0};\n\n");

    if is_main {
        out.push_str("    er_runtime_init();\n\n");
    } else {
        for i in 0..func.arity {
            out.push_str(&format!("    r[{}] = arg{};\n", i, i));
        }
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
                out.push_str(&format!(
                    "    r[{}] = (int64_t){};\n",
                    dest,
                    if *val { "ER_TAG_TRUE" } else { "ER_TAG_FALSE" }
                ));
            }
            EirInst::ConstNull { dest } => {
                out.push_str(&format!("    r[{}] = (int64_t)ER_TAG_NULL;\n", dest));
            }
            EirInst::ConstString { dest, val } => {
                let mut escaped = String::new();
                for c in val.chars() {
                    match c {
                        '\\' => escaped.push_str("\\\\"),
                        '"' => escaped.push_str("\\\""),
                        '\n' => escaped.push_str("\\n"),
                        '\r' => escaped.push_str("\\r"),
                        '\t' => escaped.push_str("\\t"),
                        other => escaped.push(other),
                    }
                }
                out.push_str(&format!(
                    "    r[{}] = er_alloc_string((const uint8_t*)\"{}\", {});\n",
                    dest,
                    escaped,
                    val.len()
                ));
            }
            EirInst::Move { dest, src, ty } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("    d[{}] = d[{}];\n", dest, src));
                } else {
                    out.push_str(&format!("    r[{}] = r[{}];\n", dest, src));
                }
            }
            EirInst::DefineGlobal { name, src } => {
                out.push_str(&format!("    er_set_global(\"{}\", r[{}]);\n", name, src));
            }
            EirInst::GetGlobal { dest, name } => {
                out.push_str(&format!("    r[{}] = er_get_global(\"{}\");\n", dest, name));
            }
            EirInst::CallDynamic { dest, callee, args } => {
                if args.is_empty() {
                    out.push_str(&format!("    r[{}] = er_call(r[{}], NULL, 0);\n", dest, callee));
                } else {
                    let args_str = args
                        .iter()
                        .map(|a| format!("r[{}]", a))
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        "    r[{}] = er_call(r[{}], (const int64_t[]){{ {} }}, {});\n",
                        dest,
                        callee,
                        args_str,
                        args.len()
                    ));
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
                    match op {
                        EirBinaryOp::Add => {
                            out.push_str(&format!("    r[{}] = er_val_add(r[{}], r[{}]);\n", dest, left, right));
                        }
                        EirBinaryOp::Sub => {
                            out.push_str(&format!("    r[{}] = r[{}] - r[{}];\n", dest, left, right));
                        }
                        EirBinaryOp::Mul => {
                            out.push_str(&format!("    r[{}] = r[{}] * r[{}];\n", dest, left, right));
                        }
                        EirBinaryOp::Div => {
                            out.push_str(&format!("    r[{}] = r[{}] ? (r[{}] / r[{}]) : 0;\n", dest, right, left, right));
                        }
                        EirBinaryOp::Mod => {
                            out.push_str(&format!("    r[{}] = r[{}] ? (r[{}] % r[{}]) : 0;\n", dest, right, left, right));
                        }
                        EirBinaryOp::BitAnd => {
                            out.push_str(&format!("    r[{}] = r[{}] & r[{}];\n", dest, left, right));
                        }
                        EirBinaryOp::BitOr => {
                            out.push_str(&format!("    r[{}] = r[{}] | r[{}];\n", dest, left, right));
                        }
                        EirBinaryOp::BitXor => {
                            out.push_str(&format!("    r[{}] = r[{}] ^ r[{}];\n", dest, left, right));
                        }
                        EirBinaryOp::Shl => {
                            out.push_str(&format!("    r[{}] = r[{}] << (r[{}] & 63);\n", dest, left, right));
                        }
                        EirBinaryOp::Shr => {
                            out.push_str(&format!("    r[{}] = r[{}] >> (r[{}] & 63);\n", dest, left, right));
                        }
                        EirBinaryOp::Eq => {
                            out.push_str(&format!("    r[{}] = (r[{}] == r[{}]);\n", dest, left, right));
                        }
                        EirBinaryOp::Ne => {
                            out.push_str(&format!("    r[{}] = (r[{}] != r[{}]);\n", dest, left, right));
                        }
                        EirBinaryOp::Lt => {
                            out.push_str(&format!("    r[{}] = (r[{}] < r[{}]);\n", dest, left, right));
                        }
                        EirBinaryOp::Le => {
                            out.push_str(&format!("    r[{}] = (r[{}] <= r[{}]);\n", dest, left, right));
                        }
                        EirBinaryOp::Gt => {
                            out.push_str(&format!("    r[{}] = (r[{}] > r[{}]);\n", dest, left, right));
                        }
                        EirBinaryOp::Ge => {
                            out.push_str(&format!("    r[{}] = (r[{}] >= r[{}]);\n", dest, left, right));
                        }
                    }
                }
            }
            EirInst::Unary { dest, op, ty, src } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("    d[{}] = -d[{}];\n", dest, src));
                } else {
                    match op {
                        EirUnaryOp::Neg => out.push_str(&format!("    r[{}] = -r[{}];\n", dest, src)),
                        EirUnaryOp::Not => out.push_str(&format!(
                            "    r[{}] = (!r[{}] || r[{}] == (int64_t)ER_TAG_FALSE || r[{}] == (int64_t)ER_TAG_NULL);\n",
                            dest, src, src, src
                        )),
                        EirUnaryOp::BitNot => out.push_str(&format!("    r[{}] = ~r[{}];\n", dest, src)),
                    }
                }
            }
            EirInst::Jump { target } => {
                out.push_str(&format!("    goto lbl_{};\n", target));
            }
            EirInst::BranchIfFalse { cond, target } => {
                out.push_str(&format!(
                    "    if (!r[{}] || r[{}] == (int64_t)ER_TAG_FALSE || r[{}] == (int64_t)ER_TAG_NULL) goto lbl_{};\n",
                    cond, cond, cond, target
                ));
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
                    out.push_str("    return 0;\n");
                }
            }
            EirInst::Println { reg, ty } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("    er_println_f64(d[{}]);\n", reg));
                } else {
                    out.push_str(&format!("    er_println_i64(r[{}]);\n", reg));
                }
            }
            EirInst::Print { reg, ty } => {
                if *ty == EirType::F64 {
                    out.push_str(&format!("    printf(\"%f\", d[{}]); fflush(stdout);\n", reg));
                } else {
                    out.push_str(&format!("    er_print_single_val(r[{}]); fflush(stdout);\n", reg));
                }
            }
            EirInst::ConstFuncPtr { dest, symbol } => {
                out.push_str(&format!("    r[{}] = (int64_t)(uintptr_t)&{};\n", dest, symbol));
            }
            EirInst::MakeClosure { dest, symbol, upvalues } => {
                out.push_str(&format!("    r[{}] = er_alloc_closure((void*)&{}, {});\n", dest, symbol, upvalues.len()));
                if !upvalues.is_empty() {
                    out.push_str(&format!("    {{\n        ErClosureObj *cl_{} = er_as_closure_obj(r[{}]);\n", dest, dest));
                    for (i, (is_local, idx)) in upvalues.iter().enumerate() {
                        if *is_local {
                            out.push_str(&format!("        if (cl_{}) {{\n            if (!upval_cells[{}]) upval_cells[{}] = er_new_upval_cell(r[{}]);\n            cl_{}->upvalues[{}] = upval_cells[{}];\n        }}\n", dest, idx, idx, idx, dest, i, idx));
                        } else {
                            out.push_str(&format!("        if (cl_{}) cl_{}->upvalues[{}] = er_current_closure ? er_current_closure->upvalues[{}] : NULL;\n", dest, dest, i, idx));
                        }
                    }
                    out.push_str("    }\n");
                }
            }
            EirInst::GetUpvalue { dest, upval_idx } => {
                out.push_str(&format!(
                    "    r[{}] = (er_current_closure && er_current_closure->upvalues[{}] != NULL) ? *(er_current_closure->upvalues[{}]) : (int64_t)ER_TAG_NULL;\n",
                    dest, upval_idx, upval_idx
                ));
            }
            EirInst::SetUpvalue { src, upval_idx } => {
                out.push_str(&format!(
                    "    if (er_current_closure && er_current_closure->upvalues[{}] != NULL) {{ *(er_current_closure->upvalues[{}]) = r[{}]; }}\n",
                    upval_idx, upval_idx, src
                ));
            }
            EirInst::MakeObject { dest, start_reg, pair_count } => {
                out.push_str(&format!("    {{\n        int64_t tmp_obj_{} = er_alloc_object({});\n", dest, pair_count));
                for i in 0..*pair_count {
                    out.push_str(&format!(
                        "        er_object_set_reg(tmp_obj_{}, r[{}], r[{}]);\n",
                        dest,
                        start_reg + i * 2,
                        start_reg + i * 2 + 1
                    ));
                }
                out.push_str(&format!("        r[{}] = tmp_obj_{};\n    }}\n", dest, dest));
            }
            EirInst::GetProperty { dest, obj, key } => {
                out.push_str(&format!("    r[{}] = er_object_get(r[{}], \"{}\");\n", dest, obj, key));
            }
            EirInst::SetProperty { obj, key, val } => {
                out.push_str(&format!("    er_object_set(r[{}], \"{}\", r[{}]);\n", obj, key, val));
            }
            EirInst::MakeArray { dest, start_reg, count } => {
                out.push_str(&format!("    {{\n        int64_t tmp_arr_{} = er_alloc_array({});\n", dest, count));
                for i in 0..*count {
                    out.push_str(&format!("        er_array_push(tmp_arr_{}, r[{}]);\n", dest, start_reg + i));
                }
                out.push_str(&format!("        r[{}] = tmp_arr_{};\n    }}\n", dest, dest));
            }
            EirInst::ArrayGet { dest, arr, index } => {
                out.push_str(&format!("    r[{}] = er_array_get(r[{}], r[{}]);\n", dest, arr, index));
            }
            EirInst::ArraySet { arr, index, val } => {
                out.push_str(&format!("    er_array_set(r[{}], r[{}], r[{}]);\n", arr, index, val));
            }
            EirInst::AllocArray { dest, cap } => {
                out.push_str(&format!("    r[{}] = er_alloc_array({});\n", dest, cap));
            }
            EirInst::ArrayPush { arr, val } => {
                out.push_str(&format!("    er_array_push(r[{}], r[{}]);\n", arr, val));
            }
            EirInst::Call { dest, func_name, args, .. } => {
                if args.is_empty() {
                    out.push_str(&format!("    r[{}] = {}(void);\n", dest, func_name));
                } else {
                    let args_str = args.iter().map(|a| format!("r[{}]", a)).collect::<Vec<_>>().join(", ");
                    out.push_str(&format!("    r[{}] = {}({});\n", dest, func_name, args_str));
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

