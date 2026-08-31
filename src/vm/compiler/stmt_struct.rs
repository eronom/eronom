use super::{Compiler, Local};
use super::structs::FlattenedStructInfo;
use crate::frontend::Stmt;
use crate::vm::bytecode::OpCode;
use crate::vm::gc::{gc_allocate, GcData};
use crate::vm::value::Value;

impl Compiler {
    pub(crate) fn compile_struct_decl(
        &mut self,
        name: &str,
        composed: &[String],
        fields: &[(String, String)],
        methods: &[(String, Vec<String>, Stmt)],
    ) -> Result<(), String> {
        let flat = self.structs.get(name).cloned().unwrap_or_else(|| FlattenedStructInfo {
            composed: composed.to_vec(),
            fields: fields.to_vec(),
            methods: methods.to_vec(),
        });
        let name_val = Value::string_from_str(name);
        let name_idx = self.current_chunk().add_constant(name_val);
        
        let mut field_names_vals = Vec::new();
        for (field_name, _) in &flat.fields {
            field_names_vals.push(Value::string_from_str(field_name.as_str()));
        }
        let array_ptr = gc_allocate(GcData::Array(field_names_vals));
        let fields_val = Value::array(array_ptr);
        let fields_idx = self.current_chunk().add_constant(fields_val);

        let mut methods_map = crate::vm::gc::get_pooled_map(flat.methods.len());
        for (m_name, m_params, m_body) in &flat.methods {
            let mut method_params = vec!["this".to_string()];
            method_params.extend(m_params.clone());

            let mut compiler = Compiler::new();
            compiler.const_globals = self.const_globals.clone();
            compiler.structs = self.structs.clone();
            compiler.interfaces = self.interfaces.clone();
            compiler.global_types = self.global_types.clone();
            compiler.current_struct = Some(name.to_string());
            compiler.function.arity = method_params.len();
            compiler.function.is_async = false;
            compiler.next_reg = method_params.len();
            compiler.begin_scope();
            for param in &method_params {
                compiler.locals.push(Local {
                    name: param.clone(),
                    depth: compiler.scope_depth,
                    is_const: false,
                    loc: crate::frontend::SourceLocation::default(),
                    ty: None,
                });
            }
            compiler.compile_stmt(m_body)?;
            compiler.current_chunk().write_instruction(OpCode::LoadNull, 0, 0, 0, 0);
            compiler.current_chunk().write_instruction(OpCode::Return, 0, 0, 0, 0);

            let func = compiler.function;
            let func_ptr = gc_allocate(GcData::Function(Box::new(func)));
            let func_val = Value::function(func_ptr);

            methods_map.insert(crate::vm::value::MapKey(Value::string_from_str(m_name.as_str())), func_val);
        }
        let methods_ptr = gc_allocate(GcData::Object(methods_map));
        let methods_val = Value::object(methods_ptr);
        let methods_idx = self.current_chunk().add_constant(methods_val);

        self.current_chunk().write_instruction(
            OpCode::DefineStruct,
            fields_idx as u8,
            methods_idx as u8,
            0,
            name_idx as u32,
        );
        Ok(())
    }
}
