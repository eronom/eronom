use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn scan_directory(dir: &Path, files: &mut HashMap<PathBuf, SystemTime>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with('.') ||
               name_str == "target" ||
               name_str == "build" ||
               name_str == "node_modules" {
                continue;
            }

            if path.is_dir() {
                scan_directory(&path, files);
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy();
                    if ext_str == "erm" || ext_str == "css" || ext_str == "js" {
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                files.insert(path, modified);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn resolve_path(base_path: &Path, target: &str) -> Option<(PathBuf, HashMap<String, String>)> {
    let parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let mut current_path = base_path.to_path_buf();
    let mut params = HashMap::new();

    for (i, part) in parts.iter().enumerate() {
        let mut found = false;
        
        let exact = current_path.join(part);
        if exact.exists() {
            current_path = exact;
            found = true;
        } else {
            let er = current_path.join(format!("{}.er", part));
            if er.exists() {
                current_path = er;
                found = true;
            } else {
                let erm = current_path.join(format!("{}.erm", part));
                if erm.exists() {
                    current_path = erm;
                    found = true;
                } else {
                    let html = current_path.join(format!("{}.html", part));
                    if html.exists() {
                        current_path = html;
                        found = true;
                    } else {
                        if let Ok(entries) = fs::read_dir(&current_path) {
                            for entry in entries.flatten() {
                                let name = entry.file_name().to_string_lossy().into_owned();
                                if name.starts_with('[') {
                                    if name.ends_with(']') {
                                        let param_name = &name[1..name.len() - 1];
                                        params.insert(param_name.to_string(), part.to_string());
                                        current_path.push(name);
                                        found = true;
                                        break;
                                    } else if name.ends_with("].er") {
                                        let param_name = &name[1..name.len() - 4];
                                        params.insert(param_name.to_string(), part.to_string());
                                        current_path.push(name);
                                        found = true;
                                        break;
                                    } else if name.ends_with("].erm") {
                                        let param_name = &name[1..name.len() - 5];
                                        params.insert(param_name.to_string(), part.to_string());
                                        current_path.push(name);
                                        found = true;
                                        break;
                                    } else if name.ends_with("].html") {
                                        let param_name = &name[1..name.len() - 6];
                                        params.insert(param_name.to_string(), part.to_string());
                                        current_path.push(name);
                                        found = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        if !found {
            let routes_er = current_path.join("routes.er");
            if routes_er.exists() {
                return Some((routes_er, params));
            }
            return None;
        }
        
        if current_path.is_file() && i < parts.len() - 1 {
            return None;
        }
    }

    if current_path.is_dir() {
        let routes_er = current_path.join("routes.er");
        if routes_er.exists() { return Some((routes_er, params)); }
        let index_erm = current_path.join("index.erm");
        if index_erm.exists() { return Some((index_erm, params)); }
        let page_erm = current_path.join("page.erm");
        if page_erm.exists() { return Some((page_erm, params)); }
        let index_html = current_path.join("index.html");
        if index_html.exists() { return Some((index_html, params)); }
        let page_html = current_path.join("page.html");
        if page_html.exists() { return Some((page_html, params)); }
    }

    Some((current_path, params))
}
