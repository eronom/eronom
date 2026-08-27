use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::RwLock;

pub const EMBED_MAGIC_HEADER: &[u8; 14] = b"ERON_EMBED_V1\0";
pub const EMBED_TRAILER_MAGIC: &[u8; 8] = b"ERONPBLD";
pub const TRAILER_SIZE: usize = 16; // 8 bytes u64 payload length + 8 bytes magic

#[derive(Clone, Debug)]
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

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        // Magic header
        buf.extend_from_slice(EMBED_MAGIC_HEADER);

        // Entrypoint
        let entry_bytes = self.entrypoint.as_bytes();
        buf.extend_from_slice(&(entry_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(entry_bytes);

        // Mode
        let mode_bytes = self.mode.as_bytes();
        buf.extend_from_slice(&(mode_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(mode_bytes);

        // File count
        buf.extend_from_slice(&(self.files.len() as u32).to_le_bytes());

        for (path, data) in &self.files {
            let path_bytes = path.as_bytes();
            buf.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(path_bytes);

            buf.extend_from_slice(&(data.len() as u64).to_le_bytes());
            buf.extend_from_slice(data);
        }

        buf
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < EMBED_MAGIC_HEADER.len() {
            return None;
        }

        if &bytes[0..EMBED_MAGIC_HEADER.len()] != EMBED_MAGIC_HEADER {
            return None;
        }

        let mut offset = EMBED_MAGIC_HEADER.len();

        // Read Entrypoint
        if offset + 4 > bytes.len() { return None; }
        let entry_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;
        if offset + entry_len > bytes.len() { return None; }
        let entrypoint = String::from_utf8(bytes[offset..offset + entry_len].to_vec()).ok()?;
        offset += entry_len;

        // Read Mode
        if offset + 4 > bytes.len() { return None; }
        let mode_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;
        if offset + mode_len > bytes.len() { return None; }
        let mode = String::from_utf8(bytes[offset..offset + mode_len].to_vec()).ok()?;
        offset += mode_len;

        // Read File count
        if offset + 4 > bytes.len() { return None; }
        let file_count = u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;

        let mut files = HashMap::with_capacity(file_count);
        for _ in 0..file_count {
            if offset + 4 > bytes.len() { return None; }
            let path_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
            offset += 4;
            if offset + path_len > bytes.len() { return None; }
            let path = String::from_utf8(bytes[offset..offset + path_len].to_vec()).ok()?;
            offset += path_len;

            if offset + 8 > bytes.len() { return None; }
            let data_len = u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?) as usize;
            offset += 8;
            if offset + data_len > bytes.len() { return None; }
            let data = bytes[offset..offset + data_len].to_vec();
            offset += data_len;

            files.insert(normalize_vfs_path(&path), data);
        }

        Some(Self {
            entrypoint,
            mode,
            files,
        })
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

/// Inspects a binary file and extracts any embedded bundle attached at the end
pub fn check_embedded_in_file(path: &Path) -> io::Result<Option<EmbeddedBundle>> {
    let mut file = File::open(path)?;
    let total_len = file.metadata()?.len();
    if total_len < TRAILER_SIZE as u64 {
        return Ok(None);
    }

    file.seek(SeekFrom::End(-(TRAILER_SIZE as i64)))?;
    let mut trailer = [0u8; TRAILER_SIZE];
    file.read_exact(&mut trailer)?;

    let magic = &trailer[8..16];
    if magic != EMBED_TRAILER_MAGIC {
        return Ok(None);
    }

    let payload_len = u64::from_le_bytes(trailer[0..8].try_into().unwrap());
    if payload_len == 0 || payload_len + (TRAILER_SIZE as u64) > total_len {
        return Ok(None);
    }

    let payload_start = total_len - (TRAILER_SIZE as u64) - payload_len;
    file.seek(SeekFrom::Start(payload_start))?;

    let mut payload_bytes = vec![0u8; payload_len as usize];
    file.read_exact(&mut payload_bytes)?;

    Ok(EmbeddedBundle::deserialize(&payload_bytes))
}

/// Checks the currently executing binary for an embedded payload and mounts it into global VFS
pub fn check_and_mount_embedded() -> io::Result<bool> {
    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };

    if let Ok(Some(bundle)) = check_embedded_in_file(&current_exe) {
        mount_embedded_bundle(bundle);
        Ok(true)
    } else {
        Ok(false)
    }
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

    // Built-in ERM client reactive runtime files
    if !files.contains_key("modules/erm/runtime.js") {
        files.insert("modules/erm/runtime.js".to_string(), include_bytes!("../../libs/init/modules/erm/runtime.js").to_vec());
    }
    if !files.contains_key("modules/erm/hmr.js") {
        files.insert("modules/erm/hmr.js".to_string(), include_bytes!("../../libs/init/modules/erm/hmr.js").to_vec());
    }

    files
}

/// Strips any previously embedded payload from a runner binary if present
pub fn strip_existing_payload(bytes: &[u8]) -> &[u8] {
    if bytes.len() < TRAILER_SIZE {
        return bytes;
    }
    let trailer = &bytes[bytes.len() - TRAILER_SIZE..];
    if &trailer[8..16] == EMBED_TRAILER_MAGIC {
        let payload_len = u64::from_le_bytes(trailer[0..8].try_into().unwrap()) as usize;
        if payload_len + TRAILER_SIZE <= bytes.len() {
            return &bytes[..bytes.len() - TRAILER_SIZE - payload_len];
        }
    }
    bytes
}

/// Creates a self-contained standalone executable binary
pub fn build_standalone_executable(
    runner_stub_bytes: &[u8],
    bundle: &EmbeddedBundle,
    output_path: &Path,
) -> anyhow::Result<()> {
    let clean_runner = strip_existing_payload(runner_stub_bytes);
    let serialized_payload = bundle.serialize();
    let payload_len = serialized_payload.len() as u64;

    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let _ = fs::remove_file(output_path);

    let mut out_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(output_path)?;

    // 1. Write the runner executable binary
    out_file.write_all(clean_runner)?;

    // 2. Write the serialized embedded bundle payload
    out_file.write_all(&serialized_payload)?;

    // 3. Write trailer: [payload_len: 8 bytes LE] + [magic: 8 bytes "ERONPBLD"]
    out_file.write_all(&payload_len.to_le_bytes())?;
    out_file.write_all(EMBED_TRAILER_MAGIC)?;
    out_file.flush()?;

    // 4. Set executable permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(output_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(output_path, perms)?;
    }

    Ok(())
}
