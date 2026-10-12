use std::path::{Path, PathBuf};
use std::fs;
use anyhow::Context;

pub const MAGIC_BYTES: &[u8; 16] = b"ERONOM_STUB_v1\0\0";

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct AppBundle {
    pub entrypoint: String,
    pub files: Vec<(String, Vec<u8>)>,
}

/// Package an Eronom script or project directory into a standalone executable using a runner binary stub.
pub fn create_standalone_binary(
    runner_stub_path: &Path,
    entry_script: &Path,
    project_dir: Option<&Path>,
    output_path: &Path,
) -> anyhow::Result<()> {
    if !runner_stub_path.exists() {
        anyhow::bail!("Runner stub executable not found at: {}", runner_stub_path.display());
    }

    let runner_bytes = fs::read(runner_stub_path)
        .with_context(|| format!("Failed to read runner stub: {}", runner_stub_path.display()))?;

    // Collect all project files into bundle
    let mut files = Vec::new();
    let entry_relative = if let Some(base_dir) = project_dir {
        collect_project_files(base_dir, &mut files)?;
        if let Ok(rel) = entry_script.strip_prefix(base_dir) {
            rel.to_string_lossy().to_string()
        } else {
            entry_script.file_name().unwrap_or_default().to_string_lossy().to_string()
        }
    } else {
        let content = fs::read(entry_script)
            .with_context(|| format!("Failed to read entry script: {}", entry_script.display()))?;
        let file_name = entry_script.file_name().unwrap_or_default().to_string_lossy().to_string();
        files.push((file_name.clone(), content));
        file_name
    };

    // Ensure entrypoint content is in the bundle
    let entry_exists = files.iter().any(|(p, _)| p == &entry_relative);
    if !entry_exists && entry_script.exists() {
        if let Ok(c) = fs::read(entry_script) {
            files.push((entry_relative.clone(), c));
        }
    }

    let bundle = AppBundle {
        entrypoint: entry_relative,
        files,
    };

    let bundle_bytes = serde_json::to_vec(&bundle)?;
    let bundle_len = (bundle_bytes.len() as u64).to_le_bytes();

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let mut out_bytes = runner_bytes;
    out_bytes.extend_from_slice(&bundle_bytes);
    out_bytes.extend_from_slice(&bundle_len);
    out_bytes.extend_from_slice(MAGIC_BYTES);

    fs::write(output_path, &out_bytes)
        .with_context(|| format!("Failed to write output executable to {}", output_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(output_path, fs::Permissions::from_mode(0o755));
    }

    Ok(())
}

fn collect_project_files(dir: &Path, files: &mut Vec<(String, Vec<u8>)>) -> anyhow::Result<()> {
    for entry in walkdir(dir)? {
        let path = entry.as_path();
        if path.is_file() {
            let rel = path.strip_prefix(dir)?;
            let rel_str = rel.to_string_lossy().to_string();

            // Ignore git, node_modules, target, and output folders
            if rel_str.starts_with(".git")
                || rel_str.starts_with("node_modules")
                || rel_str.starts_with("target")
                || rel_str.starts_with("dist")
            {
                continue;
            }

            if let Ok(bytes) = fs::read(path) {
                files.push((rel_str, bytes));
            }
        }
    }
    Ok(())
}

fn walkdir(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    if !dir.is_dir() {
        return Ok(results);
    }
    let entries = fs::read_dir(dir)?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            results.push(p.clone());
            results.extend(walkdir(&p)?);
        } else {
            results.push(p);
        }
    }
    Ok(results)
}
