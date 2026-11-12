use std::collections::HashMap;
use super::utils::*;
use super::tree::process_component_tree;

pub fn resolve_suspense_loading(
    tag_name: &str,
    tag_content: &str,
    content: &str,
    i: usize,
    tag_end: usize,
    file_path: &str,
    visited: &mut HashMap<String, String>,
    params: &HashMap<String, String>,
    if_counter: &mut usize,
    for_counter: &mut usize,
    state_var_sources: &mut HashMap<String, String>,
    scripts: &mut Vec<String>,
    styles: &mut Vec<String>,
    state_vars: &mut Vec<String>,
    html_buf: &mut String,
) -> anyhow::Result<Option<usize>> {
    let mut parent_scripts = String::new();
    let mut temp_search = 0;
    while let Some(start_idx) = content[temp_search..].find("<script") {
        let script_start = temp_search + start_idx;
        if let Some(end_idx) = content[script_start..].find("</script>") {
            let script_end = script_start + end_idx;
            parent_scripts.push_str(&content[script_start..script_end + 9]);
            parent_scripts.push('\n');
            temp_search = script_end + 9;
        } else {
            break;
        }
    }

    let mut parent_scripts_res = None;
    if !parent_scripts.is_empty() {
        let mut dummy_if = 9999;
        let mut dummy_for = 9999;
        if let Ok(res) = process_component_tree(
            file_path,
            &parent_scripts,
            visited,
            None,
            params,
            &mut dummy_if,
            &mut dummy_for,
            state_var_sources,
        ) {
            parent_scripts_res = Some(res);
        }
    }

    let fallback_html = if let Some(fallback_expr) = extract_attribute(tag_content, "fallback") {
        let mut fallback_full = parent_scripts.clone();
        fallback_full.push_str(&fallback_expr);
        let sub_res = process_component_tree(
            file_path,
            &fallback_full,
            visited,
            None,
            params,
            if_counter,
            for_counter,
            state_var_sources,
        )?;
        for s in sub_res.scripts {
            let is_parent_script = parent_scripts_res.as_ref().map_or(false, |r| r.scripts.contains(&s));
            if !is_parent_script && !scripts.contains(&s) {
                scripts.push(s);
            }
        }
        for s in sub_res.styles {
            if !styles.contains(&s) { styles.push(s); }
        }
        for v in sub_res.state_vars {
            if !state_vars.contains(&v) { state_vars.push(v); }
        }
        sub_res.html
    } else {
        "".to_string()
    };

    // Find matching </Loading> or </Suspense> tag
    let open_prefix = format!("<{}", tag_name);
    let close_tag = format!("</{}>", tag_name);
    let open_len = open_prefix.len();
    let close_len = close_tag.len();

    let mut depth = 1;
    let mut search_pos = i + tag_end + 1;
    let mut closing_idx = None;
    while search_pos < content.len() {
        if content[search_pos..].starts_with(&open_prefix)
            && content[search_pos + open_len..].chars().next().map_or(true, |c| c.is_whitespace() || c == '>' || c == '/')
        {
            depth += 1;
            search_pos += open_len;
        } else if content[search_pos..].starts_with(&close_tag) {
            depth -= 1;
            if depth == 0 {
                closing_idx = Some(search_pos);
                break;
            }
            search_pos += close_len;
        } else {
            let c = content[search_pos..].chars().next().unwrap();
            search_pos += c.len_utf8();
        }
    }

    if let Some(closing) = closing_idx {
        let children_raw = &content[i + tag_end + 1..closing];
        let mut children_full = parent_scripts.clone();
        children_full.push_str(children_raw);
        let children_res = process_component_tree(
            file_path,
            &children_full,
            visited,
            None,
            params,
            if_counter,
            for_counter,
            state_var_sources,
        )?;
        for s in children_res.scripts {
            let is_parent_script = parent_scripts_res.as_ref().map_or(false, |r| r.scripts.contains(&s));
            if !is_parent_script && !scripts.contains(&s) {
                scripts.push(s);
            }
        }
        for s in children_res.styles {
            if !styles.contains(&s) { styles.push(s); }
        }
        for v in children_res.state_vars {
            if !state_vars.contains(&v) { state_vars.push(v); }
        }

        let suspense_id = format!("erm-suspense-{}", i);
        html_buf.push_str(&format!(
            r#"<div id="{}" class="erm-suspense-container" style="display: contents;">
  <div id="{}-fallback" class="erm-suspense-fallback" style="display: block;">
    {}
  </div>
  <div id="{}-content" class="erm-suspense-content" style="display: none;">
    {}
  </div>
</div>"#,
            suspense_id, suspense_id, fallback_html, suspense_id, children_res.html
        ));

        Ok(Some(closing + close_len))
    } else {
        Err(anyhow::anyhow!("Unclosed <{}> tag", tag_name))
    }
}

pub fn resolve_custom_component(
    tag_name: &str,
    tag_content: &str,
    base_dir: &str,
    component_imports: &HashMap<String, String>,
    i: usize,
    visited: &mut HashMap<String, String>,
    params: &HashMap<String, String>,
    if_counter: &mut usize,
    for_counter: &mut usize,
    state_var_sources: &mut HashMap<String, String>,
    scripts: &mut Vec<String>,
    styles: &mut Vec<String>,
    state_vars: &mut Vec<String>,
    html_buf: &mut String,
) -> anyhow::Result<()> {
    let comp_filename = format!("{}.erm", tag_name);
    let mut comp_path = None;

    if let Some(import_path) = component_imports.get(tag_name) {
        let path_buf = std::path::PathBuf::from(base_dir).join(import_path);
        if path_buf.extension().map_or(true, |ext| ext != "erm") {
            if path_buf.exists() {
                comp_path = Some(path_buf);
            } else {
                let mut path_with_ext = path_buf.clone();
                path_with_ext.set_extension("erm");
                if path_with_ext.exists() {
                    comp_path = Some(path_with_ext);
                }
            }
        } else {
            if path_buf.exists() {
                comp_path = Some(path_buf);
            }
        }
    }

    if comp_path.is_none() {
        let mut curr = std::path::PathBuf::from(base_dir);
        loop {
            let p_comp = curr.join(&comp_filename);
            if file_exists_or_vfs(&p_comp) {
                comp_path = Some(p_comp);
                break;
            }
            let p_comp_dir = curr.join("components").join(&comp_filename);
            if file_exists_or_vfs(&p_comp_dir) {
                comp_path = Some(p_comp_dir);
                break;
            }
            if let Some(parent) = curr.parent() {
                if curr.join("eronom.toml").exists() || curr.join("Cargo.toml").exists() || curr.join(".git").exists() {
                    break;
                }
                curr = parent.to_path_buf();
            } else {
                break;
            }
        }
        if comp_path.is_none() {
            let fallback_app = std::path::PathBuf::from("app/components").join(&comp_filename);
            if file_exists_or_vfs(&fallback_app) {
                comp_path = Some(fallback_app);
            }
        }
    }

    if let Some(comp_path) = comp_path {
        let canonical_comp_path = if comp_path.exists() {
            std::fs::canonicalize(&comp_path).unwrap_or(comp_path.clone())
        } else {
            comp_path.clone()
        };
        let comp_path_str = canonical_comp_path.to_string_lossy().into_owned();

        let anchor_id = format!("erm-anchor-{}-{}", tag_name.to_lowercase(), i);
        let attrs = parse_tag_attributes(tag_content);
        let mut props_fields = Vec::new();
        for (k, v) in attrs {
            props_fields.push(format!("{}: {}", k, v));
        }
        let props_js = if props_fields.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", props_fields.join(", "))
        };

        let sub_html = if !visited.contains_key(&comp_path_str) {
            visited.insert(comp_path_str.clone(), "".to_string()); // placeholder to avoid infinite recursion
            let comp_content = read_file_or_vfs(&canonical_comp_path)?;
            let mut sub_res = process_component_tree(&comp_path_str, &comp_content, visited, None, params, if_counter, for_counter, state_var_sources)?;
            let mut sub_html = sub_res.html;
            let mut sub_scripts = sub_res.scripts;
            scope_component_ids(&mut sub_html, &mut sub_scripts);
            
            let sub_combined_script = sub_scripts.join("\n");
            let component_def = format!(
                "async function {}(anchorId, props) {{\n  const useProps = (p = props) => erm.useProps(p);\n  try {{\n    let el = document.getElementById(anchorId);\n    if (el && el.innerHTML.trim() === \"\") {{\n      el.innerHTML = `{}`;\n    }}\n  }} catch(e) {{ console.error(e); }}\n  {}\n}};\n",
                tag_name, sub_html.replace("__erm_anchor_id_prefix__", "${anchorId}").replace("`", "\\`"), sub_combined_script
            );
            
            scripts.push(component_def);
            styles.append(&mut sub_res.styles);
            for v in sub_res.state_vars {
                if !state_vars.contains(&v) { state_vars.push(v); }
            }
            
            visited.insert(comp_path_str.clone(), sub_html.clone());
            sub_html
        } else {
            visited.get(&comp_path_str).cloned().unwrap_or_default()
        };

        let sub_html_scoped = sub_html.replace("__erm_anchor_id_prefix__", &format!("__erm_anchor_id_prefix__{}", anchor_id));
        html_buf.push_str(&format!(
            "<div id=\"__erm_anchor_id_prefix__{}\" class=\"erm-anchor\">{}</div>",
            anchor_id, sub_html_scoped
        ));

        scripts.push(format!("await {}(anchorId + \"{}\", {});", tag_name, anchor_id, props_js));
        Ok(())
    } else {
        anyhow::bail!("Component '{}' not found in components/ or current directory.", tag_name);
    }
}

#[derive(Debug, Clone)]
pub struct LocalFunctionComponent {
    pub name: String,
    pub params: String,
    pub script: String,
    pub markup: String,
    pub full_erm: String,
}

pub fn extract_local_components(content: &str) -> HashMap<String, LocalFunctionComponent> {
    static RE_FN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE_FN.get_or_init(|| {
        regex::Regex::new(r"(?m)^\s*(?:export\s+(?:default\s+)?)?(?:fn|function)\s+([A-Za-z0-9_]+)\s*\(([\s\S]*?)\)\s*(?::\s*[A-Za-z0-9_<>\[\],\s]+\s*)?\{").unwrap()
    });

    let mut local_map = HashMap::new();

    for caps in re.captures_iter(content) {
        let m = caps.get(0).unwrap();
        let fn_name = caps.get(1).unwrap().as_str().to_string();
        if !fn_name.chars().next().map_or(false, |c| c.is_ascii_uppercase()) {
            continue;
        }

        let params = caps.get(2).unwrap().as_str().to_string();
        let body_start = m.end();
        if let Some(cb) = super::utils::find_matching_close_brace(&content[body_start..]) {
            let body = &content[body_start..body_start + cb];

            if !body.contains('<') && !body.contains('@') {
                continue;
            }

            let trimmed_params = params.trim();
            let clean_params = if trimmed_params.starts_with('{') {
                if let Some(cb) = super::utils::find_matching_close_brace(&trimmed_params[1..]) {
                    trimmed_params[..=cb + 1].trim()
                } else {
                    trimmed_params
                }
            } else if let Some(colon_pos) = trimmed_params.find(':') {
                trimmed_params[..colon_pos].trim()
            } else {
                trimmed_params
            };

            let mut script_lines = Vec::new();
            let mut markup_lines = Vec::new();

            if body.contains("return `") {
                let mut in_return_template = false;
                for line in body.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("return `") {
                        in_return_template = true;
                        let after = trimmed[8..].trim();
                        if !after.is_empty() && after != "`" && after != "`;" {
                            markup_lines.push(after.trim_end_matches(';').trim_end_matches('`'));
                        }
                        continue;
                    }
                    if in_return_template {
                        if trimmed == "`" || trimmed == "`;" || trimmed.ends_with("`;") {
                            in_return_template = false;
                            continue;
                        }
                        markup_lines.push(line);
                    } else {
                        script_lines.push(line);
                    }
                }
            } else {
                let mut script_mode = true;
                for line in body.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if script_mode {
                        let starts_with_markup = trimmed.starts_with('<')
                            || trimmed.starts_with('@')
                            || (trimmed.starts_with('{') && !trimmed.starts_with("{//") && !trimmed.starts_with("{/*"));
                        if starts_with_markup {
                            script_mode = false;
                        }
                    }
                    if script_mode {
                        script_lines.push(line);
                    } else {
                        markup_lines.push(line);
                    }
                }
            }

            let mut full_erm = String::new();
            full_erm.push_str("<script>\n");
            if !clean_params.is_empty() {
                if clean_params.starts_with('{') {
                    full_erm.push_str(&format!("const {} = useProps();\n", clean_params));
                } else {
                    full_erm.push_str(&format!("const {} = useProps();\n", clean_params));
                }
            } else {
                full_erm.push_str("const props = useProps();\n");
            }
            for s in &script_lines {
                full_erm.push_str(s);
                full_erm.push('\n');
            }
            full_erm.push_str("</script>\n");
            for m in &markup_lines {
                full_erm.push_str(m);
                full_erm.push('\n');
            }

            local_map.insert(
                fn_name.clone(),
                LocalFunctionComponent {
                    name: fn_name,
                    params,
                    script: script_lines.join("\n"),
                    markup: markup_lines.join("\n"),
                    full_erm,
                },
            );
        }
    }

    local_map
}

pub fn resolve_local_component(
    tag_name: &str,
    tag_content: &str,
    local_comp: &LocalFunctionComponent,
    file_path: &str,
    visited: &mut HashMap<String, String>,
    params: &HashMap<String, String>,
    if_counter: &mut usize,
    for_counter: &mut usize,
    state_var_sources: &mut HashMap<String, String>,
    scripts: &mut Vec<String>,
    styles: &mut Vec<String>,
    state_vars: &mut Vec<String>,
    html_buf: &mut String,
) -> anyhow::Result<()> {
    let comp_stack_key = format!("stack:{}:{}", file_path, tag_name);
    if visited.get(&comp_stack_key).map_or(false, |v| v == "IN_PROGRESS") {
        return Ok(());
    }
    visited.insert(comp_stack_key.clone(), "IN_PROGRESS".to_string());

    let attrs = parse_tag_attributes(tag_content);

    let mut comp_params = params.clone();
    for (k, v) in &attrs {
        let clean_v = if (v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')) {
            v[1..v.len() - 1].to_string()
        } else {
            v.clone()
        };
        comp_params.insert(k.clone(), clean_v);
    }

    let sub_res = process_component_tree(
        file_path,
        &local_comp.full_erm,
        visited,
        None,
        &comp_params,
        if_counter,
        for_counter,
        state_var_sources,
    );

    visited.remove(&comp_stack_key);
    let sub_res = sub_res?;

    *if_counter += 1;
    let instance_prefix = format!("erm-ci-{}-", *if_counter);
    let mut sub_html = sub_res.html;
    let mut sub_scripts = sub_res.scripts;
    super::utils::scope_instance_ids(&mut sub_html, &mut sub_scripts, &instance_prefix);

    for s in sub_res.styles {
        if !styles.contains(&s) {
            styles.push(s);
        }
    }
    for v in sub_res.state_vars {
        if !state_vars.contains(&v) {
            state_vars.push(v);
        }
    }
    for sc in sub_scripts {
        if !scripts.contains(&sc) {
            scripts.push(sc);
        }
    }

    html_buf.push_str(&sub_html);
    Ok(())
}
