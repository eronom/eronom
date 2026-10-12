use std::fs;

pub const MAGIC_BYTES: &[u8; 16] = b"ERONOM_STUB_v1\0\0";

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct AppBundle {
    pub entrypoint: String,
    pub files: Vec<(String, Vec<u8>)>,
}

/// Checks if the running executable has an embedded application bundle attached to its end.
/// If present, unpacks the bundle to a temporary directory and runs the entrypoint.
pub fn check_and_run_embedded_bundle() -> Option<anyhow::Result<()>> {
    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return None,
    };

    let mut file = match fs::File::open(&current_exe) {
        Ok(f) => f,
        Err(_) => return None,
    };

    use std::io::{Read, Seek, SeekFrom};
    let file_len = match file.metadata().map(|m| m.len()) {
        Ok(l) => l,
        Err(_) => return None,
    };

    if file_len < 24 {
        return None;
    }

    // 1. Check magic bytes
    if file.seek(SeekFrom::Start(file_len - 16)).is_err() {
        return None;
    }
    let mut magic_buf = [0u8; 16];
    if file.read_exact(&mut magic_buf).is_err() {
        return None;
    }
    if &magic_buf != MAGIC_BYTES {
        return None;
    }

    // 2. Read bundle length
    if file.seek(SeekFrom::Start(file_len - 24)).is_err() {
        return None;
    }
    let mut len_buf = [0u8; 8];
    if file.read_exact(&mut len_buf).is_err() {
        return None;
    }
    let payload_len = u64::from_le_bytes(len_buf);
    if payload_len + 24 > file_len {
        return Some(Err(anyhow::anyhow!("Corrupted embedded bundle payload length")));
    }

    // 3. Read bundle payload
    if file.seek(SeekFrom::Start(file_len - 24 - payload_len)).is_err() {
        return Some(Err(anyhow::anyhow!("Failed to seek to embedded bundle payload")));
    }
    let mut payload = vec![0u8; payload_len as usize];
    if let Err(e) = file.read_exact(&mut payload) {
        return Some(Err(anyhow::anyhow!("Failed to read embedded bundle: {}", e)));
    }

    let bundle: AppBundle = match serde_json::from_slice(&payload) {
        Ok(b) => b,
        Err(e) => return Some(Err(anyhow::anyhow!("Failed to deserialize embedded bundle: {}", e))),
    };

    // 4. Unpack bundle to isolated temp folder
    let temp_app_dir = std::env::temp_dir().join(format!(
        ".er_app_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    if let Err(e) = fs::create_dir_all(&temp_app_dir) {
        return Some(Err(anyhow::anyhow!("Failed to create bundle runtime dir: {}", e)));
    }

    for (rel_path, content) in &bundle.files {
        let dest = temp_app_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&dest, content);
    }

    // 5. Execute the unpacked application
    let entry_path = temp_app_dir.join(&bundle.entrypoint);
    let _ = std::env::set_current_dir(&temp_app_dir);

    let res = super::file_runner::run_file(entry_path.to_str().unwrap_or(&bundle.entrypoint));
    Some(res)
}
