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

    // Collect any imported files referenced by the script (local modules or std)
    if let Ok(imported_paths) = crate::frontend::parser::get_imported_file_paths(entry_script) {
        let canonical_entry = entry_script.canonicalize().unwrap_or_else(|_| entry_script.to_path_buf());
        let canonical_parent = canonical_entry.parent().unwrap_or_else(|| Path::new("."));
        for imp_path in imported_paths {
            if imp_path == canonical_entry {
                continue;
            }
            if let Ok(content) = fs::read(&imp_path) {
                let path_str = imp_path.to_string_lossy();
                let rel_key = if let Some(std_idx) = path_str.find("/std/") {
                    path_str[std_idx + 1..].to_string()
                } else if let Some(std_idx) = path_str.find("\\std\\") {
                    path_str[std_idx + 1..].replace('\\', "/")
                } else if let Ok(rel) = imp_path.strip_prefix(canonical_parent) {
                    rel.to_string_lossy().replace('\\', "/")
                } else {
                    imp_path.file_name().unwrap_or_default().to_string_lossy().to_string()
                };

                if !files.iter().any(|(p, _)| p == &rel_key) {
                    files.push((rel_key, content));
                }
            }
        }
    }

    // Ensure all standard library files are always present in the standalone bundle
    collect_std_files(entry_script.parent().unwrap_or_else(|| Path::new(".")), &mut files);

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

const EMBEDDED_STD_MODULES: &[(&str, &[u8])] = &[
    ("std/clock.er", include_bytes!("../../../std/clock.er")),
    ("std/crypto.er", include_bytes!("../../../std/crypto.er")),
    ("std/env.er", include_bytes!("../../../std/env.er")),
    ("std/erm.er", include_bytes!("../../../std/erm.er")),
    ("std/fs.er", include_bytes!("../../../std/fs.er")),
    ("std/http.er", include_bytes!("../../../std/http.er")),
    ("std/io.er", include_bytes!("../../../std/io.er")),
    ("std/json.er", include_bytes!("../../../std/json.er")),
    ("std/path.er", include_bytes!("../../../std/path.er")),
    ("std/process.er", include_bytes!("../../../std/process.er")),
    ("std/schedule.er", include_bytes!("../../../std/schedule.er")),
    ("std/task.er", include_bytes!("../../../std/task.er")),
    ("std/test.er", include_bytes!("../../../std/test.er")),
];

fn collect_std_files(base_dir: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    if let Some(std_dir) = find_std_dir_for_bundle(base_dir) {
        if let Ok(entries) = fs::read_dir(&std_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        if let Ok(bytes) = fs::read(&p) {
                            let rel = format!("std/{}", name);
                            if !files.iter().any(|(existing, _)| existing == &rel) {
                                files.push((rel, bytes));
                            }
                        }
                    }
                }
            }
        }
    }

    // Always ensure embedded std files are present as guaranteed fallbacks
    for (rel_path, content) in EMBEDDED_STD_MODULES {
        if !files.iter().any(|(existing, _)| existing == *rel_path) {
            files.push((rel_path.to_string(), content.to_vec()));
        }
    }
}

fn find_std_dir_for_bundle(start: &Path) -> Option<PathBuf> {
    let mut curr = Some(start);
    while let Some(dir) = curr {
        let std_dir = dir.join("std");
        if std_dir.is_dir() {
            return Some(std_dir);
        }
        curr = dir.parent();
    }
    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = Some(cwd.as_path());
        while let Some(dir) = curr {
            let std_dir = dir.join("std");
            if std_dir.is_dir() {
                return Some(std_dir);
            }
            curr = dir.parent();
        }
    }
    None
}
