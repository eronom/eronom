use std::path::{Path, PathBuf};
use super::file_runner::run_file;

pub fn find_test_files_in_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut entries_vec: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries_vec.sort_by_key(|e| e.path());
        for entry in entries_vec {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.') && name != "target" {
                    find_test_files_in_dir(&path, out);
                }
            } else if path.is_file() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name.ends_with(".er") {
                    if name.starts_with("test_") || name.ends_with(".test.er") || name.ends_with("_test.er") || path.parent().map_or(false, |p| p.ends_with("tests")) {
                        out.push(path);
                    }
                }
            }
        }
    }
}

pub fn run_test_command(file_opt: Option<PathBuf>) -> anyhow::Result<()> {
    let test_files = if let Some(path) = file_opt {
        if path.is_dir() {
            let mut files = Vec::new();
            find_test_files_in_dir(&path, &mut files);
            if files.is_empty() {
                anyhow::bail!("No test files found in directory: {}", path.display());
            }
            files
        } else if path.exists() {
            vec![path]
        } else {
            let with_er = path.with_extension("er");
            if with_er.exists() {
                vec![with_er]
            } else {
                anyhow::bail!("Test file not found: {}", path.display());
            }
        }
    } else {
        let mut files = Vec::new();
        let tests_dir = Path::new("tests");
        if tests_dir.exists() && tests_dir.is_dir() {
            find_test_files_in_dir(tests_dir, &mut files);
        }
        find_test_files_in_dir(Path::new("."), &mut files);
        files.sort();
        files.dedup();
        if files.is_empty() {
            anyhow::bail!("No test files found. (Looked for tests/*.er, *.test.er, test_*.er)");
        }
        files
    };

    let total_files = test_files.len();
    if total_files > 1 {
        println!("=== Running {} test files ===", total_files);
    }

    for test_file in &test_files {
        let path_str = test_file.to_str().unwrap_or("");
        if total_files > 1 {
            println!("\n> {}", test_file.display());
        }
        if let Err(e) = run_file(path_str) {
            eprintln!("Test failed in {}: {}", test_file.display(), e);
            std::process::exit(1);
        }
    }

    Ok(())
}
