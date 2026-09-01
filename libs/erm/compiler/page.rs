use std::collections::HashMap;
use crate::eval::{self, ErmEval};
use super::utils::*;
use super::css::get_global_ermcss;
use super::transform::{is_function_template, preprocess_function_template};
use super::tree::{process_component_tree, ProcessResult};

pub fn process_erm_component(file_path: &str, content: &str, is_prod: bool, params: &HashMap<String, String>) -> anyhow::Result<String> {
    let preprocessed;
    let content = if is_function_template(content) {
        preprocessed = preprocess_function_template(content)?;
        &preprocessed
    } else {
        content
    };
    
    let path = std::path::Path::new(file_path);
    let base_dir = if path.is_file() {
        path.parent().unwrap().to_str().unwrap()
    } else {
        file_path
    };

    let mut visited = HashMap::new();
    let mut state_var_sources = HashMap::new();
    
    // Automatic Layout support: search for layout.erm in current and parent directories.
    let mut layout_path = None;
    let mut curr = std::path::PathBuf::from(base_dir);
    loop {
        let p_layouts = curr.join("layouts").join("layout.erm");
        if file_exists_or_vfs(&p_layouts) {
            layout_path = Some(p_layouts);
            break;
        }
        let p_direct = curr.join("layout.erm");
        if file_exists_or_vfs(&p_direct) {
            layout_path = Some(p_direct);
            break;
        }
        if let Some(parent) = curr.parent() {
            if curr.join("Cargo.toml").exists() || curr.join(".git").exists() {
                break;
            }
            curr = parent.to_path_buf();
        } else {
            break;
        }
    }
    if layout_path.is_none() {
        let fallback_app_layout = std::path::PathBuf::from("app/layouts/layout.erm");
        if file_exists_or_vfs(&fallback_app_layout) {
            layout_path = Some(fallback_app_layout);
        } else {
            let fallback_layout = std::path::PathBuf::from("layouts/layout.erm");
            if file_exists_or_vfs(&fallback_layout) {
                layout_path = Some(fallback_layout);
            }
        }
    }

    // Automatic Loading support: search for loading.erm in current and parent directories.
    let mut loading_path = None;
    let mut curr_load = std::path::PathBuf::from(base_dir);
    loop {
        let p_loadings = curr_load.join("layouts").join("loading.erm");
        if file_exists_or_vfs(&p_loadings) {
            loading_path = Some(p_loadings);
            break;
        }
        let p_loading_direct = curr_load.join("loading.erm");
        if file_exists_or_vfs(&p_loading_direct) {
            loading_path = Some(p_loading_direct);
            break;
        }
        if let Some(parent) = curr_load.parent() {
            if curr_load.join("Cargo.toml").exists() || curr_load.join(".git").exists() {
                break;
            }
            curr_load = parent.to_path_buf();
        } else {
            break;
        }
    }
    if loading_path.is_none() {
        let fallback_app_loading = std::path::PathBuf::from("app/layouts/loading.erm");
        if file_exists_or_vfs(&fallback_app_loading) {
            loading_path = Some(fallback_app_loading);
        }
    }

    let mut if_counter = 0;
    let mut for_counter = 0;

    let loading_res = if let Some(ref l_path) = loading_path {
        let loading_content = read_file_or_vfs(l_path)?;
        let res = process_component_tree(&l_path.to_string_lossy(), &loading_content, &mut visited, None, params, &mut if_counter, &mut for_counter, &mut state_var_sources)?;
        Some(res)
    } else {
        None
    };

    let mut result = if let Some(lp) = layout_path {
        if !content.contains("<!DOCTYPE html>") && !content.contains("<html") {
            let layout_content = read_file_or_vfs(&lp)?;
            if content.trim() != layout_content.trim() {
                let page_res = process_component_tree(file_path, content, &mut visited, None, params, &mut if_counter, &mut for_counter, &mut state_var_sources)?;
                
                let wrapped_html = if let Some(ref l_res) = loading_res {
                    format!(
                        r#"<div id="erm-loading-container" class="erm-loading-container" style="display: contents;">
  <div id="erm-loading-fallback" class="erm-loading-fallback" style="display: block;">
    {}
  </div>
  <div id="erm-loading-content" class="erm-loading-content" style="display: none;">
    {}
  </div>
</div>"#,
                        l_res.html, page_res.html
                    )
                } else {
                    page_res.html.clone()
                };

                let mut layout_res = process_component_tree(&lp.to_string_lossy(), &layout_content, &mut visited, Some(&wrapped_html), params, &mut if_counter, &mut for_counter, &mut state_var_sources)?;
                
                for s in page_res.scripts {
                    if !layout_res.scripts.contains(&s) { layout_res.scripts.push(s); }
                }
                for s in page_res.styles {
                    if !layout_res.styles.contains(&s) { layout_res.styles.push(s); }
                }
                for v in page_res.state_vars {
                    if !layout_res.state_vars.contains(&v) { layout_res.state_vars.push(v); }
                }
                layout_res
            } else {
                process_component_tree(file_path, content, &mut visited, None, params, &mut if_counter, &mut for_counter, &mut state_var_sources)?
            }
        } else {
            process_component_tree(file_path, content, &mut visited, None, params, &mut if_counter, &mut for_counter, &mut state_var_sources)?
        }
    } else {
        let page_res = process_component_tree(file_path, content, &mut visited, None, params, &mut if_counter, &mut for_counter, &mut state_var_sources)?;
        if let Some(ref l_res) = loading_res {
            let wrapped_html = format!(
                r#"<div id="erm-loading-container" class="erm-loading-container" style="display: contents;">
  <div id="erm-loading-fallback" class="erm-loading-fallback" style="display: block;">
    {}
  </div>
  <div id="erm-loading-content" class="erm-loading-content" style="display: none;">
    {}
  </div>
</div>"#,
                l_res.html, page_res.html
            );
            ProcessResult {
                html: wrapped_html,
                scripts: page_res.scripts,
                styles: page_res.styles,
                state_vars: page_res.state_vars,
            }
        } else {
            page_res
        }
    };

    if let Some(ref l_res) = loading_res {
        for s in &l_res.scripts {
            if !result.scripts.contains(s) { result.scripts.push(s.clone()); }
        }
        for s in &l_res.styles {
            if !result.styles.contains(s) { result.styles.push(s.clone()); }
        }
        for v in &l_res.state_vars {
            if !result.state_vars.contains(v) { result.state_vars.push(v.clone()); }
        }
    }

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

    let mut script_all = String::new();
    for s in &result.scripts {
        script_all.push_str(s);
        script_all.push('\n');
    }
    let script_for_eval = script_all.replace("useParams()", "__erm_params");

    ev.parse_script_vars(&script_for_eval)?;

    let mut res_html = result.html.clone();
    
    // Extract bindings and evaluate them on the server to pre-populate spans
    for s in &result.scripts {
        if s.starts_with("window.__erm_bindings.push(") {
            if let Some(id_start) = s.find("id: \"") {
                let id_end = s[id_start + 5..].find('"').unwrap_or(0);
                let id = &s[id_start + 5..id_start + 5 + id_end];
                if let Some(get_start) = s.find("get: () => (") {
                    if let Some(get_end) = s[get_start + 12..].rfind(')') {
                        let expr = &s[get_start + 12..get_start + 12 + get_end];
                        if let Ok(val) = ev.eval(expr) {
                            if val != eval::Value::Null {
                                let val_str = val.to_string();
                                let id_pattern = format!("id=\"{}\"", id);
                                if let Some(pos) = res_html.find(&id_pattern) {
                                    if let Some(close_tag) = res_html[pos..].find('>') {
                                        let insert_pos = pos + close_tag + 1;
                                        res_html.insert_str(insert_pos, &val_str);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    res_html = evaluate_braces_in_html(&res_html, &mut ev, &result.state_vars);

    // Inject precompiled global ermcss styles if populated
    let mut has_global_ermcss = false;
    if let Ok(global_css) = get_global_ermcss() {
        if !global_css.trim().is_empty() {
            if is_prod {
                has_global_ermcss = true;
            } else {
                result.styles.push(global_css);
            }
        }
    }

    let mut style_assets = String::new();
    if has_global_ermcss {
        style_assets.push_str("\n<link rel=\"stylesheet\" id=\"__erm_styles\" href=\"/css/global.css\">\n");
    }
    if !result.styles.is_empty() {
        style_assets.push_str("\n<style id=\"__erm_scoped_styles\">\n");
        for s in &result.styles { style_assets.push_str(s); style_assets.push('\n'); }
        style_assets.push_str("</style>\n");
    }

    let mut params_js = String::from("window.__erm_params = {");
    for (k, v) in params {
        let is_json_literal = (v.starts_with('{') && v.ends_with('}'))
            || (v.starts_with('[') && v.ends_with(']'))
            || v == "true"
            || v == "false"
            || v == "null"
            || v.parse::<f64>().is_ok();
            
        if is_json_literal {
            params_js.push_str(&format!("{}: {},", k, v));
        } else {
            params_js.push_str(&format!("{}: \"{}\",", k, v.replace("\"", "\\\"")));
        }
    }
    params_js.push_str("};");
    
    let mut scripts_to_inject = result.scripts.clone();
    scripts_to_inject.insert(0, params_js);

    let combined_scripts = scripts_to_inject.join("\n");
    let mut declarations = String::from("let anchorId = \"\";\n");
    for v in &result.state_vars {
        if let Ok(re_decl) = regex::Regex::new(&format!(r#"\b(let|const|var)\s+{}\b"#, v)) {
            if !re_decl.is_match(&combined_scripts) {
                let fallback_val = match v.as_str() {
                    "activeTheme" => "'light'",
                    "count" => "0",
                    "timer" => "0",
                    "todos" => "[]",
                    "showExtraContent" => "true",
                    "submitted" => "false",
                    _ => "null",
                };
                let scoped_name = if let Some(source_file) = state_var_sources.get(v) {
                    let rel_path = if let Ok(cwd) = std::env::current_dir() {
                        if let Ok(rel) = std::path::Path::new(source_file).strip_prefix(&cwd) {
                            rel.to_string_lossy().into_owned()
                        } else {
                            source_file.clone()
                        }
                    } else {
                        source_file.clone()
                    };
                    let source_file_id: String = rel_path
                        .chars()
                        .map(|c| if c.is_alphanumeric() { c } else { '_' })
                        .collect();
                    format!("{}__{}", source_file_id, v)
                } else {
                    v.clone()
                };
                declarations.push_str(&format!(
                    "let {} = useState({}, \"{}\");\n",
                    v, fallback_val, scoped_name
                ));
            }
        }
    }
    if !declarations.is_empty() {
        scripts_to_inject.insert(0, declarations);
    }

    let mut script_assets = String::new();
    if !scripts_to_inject.is_empty() || !result.state_vars.is_empty() {
        script_assets.push_str("<script type=\"module\" class=\"__erm_script\">\n");
        script_assets.push_str("import { useState, useEffect, onMount, useParams, effect } from '/modules/erm/runtime.js';\n");
        script_assets.push_str("{\n");
        for s in &scripts_to_inject { script_assets.push_str(s); script_assets.push('\n'); }
        script_assets.push_str("}\n");
        script_assets.push_str("</script>\n");
    }

    let mut output = res_html.replace("__erm_anchor_id_prefix__", "");

    if !output.contains("<html") {
        let mut final_res = String::new();
        final_res.push_str("<!DOCTYPE html><html><head>");
        if !is_prod {
            final_res.push_str("<script src=\"/modules/erm/hmr.js\"></script>\n");
        }
        final_res.push_str(&style_assets);
        final_res.push_str("</head><body>\n");
        final_res.push_str(&output);
        final_res.push_str("\n");
        final_res.push_str(&script_assets);
        final_res.push_str("</body></html>");
        return Ok(final_res);
    }

    if !is_prod {
        let hmr_script = "<script src=\"/modules/erm/hmr.js\"></script>";
        if let Some(pos) = output.find("<head>") {
            output.insert_str(pos + 6, hmr_script);
        } else {
            output.insert_str(0, hmr_script);
        }
    }

    if let Some(pos) = output.find("</head>") {
        output.insert_str(pos, &style_assets);
    } else {
        output.insert_str(0, &style_assets);
    }

    if let Some(pos) = output.find("</body>") {
        output.insert_str(pos, &script_assets);
    } else {
        output.push_str(&script_assets);
    }

    Ok(output)
}
