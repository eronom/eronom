pub mod string_methods;
pub mod array_methods;
pub mod array_methods_core;
pub mod array_methods_iter;
pub mod object_methods;

pub use string_methods::{get_string_builtin_method_id, execute_string_method};
pub use array_methods::{get_array_builtin_method_id, execute_array_method};
pub use object_methods::{get_object_builtin_method_id, execute_object_method};

use crate::vm::value::Value;
use crate::vm::gc::BuiltinMethodId;
use crate::vm::execute::types::VM;

impl VM {
    pub fn execute_builtin_method(
        &mut self,
        receiver: Value,
        method: BuiltinMethodId,
        args: &[Value],
    ) -> Result<Value, String> {
        use BuiltinMethodId::*;
        match method {
            StringToUpperCase | StringToLowerCase | StringTrim | StringTrimStart | StringTrimEnd
            | StringSplit | StringSlice | StringSubstring | StringIndexOf | StringLastIndexOf
            | StringIncludes | StringStartsWith | StringEndsWith | StringReplace | StringReplaceAll
            | StringCharAt | StringCharCodeAt | StringRepeat | StringPadStart | StringPadEnd
            | StringConcat => {
                execute_string_method(receiver, method, args)
            }
            ArrayPush | ArrayPop | ArrayShift | ArrayUnshift | ArrayMap | ArrayFilter
            | ArrayReduce | ArrayForEach | ArrayFind | ArrayFindIndex | ArraySome | ArrayEvery
            | ArrayIncludes | ArrayIndexOf | ArrayLastIndexOf | ArraySlice | ArrayJoin
            | ArrayConcat | ArrayReverse | ArraySort | ArrayFlat | ArrayFlatMap | ArrayFill => {
                execute_array_method(self, receiver, method, args)
            }
            ObjectKeys | ObjectValues | ObjectEntries | ObjectHasOwnProperty => {
                execute_object_method(receiver, method, args)
            }
        }
    }
}
