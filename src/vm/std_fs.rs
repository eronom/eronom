use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

use super::gc::{
    gc_alloc_string, gc_allocate, get_or_create_string, get_pooled_map, get_pooled_vec, GcData,
};
use super::value::{MapKey, Value};
use super::execute::VM;

struct ReadStreamState {
    file: File,
}

thread_local! {
    static NEXT_STREAM_ID: RefCell<u64> = const { RefCell::new(1) };
    static READ_STREAMS: RefCell<HashMap<u64, ReadStreamState>> = RefCell::new(HashMap::new());
}

pub fn native_fs_read_dir(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };

    let with_types = if args.len() > 1 {
        if args[1].is_boolean() {
            args[1].as_boolean()
        } else if args[1].is_object() {
            unsafe {
                if let GcData::Object(ref map) = (*args[1].as_gc_ptr()).data {
                    let key = MapKey(Value::string(get_or_create_string("withFileTypes")));
                    map.get(&key).map_or(false, |v| v.is_boolean() && v.as_boolean())
                } else {
                    false
                }
            }
        } else {
            false
        }
    } else {
        false
    };

    let read_res = fs::read_dir(Path::new(path_str));
    let entries = match read_res {
        Ok(e) => e,
        Err(_) => return Value::null(),
    };

    let mut result_list = get_pooled_vec(16);

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if with_types {
            let file_type = entry.file_type().ok();
            let is_dir = file_type.map_or(false, |ft| ft.is_dir());
            let is_file = file_type.map_or(false, |ft| ft.is_file());
            let is_symlink = file_type.map_or(false, |ft| ft.is_symlink());

            let mut obj_map = get_pooled_map(4);
            let name_key = MapKey(Value::string(get_or_create_string("name")));
            let name_val = Value::string(gc_alloc_string(&name));
            obj_map.insert(name_key, name_val);

            let dir_key = MapKey(Value::string(get_or_create_string("isDirectory")));
            obj_map.insert(dir_key, Value::boolean(is_dir));

            let file_key = MapKey(Value::string(get_or_create_string("isFile")));
            obj_map.insert(file_key, Value::boolean(is_file));

            let symlink_key = MapKey(Value::string(get_or_create_string("isSymlink")));
            obj_map.insert(symlink_key, Value::boolean(is_symlink));

            let ptr = gc_allocate(GcData::Object(obj_map));
            result_list.push(Value::object(ptr));
        } else {
            let ptr = gc_alloc_string(&name);
            result_list.push(Value::string(ptr));
        }
    }

    let ptr = gc_allocate(GcData::Array(result_list));
    Value::array(ptr)
}

pub fn native_fs_make_dir(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };

    let recursive = if args.len() > 1 {
        if args[1].is_boolean() {
            args[1].as_boolean()
        } else if args[1].is_object() {
            unsafe {
                if let GcData::Object(ref map) = (*args[1].as_gc_ptr()).data {
                    let key = MapKey(Value::string(get_or_create_string("recursive")));
                    map.get(&key).map_or(false, |v| v.is_boolean() && v.as_boolean())
                } else {
                    false
                }
            }
        } else {
            false
        }
    } else {
        false
    };

    let path = Path::new(path_str);
    let res = if recursive {
        fs::create_dir_all(path)
    } else {
        fs::create_dir(path)
    };

    Value::boolean(res.is_ok())
}

pub fn native_fs_remove_dir(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };

    let recursive = if args.len() > 1 {
        if args[1].is_boolean() {
            args[1].as_boolean()
        } else if args[1].is_object() {
            unsafe {
                if let GcData::Object(ref map) = (*args[1].as_gc_ptr()).data {
                    let key = MapKey(Value::string(get_or_create_string("recursive")));
                    map.get(&key).map_or(false, |v| v.is_boolean() && v.as_boolean())
                } else {
                    false
                }
            }
        } else {
            false
        }
    } else {
        false
    };

    let path = Path::new(path_str);
    let res = if recursive {
        fs::remove_dir_all(path)
    } else {
        fs::remove_dir(path)
    };

    Value::boolean(res.is_ok())
}

pub fn native_fs_exists(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    if Path::new(path_str).exists() {
        Value::boolean(true)
    } else {
        Value::boolean(super::embedded::has_vfs_file(path_str))
    }
}

pub fn native_fs_stat(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };

    if let Ok(meta) = fs::metadata(Path::new(path_str)) {
        let mut obj_map = get_pooled_map(8);

        let size_key = MapKey(Value::string(get_or_create_string("size")));
        obj_map.insert(size_key, Value::number(meta.len() as f64));

        let is_file_key = MapKey(Value::string(get_or_create_string("isFile")));
        obj_map.insert(is_file_key, Value::boolean(meta.is_file()));

        let is_dir_key = MapKey(Value::string(get_or_create_string("isDirectory")));
        obj_map.insert(is_dir_key, Value::boolean(meta.is_dir()));

        let is_sym_key = MapKey(Value::string(get_or_create_string("isSymlink")));
        obj_map.insert(is_sym_key, Value::boolean(meta.is_symlink()));

        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0.0, |d| d.as_millis() as f64);
        let mtime_key = MapKey(Value::string(get_or_create_string("mtime")));
        obj_map.insert(mtime_key, Value::number(mtime_ms));

        let created_ms = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0.0, |d| d.as_millis() as f64);
        let created_key = MapKey(Value::string(get_or_create_string("created")));
        obj_map.insert(created_key, Value::number(created_ms));

        let readonly_key = MapKey(Value::string(get_or_create_string("readonly")));
        obj_map.insert(readonly_key, Value::boolean(meta.permissions().readonly()));

        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode() as f64
        };
        #[cfg(not(unix))]
        let mode = 0.0;
        let mode_key = MapKey(Value::string(get_or_create_string("mode")));
        obj_map.insert(mode_key, Value::number(mode));

        let ptr = gc_allocate(GcData::Object(obj_map));
        return Value::object(ptr);
    }

    if let Some(vfs_bytes) = super::embedded::get_vfs_file(path_str) {
        let mut obj_map = get_pooled_map(8);

        let size_key = MapKey(Value::string(get_or_create_string("size")));
        obj_map.insert(size_key, Value::number(vfs_bytes.len() as f64));

        let is_file_key = MapKey(Value::string(get_or_create_string("isFile")));
        obj_map.insert(is_file_key, Value::boolean(true));

        let is_dir_key = MapKey(Value::string(get_or_create_string("isDirectory")));
        obj_map.insert(is_dir_key, Value::boolean(false));

        let is_sym_key = MapKey(Value::string(get_or_create_string("isSymlink")));
        obj_map.insert(is_sym_key, Value::boolean(false));

        let mtime_key = MapKey(Value::string(get_or_create_string("mtime")));
        obj_map.insert(mtime_key, Value::number(0.0));

        let created_key = MapKey(Value::string(get_or_create_string("created")));
        obj_map.insert(created_key, Value::number(0.0));

        let readonly_key = MapKey(Value::string(get_or_create_string("readonly")));
        obj_map.insert(readonly_key, Value::boolean(true));

        let mode_key = MapKey(Value::string(get_or_create_string("mode")));
        obj_map.insert(mode_key, Value::number(0.0));

        let ptr = gc_allocate(GcData::Object(obj_map));
        return Value::object(ptr);
    }

    Value::null()
}

pub fn native_fs_read_text(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };

    if let Ok(content) = fs::read_to_string(Path::new(path_str)) {
        let ptr = gc_alloc_string(&content);
        return Value::string(ptr);
    }

    if let Some(vfs_text) = super::embedded::get_vfs_text(path_str) {
        let ptr = gc_alloc_string(&vfs_text);
        return Value::string(ptr);
    }

    Value::null()
}

pub fn native_fs_write_text(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    let content = match args[1].as_str() {
        Some(s) => s.to_string(),
        None => args[1].to_string(),
    };

    match fs::write(Path::new(path_str), content) {
        Ok(_) => Value::boolean(true),
        Err(_) => Value::boolean(false),
    }
}

pub fn native_fs_append_text(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    let content = match args[1].as_str() {
        Some(s) => s.to_string(),
        None => args[1].to_string(),
    };

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new(path_str));

    match file {
        Ok(mut f) => match f.write_all(content.as_bytes()) {
            Ok(_) => Value::boolean(true),
            Err(_) => Value::boolean(false),
        },
        Err(_) => Value::boolean(false),
    }
}

pub fn native_fs_read_binary(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };

    if let Ok(bytes) = fs::read(Path::new(path_str)) {
        let mut arr = get_pooled_vec(bytes.len());
        for b in bytes {
            arr.push(Value::number(b as f64));
        }
        let ptr = gc_allocate(GcData::Array(arr));
        return Value::array(ptr);
    }

    if let Some(bytes) = super::embedded::get_vfs_file(path_str) {
        let mut arr = get_pooled_vec(bytes.len());
        for b in bytes {
            arr.push(Value::number(b as f64));
        }
        let ptr = gc_allocate(GcData::Array(arr));
        return Value::array(ptr);
    }

    Value::null()
}

pub fn native_fs_write_binary(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };

    let mut bytes = Vec::new();
    if args[1].is_array() {
        unsafe {
            if let GcData::Array(ref arr) = (*args[1].as_gc_ptr()).data {
                bytes.reserve(arr.len());
                for val in arr {
                    bytes.push(val.as_number() as u8);
                }
            }
        }
    } else if let Some(s) = args[1].as_str() {
        bytes.extend_from_slice(s.as_bytes());
    } else {
        return Value::boolean(false);
    }

    match fs::write(Path::new(path_str), bytes) {
        Ok(_) => Value::boolean(true),
        Err(_) => Value::boolean(false),
    }
}

pub fn native_fs_remove_file(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    Value::boolean(fs::remove_file(Path::new(path_str)).is_ok())
}

pub fn native_fs_copy_file(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let src = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    let dest = match args[1].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    match fs::copy(Path::new(src), Path::new(dest)) {
        Ok(bytes) => Value::number(bytes as f64),
        Err(_) => Value::boolean(false),
    }
}

pub fn native_fs_rename(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let from = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    let to = match args[1].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    Value::boolean(fs::rename(Path::new(from), Path::new(to)).is_ok())
}

pub fn native_fs_open_read_stream(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };

    let file = match File::open(Path::new(path_str)) {
        Ok(f) => f,
        Err(_) => return Value::null(),
    };

    let id = NEXT_STREAM_ID.with(|id_cell| {
        let cur = *id_cell.borrow();
        *id_cell.borrow_mut() = cur + 1;
        cur
    });

    READ_STREAMS.with(|map_cell| {
        map_cell.borrow_mut().insert(id, ReadStreamState { file });
    });

    Value::number(id as f64)
}

pub fn native_fs_read_stream_chunk(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let id = args[0].as_number() as u64;
    let chunk_size = if args.len() > 1 && args[1].is_number() {
        args[1].as_number() as usize
    } else {
        65536
    };

    READ_STREAMS.with(|map_cell| {
        let mut map = map_cell.borrow_mut();
        if let Some(state) = map.get_mut(&id) {
            let mut buf = vec![0u8; chunk_size];
            match state.file.read(&mut buf) {
                Ok(0) => {
                    map.remove(&id);
                    Value::null()
                }
                Ok(n) => {
                    buf.truncate(n);
                    let s = String::from_utf8_lossy(&buf);
                    let ptr = gc_alloc_string(&s);
                    Value::string(ptr)
                }
                Err(_) => {
                    map.remove(&id);
                    Value::null()
                }
            }
        } else {
            Value::null()
        }
    })
}

pub fn native_fs_read_stream_binary_chunk(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let id = args[0].as_number() as u64;
    let chunk_size = if args.len() > 1 && args[1].is_number() {
        args[1].as_number() as usize
    } else {
        65536
    };

    READ_STREAMS.with(|map_cell| {
        let mut map = map_cell.borrow_mut();
        if let Some(state) = map.get_mut(&id) {
            let mut buf = vec![0u8; chunk_size];
            match state.file.read(&mut buf) {
                Ok(0) => {
                    map.remove(&id);
                    Value::null()
                }
                Ok(n) => {
                    buf.truncate(n);
                    let mut arr = get_pooled_vec(n);
                    for b in buf {
                        arr.push(Value::number(b as f64));
                    }
                    let ptr = gc_allocate(GcData::Array(arr));
                    Value::array(ptr)
                }
                Err(_) => {
                    map.remove(&id);
                    Value::null()
                }
            }
        } else {
            Value::null()
        }
    })
}

pub fn native_fs_close_read_stream(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let id = args[0].as_number() as u64;
    let removed = READ_STREAMS.with(|map_cell| map_cell.borrow_mut().remove(&id).is_some());
    Value::boolean(removed)
}

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
