use std::collections::HashMap;
use std::fs;
use std::sync::RwLock;

#[derive(Clone, Debug, Default)]
pub struct EmbeddedBundle {
    pub entrypoint: String,
    pub mode: String, // "erm_app" or "single_script"
    pub files: HashMap<String, Vec<u8>>,
}

static GLOBAL_VFS: RwLock<Option<EmbeddedBundle>> = RwLock::new(None);

impl EmbeddedBundle {
    pub fn new(entrypoint: &str, mode: &str) -> Self {
        Self {
            entrypoint: entrypoint.to_string(),
            mode: mode.to_string(),
            files: HashMap::new(),
        }
    }

    pub fn add_file(&mut self, path: &str, data: Vec<u8>) {
        let normalized = normalize_vfs_path(path);
        self.files.insert(normalized, data);
    }
}

pub fn normalize_vfs_path(p: &str) -> String {
    let mut s = p.replace('\\', "/");
    while s.starts_with("./") {
        s = s[2..].to_string();
    }
    if s.starts_with('/') {
        s = s[1..].to_string();
    }
    s
}

pub fn mount_embedded_bundle(bundle: EmbeddedBundle) {
    let mut vfs = GLOBAL_VFS.write().unwrap();
    *vfs = Some(bundle);
}

pub fn is_embedded() -> bool {
    GLOBAL_VFS.read().unwrap().is_some()
}

pub fn get_vfs_entrypoint() -> Option<String> {
    GLOBAL_VFS.read().unwrap().as_ref().map(|b| b.entrypoint.clone())
}

pub fn get_vfs_mode() -> Option<String> {
    GLOBAL_VFS.read().unwrap().as_ref().map(|b| b.mode.clone())
}

pub fn get_vfs_file(path: &str) -> Option<Vec<u8>> {
    let vfs_guard = GLOBAL_VFS.read().unwrap();
    let bundle = vfs_guard.as_ref()?;
    let norm = normalize_vfs_path(path);

    if let Some(data) = bundle.files.get(&norm) {
        return Some(data.clone());
    }

    // Try without leading folder prefix or matching base name
    for (k, v) in &bundle.files {
        if k.ends_with(&norm) || norm.ends_with(k) {
            return Some(v.clone());
        }
    }

    None
}

pub fn get_vfs_text(path: &str) -> Option<String> {
    let bytes = get_vfs_file(path)?;
    String::from_utf8(bytes).ok()
}

pub fn has_vfs_file(path: &str) -> bool {
    let vfs_guard = GLOBAL_VFS.read().unwrap();
    let bundle = match vfs_guard.as_ref() {
        Some(b) => b,
        None => return false,
    };
    let norm = normalize_vfs_path(path);

    if bundle.files.contains_key(&norm) {
        return true;
    }

    for k in bundle.files.keys() {
        if k.ends_with(&norm) || norm.ends_with(k) {
            return true;
        }
    }

    false
}

pub fn list_vfs_files() -> Vec<String> {
    let vfs_guard = GLOBAL_VFS.read().unwrap();
    match vfs_guard.as_ref() {
        Some(b) => b.files.keys().cloned().collect(),
        None => Vec::new(),
    }
}

/// Reads standard library `.er` files from local `std/` directory if present
pub fn collect_std_library_files() -> HashMap<String, Vec<u8>> {
    let mut files = HashMap::new();

    // 1. Look relative to current executable
    let mut search_dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            search_dirs.push(parent.join("std"));
            if let Some(grandparent) = parent.parent() {
                search_dirs.push(grandparent.join("std"));
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        search_dirs.push(cwd.join("std"));
    }

    let mut found = false;
    for dir in search_dirs {
        if dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() && p.extension().map_or(false, |e| e == "er") {
                        if let Ok(content) = fs::read(&p) {
                            let file_name = p.file_name().unwrap_or_default().to_string_lossy();
                            let vfs_key = format!("std/{}", file_name);
                            files.insert(vfs_key, content);
                            found = true;
                        }
                    }
                }
            }
            if found {
                break;
            }
        }
    }

    // Built-in standard library fallbacks if not found on disk
    if !files.contains_key("std/http.er") {
        files.insert("std/http.er".to_string(), include_bytes!("../../std/http.er").to_vec());
    }
    if !files.contains_key("std/fs.er") {
        files.insert("std/fs.er".to_string(), include_bytes!("../../std/fs.er").to_vec());
    }
    if !files.contains_key("std/crypto.er") {
        files.insert("std/crypto.er".to_string(), include_bytes!("../../std/crypto.er").to_vec());
    }
    if !files.contains_key("std/io.er") {
        files.insert("std/io.er".to_string(), include_bytes!("../../std/io.er").to_vec());
    }
    if !files.contains_key("std/json.er") {
        files.insert("std/json.er".to_string(), include_bytes!("../../std/json.er").to_vec());
    }
    if !files.contains_key("std/path.er") {
        files.insert("std/path.er".to_string(), include_bytes!("../../std/path.er").to_vec());
    }
    if !files.contains_key("std/process.er") {
        files.insert("std/process.er".to_string(), include_bytes!("../../std/process.er").to_vec());
    }
    if !files.contains_key("std/env.er") {
        files.insert("std/env.er".to_string(), include_bytes!("../../std/env.er").to_vec());
    }
    if !files.contains_key("std/test.er") {
        files.insert("std/test.er".to_string(), include_bytes!("../../std/test.er").to_vec());
    }
    if !files.contains_key("std/erm.er") {
        files.insert("std/erm.er".to_string(), include_bytes!("../../std/erm.er").to_vec());
    }
    if !files.contains_key("std/task.er") {
        files.insert("std/task.er".to_string(), include_bytes!("../../std/task.er").to_vec());
    }
    if !files.contains_key("std/schedule.er") {
        files.insert("std/schedule.er".to_string(), include_bytes!("../../std/schedule.er").to_vec());
    }
    if !files.contains_key("std/clock.er") {
        files.insert("std/clock.er".to_string(), include_bytes!("../../std/clock.er").to_vec());
    }

    // Built-in ERM client reactive runtime files
    if !files.contains_key("modules/erm/runtime.js") {
        files.insert("modules/erm/runtime.js".to_string(), include_bytes!("../../libs/init/modules/erm/runtime.js").to_vec());
    }
    if !files.contains_key("modules/erm/hmr.js") {
        files.insert("modules/erm/hmr.js".to_string(), include_bytes!("../../libs/init/modules/erm/hmr.js").to_vec());
    }

    files
}
