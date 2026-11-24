
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
                if trimmed.contains('%') || trimmed == "to" || trimmed == "from" || trimmed.starts_with("body") || trimmed.starts_with("html") || trimmed.starts_with(":root") || trimmed.starts_with('@') {
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

pub fn is_eds_enabled(base_path: &std::path::Path) -> bool {
    let toml_path = base_path.join("eronom.toml");
    let toml_content_opt = if toml_path.exists() {
        std::fs::read_to_string(&toml_path).ok()
    } else {
        None
    };

    if let Some(content) = toml_content_opt {
        if let Ok(toml_val) = toml::from_str::<toml::Value>(&content) {
            if let Some(ds) = toml_val.get("eds").or_else(|| toml_val.get("design_system")) {
                if let Some(enabled) = ds.get("enabled").and_then(|v| v.as_bool()) {
                    return enabled;
                }
            }
        }
    }

    true
}

fn camel_to_kebab(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if c.is_uppercase() {
            result.push('-');
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

pub fn extract_use_style(script: &str, scope_id: &str) -> (String, Vec<String>) {
    let mut result_script = String::new();
    let mut generated_css = Vec::new();
    let clean_scope = scope_id.trim_matches(|c| c == '[' || c == ']' || c == '"');
    let mut i = 0;
    while i < script.len() {
        if script[i..].starts_with("useStyle") {
            let mut j = i + 8;
            while j < script.len() && script[j..].chars().next().unwrap().is_whitespace() {
                j += 1;
            }
            if j < script.len() && script[j..].starts_with('(') {
                let mut paren_depth = 1;
                let mut in_string: Option<char> = None;
                let mut k = j + 1;
                while k < script.len() {
                    let c = script[k..].chars().next().unwrap();
                    if let Some(q) = in_string {
                        if c == q && (k == 0 || script[..k].chars().last() != Some('\\')) {
                            in_string = None;
                        }
                    } else if c == '\'' || c == '"' || c == '`' {
                        in_string = Some(c);
                    } else if c == '(' {
                        paren_depth += 1;
                    } else if c == ')' {
                        paren_depth -= 1;
                        if paren_depth == 0 {
                            break;
                        }
                    }
                    k += c.len_utf8();
                }

                if paren_depth == 0 {
                    let inner_arg = script[j + 1..k].trim();
                    if inner_arg.starts_with('{') && inner_arg.ends_with('}') {
                        let inner_obj = inner_arg[1..inner_arg.len() - 1].trim();
                        let mut class_map = Vec::new();
                        let mut p = 0;
                        while p < inner_obj.len() {
                            while p < inner_obj.len() && (inner_obj[p..].chars().next().unwrap().is_whitespace() || inner_obj[p..].starts_with(',')) {
                                p += 1;
                            }
                            if p >= inner_obj.len() { break; }
                            if let Some(colon_pos) = inner_obj[p..].find(':') {
                                let key = inner_obj[p..p + colon_pos].trim().trim_matches('\'').trim_matches('"');
                                let after_colon = p + colon_pos + 1;
                                let mut brace_start = after_colon;
                                while brace_start < inner_obj.len() && inner_obj[brace_start..].chars().next().unwrap().is_whitespace() {
                                    brace_start += 1;
                                }
                                if brace_start < inner_obj.len() && inner_obj[brace_start..].starts_with('{') {
                                    if let Some(cb) = super::utils::find_matching_close_brace(&inner_obj[brace_start + 1..]) {
                                        let props_str = &inner_obj[brace_start + 1..brace_start + 1 + cb];
                                        let cls_name = format!("erm-{}-{}", key, clean_scope);
                                        
                                        let mut css_rule = format!(".{} {{\n", cls_name);
                                        for line in props_str.split(',') {
                                            let line = line.trim();
                                            if let Some(cp) = line.find(':') {
                                                let prop = line[..cp].trim().trim_matches('\'').trim_matches('"');
                                                let val = line[cp + 1..].trim().trim_matches('\'').trim_matches('"').trim_matches(';');
                                                let kebab = camel_to_kebab(prop);
                                                css_rule.push_str(&format!("  {}: {};\n", kebab, val));
                                            }
                                        }
                                        css_rule.push_str("}\n");
                                        generated_css.push(css_rule);
                                        class_map.push((key.to_string(), cls_name));
                                        p = brace_start + 1 + cb + 1;
                                        continue;
                                    }
                                }
                            }
                            p += 1;
                        }

                        if !class_map.is_empty() {
                            let mut replacement_dict = String::from("{\n");
                            for (key, cls_name) in class_map {
                                replacement_dict.push_str(&format!("  {}: \"{}\",\n", key, cls_name));
                            }
                            replacement_dict.push_str("}");
                            result_script.push_str(&replacement_dict);
                            i = k + 1;
                            continue;
                        }
                    }
                }
            }
        }
        let c = script[i..].chars().next().unwrap();
        result_script.push(c);
        i += c.len_utf8();
    }
    (result_script, generated_css)
}
