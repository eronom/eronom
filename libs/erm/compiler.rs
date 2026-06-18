use std::collections::HashMap;
use crate::eval::{self, ErmEval};
use fnv::FnvHasher;
use std::hash::Hasher;
use base64::{Engine as _, engine::general_purpose};

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

pub struct ProcessResult {
    pub html: String,
    pub scripts: Vec<String>,
    pub styles: Vec<String>,
    pub state_vars: Vec<String>,
}

pub fn replace_word(input: &str, word: &str, suffix: &str) -> String {
    if word.len() <= 1 { return input.to_string(); }
    let mut res = String::new();
    let mut i = 0;
    let mut in_string: Option<char> = None;
    while i < input.len() {
        let c = input[i..].chars().next().unwrap();
        if let Some(quote) = in_string {
            if c == quote && (i == 0 || input[..i].chars().last() != Some('\\')) {
                in_string = None;
            }
            res.push(c);
            i += c.len_utf8();
            continue;
        }

        if c == '"' || c == '\'' {
            in_string = Some(c);
            res.push(c);
            i += 1;
            continue;
        }

        if input[i..].starts_with(word) {
            let end = i + word.len();
            let before_ok = i == 0 || {
                let prev_c = input[..i].chars().last().unwrap();
                !prev_c.is_alphanumeric() && prev_c != '_' && prev_c != '$'
            };
            let after_ok = end == input.len() || {
                let next_c = input[end..].chars().next().unwrap();
                !next_c.is_alphanumeric() && next_c != '_' && next_c != '$'
            };

            if before_ok && after_ok {
                let mut is_decl = false;
                for kw in ["let", "const", "var"] {
                    if i >= kw.len() + 1 {
                        let start = i - kw.len() - 1;
                        if &input[start..i - 1] == kw && input[i-1..i].chars().next().unwrap().is_whitespace() {
                            let pre_kw_ok = start == 0 || {
                                let pre_c = input[..start].chars().last().unwrap();
                                !pre_c.is_alphanumeric()
                            };
                            if pre_kw_ok {
                                is_decl = true;
                                break;
                            }
                        }
                    }
                }

                if is_decl {
                    res.push_str(word);
                } else if input[end..].starts_with(suffix) {
                    res.push_str(word);
                } else {
                    res.push_str(word);
                    res.push_str(suffix);
                }
                i = end;
                continue;
            }
        }
        res.push(c);
        i += c.len_utf8();
    }
    res
}

pub fn inject_state_name(input: &str, name: &str) -> String {
    let mut res = String::new();
    let mut i = 0;
    while i < input.len() {
        if input[i..].starts_with(name) {
            let end = i + name.len();
            let mut k = end;
            while k < input.len() {
                let c = input[k..].chars().next().unwrap();
                if c.is_whitespace() || c == ':' {
                    k += c.len_utf8();
                } else {
                    break;
                }
            }
            if k < input.len() && input[k..].starts_with('=') {
                k += 1;
                while k < input.len() {
                    let c = input[k..].chars().next().unwrap();
                    if c.is_whitespace() {
                        k += c.len_utf8();
                    } else {
                        break;
                    }
                }
                if input[k..].starts_with("useState(") {
                    let mut depth = 1;
                    let mut j = k + 9;
                    while j < input.len() && depth > 0 {
                        let c = input[j..].chars().next().unwrap();
                        if c == '(' { depth += 1; }
                        else if c == ')' { depth -= 1; }
                        j += c.len_utf8();
                    }
                    if depth == 0 {
                        res.push_str(&input[i..j - 1]);
                        res.push_str(", \"");
                        res.push_str(name);
                        res.push_str("\")");
                        res.push_str(&inject_state_name(&input[j..], name));
                        return res;
                    }
                }
            }
        }
        let c = input[i..].chars().next().unwrap();
        res.push(c);
        i += c.len_utf8();
    }
    res
}

pub fn process_component_tree(base_dir: &str, content: &str, visited: &mut HashMap<String, bool>, slot_html: Option<&str>) -> anyhow::Result<ProcessResult> {
    let mut scripts = Vec::new();
    let mut styles = Vec::new();
    let mut state_vars = Vec::new();

    let mut component_imports = HashMap::new();
    let re_import = regex::Regex::new(r#"import\s+([A-Za-z0-9_]+)\s+from\s+['"]([^'"]+)['"]\s*;?"#).unwrap();

    let mut search_idx = 0;
    while let Some(start_idx) = content[search_idx..].find("<script") {
        let script_start = search_idx + start_idx;
        if let Some(end_idx) = content[script_start..].find("</script>") {
            let script_end = script_start + end_idx;
            let script_tag = &content[script_start..script_end + 9];
            let content_start = script_tag.find('>').unwrap_or(0) + 1;
            let script_content = &script_tag[content_start..script_tag.len() - 9];

            for line in script_content.lines() {
                if let Some(caps) = re_import.captures(line) {
                    let comp_name = caps.get(1).unwrap().as_str().to_string();
                    let comp_path_val = caps.get(2).unwrap().as_str().to_string();
                    component_imports.insert(comp_name, comp_path_val);
                }
            }
            search_idx = script_end + 9;
        } else {
            break;
        }
    }

    let mut hasher = FnvHasher::default();
    hasher.write(content.as_bytes());
    let scope_id = format!("data-e-{:x}", hasher.finish());

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

            // Find state vars in script
            let mut aj = 0;
            while let Some(idx) = script_content[aj..].find("useState(") {
                let call_pos = aj + idx;
                // find variable name
                let mut k = call_pos;
                let mut eq_pos = None;
                while k > aj {
                    k -= 1;
                    if script_content.as_bytes()[k] == b'=' {
                        eq_pos = Some(k);
                        break;
                    }
                    if matches!(script_content.as_bytes()[k], b';' | b'{' | b'}') { break; }
                }
                if let Some(ep) = eq_pos {
                    let mut m = ep;
                    let mut name_end = None;
                    while m > aj {
                        m -= 1;
                        if script_content.as_bytes()[m].is_ascii_whitespace() { continue; }
                        if script_content.as_bytes()[m].is_ascii_alphanumeric() || script_content.as_bytes()[m] == b'_' || script_content.as_bytes()[m] == b'$' {
                            name_end = Some(m + 1);
                            break;
                        }
                        break;
                    }
                    if let Some(ne) = name_end {
                        let mut m_start = ne;
                        while m_start > aj {
                            m_start -= 1;
                            if !(script_content.as_bytes()[m_start].is_ascii_alphanumeric() || script_content.as_bytes()[m_start] == b'_' || script_content.as_bytes()[m_start] == b'$') {
                                m_start += 1;
                                break;
                            }
                        }
                        let name = &script_content[m_start..ne];
                        if !state_vars.contains(&name.to_string()) {
                            state_vars.push(name.to_string());
                        }
                    }
                }
                aj = call_pos + 9;
            }

            let mut cleaned_script = String::new();
            for line in script_content.lines() {
                if re_import.is_match(line) {
                    continue;
                }
                cleaned_script.push_str(line);
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
            let tag_end = match content[i..].find('>') {
                Some(idx) => idx,
                None => {
                    html_buf.push_str(&content[i..]);
                    break;
                }
            };
            let tag_content = &content[i + 1..i + tag_end];
            if !tag_content.is_empty() {
                if tag_content.starts_with('/') {
                    let closing_tag_name = tag_content[1..].trim();
                    if closing_tag_name == "Link" {
                        html_buf.push_str("</a>");
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
                    if !tag_name.is_empty() && tag_name.chars().next().unwrap().is_ascii_uppercase() {
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
                                if p_comp.exists() {
                                    comp_path = Some(p_comp);
                                    break;
                                }
                                let p_comp_dir = curr.join("components").join(&comp_filename);
                                if p_comp_dir.exists() {
                                    comp_path = Some(p_comp_dir);
                                    break;
                                }
                                if let Some(parent) = curr.parent() {
                                    if curr.join("config.er").exists() || curr.join("Cargo.toml").exists() || curr.join(".git").exists() {
                                        break;
                                    }
                                    curr = parent.to_path_buf();
                                } else {
                                    break;
                                }
                            }
                        }

                        if let Some(comp_path) = comp_path {
                            let canonical_comp_path = std::fs::canonicalize(&comp_path).unwrap_or(comp_path);
                            let comp_path_str = canonical_comp_path.to_string_lossy().into_owned();
                            if !visited.contains_key(&comp_path_str) {
                                visited.insert(comp_path_str.clone(), true);
                                let comp_content = std::fs::read_to_string(&canonical_comp_path)?;
                                let comp_dir = canonical_comp_path.parent().unwrap().to_string_lossy();
                                let mut sub_res = process_component_tree(&comp_dir, &comp_content, visited, None)?;
                                html_buf.push_str(&sub_res.html);
                                scripts.append(&mut sub_res.scripts);
                                styles.append(&mut sub_res.styles);
                                for v in sub_res.state_vars {
                                    if !state_vars.contains(&v) { state_vars.push(v); }
                                }
                                i += tag_end + 1;
                                continue;
                            }
                        }
                    }
                    if tag_name == "slot" {
                        if let Some(s) = slot_html {
                            html_buf.push_str(s);
                        }
                        i += tag_end + 1;
                        continue;
                    }
                }
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
        let mut transformed = s.clone();
        for sig in &state_vars {
            transformed = inject_state_name(&transformed, sig);
        }
        for sig in &state_vars {
            transformed = replace_word(&transformed, sig, ".value");
        }
        transformed = transformed.replace("import.meta.hot", "window.hmr");
        *s = transformed;
    }

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

fn parse_reactivity(html: &str, bindings: &mut Vec<String>, events: &mut Vec<String>, states: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    let mut in_tag = false;
    let mut block_depth = 0;

    while i < html.len() {
        let c = html[i..].chars().next().unwrap();

        if c == '{' && i + 1 < html.len() {
            if html[i..].starts_with("{#for ") { block_depth += 1; }
            else if html[i..].starts_with("{/for}") { if block_depth > 0 { block_depth -= 1; } }
        }

        if !in_tag {
            if c == '<' {
                in_tag = true;
                out.push(c);
                i += 1;
                continue;
            }
            if block_depth == 0 && c == '{' && i + 1 < html.len() {
                let next_c = html[i + 1..].chars().next().unwrap();
                if !matches!(next_c, '#' | '/' | ':') {
                    let mut depth = 1;
                    let mut j = i + 1;
                    while j < html.len() && depth > 0 {
                        let cur_c = html[j..].chars().next().unwrap();
                        if cur_c == '{' { depth += 1; }
                        else if cur_c == '}' { depth -= 1; }
                        j += cur_c.len_utf8();
                    }
                    if depth == 0 {
                        let mut expr = html[i + 1..j - 1].to_string();
                        for sig in states {
                            expr = replace_word(&expr, sig, ".value");
                        }
                        let id = format!("erm-bind-{}", j);
                        out.push_str(&format!("<span id=\"{}\"></span>", id));
                        bindings.push(format!("window.__erm_bindings.push({{ id: \"{}\", get: () => ({}) }});", id, expr));
                        i = j;
                        continue;
                    }
                }
            }
        } else {
            if c == '>' {
                in_tag = false;
                out.push(c);
                i += 1;
                continue;
            }
            if i > 0 && html[i-1..i].chars().next().unwrap().is_ascii_whitespace() && html[i..].starts_with("on") {
                let mut k = i + 2;
                while k < html.len() && html[k..k+1].chars().next().unwrap().is_ascii_alphabetic() { k += 1; }
                if k < html.len() && html[k..k+1].starts_with('=') {
                    let attr_name = &html[i..k];
                    if k + 1 < html.len() && html[k+1..k+2].starts_with('{') {
                        let mut depth = 1;
                        let mut j = k + 2;
                        while j < html.len() && depth > 0 {
                            let cur_c = html[j..].chars().next().unwrap();
                            if cur_c == '{' { depth += 1; }
                            else if cur_c == '}' { depth -= 1; }
                            j += cur_c.len_utf8();
                        }
                        if depth == 0 {
                            let mut expr = html[k + 2..j - 1].to_string();
                            for sig in states {
                                expr = replace_word(&expr, sig, ".value");
                            }
                            let event_type = attr_name[2..].to_lowercase();
                            let id = format!("erm-evt-{}", j);
                            out.push_str(&format!("id=\"{}\" ", id));
                            events.push(format!("window.__erm_events.push({{ id: \"{}\", event: \"{}\", handler: (event) => {{ ({})(event); if (typeof window.__erm_update === 'function') window.__erm_update(); }} }});", id, event_type, expr));
                            i = j;
                            continue;
                        }
                    }
                }
            }
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}



pub fn process_erm_component(base_dir: &str, content: &str, is_prod: bool, params: &HashMap<String, String>) -> anyhow::Result<String> {
    let mut visited = HashMap::new();
    
    // Automatic Layout support: search for layout.erm in current and parent directories.
    let mut layout_path = None;
    let mut curr = std::path::PathBuf::from(base_dir);
    loop {
        let p_layouts = curr.join("layouts").join("layout.erm");
        if p_layouts.exists() {
            layout_path = Some(p_layouts);
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

    let result = if let Some(lp) = layout_path {
        if !content.contains("<!DOCTYPE html>") && !content.contains("<html") {
            let layout_content = std::fs::read_to_string(&lp)?;
            if content.trim() != layout_content.trim() {
                let page_res = process_component_tree(base_dir, content, &mut visited, None)?;
                let mut layout_res = process_component_tree(&lp.parent().unwrap().to_string_lossy(), &layout_content, &mut visited, Some(&page_res.html))?;
                
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
                process_component_tree(base_dir, content, &mut visited, None)?
            }
        } else {
            process_component_tree(base_dir, content, &mut visited, None)?
        }
    } else {
        process_component_tree(base_dir, content, &mut visited, None)?
    };

    let mut ev = ErmEval::new();
    let mut params_map = HashMap::new();
    for (k, v) in params {
        params_map.insert(k.clone(), eval::Value::String(v.clone()));
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
    let mut block_logic = Vec::new();
    let mut for_counter = 0;
    let mut if_counter = 0;

    // Process {#for}
    while let Some(start_idx) = res_html.find("{#for ") {
        let end_for_idx = match res_html[start_idx..].find("{/for}") {
            Some(idx) => idx,
            None => break,
        };
        let full_end_idx = start_idx + end_for_idx + 6;
        let header_start = start_idx + 6;
        let header_end = match res_html[header_start..].find('}') {
            Some(idx) => idx,
            None => break,
        };
        let full_header_end = header_start + header_end;
        let header = &res_html[header_start..full_header_end];
        let body = &res_html[full_header_end + 1..start_idx + end_for_idx];
        let in_idx = match header.find(" in ") {
            Some(idx) => idx,
            None => break,
        };
        let vars_part = header[0..in_idx].trim();
        let collection_expr_raw = header[in_idx + 4..].trim();
        let mut collection_expr = collection_expr_raw.to_string();
        for sig in &result.state_vars {
            collection_expr = replace_word(&collection_expr, sig, ".value");
        }
        let (item_name, index_name) = if let Some(comma_idx) = vars_part.find(',') {
            (vars_part[0..comma_idx].trim(), vars_part[comma_idx + 1..].trim())
        } else {
            (vars_part, "")
        };
        let anchor_id = format!("erm-for-{}", for_counter);
        for_counter += 1;
        let mut ssr_html = String::new();
        let collection_val = ev.eval(&collection_expr).unwrap_or(eval::Value::Null);
        if let eval::Value::List(items) = collection_val {
            for (idx, item) in items.iter().enumerate() {
                let mut sub_ev = ev.clone();
                sub_ev.set(item_name, item.clone());
                if !index_name.is_empty() {
                    sub_ev.set(index_name, eval::Value::Number(idx as f64));
                }
                let mut bit = 0;
                while bit < body.len() {
                    let c = body[bit..].chars().next().unwrap();
                    if c == '{' && !body[bit..].starts_with("{#") && !body[bit..].starts_with("{/") && !body[bit..].starts_with("{:") {
                        if let Some(brace_end) = body[bit..].find('}') {
                            let mut sub_expr = body[bit + 1..bit + brace_end].to_string();
                            for sig in &result.state_vars {
                                sub_expr = replace_word(&sub_expr, sig, ".value");
                            }
                            let val = sub_ev.eval(&sub_expr).unwrap_or(eval::Value::Null);
                            ssr_html.push_str(&val.to_string());
                            bit += brace_end + 1;
                            continue;
                        }
                    }
                    ssr_html.push(c);
                    bit += c.len_utf8();
                }
            }
        }
        let body_b64 = general_purpose::STANDARD.encode(body);
        let js_params = if !index_name.is_empty() { format!("{}, {}", item_name, index_name) } else { item_name.to_string() };
        let logic = format!(
            r#"window.__erm_register_for("{}", () => ({}), "{}", ({}) => window.__erm_render_template(window.__erm_b64utf8("{}"), expr => eval(expr)));"#,
            anchor_id, collection_expr, body_b64, js_params, body_b64
        );
        block_logic.push(logic);
        let anchor_html = format!("<span id=\"{}\" style=\"display:contents;\">{}</span>", anchor_id, ssr_html);
        res_html = res_html.replace(&res_html[start_idx..full_end_idx], &anchor_html);
    }

    // Process {#if}
    while let Some(start_idx) = res_html.find("{#if ") {
        let mut end_if_idx = None;
        let mut depth = 1;
        let mut j = start_idx + 5;
        while j < res_html.len() {
            if res_html[j..].starts_with("{#if ") { depth += 1; j += 5; }
            else if res_html[j..].starts_with("{/if}") { depth -= 1; if depth == 0 { end_if_idx = Some(j); break; } j += 5; }
            else { let c = res_html[j..].chars().next().unwrap(); j += c.len_utf8(); }
        }
        let eidx = match end_if_idx { Some(idx) => idx, None => break };
        let full_block = &res_html[start_idx..eidx + 5];
        let anchor_id = format!("erm-if-{}", if_counter);
        if_counter += 1;
        let mut branches_js = String::new();
        let mut ssr_html_res = String::new();
        let mut ssr_found = false;
        let mut rem = full_block;
        while !rem.is_empty() {
            let (mut cond_expr, body_start, is_else) = if rem.starts_with("{#if ") {
                let end_brace = rem.find('}').unwrap_or(0);
                (rem[5..end_brace].trim().to_string(), end_brace + 1, false)
            } else if rem.starts_with("{:else if ") {
                let end_brace = rem.find('}').unwrap_or(0);
                (rem[10..end_brace].trim().to_string(), end_brace + 1, false)
            } else if rem.starts_with("{:else}") {
                ("true".to_string(), 7, true)
            } else {
                break;
            };
            for sig in &result.state_vars {
                cond_expr = replace_word(&cond_expr, sig, ".value");
            }
            let next_else = rem[body_start..].find("{:else").unwrap_or_else(|| rem[body_start..].find("{/if}").unwrap_or(0));
            let body = &rem[body_start..body_start + next_else];
            let body_b64 = general_purpose::STANDARD.encode(body);
            if !ssr_found {
                let cond_val = if is_else { true } else { ev.eval_bool(&cond_expr).unwrap_or(false) };
                if cond_val { ssr_html_res = body.to_string(); ssr_found = true; }
            }
            if branches_js.is_empty() { branches_js.push_str(&format!("if ({}) {{ __erm_new = __erm_b64utf8(\"{}\"); }}", cond_expr, body_b64)); }
            else if is_else { branches_js.push_str(&format!(" else {{ __erm_new = __erm_b64utf8(\"{}\"); }}", body_b64)); }
            else { branches_js.push_str(&format!(" else if ({}) {{ __erm_new = __erm_b64utf8(\"{}\"); }}", cond_expr, body_b64)); }
            rem = &rem[body_start + next_else..];
            if rem.starts_with("{/if}") { break; }
        }
        let logic = format!(
            r#"window.__erm_register_if("{}", () => {{ let __erm_new = ""; {}; return __erm_new; }});"#,
            anchor_id, branches_js
        );
        block_logic.push(logic);
        let anchor_html = format!("<span id=\"{}\" style=\"display:contents;\">{}</span>", anchor_id, ssr_html_res);
        res_html = res_html.replace(full_block, &anchor_html);
    }

    let mut assets = String::new();
    if !result.styles.is_empty() {
        assets.push_str("\n<style id=\"__erm_styles\">\n");
        for s in &result.styles { assets.push_str(s); assets.push('\n'); }
        assets.push_str("</style>\n");
    }

    let runtime = include_str!("runtime.js");


    let mut params_js = String::from("window.__erm_params = {");
    for (k, v) in params {
        params_js.push_str(&format!("{}: \"{}\",", k, v.replace("\"", "\\\"")));
    }
    params_js.push_str("};");
    
    let mut scripts_to_inject = result.scripts.clone();
    scripts_to_inject.insert(0, params_js);

    if !scripts_to_inject.is_empty() || !result.state_vars.is_empty() || !block_logic.is_empty() {
        assets.push_str("<script class=\"__erm_script\">\n");
        assets.push_str(runtime);
        assets.push('\n');
        assets.push_str("{\n");
        for s in &scripts_to_inject { assets.push_str(s); assets.push('\n'); }
        for s in &block_logic { assets.push_str(s); assets.push('\n'); }
        assets.push_str("}\n");
        assets.push_str("</script>\n");
    }

    let mut output = res_html;
    
    if !is_prod {
        let hmr_script = format!(
            "<script>\n{}\n</script>",
            include_str!("hmr.js")
        );
        if let Some(pos) = output.find("<head>") {
            output.insert_str(pos + 6, &hmr_script);
        } else {
            output.insert_str(0, &hmr_script);
        }
    }

    if let Some(pos) = output.find("</head>") {
        output.insert_str(pos, &assets);
    } else if let Some(pos) = output.find("</body>") {
        output.insert_str(pos, &assets);
    } else {
        output.push_str(&assets);
    }

    if !output.contains("<html") {
        let mut final_res = String::new();
        final_res.push_str("<!DOCTYPE html><html><head></head><body>");
        final_res.push_str(&output);
        final_res.push_str("</body></html>");
        return Ok(final_res);
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_compilation() {
        let content = "<Link to=\"/contact\">Contact</Link>";
        let mut visited = std::collections::HashMap::new();
        let res = process_component_tree(".", content, &mut visited, None).unwrap();
        assert!(res.html.contains("<a"));
        assert!(res.html.contains("href=\"/contact\""));
        assert!(res.html.contains(">Contact</a>"));
    }

    #[test]
    fn test_use_state_compilation() {
        let content = r#"
        <script>
            let count = useState(0);
        </script>
        <button onClick={()=>{count++}}>Count {count}</button>
        "#;
        let mut visited = std::collections::HashMap::new();
        let res = process_component_tree(".", content, &mut visited, None).unwrap();
        assert!(res.state_vars.contains(&"count".to_string()));
        let combined = res.scripts.join("\n");
        assert!(combined.contains("useState(0, \"count\")"));
        assert!(combined.contains("count.value++"));
    }
}

