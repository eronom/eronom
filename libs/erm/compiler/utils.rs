use std::collections::HashMap;
use std::sync::OnceLock;
use crate::eval;

pub fn get_re_attr_brace() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"^([A-Za-z0-9_-]+)=\{"#).unwrap())
}

pub fn get_re_state() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"let\s+([A-Za-z0-9_]+)\s*=\s*useState\("#).unwrap())
}

pub fn get_re_import_named() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"import\s*\{\s*([A-Za-z0-9_,\s]+)\s*\}\s*from\s+['"]([^'"]*)['"]\s*;?"#).unwrap())
}

pub fn get_re_import_default() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"import\s+([A-Za-z0-9_]+)\s+from\s+['"]([^'"]*)['"]\s*;?"#).unwrap())
}

pub fn get_re_export() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"^(\s*)export\s+"#).unwrap())
}

pub fn file_exists_or_vfs(path: &std::path::Path) -> bool {
    if path.exists() {
        return true;
    }
    let s = path.to_string_lossy();
    crate::vm::embedded::has_vfs_file(&s)
}

pub fn read_file_or_vfs(path: &std::path::Path) -> anyhow::Result<String> {
    if path.exists() && path.is_file() {
        return Ok(std::fs::read_to_string(path)?);
    }
    let s = path.to_string_lossy();
    if let Some(text) = crate::vm::embedded::get_vfs_text(&s) {
        return Ok(text);
    }
    anyhow::bail!("File not found on disk or in VFS: {}", path.display())
}

pub fn resolve_import_path(base_dir: &str, import_path: &str) -> Option<String> {
    let path_buf = if import_path.starts_with("@pages/") {
        let mut curr = std::path::PathBuf::from(base_dir);
        let mut resolved = None;
        loop {
            let pages_dir = curr.join("app").join("pages");
            if pages_dir.exists() {
                resolved = Some(pages_dir.join(&import_path["@pages/".len()..]));
                break;
            }
            let pages_dir_alt = curr.join("pages");
            if pages_dir_alt.exists() && curr.join("eronom.toml").exists() {
                resolved = Some(pages_dir_alt.join(&import_path["@pages/".len()..]));
                break;
            }
            if let Some(parent) = curr.parent() {
                curr = parent.to_path_buf();
            } else {
                break;
            }
        }
        resolved
    } else if import_path.starts_with("@components/") {
        let mut curr = std::path::PathBuf::from(base_dir);
        let mut resolved = None;
        loop {
            let comp_dir = curr.join("app").join("components");
            if comp_dir.exists() {
                resolved = Some(comp_dir.join(&import_path["@components/".len()..]));
                break;
            }
            let comp_dir_alt = curr.join("components");
            if comp_dir_alt.exists() && curr.join("eronom.toml").exists() {
                resolved = Some(comp_dir_alt.join(&import_path["@components/".len()..]));
                break;
            }
            if let Some(parent) = curr.parent() {
                curr = parent.to_path_buf();
            } else {
                break;
            }
        }
        resolved
    } else {
        let p = std::path::PathBuf::from(base_dir).join(import_path);
        Some(p)
    };

    path_buf.map(|p| std::fs::canonicalize(&p).unwrap_or(p).to_string_lossy().into_owned())
}

pub fn parse_tag_attributes(tag_content: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    static RE_ATTR: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE_ATTR.get_or_init(|| {
        regex::Regex::new(r#"([a-zA-Z0-9_\-]+)\s*=\s*(?:\{([^{}]+)\}|"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap()
    });
    for cap in re.captures_iter(tag_content) {
        let name = cap.get(1).unwrap().as_str().to_string();
        let val = if let Some(expr) = cap.get(2) {
            format!("() => ({})", expr.as_str())
        } else if let Some(dq) = cap.get(3) {
            format!("() => \"{}\"", dq.as_str())
        } else if let Some(sq) = cap.get(4) {
            format!("() => '{}'", sq.as_str())
        } else if let Some(raw) = cap.get(5) {
            format!("() => ({})", raw.as_str())
        } else {
            "() => null".to_string()
        };
        attrs.insert(name, val);
    }
    attrs
}

pub fn parse_raw_attributes(tag_content: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let bytes = tag_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // Skip the tag name (first word)
    while i < len && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' && bytes[i] != b'>' {
        i += 1;
    }

    while i < len {
        // Skip whitespace
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= len || bytes[i] == b'/' || bytes[i] == b'>' {
            break;
        }

        // Read attribute name
        let name_start = i;
        while i < len && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' && bytes[i] != b'/' && bytes[i] != b'>' {
            i += 1;
        }
        let name = tag_content[name_start..i].trim().to_string();
        if name.is_empty() || name == "/" {
            continue;
        }

        // Skip whitespace before '='
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }

        if i < len && bytes[i] == b'=' {
            i += 1; // skip '='
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= len {
                attrs.insert(name, String::new());
                break;
            }

            if bytes[i] == b'"' {
                i += 1; // skip opening quote
                let val_start = i;
                while i < len && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                let val = &tag_content[val_start..i];
                if i < len && bytes[i] == b'"' {
                    i += 1; // skip closing quote
                }
                attrs.insert(name, val.to_string());
            } else if bytes[i] == b'\'' {
                i += 1; // skip opening quote
                let val_start = i;
                while i < len && bytes[i] != b'\'' {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                let val = &tag_content[val_start..i];
                if i < len && bytes[i] == b'\'' {
                    i += 1; // skip closing quote
                }
                attrs.insert(name, val.to_string());
            } else if bytes[i] == b'{' {
                let val_start = i;
                let mut depth = 1;
                i += 1;
                while i < len && depth > 0 {
                    if bytes[i] == b'{' {
                        depth += 1;
                    } else if bytes[i] == b'}' {
                        depth -= 1;
                    } else if bytes[i] == b'"' {
                        i += 1;
                        while i < len && bytes[i] != b'"' {
                            if bytes[i] == b'\\' && i + 1 < len {
                                i += 2;
                            } else {
                                i += 1;
                            }
                        }
                    } else if bytes[i] == b'\'' {
                        i += 1;
                        while i < len && bytes[i] != b'\'' {
                            if bytes[i] == b'\\' && i + 1 < len {
                                i += 2;
                            } else {
                                i += 1;
                            }
                        }
                    }
                    if i < len {
                        i += 1;
                    }
                }
                let val = &tag_content[val_start..i];
                attrs.insert(name, val.to_string());
            } else {
                let val_start = i;
                while i < len && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' && bytes[i] != b'>' {
                    i += 1;
                }
                let val = &tag_content[val_start..i];
                attrs.insert(name, val.to_string());
            }
        } else {
            // Boolean attribute without '='
            attrs.insert(name, String::new());
        }
    }

    attrs
}

pub fn scope_component_ids(html: &mut String, scripts: &mut Vec<String>) {
    static RE_ID: OnceLock<regex::Regex> = OnceLock::new();
    let re_id = RE_ID.get_or_init(|| regex::Regex::new(r#"id\s*=\s*["']([^"']+)["']"#).unwrap());
    let mut ids = Vec::new();
    for cap in re_id.captures_iter(html) {
        let id_val = cap.get(1).unwrap().as_str().to_string();
        if !ids.contains(&id_val) {
            ids.push(id_val);
        }
    }
    for id in &ids {
        let old_pattern_dq = format!("id=\"{}\"", id);
        let new_pattern_dq = format!("id=\"__erm_anchor_id_prefix__{}\"", id);
        *html = html.replace(&old_pattern_dq, &new_pattern_dq);
        let old_pattern_sq = format!("id='{}'", id);
        let new_pattern_sq = format!("id='__erm_anchor_id_prefix__{}'", id);
        *html = html.replace(&old_pattern_sq, &new_pattern_sq);
    }
    for id in &ids {
        let dq_pattern = format!("\"{}\"", id);
        let dq_replace = format!("anchorId + \"{}\"", id);
        let sq_pattern = format!("'{}'", id);
        let sq_replace = format!("anchorId + '{}'", id);
        for s in scripts.iter_mut() {
            *s = s.replace(&dq_pattern, &dq_replace).replace(&sq_pattern, &sq_replace);
        }
    }
}

pub fn inject_state_name(input: &str, name: &str, scoped_name: &str) -> String {
    let mut result = String::new();
    let mut i = 0;
    let name_pattern = format!("let {} = useState(", name);
    let name_pattern_const = format!("const {} = useState(", name);
    let name_pattern_var = format!("var {} = useState(", name);

    while i < input.len() {
        let matched_pat = if input[i..].starts_with(&name_pattern) {
            Some(&name_pattern)
        } else if input[i..].starts_with(&name_pattern_const) {
            Some(&name_pattern_const)
        } else if input[i..].starts_with(&name_pattern_var) {
            Some(&name_pattern_var)
        } else {
            None
        };

        if let Some(pat) = matched_pat {
            result.push_str(pat);
            i += pat.len();
            let mut depth = 1;
            let mut j = i;
            while j < input.len() && depth > 0 {
                let cur_c = input[j..].chars().next().unwrap();
                if cur_c == '(' { depth += 1; }
                else if cur_c == ')' { depth -= 1; }
                j += cur_c.len_utf8();
            }
            if depth == 0 {
                let init_expr = input[i..j - 1].trim();
                let mut has_top_level_comma = false;
                let mut paren_d = 0;
                let mut brace_d = 0;
                let mut bracket_d = 0;
                let mut in_str = None;
                let mut escaped = false;
                for ch in init_expr.chars() {
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if ch == '\\' {
                        escaped = true;
                        continue;
                    }
                    if let Some(quote) = in_str {
                        if ch == quote {
                            in_str = None;
                        }
                    } else if ch == '"' || ch == '\'' || ch == '`' {
                        in_str = Some(ch);
                    } else if ch == '(' {
                        paren_d += 1;
                    } else if ch == ')' {
                        if paren_d > 0 { paren_d -= 1; }
                    } else if ch == '{' {
                        brace_d += 1;
                    } else if ch == '}' {
                        if brace_d > 0 { brace_d -= 1; }
                    } else if ch == '[' {
                        bracket_d += 1;
                    } else if ch == ']' {
                        if bracket_d > 0 { bracket_d -= 1; }
                    } else if ch == ',' && paren_d == 0 && brace_d == 0 && bracket_d == 0 {
                        has_top_level_comma = true;
                        break;
                    }
                }
                if has_top_level_comma {
                    result.push_str(init_expr);
                } else {
                    result.push_str(&format!("{}, \"{}\"", init_expr, scoped_name));
                }
                result.push(')');
                i = j;
                continue;
            }
        }
        let c = input[i..].chars().next().unwrap();
        result.push(c);
        i += c.len_utf8();
    }
    result
}

pub fn find_state_variables(script_content: &str, init_fn: &str, state_vars: &mut Vec<String>) {
    let mut s_idx = 0;
    while let Some(v_idx) = script_content[s_idx..].find(init_fn) {
        let before_use = &script_content[..s_idx + v_idx];
        if let Some(eq_pos) = before_use.rfind('=') {
            let before_eq = &before_use[..eq_pos].trim_end();
            if let Some(space_pos) = before_eq.rfind(|c: char| c.is_whitespace() || c == ';') {
                let var_name = before_eq[space_pos + 1..].trim();
                if !var_name.is_empty() && !state_vars.contains(&var_name.to_string()) {
                    state_vars.push(var_name.to_string());
                }
            } else if !before_eq.is_empty() && !state_vars.contains(&before_eq.to_string()) {
                state_vars.push(before_eq.to_string());
            }
        }
        s_idx += v_idx + init_fn.len();
    }
}

pub fn extract_attribute(tag_content: &str, attr_name: &str) -> Option<String> {
    for part in tag_content.split_whitespace() {
        if part.starts_with(&format!("{}=", attr_name)) {
            let val_part = &part[attr_name.len() + 1..];
            if val_part.starts_with('{') {
                let mut depth = 0;
                let mut end_idx = None;
                for (i, c) in val_part.chars().enumerate() {
                    if c == '{' { depth += 1; }
                    else if c == '}' {
                        depth -= 1;
                        if depth == 0 {
                            end_idx = Some(i);
                            break;
                        }
                    }
                }
                if let Some(e) = end_idx {
                    return Some(val_part[1..e].trim().to_string());
                }
            } else if val_part.starts_with('"') {
                if let Some(end) = val_part[1..].find('"') {
                    return Some(val_part[1..end + 1].to_string());
                }
            } else if val_part.starts_with('\'') {
                if let Some(end) = val_part[1..].find('\'') {
                    return Some(val_part[1..end + 1].to_string());
                }
            } else {
                let end = val_part.find(char::is_whitespace).unwrap_or(val_part.len());
                return Some(val_part[..end].to_string());
            }
        }
    }
    if let Some(pos) = tag_content.find(&format!("{}=", attr_name)) {
        let val_part = tag_content[pos + attr_name.len() + 1..].trim_start();
        if val_part.starts_with('{') {
            let mut depth = 0;
            let mut end_idx = None;
            for (i, c) in val_part.chars().enumerate() {
                if c == '{' { depth += 1; }
                else if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        end_idx = Some(i);
                        break;
                    }
                }
            }
            if let Some(e) = end_idx {
                return Some(val_part[1..e].trim().to_string());
            }
        } else if val_part.starts_with('"') {
            if let Some(end) = val_part[1..].find('"') {
                return Some(val_part[1..end + 1].to_string());
            }
        } else if val_part.starts_with('\'') {
            if let Some(end) = val_part[1..].find('\'') {
                return Some(val_part[1..end + 1].to_string());
            }
        } else {
            let end = val_part.find(char::is_whitespace).unwrap_or(val_part.len());
            return Some(val_part[..end].to_string());
        }
    }
    None
}

pub fn find_matching_close_brace(s: &str) -> Option<usize> {
    let mut depth = 1;
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c == '{' {
            depth += 1;
            i += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
            i += 1;
        } else {
            i += c.len_utf8();
        }
    }
    None
}

pub fn evaluate_braces_in_html(html: &str, ev: &mut eval::ErmEval, state_vars: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < html.len() {
        let c = html[i..].chars().next().unwrap();
        if c == '{' {
            if let Some(close_idx) = find_matching_close_brace(&html[i + 1..]) {
                let brace_end = i + 1 + close_idx;
                let raw_expr = &html[i + 1..brace_end];
                let sub_expr = crate::frontend::transpile_expr_reactivity(raw_expr, state_vars)
                    .unwrap_or_else(|| raw_expr.to_string());
                
                let prefix = &html[..i];
                if let Some(_event_type) = super::reactivity::get_event_attribute_name(prefix) {
                    let mut temp_out = out.trim_end().to_string();
                    if temp_out.ends_with('=') {
                        temp_out.pop();
                        let temp_out_trimmed = temp_out.trim_end().to_string();
                        let trimmed_prefix = prefix.trim_end();
                        let name_part = &trimmed_prefix[..trimmed_prefix.len() - 1].trim_end();
                        if let Some(last_word) = name_part.split(|c: char| c.is_ascii_whitespace() || c == '<' || c == '>').last() {
                            if temp_out_trimmed.ends_with(last_word) {
                                out = temp_out_trimmed[..temp_out_trimmed.len() - last_word.len()].to_string();
                            } else {
                                out = temp_out_trimmed;
                            }
                        } else {
                            out = temp_out_trimmed;
                        }
                    }
                } else {
                    let val = ev.eval(&sub_expr).unwrap_or(eval::Value::Null);
                    if val != eval::Value::Null {
                        out.push_str(&val.to_string());
                    }
                }
                i = brace_end + 1;
            } else {
                out.push(c);
                i += c.len_utf8();
            }
        } else {
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

pub fn wrap_unquoted_braces(html: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let mut in_tag = false;
    let mut in_quote = None;

    while i < html.len() {
        let c = html[i..].chars().next().unwrap();

        if !in_tag {
            if c == '<' {
                in_tag = true;
                in_quote = None;
            }
            out.push(c);
            i += c.len_utf8();
        } else {
            if c == '>' && in_quote.is_none() {
                in_tag = false;
                out.push(c);
                i += c.len_utf8();
                continue;
            }

            if c == '"' || c == '\'' {
                if let Some(q) = in_quote {
                    if q == c {
                        in_quote = None;
                    }
                } else {
                    in_quote = Some(c);
                }
                out.push(c);
                i += c.len_utf8();
                continue;
            }

            if in_quote.is_none() && i > 0 && html[i-1..i].chars().next().unwrap().is_ascii_whitespace() && !html[i..].starts_with("on") {
                if let Some(caps) = get_re_attr_brace().captures(&html[i..]) {
                    let attr_name = caps.get(1).unwrap().as_str();
                    let start_expr = i + attr_name.len() + 2;
                    let mut depth = 1;
                    let mut j = start_expr;
                    while j < html.len() && depth > 0 {
                        let cur_c = html[j..].chars().next().unwrap();
                        if cur_c == '{' { depth += 1; }
                        else if cur_c == '}' { depth -= 1; }
                        j += cur_c.len_utf8();
                    }
                    if depth == 0 {
                        let expr = &html[start_expr..j-1];
                        out.push_str(&format!("{}=\"{{{}}}\" ", attr_name, expr));
                        i = j;
                        continue;
                    }
                }
            }

            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}
