use std::path::{Path, PathBuf};
use std::collections::{HashSet, HashMap};
use super::Parser;
use crate::frontend::ast::Stmt;

pub fn parse_and_resolve_imports(path: &Path) -> Result<Vec<Stmt>, String> {
    let mut visited = HashSet::new();
    let mut visited_exports = HashMap::new();
    resolve_imports_recursive(path, &mut visited, &mut visited_exports)
}

fn get_exported_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    for stmt in stmts {
        if let Stmt::Export(inner) = stmt {
            match &**inner {
                Stmt::VarDecl(name, _, _, _, _) => {
                    names.insert(name.clone());
                }
                Stmt::Struct(name, _, _, _, _) => {
                    names.insert(name.clone());
                }
                Stmt::Interface(name, _, _, _) => {
                    names.insert(name.clone());
                }
                _ => {}
            }
        }
    }
    names
}

fn find_std_dir(start_dir: &Path) -> Option<PathBuf> {
    // 1. Search upwards from the compiling file's directory
    let mut current = Some(start_dir);
    while let Some(dir) = current {
        let std_dir = dir.join("std");
        if std_dir.is_dir() {
            return Some(std_dir);
        }
        current = dir.parent();
    }

    // 2. Search upwards from current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = Some(cwd.as_path());
        while let Some(dir) = current {
            let std_dir = dir.join("std");
            if std_dir.is_dir() {
                return Some(std_dir);
            }
            current = dir.parent();
        }
    }

    // 3. Search relative to executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let mut current = Some(exe_dir);
            while let Some(dir) = current {
                let std_dir = dir.join("std");
                if std_dir.is_dir() {
                    return Some(std_dir);
                }
                current = dir.parent();
            }
        }
    }

    None
}

fn resolve_imports_recursive(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    visited_exports: &mut HashMap<PathBuf, HashSet<String>>,
) -> Result<Vec<Stmt>, String> {
    let path_str = path.to_string_lossy().to_string();

    let (canonical, content) = if path.exists() {
        let canonical = path.canonicalize()
            .map_err(|e| format!("Failed to canonicalize path {:?}: {}", path, e))?;
        let content = std::fs::read_to_string(&canonical)
            .map_err(|e| format!("Failed to read file {:?}: {}", canonical, e))?;
        (canonical, content)
    } else if let Some(vfs_text) = crate::vm::embedded::get_vfs_text(&path_str) {
        (PathBuf::from(&path_str), vfs_text)
    } else {
        return Err(format!("File not found on disk or in embedded VFS: {:?}", path));
    };

    if visited.contains(&canonical) {
        return Ok(Vec::new());
    }
    visited.insert(canonical.clone());

    let tokens = crate::frontend::lexer::lex(&content);
    let mut parser = Parser::new(tokens).with_file_path(canonical.to_string_lossy().to_string());
    let stmts = parser.parse()?;

    // Populate visited_exports for this file with its direct exports
    let direct_exports = get_exported_names(&stmts);
    visited_exports.insert(canonical.clone(), direct_exports);

    let mut resolved_stmts = Vec::new();
    let parent_dir = canonical.parent().unwrap_or(Path::new(""));

    for stmt in stmts {
        match stmt {
            Stmt::Import(names, import_path) => {
                let is_std_import = import_path.starts_with("std/") || import_path == "std" || import_path.starts_with("std\\");
                let mut resolved_path = if is_std_import {
                    if let Some(std_root) = find_std_dir(parent_dir) {
                        let std_parent = std_root.parent().unwrap_or(&std_root);
                        std_parent.join(&import_path)
                    } else {
                        parent_dir.join(&import_path)
                    }
                } else {
                    parent_dir.join(&import_path)
                };
                
                // Fallbacks on disk
                if !resolved_path.exists() {
                    if import_path.ends_with(".js") {
                        let er_path = resolved_path.with_extension("er");
                        if er_path.exists() {
                            resolved_path = er_path;
                        }
                    }
                }
                
                if !resolved_path.exists() {
                    let er_path = resolved_path.with_extension("er");
                    if er_path.exists() {
                        resolved_path = er_path;
                    }
                }

                // Check VFS if not on disk
                let resolved_path_str = resolved_path.to_string_lossy().to_string();
                let is_in_vfs = !resolved_path.exists() && (
                    crate::vm::embedded::has_vfs_file(&resolved_path_str) ||
                    crate::vm::embedded::has_vfs_file(&import_path) ||
                    (import_path.ends_with(".js") && crate::vm::embedded::has_vfs_file(&import_path.replace(".js", ".er"))) ||
                    (!import_path.ends_with(".er") && crate::vm::embedded::has_vfs_file(&format!("{}.er", import_path))) ||
                    (is_std_import && !import_path.ends_with(".er") && crate::vm::embedded::has_vfs_file(&format!("{}.er", import_path)))
                );

                if !resolved_path.exists() && !is_in_vfs {
                    return Err(format!(
                        "Imported file not found: {:?} (specified as {})",
                        resolved_path, import_path
                    ));
                }

                let final_path = if is_in_vfs {
                    if crate::vm::embedded::has_vfs_file(&resolved_path_str) {
                        resolved_path
                    } else if crate::vm::embedded::has_vfs_file(&import_path) {
                        PathBuf::from(&import_path)
                    } else if import_path.ends_with(".js") && crate::vm::embedded::has_vfs_file(&import_path.replace(".js", ".er")) {
                        PathBuf::from(import_path.replace(".js", ".er"))
                    } else if !import_path.ends_with(".er") && crate::vm::embedded::has_vfs_file(&format!("{}.er", import_path)) {
                        PathBuf::from(format!("{}.er", import_path))
                    } else {
                        resolved_path
                    }
                } else {
                    resolved_path.canonicalize()
                        .map_err(|e| format!("Failed to canonicalize path {:?}: {}", resolved_path, e))?
                };

                let sub_stmts = if visited.contains(&final_path) {
                    Vec::new()
                } else {
                    resolve_imports_recursive(&final_path, visited, visited_exports)?
                };
                
                let exports = visited_exports.get(&final_path).cloned().unwrap_or_default();
                for name in &names {
                    if !exports.contains(name) {
                        return Err(format!(
                            "Name '{}' is not exported by {:?}",
                            name, final_path
                        ));
                    }
                }
                
                resolved_stmts.extend(sub_stmts);
            }
            Stmt::Export(inner) => {
                resolved_stmts.push(Stmt::Export(inner));
            }
            _ => {
                resolved_stmts.push(stmt);
            }
        }
    }

    Ok(resolved_stmts)
}
