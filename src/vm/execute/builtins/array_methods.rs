use crate::vm::value::Value;
use crate::vm::gc::BuiltinMethodId;
use crate::vm::execute::types::VM;

pub use super::array_methods_core::execute_array_core_method;
pub use super::array_methods_iter::execute_array_iter_method;

pub fn get_array_builtin_method_id(name: &str) -> Option<BuiltinMethodId> {
    use BuiltinMethodId::*;
    match name {
        "push" => Some(ArrayPush),
        "pop" => Some(ArrayPop),
        "shift" => Some(ArrayShift),
        "unshift" => Some(ArrayUnshift),
        "map" => Some(ArrayMap),
        "filter" => Some(ArrayFilter),
        "reduce" => Some(ArrayReduce),
        "forEach" => Some(ArrayForEach),
        "find" => Some(ArrayFind),
        "findIndex" => Some(ArrayFindIndex),
        "some" => Some(ArraySome),
        "every" => Some(ArrayEvery),
        "includes" => Some(ArrayIncludes),
        "indexOf" => Some(ArrayIndexOf),
        "lastIndexOf" => Some(ArrayLastIndexOf),
        "slice" => Some(ArraySlice),
        "join" => Some(ArrayJoin),
        "concat" => Some(ArrayConcat),
        "reverse" => Some(ArrayReverse),
        "sort" => Some(ArraySort),
        "flat" => Some(ArrayFlat),
        "flatMap" => Some(ArrayFlatMap),
        "fill" => Some(ArrayFill),
        _ => None,
    }
}

pub fn execute_array_method(
    vm: &mut VM,
    receiver: Value,
    method: BuiltinMethodId,
    args: &[Value],
) -> Result<Value, String> {
    use BuiltinMethodId::*;
    match method {
        ArrayMap | ArrayFilter | ArrayReduce | ArrayForEach | ArrayFind | ArrayFindIndex
        | ArraySome | ArrayEvery | ArrayFlatMap => {
            execute_array_iter_method(vm, receiver, method, args)
        }
        _ => {
            execute_array_core_method(vm, receiver, method, args)
        }
    }
}
