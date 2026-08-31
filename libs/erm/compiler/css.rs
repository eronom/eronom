use std::collections::HashSet;

pub fn scope_css(css: &str, scope_id: &str) -> anyhow::Result<String> {
    let mut result = String::new();
    let mut i = 0;
    while i < css.len() {
        let brace_idx = match css[i..].find('{') {
            Some(idx) => i + idx,
            None => break,
        };
        let selector = &css[i..brace_idx];
        let block_end = match css[brace_idx..].find('}') {
            Some(idx) => brace_idx + idx,
            None => break,
        };
        let block = &css[brace_idx..block_end + 1];

        let mut first = true;
        for s in selector.split(',') {
            if !first { result.push_str(", "); }
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                if trimmed.contains('%') || trimmed == "to" || trimmed == "from" || trimmed.starts_with("body") || trimmed.starts_with("html") {
                    result.push_str(trimmed);
                } else {
                    if let Some(idx) = trimmed.find(':') {
                        result.push_str(&trimmed[..idx]);
                        result.push('[');
                        result.push_str(scope_id);
                        result.push(']');
                        result.push_str(&trimmed[idx..]);
                    } else {
                        result.push_str(trimmed);
                        result.push('[');
                        result.push_str(scope_id);
                        result.push(']');
                    }
                }
            }
            first = false;
        }
        result.push(' ');
        result.push_str(block);
        i = block_end + 1;
    }
    if i < css.len() { result.push_str(&css[i..]); }
    Ok(result)
}

pub fn scope_html(html: &str, scope_id: &str) -> anyhow::Result<String> {
    let mut result = String::new();
    let mut i = 0;
    while i < html.len() {
        let tag_start = match html[i..].find('<') {
            Some(idx) => i + idx,
            None => {
                result.push_str(&html[i..]);
                break;
            }
        };
        result.push_str(&html[i..tag_start]);

        let tag_end = match html[tag_start..].find('>') {
            Some(idx) => tag_start + idx,
            None => {
                result.push_str(&html[tag_start..]);
                break;
            }
        };

        let tag_content = &html[tag_start + 1..tag_end];
        if !tag_content.is_empty() && !tag_content.starts_with('/') {
            let mut parts = tag_content.split_whitespace();
            let tag_name = parts.next().unwrap_or("");

            let is_component = !tag_name.is_empty() && tag_name.chars().next().unwrap().is_ascii_uppercase();
            let is_global = matches!(tag_name, "html" | "head" | "body" | "!DOCTYPE" | "script" | "style");

            if !is_component && !is_global {
                result.push('<');
                result.push_str(tag_name);
                result.push(' ');
                result.push_str(scope_id);
                result.push_str(&tag_content[tag_name.len()..]);
                result.push('>');
            } else {
                result.push('<');
                result.push_str(tag_content);
                result.push('>');
            }
        } else {
            result.push('<');
            result.push_str(tag_content);
            result.push('>');
        }
        i = tag_end + 1;
    }
    Ok(result)
}

pub fn find_ermcss_path(file_path: &str) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(file_path);
    let parent_dir = path.parent().unwrap_or(std::path::Path::new("."));
    
    // 1. Search upwards from the compiling file's directory
    let mut current = Some(parent_dir);
    while let Some(dir) = current {
        let ermcss_path = dir.join("ermcss").join("compiler.er");
        if ermcss_path.is_file() {
            return Some(ermcss_path);
        }
        let ermcss_path_sibling = dir.join("compiler.er");
        if dir.file_name().and_then(|n| n.to_str()) == Some("ermcss") && ermcss_path_sibling.is_file() {
            return Some(ermcss_path_sibling);
        }
        let libs_ermcss_path = dir.join("libs").join("ermcss").join("compiler.er");
        if libs_ermcss_path.is_file() {
            return Some(libs_ermcss_path);
        }
        current = dir.parent();
    }

    // 2. Search upwards from current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = Some(cwd.as_path());
        while let Some(dir) = current {
            let ermcss_path = dir.join("ermcss").join("compiler.er");
            if ermcss_path.is_file() {
                return Some(ermcss_path);
            }
            let libs_ermcss_path = dir.join("libs").join("ermcss").join("compiler.er");
            if libs_ermcss_path.is_file() {
                return Some(libs_ermcss_path);
            }
            current = dir.parent();
        }
    }

    // 3. Search relative to executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let mut current = Some(exe_dir);
            while let Some(dir) = current {
                let ermcss_path = dir.join("ermcss").join("compiler.er");
                if ermcss_path.is_file() {
                    return Some(ermcss_path);
                }
                let libs_ermcss_path = dir.join("libs").join("ermcss").join("compiler.er");
                if libs_ermcss_path.is_file() {
                    return Some(libs_ermcss_path);
                }
                current = dir.parent();
            }
        }
    }

    None
}

pub fn run_ermcss_compiler(compiler_path: &std::path::Path, base_path: &std::path::Path, classes: &[String]) -> anyhow::Result<String> {
    let stmts = match crate::frontend::parse_and_resolve_imports(compiler_path) {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Compile/Import error for ermcss: {}", e),
    };

    let compiler = crate::vm::compiler::Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => anyhow::bail!("Compile error for ermcss: {}", e),
    };

    let mut vm = crate::vm::execute::VM::new();
    vm.use_jit = false;
    vm.register_global("print", crate::vm::value::Value::native_function(|args| {
        let mut outputs = Vec::new();
        for arg in args {
            outputs.push(arg.to_string());
        }
        println!("{}", outputs.join(" "));
        crate::vm::value::Value::null()
    }));

    let lbrace_ptr = crate::vm::gc::get_or_create_string("{");
    vm.register_global("LBRACE", crate::vm::value::Value::string(lbrace_ptr));
    let rbrace_ptr = crate::vm::gc::get_or_create_string("}");
    vm.register_global("RBRACE", crate::vm::value::Value::string(rbrace_ptr));
    
    let base_path_str = base_path.to_string_lossy().replace('\\', "/");
    let base_path_ptr = crate::vm::gc::get_or_create_string(&base_path_str);
    vm.register_global("PROJECT_DIR", crate::vm::value::Value::string(base_path_ptr));

    crate::vm::er_http::register_eronom_file_api(&mut vm).map_err(|e| anyhow::anyhow!("{}", e))?;

    if let Err(e) = vm.run(function) {
        anyhow::bail!("VM Runtime error for ermcss: {}", e);
    }
    if let Err(e) = vm.run_event_loop() {
        anyhow::bail!("VM Event loop error for ermcss: {}", e);
    }

    let compile_val = match vm.globals.get("compile") {
        Some(val) => *val,
        None => anyhow::bail!("Global 'compile' function not found in compiler.er"),
    };

    let mut parts = Vec::new();
    for cls in classes {
        let ptr = crate::vm::gc::get_or_create_string(cls);
        parts.push(crate::vm::value::Value::string(ptr));
    }
    let array_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Array(parts));
    let classes_array_val = crate::vm::value::Value::array(array_ptr);

    let res = vm.call_function_reentrant(compile_val, vec![classes_array_val]);
    let res_val = match res {
        Ok(v) => v,
        Err(e) => anyhow::bail!("VM compilation function error: {}", e),
    };

    let css_string = match res_val.as_str() {
        Some(s) => s.to_string().replace("\\n", "\n"),
        None => anyhow::bail!("Expected compile() to return a string, got: {:?}", res_val),
    };

    Ok(css_string)
}

pub static GLOBAL_ERMCSS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn set_global_ermcss(css: String) {
    if let Ok(mut lock) = GLOBAL_ERMCSS.lock() {
        *lock = css;
    }
}

pub fn get_global_ermcss() -> anyhow::Result<String> {
    if let Ok(lock) = GLOBAL_ERMCSS.lock() {
        Ok(lock.clone())
    } else {
        anyhow::bail!("Failed to lock GLOBAL_ERMCSS")
    }
}

pub struct ErmcssConfig {
    pub enabled: bool,
    pub content: Vec<String>,
}

pub fn parse_ermcss_config(base_path: &std::path::Path) -> ErmcssConfig {
    let mut config = ErmcssConfig {
        enabled: false,
        content: Vec::new(),
    };
    let toml_path = base_path.join("eronom.toml");
    let toml_content_opt = if toml_path.exists() {
        std::fs::read_to_string(&toml_path).ok()
    } else {
        crate::vm::embedded::get_vfs_text("eronom.toml")
    };

    let mut explicitly_disabled = false;

    if let Some(content) = toml_content_opt {
        if let Ok(toml_val) = toml::from_str::<toml::Value>(&content) {
            if let Some(package) = toml_val.get("package") {
                if let Some(ermcss) = package.get("ermcss") {
                    if let Some(b) = ermcss.as_bool() {
                        if b {
                            config.enabled = true;
                        } else {
                            explicitly_disabled = true;
                        }
                    }
                }
            }
            if let Some(ermcss) = toml_val.get("ermcss") {
                if let Some(enabled) = ermcss.get("enabled").and_then(|v| v.as_bool()) {
                    if enabled {
                        config.enabled = true;
                    } else {
                        explicitly_disabled = true;
                    }
                } else {
                    config.enabled = true;
                }
                if let Some(content_arr) = ermcss.get("content") {
                    if let Some(arr) = content_arr.as_array() {
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                config.content.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Auto-enable ermcss if not explicitly disabled and compiler is found
    if !explicitly_disabled && !config.enabled {
        if find_ermcss_path(base_path.to_str().unwrap_or(".")).is_some() {
            config.enabled = true;
        }
    }

    if config.enabled && config.content.is_empty() {
        config.content = vec![
            "./app/**/*.erm".to_string(),
            "./pages/**/*.erm".to_string(),
            "./**/*.erm".to_string(),
        ];
    }
    config
}

pub fn matches_glob(base_path: &std::path::Path, path: &std::path::Path, glob: &str) -> bool {
    let rel_path = path.strip_prefix(base_path).unwrap_or(path);
    let path_str = rel_path.to_string_lossy().replace('\\', "/");
    
    let mut glob_clean = glob.replace('\\', "/");
    if glob_clean.starts_with("./") {
        glob_clean = glob_clean[2..].to_string();
    }
    
    let mut regex_pattern = String::new();
    let mut chars = glob_clean.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => {
                regex_pattern.push_str(r#"\."#);
            }
            '/' => {
                regex_pattern.push_str("/");
            }
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        regex_pattern.push_str("(?:.*/)?");
                    } else {
                        regex_pattern.push_str(".*");
                    }
                } else {
                    regex_pattern.push_str("[^/]*");
                }
            }
            '?' => {
                regex_pattern.push_str(".");
            }
            other => {
                regex_pattern.push_str(&regex::escape(&other.to_string()));
            }
        }
    }
    
    if let Ok(re) = regex::Regex::new(&format!("(?i)^{}$", regex_pattern)) {
        re.is_match(&path_str)
    } else {
        false
    }
}

pub fn compile_project_ermcss(base_path: &std::path::Path, content_globs: &[String]) -> anyhow::Result<String> {
    let mut classes_set = HashSet::new();
    
    let re_class1 = regex::Regex::new(r#"class\s*=\s*"([^"]*)""#).ok();
    let re_class2 = regex::Regex::new(r#"class\s*=\s*'([^']*)'"#).ok();
    
    fn scan_for_classes(
        dir: &std::path::Path,
        base_path: &std::path::Path,
        globs: &[String],
        classes: &mut HashSet<String>,
        re1: Option<&regex::Regex>,
        re2: Option<&regex::Regex>,
    ) {
        if let Ok(entries) = std::fs::read_dir(dir) {
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
                    scan_for_classes(&path, base_path, globs, classes, re1, re2);
                } else if path.is_file() {
                    let mut matched = false;
                    for glob in globs {
                        if matches_glob(base_path, &path, glob) {
                            matched = true;
                            break;
                        }
                    }
                    
                    if matched {
                        if let Ok(html_content) = std::fs::read_to_string(&path) {
                            if let Some(re) = re1 {
                                for cap in re.captures_iter(&html_content) {
                                    for cls in cap[1].split_whitespace() {
                                        classes.insert(cls.to_string());
                                    }
                                }
                            }
                            if let Some(re) = re2 {
                                for cap in re.captures_iter(&html_content) {
                                    for cls in cap[1].split_whitespace() {
                                        classes.insert(cls.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    scan_for_classes(base_path, base_path, content_globs, &mut classes_set, re_class1.as_ref(), re_class2.as_ref());
    
    if classes_set.is_empty() {
        return Ok(String::new());
    }
    
    if let Some(compiler_path) = find_ermcss_path(base_path.to_str().unwrap_or(".")) {
        let mut classes_vec: Vec<String> = classes_set.into_iter().collect();
        classes_vec.sort();
        run_ermcss_compiler(&compiler_path, base_path, &classes_vec)
    } else {
        anyhow::bail!("compiler.er not found")
    }
}
