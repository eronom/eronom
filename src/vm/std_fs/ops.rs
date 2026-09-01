use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::vm::gc::{
    gc_alloc_string, gc_allocate, get_or_create_string, get_pooled_map, get_pooled_vec, GcData,
};
use crate::vm::value::{MapKey, Value};

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
        Value::boolean(crate::vm::embedded::has_vfs_file(path_str))
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

    if let Some(vfs_bytes) = crate::vm::embedded::get_vfs_file(path_str) {
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

    if let Some(vfs_text) = crate::vm::embedded::get_vfs_text(path_str) {
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

    if let Some(bytes) = crate::vm::embedded::get_vfs_file(path_str) {
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
