mod ops;
mod stream;

pub use ops::*;
pub use stream::*;

use crate::vm::execute::VM;
use crate::vm::value::Value;

pub fn register_fs_natives(vm: &mut VM) {
    vm.register_global("Eronom_nativeReadDir", Value::native_function(native_fs_read_dir));
    vm.register_global("Eronom_nativeMakeDir", Value::native_function(native_fs_make_dir));
    vm.register_global("Eronom_nativeRemoveDir", Value::native_function(native_fs_remove_dir));
    vm.register_global("Eronom_nativeExists", Value::native_function(native_fs_exists));
    vm.register_global("Eronom_nativeStat", Value::native_function(native_fs_stat));
    vm.register_global("Eronom_nativeReadText", Value::native_function(native_fs_read_text));
    vm.register_global("Eronom_nativeWriteText", Value::native_function(native_fs_write_text));
    vm.register_global("Eronom_nativeAppendText", Value::native_function(native_fs_append_text));
    vm.register_global("Eronom_nativeReadBinary", Value::native_function(native_fs_read_binary));
    vm.register_global("Eronom_nativeWriteBinary", Value::native_function(native_fs_write_binary));
    vm.register_global("Eronom_nativeRemoveFile", Value::native_function(native_fs_remove_file));
    vm.register_global("Eronom_nativeCopyFile", Value::native_function(native_fs_copy_file));
    vm.register_global("Eronom_nativeRename", Value::native_function(native_fs_rename));
    vm.register_global("Eronom_nativeOpenReadStream", Value::native_function(native_fs_open_read_stream));
    vm.register_global("Eronom_nativeReadStreamChunk", Value::native_function(native_fs_read_stream_chunk));
    vm.register_global("Eronom_nativeReadStreamBinaryChunk", Value::native_function(native_fs_read_stream_binary_chunk));
    vm.register_global("Eronom_nativeCloseReadStream", Value::native_function(native_fs_close_read_stream));
}
