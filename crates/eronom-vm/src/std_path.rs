use std::path::{Component, Path, PathBuf};
use super::gc::gc_alloc_string;
use super::value::Value;
use super::execute::VM;

fn normalize_path_str(path_str: &str) -> String {
    if path_str.is_empty() {
        return ".".to_string();
    }

    let is_abs = path_str.starts_with('/') || path_str.starts_with('\\');
    let mut parts: Vec<&str> = Vec::new();

    for segment in path_str.split(['/', '\\']) {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if !parts.is_empty() && *parts.last().unwrap() != ".." {
                parts.pop();
            } else if !is_abs {
                parts.push("..");
            }
        } else {
            parts.push(segment);
        }
    }

    if is_abs {
        format!("/{}", parts.join("/"))
    } else if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

pub fn native_path_join(args: Vec<Value>) -> Value {
    if args.is_empty() {
        let ptr = gc_alloc_string(".");
        return Value::string(ptr);
    }

    let mut path_segments = Vec::new();
    for arg in &args {
        if let Some(s) = arg.as_str() {
            if !s.is_empty() {
                path_segments.push(s);
            }
        }
    }

    if path_segments.is_empty() {
        let ptr = gc_alloc_string(".");
        return Value::string(ptr);
    }

    let joined = path_segments.join("/");
    let normalized = normalize_path_str(&joined);
    let ptr = gc_alloc_string(&normalized);
    Value::string(ptr)
}

pub fn native_path_resolve(args: Vec<Value>) -> Value {
    let mut resolved = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

    for arg in &args {
        if let Some(s) = arg.as_str() {
            if s.is_empty() {
                continue;
            }
            let p = Path::new(s);
            if p.is_absolute() {
                resolved = p.to_path_buf();
            } else {
                resolved.push(p);
            }
        }
    }

    let mut components = Vec::new();
    for comp in resolved.components() {
        match comp {
            Component::RootDir => components.push(""),
            Component::Normal(c) => components.push(c.to_str().unwrap_or("")),
            Component::ParentDir => {
                if components.len() > 1 {
                    components.pop();
                }
            }
            _ => {}
        }
    }

    let result = if components.is_empty() {
        "/".to_string()
    } else if components.len() == 1 && components[0].is_empty() {
        "/".to_string()
    } else {
        components.join("/")
    };

    let ptr = gc_alloc_string(&result);
    Value::string(ptr)
}

pub fn native_path_dirname(args: Vec<Value>) -> Value {
    if args.is_empty() {
        let ptr = gc_alloc_string(".");
        return Value::string(ptr);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };

    let p = Path::new(s);
    let parent = p.parent().and_then(|p| p.to_str()).unwrap_or("");
    let res = if parent.is_empty() {
        if s.starts_with('/') || s.starts_with('\\') {
            "/"
        } else {
            "."
        }
    } else {
        parent
    };

    let ptr = gc_alloc_string(res);
    Value::string(ptr)
}

pub fn native_path_basename(args: Vec<Value>) -> Value {
    if args.is_empty() {
        let ptr = gc_alloc_string("");
        return Value::string(ptr);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };

    let p = Path::new(s);
    let mut base = p
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("")
        .to_string();

    if base.is_empty() && (s == "/" || s == "\\") {
        base = "".to_string();
    }

    if args.len() > 1 {
        if let Some(ext) = args[1].as_str() {
            if !ext.is_empty() && base.ends_with(ext) {
                base.truncate(base.len() - ext.len());
            }
        }
    }

    let ptr = gc_alloc_string(&base);
    Value::string(ptr)
}

pub fn native_path_extname(args: Vec<Value>) -> Value {
    if args.is_empty() {
        let ptr = gc_alloc_string("");
        return Value::string(ptr);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };

    let p = Path::new(s);
    let file_name = p.file_name().and_then(|f| f.to_str()).unwrap_or("");

    if let Some(idx) = file_name.rfind('.') {
        if idx == 0 {
            // E.g. .gitignore has no extension
            let ptr = gc_alloc_string("");
            return Value::string(ptr);
        }
        let ext = &file_name[idx..];
        let ptr = gc_alloc_string(ext);
        Value::string(ptr)
    } else {
        let ptr = gc_alloc_string("");
        Value::string(ptr)
    }
}

pub fn native_path_normalize(args: Vec<Value>) -> Value {
    if args.is_empty() {
        let ptr = gc_alloc_string(".");
        return Value::string(ptr);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };

    let normalized = normalize_path_str(s);
    let ptr = gc_alloc_string(&normalized);
    Value::string(ptr)
}

pub fn native_path_is_absolute(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    Value::boolean(Path::new(s).is_absolute())
}

pub fn register_path_natives(vm: &mut VM) {
    vm.register_global("Eronom_nativePathJoin", Value::native_function(native_path_join));
    vm.register_global("Eronom_nativePathResolve", Value::native_function(native_path_resolve));
    vm.register_global("Eronom_nativePathDirname", Value::native_function(native_path_dirname));
    vm.register_global("Eronom_nativePathBasename", Value::native_function(native_path_basename));
    vm.register_global("Eronom_nativePathExtname", Value::native_function(native_path_extname));
    vm.register_global("Eronom_nativePathNormalize", Value::native_function(native_path_normalize));
    vm.register_global("Eronom_nativePathIsAbsolute", Value::native_function(native_path_is_absolute));
}
