use std::path::{Path, PathBuf};
use std::fs;
use anyhow::Context;

pub const MAGIC_BYTES: &[u8; 16] = b"ERONOM_STUB_v1\0\0";

#[derive(Debug, Clone)]
pub struct AppBundle {
    pub entrypoint: String,
    pub files: Vec<(String, Vec<u8>)>,
}

impl AppBundle {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = String::new();
        out.push_str("{\"entrypoint\":");
        eronom_core::json::escape_json_string(&mut out, &self.entrypoint);
        out.push_str(",\"files\":[");
        for (f_idx, (name, content)) in self.files.iter().enumerate() {
            if f_idx > 0 {
                out.push(',');
            }
            out.push('[');
            eronom_core::json::escape_json_string(&mut out, name);
            out.push_str(",[");
            for (b_idx, &b) in content.iter().enumerate() {
                if b_idx > 0 {
                    out.push(',');
                }
                out.push_str(&b.to_string());
            }
            out.push_str("]]");
        }
        out.push_str("]}");
        out.into_bytes()
    }

    pub fn decode(data: &[u8]) -> anyhow::Result<Self> {
        if data.starts_with(b"{") {
            let s = std::str::from_utf8(data)?;
            let mut dec = eronom_core::json::syntax::JsonDecoder::new(s);
            let mut entrypoint = String::new();
            let mut files = Vec::new();

            match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                Some(eronom_core::json::syntax::Token::BeginObject) => {}
                _ => anyhow::bail!("Expected BeginObject"),
            }

            while let Some(tok) = dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                match tok {
                    eronom_core::json::syntax::Token::EndObject => break,
                    eronom_core::json::syntax::Token::Comma => continue,
                    eronom_core::json::syntax::Token::String(key) => {
                        match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                            Some(eronom_core::json::syntax::Token::Colon) => {}
                            _ => anyhow::bail!("Expected colon after key"),
                        }
                        if key == "entrypoint" {
                            match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                                Some(eronom_core::json::syntax::Token::String(ep)) => {
                                    entrypoint = ep.into_owned();
                                }
                                _ => anyhow::bail!("Expected string entrypoint"),
                            }
                        } else if key == "files" {
                            match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                                Some(eronom_core::json::syntax::Token::BeginArray) => {}
                                _ => anyhow::bail!("Expected BeginArray for files"),
                            }
                            loop {
                                match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                                    Some(eronom_core::json::syntax::Token::EndArray) => break,
                                    Some(eronom_core::json::syntax::Token::Comma) => continue,
                                    Some(eronom_core::json::syntax::Token::BeginArray) => {
                                        let name = match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                                            Some(eronom_core::json::syntax::Token::String(n)) => n.into_owned(),
                                            _ => anyhow::bail!("Expected file name string"),
                                        };
                                        match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                                            Some(eronom_core::json::syntax::Token::Comma) => {}
                                            _ => anyhow::bail!("Expected comma in file entry"),
                                        }
                                        match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                                            Some(eronom_core::json::syntax::Token::BeginArray) => {}
                                            _ => anyhow::bail!("Expected BeginArray for file bytes"),
                                        }
                                        let mut bytes = Vec::new();
                                        loop {
                                            match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                                                Some(eronom_core::json::syntax::Token::EndArray) => break,
                                                Some(eronom_core::json::syntax::Token::Comma) => continue,
                                                Some(eronom_core::json::syntax::Token::Number(num_str)) => {
                                                    let b: u8 = num_str.parse().map_err(|_| anyhow::anyhow!("Invalid byte value"))?;
                                                    bytes.push(b);
                                                }
                                                _ => anyhow::bail!("Expected byte number in file bytes"),
                                            }
                                        }
                                        match dec.next_token().map_err(|e| anyhow::anyhow!("{}", e))? {
                                            Some(eronom_core::json::syntax::Token::EndArray) => {}
                                            _ => anyhow::bail!("Expected EndArray for file pair"),
                                        }
                                        files.push((name, bytes));
                                    }
                                    _ => anyhow::bail!("Unexpected token in files"),
                                }
                            }
                        }
                    }
                    _ => anyhow::bail!("Unexpected token in bundle object"),
                }
            }

            Ok(AppBundle { entrypoint, files })
        } else {
            if data.len() < 4 {
                anyhow::bail!("Corrupted bundle: payload too short");
            }
            let ep_len = u32::from_le_bytes(data[..4].try_into().map_err(|_| anyhow::anyhow!("Invalid entrypoint length"))?) as usize;
            let mut rem = &data[4..];
            if rem.len() < ep_len + 4 {
                anyhow::bail!("Corrupted bundle: entrypoint truncated");
            }
            let entrypoint = String::from_utf8(rem[..ep_len].to_vec())?;
            rem = &rem[ep_len..];
            let file_count = u32::from_le_bytes(rem[..4].try_into().map_err(|_| anyhow::anyhow!("Invalid file count"))?) as usize;
            rem = &rem[4..];
            let mut files = Vec::with_capacity(file_count);
            for _ in 0..file_count {
                if rem.len() < 4 {
                    anyhow::bail!("Corrupted bundle: file name length missing");
                }
                let name_len = u32::from_le_bytes(rem[..4].try_into().map_err(|_| anyhow::anyhow!("Invalid file name length"))?) as usize;
                rem = &rem[4..];
                if rem.len() < name_len + 4 {
                    anyhow::bail!("Corrupted bundle: file name truncated");
                }
                let name = String::from_utf8(rem[..name_len].to_vec())?;
                rem = &rem[name_len..];
                let content_len = u32::from_le_bytes(rem[..4].try_into().map_err(|_| anyhow::anyhow!("Invalid file content length"))?) as usize;
                rem = &rem[4..];
                if rem.len() < content_len {
                    anyhow::bail!("Corrupted bundle: file content truncated");
                }
                let content = rem[..content_len].to_vec();
                rem = &rem[content_len..];
                files.push((name, content));
            }
            Ok(AppBundle { entrypoint, files })
        }
    }
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

    let bundle_bytes = bundle.encode();
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

include!(concat!(env!("OUT_DIR"), "/embedded_std.rs"));

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
