use std::collections::HashMap;
use fnv::FnvHasher;
use std::hash::Hasher;
use crate::eval::{self, ErmEval};
use super::utils::*;
use super::css::{scope_css, scope_html};
use super::transform::{is_function_template, preprocess_function_template, transform_use_effect};
use super::reactivity::parse_reactivity;
use super::blocks::{find_last_for_block, find_last_if_block, find_tag_end, process_for_block_at, process_if_block_at};
use super::components::{resolve_custom_component, resolve_suspense_loading};

pub struct ProcessResult {
    pub html: String,
    pub scripts: Vec<String>,
    pub styles: Vec<String>,
    pub state_vars: Vec<String>,
}

pub fn process_component_tree(
    file_path: &str,
    content: &str,
    visited: &mut HashMap<String, String>,
    slot_html: Option<&str>,
    params: &HashMap<String, String>,
    if_counter: &mut usize,
    for_counter: &mut usize,
    state_var_sources: &mut HashMap<String, String>,
) -> anyhow::Result<ProcessResult> {
    let preprocessed;
    let content = if is_function_template(content) {
        preprocessed = preprocess_function_template(content)?;
        &preprocessed
    } else {
        content
    };
    
    let path = std::path::Path::new(file_path);
    let base_dir = if path.is_file() {
        path.parent().unwrap().to_string_lossy().into_owned()
    } else {
        file_path.to_string()
    };

    let relative_path = if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = std::path::Path::new(file_path).strip_prefix(&cwd) {
            rel.to_string_lossy().into_owned()
        } else {
            file_path.to_string()
        }
    } else {
        file_path.to_string()
    };
    let file_id: String = relative_path
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();

    // 1. Initialize local ErmEval and parse parameters/scripts for SSR block evaluation
    let mut ev = ErmEval::new();
    let mut params_map = HashMap::new();
    for (k, v) in params {
        if let Ok((Some(parsed_val), _)) = eval::parse_js_value(v, None) {
            params_map.insert(k.clone(), parsed_val);
        } else {
            params_map.insert(k.clone(), eval::Value::String(v.clone()));
        }
    }
    ev.set("__erm_params", eval::Value::Map(params_map));

    let mut temp_search = 0;
    while let Some(start_idx) = content[temp_search..].find("<script") {
        let script_start = temp_search + start_idx;
        if let Some(end_idx) = content[script_start..].find("</script>") {
            let script_end = script_start + end_idx;
            let script_tag = &content[script_start..script_end + 9];
            let content_start = script_tag.find('>').unwrap_or(0) + 1;
            let script_content = &script_tag[content_start..script_tag.len() - 9];
            let script_for_eval = script_content.replace("useParams()", "__erm_params");
            let _ = ev.parse_script_vars(&script_for_eval);
            temp_search = script_end + 9;
        } else {
            break;
        }
    }

    // 2. Scan and extract all state variable names (so replace_word knows what to convert to .value)
    let mut local_state_vars = Vec::new();
    let mut temp_search2 = 0;
    while let Some(start_idx) = content[temp_search2..].find("<script") {
        let script_start = temp_search2 + start_idx;
        if let Some(end_idx) = content[script_start..].find("</script>") {
            let script_end = script_start + end_idx;
            let script_tag = &content[script_start..script_end + 9];
            let content_start = script_tag.find('>').unwrap_or(0) + 1;
            let script_content = &script_tag[content_start..script_tag.len() - 9];
            for cap in get_re_state().captures_iter(script_content) {
                let var_name = cap.get(1).unwrap().as_str().to_string();
                if !local_state_vars.contains(&var_name) {
                    local_state_vars.push(var_name.clone());
                }
                state_var_sources.insert(var_name, file_path.to_string());
            }
            for line in script_content.lines() {
                let line_trimmed = line.trim();
                if let Some(caps) = get_re_import_named().captures(line_trimmed) {
                    let names_str = caps.get(1).unwrap().as_str();
                    let comp_path_val = caps.get(2).unwrap().as_str().to_string();
                    for name in names_str.split(',') {
                        let name = name.trim().to_string();
                        if !name.is_empty() && name.chars().next().map_or(false, |c| c.is_ascii_lowercase()) {
                            if !local_state_vars.contains(&name) {
                                  local_state_vars.push(name.clone());
                            }
                            if let Some(resolved_path) = resolve_import_path(&base_dir, &comp_path_val) {
                                state_var_sources.insert(name, resolved_path);
                            }
                        }
                    }
                }
            }
            temp_search2 = script_end + 9;
        } else {
            break;
        }
    }
    let mut hasher = FnvHasher::default();
    hasher.write(content.as_bytes());
    let scope_id = format!("data-e-{:x}", hasher.finish());

    // 3. Compile all blocks (new if syntax and new for syntax) from inside out
    let mut compiled_content = content.to_string();
    let mut block_logic = Vec::new();
    loop {
        let last_if = find_last_if_block(&compiled_content);
        let last_for = find_last_for_block(&compiled_content);
        match (last_if, last_for) {
            (Some((if_idx, cond, body_start)), Some((for_idx, header, for_body_start))) => {
                if if_idx > for_idx {
                    process_if_block_at(if_idx, &mut compiled_content, if_counter, &mut block_logic, &local_state_vars, &mut ev, cond, body_start, &scope_id)?;
                } else {
                    process_for_block_at(for_idx, &mut compiled_content, for_counter, &mut block_logic, &local_state_vars, &mut ev, header, for_body_start, &scope_id)?;
                }
            }
            (Some((if_idx, cond, body_start)), None) => {
                process_if_block_at(if_idx, &mut compiled_content, if_counter, &mut block_logic, &local_state_vars, &mut ev, cond, body_start, &scope_id)?;
            }
            (None, Some((for_idx, header, for_body_start))) => {
                process_for_block_at(for_idx, &mut compiled_content, for_counter, &mut block_logic, &local_state_vars, &mut ev, header, for_body_start, &scope_id)?;
            }
            (None, None) => break,
        }
    }

    let content = &compiled_content;

    let mut scripts = Vec::new();
    let mut styles = Vec::new();
    let mut state_vars = Vec::new();

    let mut component_imports = HashMap::new();
    let mut state_variable_imports = HashMap::new();

    let mut search_idx = 0;
    while let Some(start_idx) = content[search_idx..].find("<script") {
        let script_start = search_idx + start_idx;
        if let Some(end_idx) = content[script_start..].find("</script>") {
            let script_end = script_start + end_idx;
            let script_tag = &content[script_start..script_end + 9];
            let content_start = script_tag.find('>').unwrap_or(0) + 1;
            let script_content = &script_tag[content_start..script_tag.len() - 9];

            for line in script_content.lines() {
                let line_trimmed = line.trim();
                if let Some(caps) = get_re_import_default().captures(line_trimmed) {
                    let comp_name = caps.get(1).unwrap().as_str().to_string();
                    let comp_path_val = caps.get(2).unwrap().as_str().to_string();
                    if comp_name.chars().next().map_or(false, |c| c.is_ascii_uppercase()) {
                        component_imports.insert(comp_name, comp_path_val);
                    }
                } else if let Some(caps) = get_re_import_named().captures(line_trimmed) {
                    let names_str = caps.get(1).unwrap().as_str();
                    let comp_path_val = caps.get(2).unwrap().as_str().to_string();
                    for name in names_str.split(',') {
                        let name = name.trim().to_string();
                        if !name.is_empty() {
                            if name.chars().next().map_or(false, |c| c.is_ascii_uppercase()) {
                                component_imports.insert(name, comp_path_val.clone());
                            } else {
                                state_variable_imports.insert(name.clone(), comp_path_val.clone());
                                if let Some(resolved_path) = resolve_import_path(&base_dir, &comp_path_val) {
                                    state_var_sources.insert(name, resolved_path);
                                }
                            }
                        }
                    }
                }
            }
            search_idx = script_end + 9;
        } else {
            break;
        }
    }

    let mut html_buf = String::new();
    let mut i = 0;
    while i < content.len() {
        if content[i..].starts_with("<script") {
            let end = match content[i..].find("</script>") {
                Some(idx) => idx,
                None => {
                    html_buf.push_str(&content[i..]);
                    break;
                }
            };
            let script_tag = &content[i..i + end + 9];
            let content_start = script_tag.find('>').unwrap_or(0) + 1;
            let script_content = script_tag[content_start..script_tag.len() - 9].trim();

            // Find state vars (useState) in script
            find_state_variables(script_content, "useState(", &mut state_vars);

            // Also check imported variables that are lowercased (state imports)
            for line in script_content.lines() {
                let line_trimmed = line.trim();
                if let Some(caps) = get_re_import_named().captures(line_trimmed) {
                    let names_str = caps.get(1).unwrap().as_str();
                    let comp_path_val = caps.get(2).unwrap().as_str().to_string();
                    for name in names_str.split(',') {
                        let name = name.trim().to_string();
                        if !name.is_empty() && name.chars().next().map_or(false, |c| c.is_ascii_lowercase()) {
                            if !state_vars.contains(&name) {
                                state_vars.push(name.clone());
                            }
                            if let Some(resolved_path) = resolve_import_path(&base_dir, &comp_path_val) {
                                state_var_sources.insert(name, resolved_path);
                            }
                        }
                    }
                }
            }

            let mut cleaned_script = String::new();
            for line in script_content.lines() {
                let line_trimmed = line.trim();
                if get_re_import_default().is_match(line_trimmed) || get_re_import_named().is_match(line_trimmed) {
                    continue;
                }
                let processed_line = get_re_export().replace(line, "$1");
                cleaned_script.push_str(&processed_line);
                cleaned_script.push('\n');
            }
            scripts.push(cleaned_script.trim().to_string());
            i += end + 9;
        } else if content[i..].starts_with("<style") {
            let end = match content[i..].find("</style>") {
                Some(idx) => idx,
                None => {
                    html_buf.push_str(&content[i..]);
                    break;
                }
            };
            let style_tag = &content[i..i + end + 8];
            let content_start = style_tag.find('>').unwrap_or(0) + 1;
            let style_content = style_tag[content_start..style_tag.len() - 8].trim();
            styles.push(scope_css(style_content, &scope_id)?);
            i += end + 8;
        } else if content[i..].starts_with('<') {
            let tag_end = match find_tag_end(content, i) {
                Some(end_idx) => end_idx - i,
                None => {
                    html_buf.push_str(&content[i..]);
                    break;
                }
            };
            let tag_content = content[i + 1..i + tag_end].trim();
            if !tag_content.is_empty() {
                if tag_content.starts_with('/') {
                    let closing_tag_name = tag_content[1..].trim();
                    if closing_tag_name.is_empty() {
                        // Fragment: </>
                        i += tag_end + 1;
                        continue;
                    }
                    if closing_tag_name == "Link" {
                        html_buf.push_str("</a>");
                        i += tag_end + 1;
                        continue;
                    } else if closing_tag_name == "ContextProvider" || closing_tag_name.ends_with(".Provider") {
                        html_buf.push_str("</span>");
                        i += tag_end + 1;
                        continue;
                    }
                } else {
                    let mut parts = tag_content.split_whitespace();
                    let mut tag_name_str = parts.next().unwrap_or("").to_string();
                    if tag_name_str.ends_with('/') {
                        tag_name_str.pop();
                    }
                    let tag_name = &tag_name_str;
                    if tag_name == "Link" {
                        let mut new_tag_content = tag_content.to_string();
                        new_tag_content = new_tag_content.replacen("Link", "a", 1);
                        new_tag_content = new_tag_content.replace("to=", "href=");
                        new_tag_content = new_tag_content.replace("to =", "href=");
                        new_tag_content = new_tag_content.replace("to  =", "href=");
                        html_buf.push('<');
                        html_buf.push_str(&new_tag_content);
                        html_buf.push('>');
                        i += tag_end + 1;
                        continue;
                    }
                    let is_context_provider = tag_name == "ContextProvider" || tag_name.ends_with(".Provider");
                    if is_context_provider {
                        let context_var = if tag_name.ends_with(".Provider") {
                            tag_name[..tag_name.len() - 9].to_string()
                        } else {
                            extract_attribute(tag_content, "context").unwrap_or_default()
                        };
                        let value_expr = extract_attribute(tag_content, "value").unwrap_or_else(|| "null".to_string());
                        
                        let provider_id = format!("erm-prov-{}", i);
                        html_buf.push_str(&format!("<span id=\"{}\" style=\"display:contents;\">", provider_id));
                        
                        let logic = format!(
                            "bindProvider(\"{}\", {}, () => ({}));",
                            provider_id, context_var, value_expr
                        );
                        scripts.push(logic);
                        i += tag_end + 1;
                        continue;
                    }
                    if tag_name == "Loading" || tag_name == "Suspense" {
                        if let Some(next_i) = resolve_suspense_loading(
                            tag_name,
                            tag_content,
                            content,
                            i,
                            tag_end,
                            file_path,
                            visited,
                            params,
                            if_counter,
                            for_counter,
                            state_var_sources,
                            &mut scripts,
                            &mut styles,
                            &mut state_vars,
                            &mut html_buf,
                        )? {
                            i = next_i;
                            continue;
                        }
                    }

                    if !tag_name.is_empty() && tag_name.chars().next().unwrap().is_ascii_uppercase() {
                        resolve_custom_component(
                            tag_name,
                            tag_content,
                            &base_dir,
                            &component_imports,
                            i,
                            visited,
                            params,
                            if_counter,
                            for_counter,
                            state_var_sources,
                            &mut scripts,
                            &mut styles,
                            &mut state_vars,
                            &mut html_buf,
                        )?;
                        i += tag_end + 1;
                        continue;
                    }
                    if tag_name == "slot" {
                        if let Some(s) = slot_html {
                            html_buf.push_str(s);
                        }
                        i += tag_end + 1;
                        continue;
                    }
                }
            } else {
                // Fragment: <>
                i += tag_end + 1;
                continue;
            }
            html_buf.push('<');
            i += 1;
        } else {
            let c = content[i..].chars().next().unwrap();
            html_buf.push(c);
            i += c.len_utf8();
        }
    }

    // Transform scripts
    for s in scripts.iter_mut() {
        let mut transformed = transform_use_effect(s);
        for sig in &state_vars {
            let scoped_name = if let Some(import_path) = state_variable_imports.get(sig) {
                if let Some(resolved_path) = resolve_import_path(&base_dir, import_path) {
                    let rel_path = if let Ok(cwd) = std::env::current_dir() {
                        if let Ok(rel) = std::path::Path::new(&resolved_path).strip_prefix(&cwd) {
                            rel.to_string_lossy().into_owned()
                        } else {
                            resolved_path.clone()
                        }
                    } else {
                        resolved_path.clone()
                    };
                    let imported_file_id: String = rel_path
                        .chars()
                        .map(|c| if c.is_alphanumeric() { c } else { '_' })
                        .collect();
                    format!("{}__{}", imported_file_id, sig)
                } else {
                    format!("{}__{}", file_id, sig)
                }
            } else {
                format!("{}__{}", file_id, sig)
            };
            transformed = inject_state_name(&transformed, sig, &scoped_name);
        }
        for sig in &state_vars {
            transformed = replace_word(&transformed, sig, ".value");
        }
        transformed = transformed.replace("import.meta.hot", "window.hmr");
        *s = transformed;
    }

    scripts.extend(block_logic);

    let mut bindings = Vec::new();
    let mut events = Vec::new();
    let reactive_html = parse_reactivity(&html_buf, &mut bindings, &mut events, &state_vars);
    let scoped_html = scope_html(&reactive_html, &scope_id)?;

    scripts.append(&mut bindings);
    scripts.append(&mut events);

    Ok(ProcessResult {
        html: scoped_html,
        scripts,
        styles,
        state_vars,
    })
}
