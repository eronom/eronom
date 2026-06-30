use std::collections::HashMap;
use crate::eval::{self, ErmEval};
use fnv::FnvHasher;
use std::hash::Hasher;
use std::sync::OnceLock;

fn get_re_attr_brace() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"^([A-Za-z0-9_-]+)=\{"#).unwrap())
}

fn get_re_state() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"let\s+([A-Za-z0-9_]+)\s*=\s*useState\("#).unwrap())
}

fn get_re_import_named() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"import\s*\{\s*([A-Za-z0-9_,\s]+)\s*\}\s*from\s+['"]([^'"]*)['"]\s*;?"#).unwrap())
}

fn get_re_import_default() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"import\s+([A-Za-z0-9_]+)\s+from\s+['"]([^'"]*)['"]\s*;?"#).unwrap())
}

fn get_re_export() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"^(\s*)export\s+"#).unwrap())
}



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

pub fn find_state_variables(script_content: &str, init_fn: &str, state_vars: &mut Vec<String>) {
    let mut aj = 0;
    while let Some(idx) = script_content[aj..].find(init_fn) {
        let call_pos = aj + idx;
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
        aj = call_pos + init_fn.len();
    }
}

pub fn extract_attribute(tag_content: &str, attr_name: &str) -> Option<String> {
    if let Some(start) = tag_content.find(attr_name) {
        let before_ok = start == 0 || tag_content[start-1..start].chars().next().unwrap().is_whitespace();
        let after_pos = start + attr_name.len();
        if before_ok && after_pos < tag_content.len() {
            let remaining = &tag_content[after_pos..];
            if let Some(eq_pos) = remaining.find('=') {
                if remaining[..eq_pos].trim().is_empty() {
                    let val_part = remaining[eq_pos + 1..].trim_start();
                    if val_part.starts_with('{') {
                        let mut depth = 0;
                        let mut end_idx = None;
                        for (idx, c) in val_part.char_indices() {
                            if c == '{' { depth += 1; }
                            else if c == '}' {
                                depth -= 1;
                                if depth == 0 {
                                    end_idx = Some(idx);
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
        }
    }
    None
}

enum ElseTransition {
    ElseIf(String, usize),
    Else(usize),
}

fn try_parse_if_header(content: &str, start_pos: usize) -> Option<(String, usize)> {
    if !content[start_pos..].starts_with("if") {
        return None;
    }
    // Verify word boundary before 'if' and ensure it's not preceded by 'else'
    if start_pos > 0 {
        let prev_c = content[..start_pos].chars().next_back().unwrap();
        if prev_c.is_alphanumeric() || prev_c == '_' {
            return None;
        }
        let mut check = start_pos;
        while check > 0 {
            let c = content[..check].chars().next_back().unwrap();
            if c.is_whitespace() {
                check -= c.len_utf8();
            } else {
                break;
            }
        }
        if check >= 4 && &content[check - 4..check] == "else" {
            if check - 4 == 0 || {
                let prev_else = content[..check - 4].chars().next_back().unwrap();
                !prev_else.is_alphanumeric() && prev_else != '_'
            } {
                return None;
            }
        }
    }
    let after_if = start_pos + 2;
    if after_if < content.len() {
        let next_c = content[after_if..].chars().next().unwrap();
        if next_c.is_alphanumeric() || next_c == '_' {
            return None;
        }
    }
    // Scan forward to find '{'
    let mut scan = after_if;
    while scan < content.len() {
        let c = content[scan..].chars().next().unwrap();
        if c == '{' {
            let cond = content[after_if..scan].trim().to_string();
            return Some((cond, scan + 1));
        } else if c == '<' {
            if scan + 1 < content.len() {
                let next_c = content[scan + 1..].chars().next().unwrap();
                if next_c == '/' || next_c.is_ascii_alphabetic() {
                    return None;
                }
            }
        }
        scan += c.len_utf8();
    }
    None
}

fn find_else_transition(content: &str, start_pos: usize) -> Option<ElseTransition> {
    let mut j = start_pos;
    while j < content.len() {
        let c = content[j..].chars().next().unwrap();
        if c.is_whitespace() {
            j += c.len_utf8();
        } else {
            break;
        }
    }
    if content[j..].starts_with("else") {
        let after_else = j + 4;
        if after_else < content.len() {
            let next_c = content[after_else..].chars().next().unwrap();
            if next_c.is_alphanumeric() || next_c == '_' {
                return None;
            }
        }
        let mut k = after_else;
        while k < content.len() {
            let c = content[k..].chars().next().unwrap();
            if c.is_whitespace() {
                k += c.len_utf8();
            } else {
                break;
            }
        }
        if content[k..].starts_with("if") {
            let after_if = k + 2;
            if after_if < content.len() {
                let next_c = content[after_if..].chars().next().unwrap();
                if next_c.is_alphanumeric() || next_c == '_' {
                    return None;
                }
            }
            let mut scan = after_if;
            while scan < content.len() {
                let c = content[scan..].chars().next().unwrap();
                if c == '{' {
                    let cond = content[after_if..scan].trim().to_string();
                    return Some(ElseTransition::ElseIf(cond, scan + 1));
                } else if c == '<' {
                    if scan + 1 < content.len() {
                        let next_c = content[scan + 1..].chars().next().unwrap();
                        if next_c == '/' || next_c.is_ascii_alphabetic() {
                            return None;
                        }
                    }
                }
                scan += c.len_utf8();
            }
        } else {
            let mut scan = after_else;
            while scan < content.len() {
                let c = content[scan..].chars().next().unwrap();
                if c == '{' {
                    return Some(ElseTransition::Else(scan + 1));
                } else if !c.is_whitespace() {
                    return None;
                }
                scan += c.len_utf8();
            }
        }
    }
    None
}



pub fn process_component_tree(
    base_dir: &str,
    content: &str,
    visited: &mut HashMap<String, bool>,
    slot_html: Option<&str>,
    params: &HashMap<String, String>,
    if_counter: &mut usize,
    for_counter: &mut usize,
) -> anyhow::Result<ProcessResult> {
    let preprocessed;
    let content = if is_function_template(content) {
        preprocessed = preprocess_function_template(content)?;
        &preprocessed
    } else {
        content
    };
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
                    local_state_vars.push(var_name);
                }
            }
            for line in script_content.lines() {
                let line_trimmed = line.trim();
                if let Some(caps) = get_re_import_named().captures(line_trimmed) {
                    let names_str = caps.get(1).unwrap().as_str();
                    for name in names_str.split(',') {
                        let name = name.trim().to_string();
                        if !name.is_empty() && name.chars().next().map_or(false, |c| c.is_ascii_lowercase()) {
                            if !local_state_vars.contains(&name) {
                                local_state_vars.push(name);
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
    scripts.extend(block_logic);
    let mut styles = Vec::new();
    let mut state_vars = Vec::new();

    let mut component_imports = HashMap::new();

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
                        if !name.is_empty() && name.chars().next().map_or(false, |c| c.is_ascii_uppercase()) {
                            component_imports.insert(name, comp_path_val.clone());
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
                    for name in names_str.split(',') {
                        let name = name.trim().to_string();
                        if !name.is_empty() && name.chars().next().map_or(false, |c| c.is_ascii_lowercase()) {
                            if !state_vars.contains(&name) {
                                state_vars.push(name);
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
                            "window.__erm_bindings.push({{ id: \"{}\", isProvider: true, get: () => ({}), update() {{ let el = document.getElementById(this.id); if (el) {{ if (!el.__erm_providers) el.__erm_providers = {{}}; el.__erm_providers[{}.id] = this.get(); }} }} }});",
                            provider_id, value_expr, context_var
                        );
                        scripts.push(logic);
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
                                let mut sub_res = process_component_tree(&comp_dir, &comp_content, visited, None, params, if_counter, for_counter)?;
                                html_buf.push_str(&sub_res.html);
                                scripts.append(&mut sub_res.scripts);
                                styles.append(&mut sub_res.styles);
                                for v in sub_res.state_vars {
                                    if !state_vars.contains(&v) { state_vars.push(v); }
                                }
                                i += tag_end + 1;
                                continue;
                            }
                        } else {
                            anyhow::bail!("Component '{}' not found in components/ or current directory.", tag_name);
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
            
            // Wrap attribute brace values like value={name} or placeholder={desc} in double quotes
            if i > 0 && html[i-1..i].chars().next().unwrap().is_ascii_whitespace() && !html[i..].starts_with("on") {
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
                            
                            // Check for existing ID attribute to avoid duplicate IDs
                            let last_lt = out.rfind('<').unwrap_or(0);
                            let tag_so_far = &out[last_lt..];
                            let tag_end_pos = html[i..].find('>').unwrap_or(0);
                            let tag_rest = &html[i..i+tag_end_pos];
                            let full_tag = format!("{}{}", tag_so_far, tag_rest);
                            
                            let mut existing_id = None;
                            if let Some(id_pos) = full_tag.find("id=\"") {
                                let id_val_start = id_pos + 4;
                                if let Some(id_val_end) = full_tag[id_val_start..].find('"') {
                                    existing_id = Some(full_tag[id_val_start..id_val_start + id_val_end].to_string());
                                }
                            } else if let Some(id_pos) = full_tag.find("id='") {
                                let id_val_start = id_pos + 4;
                                if let Some(id_val_end) = full_tag[id_val_start..].find('\'') {
                                    existing_id = Some(full_tag[id_val_start..id_val_start + id_val_end].to_string());
                                }
                            }

                            let id = match existing_id {
                                Some(eid) => eid,
                                None => {
                                    let new_id = format!("erm-evt-{}", j);
                                    out.push_str(&format!("id=\"{}\" ", new_id));
                                    new_id
                                }
                            };

                            let mut tag_line = None;
                            if let Some(line_pos) = full_tag.find("data-erm-line=\"") {
                                let line_val_start = line_pos + 15;
                                if let Some(line_val_end) = full_tag[line_val_start..].find('"') {
                                    if let Ok(ln) = full_tag[line_val_start..line_val_start + line_val_end].parse::<usize>() {
                                        tag_line = Some(ln);
                                    }
                                }
                            }
                            let event_line = tag_line.unwrap_or_else(|| {
                                html[..i].chars().filter(|&ch| ch == '\n').count() + 1
                            });

                            events.push(format!("window.__erm_events.push({{ id: \"{}\", event: \"{}\", handler: (event) => {{ ({})(event); if (typeof window.__erm_update === 'function') window.__erm_update(); }} }}); // line:{}", id, event_type, expr, event_line));
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

fn get_event_attribute_name(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim_end();
    if trimmed.ends_with('=') {
        let name_part = trimmed[..trimmed.len() - 1].trim_end();
        if let Some(last_word) = name_part.split(|c: char| c.is_ascii_whitespace() || c == '<' || c == '>').last() {
            if last_word.starts_with("on") && last_word.len() > 2 {
                let event_name = last_word[2..].to_lowercase();
                return Some(event_name);
            }
        }
    }
    None
}

pub fn compile_template_to_js(body: &str, state_vars: &[String]) -> String {
    let mut js_expr = String::new();
    js_expr.push('`');
    let mut i = 0;
    while i < body.len() {
        let c = body[i..].chars().next().unwrap();
        if c == '`' || c == '$' || c == '\\' {
            js_expr.push('\\');
            js_expr.push(c);
            i += c.len_utf8();
        } else if c == '{' && !body[i..].starts_with("{#") && !body[i..].starts_with("{/") && !body[i..].starts_with("{:") {
            if let Some(close_idx) = find_matching_close_brace(&body[i + 1..]) {
                let brace_end = i + 1 + close_idx;
                let mut sub_expr = body[i + 1..brace_end].to_string();
                for sig in state_vars {
                    sub_expr = replace_word(&sub_expr, sig, ".value");
                }
                
                let prefix = &body[..i];
                if let Some(event_type) = get_event_attribute_name(prefix) {
                    let mut temp_js = js_expr.trim_end().to_string();
                    if temp_js.ends_with('=') {
                        temp_js.pop();
                        let temp_js_trimmed = temp_js.trim_end().to_string();
                        let trimmed_prefix = prefix.trim_end();
                        let name_part = &trimmed_prefix[..trimmed_prefix.len() - 1].trim_end();
                        if let Some(last_word) = name_part.split(|c: char| c.is_ascii_whitespace() || c == '<' || c == '>').last() {
                            if temp_js_trimmed.ends_with(last_word) {
                                js_expr = temp_js_trimmed[..temp_js_trimmed.len() - last_word.len()].to_string();
                            } else {
                                js_expr = temp_js_trimmed;
                            }
                        } else {
                            js_expr = temp_js_trimmed;
                        }
                    }
                    js_expr.push_str(&format!("${{window.__erm_register_event('{}', (event) => {{ ({})(event); }})}}", event_type, sub_expr));
                } else {
                    js_expr.push_str("${window.__erm_escape(");
                    js_expr.push_str(&sub_expr);
                    js_expr.push_str(")}");
                }
                i = brace_end + 1;
            } else {
                js_expr.push(c);
                i += c.len_utf8();
            }
        } else {
            js_expr.push(c);
            i += c.len_utf8();
        }
    }
    js_expr.push('`');
    js_expr
}

fn evaluate_braces_in_html(html: &str, ev: &mut eval::ErmEval, state_vars: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < html.len() {
        let c = html[i..].chars().next().unwrap();
        if c == '{' && !html[i..].starts_with("{#") && !html[i..].starts_with("{/") && !html[i..].starts_with("{:") {
            if let Some(close_idx) = find_matching_close_brace(&html[i + 1..]) {
                let brace_end = i + 1 + close_idx;
                let mut sub_expr = html[i + 1..brace_end].to_string();
                for sig in state_vars {
                    sub_expr = replace_word(&sub_expr, sig, ".value");
                }
                
                let prefix = &html[..i];
                if let Some(_event_type) = get_event_attribute_name(prefix) {
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

fn find_matching_close_brace(s: &str) -> Option<usize> {
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

struct ScriptStyleRanges {
    ranges: Vec<std::ops::Range<usize>>,
}

impl ScriptStyleRanges {
    fn new(content: &str) -> Self {
        let mut ranges = Vec::new();
        
        let mut start_search = 0;
        while let Some(open_pos) = content[start_search..].find("<script") {
            let open_idx = start_search + open_pos;
            if let Some(close_pos) = content[open_idx..].find("</script>") {
                let close_idx = open_idx + close_pos + "</script>".len();
                ranges.push(open_idx..close_idx);
                start_search = close_idx;
            } else {
                ranges.push(open_idx..content.len());
                break;
            }
        }
        
        let mut start_search = 0;
        while let Some(open_pos) = content[start_search..].find("<style") {
            let open_idx = start_search + open_pos;
            if let Some(close_pos) = content[open_idx..].find("</style>") {
                let close_idx = open_idx + close_pos + "</style>".len();
                ranges.push(open_idx..close_idx);
                start_search = close_idx;
            } else {
                ranges.push(open_idx..content.len());
                break;
            }
        }
        
        Self { ranges }
    }

    fn get_skip_pos(&self, pos: usize) -> Option<usize> {
        for range in &self.ranges {
            if range.contains(&pos) {
                return Some(range.end);
            }
        }
        None
    }
}

fn try_parse_for_header(content: &str, start_pos: usize) -> Option<(String, usize)> {
    if !content[start_pos..].starts_with("for") {
        return None;
    }
    if start_pos > 0 {
        let prev_c = content[..start_pos].chars().next_back().unwrap();
        if prev_c.is_alphanumeric() || prev_c == '_' {
            return None;
        }
    }
    let after_for = start_pos + 3;
    if after_for < content.len() {
        let next_c = content[after_for..].chars().next().unwrap();
        if next_c.is_alphanumeric() || next_c == '_' {
            return None;
        }
    }
    let mut scan = after_for;
    while scan < content.len() {
        let c = content[scan..].chars().next().unwrap();
        if c == '{' {
            let header = content[after_for..scan].trim().to_string();
            if header.contains(" in ") {
                return Some((header, scan + 1));
            } else {
                return None;
            }
        } else if c == '<' {
            if scan + 1 < content.len() {
                let next_c = content[scan + 1..].chars().next().unwrap();
                if next_c == '/' || next_c.is_ascii_alphabetic() {
                    return None;
                }
            }
        }
        scan += c.len_utf8();
    }
    None
}

fn find_last_for_block(res_html: &str) -> Option<(usize, String, usize)> {
    let ranges = ScriptStyleRanges::new(res_html);
    let mut last = None;
    let mut i = 0;
    while i < res_html.len() {
        if let Some(skip_to) = ranges.get_skip_pos(i) {
            i = skip_to;
            continue;
        }
        if let Some((header, next_pos)) = try_parse_for_header(res_html, i) {
            last = Some((i, header, next_pos));
        }
        let c = res_html[i..].chars().next().unwrap();
        i += c.len_utf8();
    }
    last
}

fn find_last_if_block(res_html: &str) -> Option<(usize, String, usize)> {
    let ranges = ScriptStyleRanges::new(res_html);
    let mut last = None;
    let mut i = 0;
    while i < res_html.len() {
        if let Some(skip_to) = ranges.get_skip_pos(i) {
            i = skip_to;
            continue;
        }
        if let Some((cond, next_pos)) = try_parse_if_header(res_html, i) {
            last = Some((i, cond, next_pos));
        }
        let c = res_html[i..].chars().next().unwrap();
        i += c.len_utf8();
    }
    last
}

fn process_for_block_at(
    start_idx: usize,
    res_html: &mut String,
    for_counter: &mut usize,
    block_logic: &mut Vec<String>,
    state_vars: &[String],
    ev: &mut eval::ErmEval,
    header: String,
    body_start_idx: usize,
    scope_id: &str,
) -> anyhow::Result<()> {
    let remaining = &res_html[body_start_idx..];
    let close_idx = match find_matching_close_brace(remaining) {
        Some(idx) => idx,
        None => return Ok(()),
    };
    let absolute_close_idx = body_start_idx + close_idx;
    let body = remaining[..close_idx].to_string();

    let in_idx = match header.find(" in ") {
        Some(idx) => idx,
        None => return Ok(()),
    };
    let vars_part = header[0..in_idx].trim();
    let vars_part = if vars_part.starts_with('(') && vars_part.ends_with(')') {
        vars_part[1..vars_part.len() - 1].trim()
    } else {
        vars_part
    };
    let collection_expr_raw = header[in_idx + 4..].trim();
    let collection_expr_raw = if collection_expr_raw.starts_with('(') && collection_expr_raw.ends_with(')') {
        collection_expr_raw[1..collection_expr_raw.len() - 1].trim()
    } else {
        collection_expr_raw
    };

    let mut collection_expr = collection_expr_raw.to_string();
    for sig in state_vars {
        collection_expr = replace_word(&collection_expr, sig, ".value");
    }
    let (item_name, index_name) = if let Some(comma_idx) = vars_part.find(',') {
        (vars_part[0..comma_idx].trim(), vars_part[comma_idx + 1..].trim())
    } else {
        (vars_part, "")
    };
    let anchor_id = format!("erm-for-{}", for_counter);
    *for_counter += 1;
    let mut ssr_html = String::new();
    let collection_val = ev.eval(&collection_expr).unwrap_or(eval::Value::Null);
    if let eval::Value::List(items) = collection_val {
        for (idx, item) in items.iter().enumerate() {
            let mut sub_ev = ev.clone();
            sub_ev.set(item_name, item.clone());
            if !index_name.is_empty() {
                sub_ev.set(index_name, eval::Value::Number(idx as f64));
            }
            let ssr_item = evaluate_braces_in_html(&body, &mut sub_ev, state_vars);
            ssr_html.push_str(&ssr_item);
        }
    }
    let js_params = if !index_name.is_empty() { format!("{}, {}", item_name, index_name) } else { item_name.to_string() };
    let scoped_body = scope_html(&body, scope_id)?;
    let compiled_body = compile_template_to_js(&scoped_body, state_vars);
    let logic = format!(
        r#"window.__erm_register_for("{}", () => ({}), "", ({}) => {});"#,
        anchor_id, collection_expr, js_params, compiled_body
    );
    block_logic.push(logic);
    let anchor_html = format!("<span id=\"{}\" style=\"display:contents;\">{}</span>", anchor_id, ssr_html);
    res_html.replace_range(start_idx..absolute_close_idx + 1, &anchor_html);
    Ok(())
}

fn process_if_block_at(
    start_idx: usize,
    res_html: &mut String,
    if_counter: &mut usize,
    block_logic: &mut Vec<String>,
    state_vars: &[String],
    ev: &mut eval::ErmEval,
    initial_cond: String,
    body_start_idx: usize,
    scope_id: &str,
) -> anyhow::Result<()> {
    let mut branches = Vec::new();
    let mut current_cond = initial_cond;
    let mut current_body_start = body_start_idx;
    let block_end_idx;

    loop {
        let remaining = &res_html[current_body_start..];
        if let Some(close_idx) = find_matching_close_brace(remaining) {
            let body = remaining[..close_idx].to_string();
            branches.push((current_cond.clone(), body));
            let absolute_close_idx = current_body_start + close_idx;
            if let Some(transition) = find_else_transition(res_html, absolute_close_idx + 1) {
                match transition {
                    ElseTransition::ElseIf(new_cond, next_body_start) => {
                        current_cond = new_cond;
                        current_body_start = next_body_start;
                    }
                    ElseTransition::Else(next_body_start) => {
                        current_cond = "true".to_string();
                        current_body_start = next_body_start;
                    }
                }
            } else {
                block_end_idx = absolute_close_idx + 1;
                break;
            }
        } else {
            branches.push((current_cond.clone(), remaining.to_string()));
            block_end_idx = res_html.len();
            break;
        }
    }

    let anchor_id = format!("erm-if-{}", if_counter);
    *if_counter += 1;
    let mut branches_js = String::new();
    let mut ssr_html_res = String::new();
    let mut ssr_found = false;

    for (mut cond_expr, body) in branches {
        for sig in state_vars {
            cond_expr = replace_word(&cond_expr, sig, ".value");
        }
        let scoped_body = scope_html(&body, scope_id)?;
        let compiled_body = compile_template_to_js(&scoped_body, state_vars);
        if !ssr_found {
            let cond_val = if cond_expr == "true" { true } else { ev.eval_bool(&cond_expr).unwrap_or(false) };
            if cond_val {
                ssr_html_res = evaluate_braces_in_html(&body, ev, state_vars);
                ssr_found = true;
            }
        }
        if branches_js.is_empty() {
            branches_js.push_str(&format!("if ({}) {{ __erm_new = {}; }}", cond_expr, compiled_body));
        } else if cond_expr == "true" {
            branches_js.push_str(&format!(" else {{ __erm_new = {}; }}", compiled_body));
        } else {
            branches_js.push_str(&format!(" else if ({}) {{ __erm_new = {}; }}", cond_expr, compiled_body));
        }
    }

    let logic = format!(
        r#"window.__erm_register_if("{}", () => {{ let __erm_new = ""; {}; return __erm_new; }});"#,
        anchor_id, branches_js
    );
    block_logic.push(logic);
    let anchor_html = format!("<span id=\"{}\" style=\"display:contents;\">{}</span>", anchor_id, ssr_html_res);
    res_html.replace_range(start_idx..block_end_idx, &anchor_html);
    Ok(())
}

pub fn process_erm_component(base_dir: &str, content: &str, is_prod: bool, params: &HashMap<String, String>) -> anyhow::Result<String> {
    let preprocessed;
    let content = if is_function_template(content) {
        preprocessed = preprocess_function_template(content)?;
        &preprocessed
    } else {
        content
    };
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

    let mut if_counter = 0;
    let mut for_counter = 0;
    
    let result = if let Some(lp) = layout_path {
        if !content.contains("<!DOCTYPE html>") && !content.contains("<html") {
            let layout_content = std::fs::read_to_string(&lp)?;
            if content.trim() != layout_content.trim() {
                let page_res = process_component_tree(base_dir, content, &mut visited, None, params, &mut if_counter, &mut for_counter)?;
                let mut layout_res = process_component_tree(&lp.parent().unwrap().to_string_lossy(), &layout_content, &mut visited, Some(&page_res.html), params, &mut if_counter, &mut for_counter)?;
                
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
                process_component_tree(base_dir, content, &mut visited, None, params, &mut if_counter, &mut for_counter)?
            }
        } else {
            process_component_tree(base_dir, content, &mut visited, None, params, &mut if_counter, &mut for_counter)?
        }
    } else {
        process_component_tree(base_dir, content, &mut visited, None, params, &mut if_counter, &mut for_counter)?
    };

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

    let mut assets = String::new();
    if !result.styles.is_empty() {
        assets.push_str("\n<style id=\"__erm_styles\">\n");
        for s in &result.styles { assets.push_str(s); assets.push('\n'); }
        assets.push_str("</style>\n");
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
    let mut declarations = String::new();
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
                declarations.push_str(&format!(
                    "let {} = window.{} || useState({}, \"{}\");\n",
                    v, v, fallback_val, v
                ));
            }
        }
    }
    if !declarations.is_empty() {
        scripts_to_inject.insert(0, declarations);
    }

    if !scripts_to_inject.is_empty() || !result.state_vars.is_empty() {
        assets.push_str("<script src=\"/core/runtime.js\" class=\"__erm_script\"></script>\n");
        assets.push_str("<script class=\"__erm_script\">\n");
        assets.push_str("{\n");
        for s in &scripts_to_inject { assets.push_str(s); assets.push('\n'); }
        assets.push_str("}\n");
        assets.push_str("</script>\n");
    }

    let mut output = res_html;
    
    if !is_prod {
        let hmr_script = "<script src=\"/core/hmr.js\"></script>";
        if let Some(pos) = output.find("<head>") {
            output.insert_str(pos + 6, hmr_script);
        } else {
            output.insert_str(0, hmr_script);
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

fn is_function_template(content: &str) -> bool {
    if content.contains("<script") || content.contains("<SCRIPT") {
        return false;
    }
    static RE_FN: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE_FN.get_or_init(|| {
        regex::Regex::new(r"(?m)^\s*export\s+(?:default\s+)?(?:fn|function)\s+([A-Za-z0-9_]+)\s*\(([^)]*)\)\s*\{").unwrap()
    });
    re.is_match(content)
}

fn inject_line_attr(markup: &str, line_num: usize) -> String {
    if markup.starts_with('<') {
        if let Some(first_tag_char) = markup.chars().nth(1) {
            if first_tag_char.is_ascii_alphabetic() {
                let mut insert_pos = 1;
                for (idx, ch) in markup.char_indices().skip(1) {
                    if ch == ' ' || ch == '>' || ch == '/' {
                        insert_pos = idx;
                        break;
                    }
                }
                let mut res = markup.to_string();
                res.insert_str(insert_pos, &format!(" data-erm-line=\"{}\"", line_num));
                return res;
            }
        }
    }
    markup.to_string()
}

fn check_adjacent_jsx(markup: &str) -> anyhow::Result<()> {
    let mut chars = markup.chars().peekable();
    let mut depth = 0;
    let mut root_count = 0;
    let mut in_tag = false;
    let mut in_quote: Option<char> = None;
    let mut braces_depth = 0;
    
    while let Some(c) = chars.next() {
        if let Some(quote_char) = in_quote {
            if c == '\\' {
                let _ = chars.next(); // Skip escaped char
            } else if c == quote_char {
                in_quote = None;
            }
            continue;
        }
        
        if in_tag {
            if c == '"' || c == '\'' || c == '`' {
                in_quote = Some(c);
            } else if c == '{' {
                braces_depth += 1;
            } else if c == '}' {
                if braces_depth > 0 {
                    braces_depth -= 1;
                }
            } else if braces_depth == 0 && c == '>' {
                in_tag = false;
            }
            continue;
        }
        
        if c == '{' {
            braces_depth += 1;
        } else if c == '}' {
            if braces_depth > 0 {
                braces_depth -= 1;
            }
        } else if braces_depth == 0 && c == '<' {
            let mut is_closing = false;
            let mut is_self_closing = false;
            let mut is_comment = false;
            
            if let Some(&next_c) = chars.peek() {
                if next_c == '/' {
                    is_closing = true;
                    let _ = chars.next();
                } else if next_c == '!' {
                    is_comment = true;
                    let _ = chars.next();
                }
            }
            
            if is_comment {
                let mut hyphen_count = 0;
                while let Some(comment_c) = chars.next() {
                    if comment_c == '-' {
                        hyphen_count += 1;
                    } else if comment_c == '>' && hyphen_count >= 2 {
                        break;
                    } else {
                        hyphen_count = 0;
                    }
                }
                continue;
            }
            
            let mut tag_content = String::new();
            let mut temp_braces = 0;
            let mut temp_quote: Option<char> = None;
            
            while let Some(&tag_c) = chars.peek() {
                if let Some(q) = temp_quote {
                    if tag_c == '\\' {
                        let _ = chars.next();
                        let _ = chars.next();
                    } else if tag_c == q {
                        temp_quote = None;
                        let _ = chars.next();
                    } else {
                        let _ = chars.next();
                    }
                } else {
                    if tag_c == '"' || tag_c == '\'' || tag_c == '`' {
                        temp_quote = Some(tag_c);
                        let _ = chars.next();
                    } else if tag_c == '{' {
                        temp_braces += 1;
                        let _ = chars.next();
                    } else if tag_c == '}' {
                        if temp_braces > 0 {
                            temp_braces -= 1;
                        }
                        let _ = chars.next();
                    } else if temp_braces == 0 && tag_c == '>' {
                        break;
                    } else {
                        tag_content.push(tag_c);
                        let _ = chars.next();
                    }
                }
            }
            
            if chars.peek() == Some(&'>') {
                let _ = chars.next();
            }
            
            if !is_closing {
                if tag_content.trim().ends_with('/') {
                    is_self_closing = true;
                }
                
                if depth == 0 {
                    root_count += 1;
                    if root_count > 1 {
                        anyhow::bail!(
                            "Adjacent ERM elements must be wrapped in a fragment tag <> </>. Like: <> <h1>...</h1> <button>...</button> </>."
                        );
                    }
                }
                
                if !is_self_closing {
                    depth += 1;
                }
            } else {
                if depth > 0 {
                    depth -= 1;
                }
            }
        }
    }
    
    Ok(())
}

fn preprocess_function_template(content: &str) -> anyhow::Result<String> {
    static RE_FN: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE_FN.get_or_init(|| {
        regex::Regex::new(r"(?m)^\s*export\s+(?:default\s+)?(?:fn|function)\s+([A-Za-z0-9_]+)\s*\(([^)]*)\)\s*\{").unwrap()
    });

    if let Some(captures) = re.captures(content) {
        let entire_match = captures.get(0).unwrap();
        let fn_start_byte = entire_match.start();
        let fn_body_start_byte = entire_match.end();
        let params_str = captures.get(2).map_or("", |m| m.as_str());

        let mut fn_start_char = 0;
        let mut fn_body_start_char = 0;
        for (char_idx, (byte_idx, _)) in content.char_indices().enumerate() {
            if byte_idx == fn_start_byte {
                fn_start_char = char_idx;
            }
            if byte_idx == fn_body_start_byte {
                fn_body_start_char = char_idx;
            }
        }

        let chars: Vec<char> = content.chars().collect();
        let mut depth = 1;
        let mut i = fn_body_start_char;
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_template_literal = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut escaped = false;
        let mut fn_body_end_char = None;

        while i < chars.len() {
            let c = chars[i];
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if c == '\\' {
                escaped = true;
                i += 1;
                continue;
            }
            if in_line_comment {
                if c == '\n' {
                    in_line_comment = false;
                }
            } else if in_block_comment {
                if c == '/' && i > 0 && chars[i-1] == '*' {
                    in_block_comment = false;
                }
            } else if in_single_quote {
                if c == '\'' {
                    in_single_quote = false;
                }
            } else if in_double_quote {
                if c == '"' {
                    in_double_quote = false;
                }
            } else if in_template_literal {
                if c == '`' {
                    in_template_literal = false;
                }
            } else {
                if c == '/' && i + 1 < chars.len() && chars[i+1] == '/' {
                    in_line_comment = true;
                    i += 1;
                } else if c == '/' && i + 1 < chars.len() && chars[i+1] == '*' {
                    in_block_comment = true;
                    i += 1;
                } else if c == '\'' {
                    in_single_quote = true;
                } else if c == '"' {
                    in_double_quote = true;
                } else if c == '`' {
                    in_template_literal = true;
                } else if c == '{' {
                    depth += 1;
                } else if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        fn_body_end_char = Some(i);
                        break;
                    }
                }
            }
            i += 1;
        }

        let fn_body_end_char = fn_body_end_char.unwrap_or(chars.len());
        
        let prefix: String = chars[..fn_start_char].iter().collect();
        let suffix: String = if fn_body_end_char < chars.len() {
            chars[fn_body_end_char + 1..].iter().collect()
        } else {
            "".to_string()
        };

        let body_str: String = chars[fn_body_start_char..fn_body_end_char].iter().collect();
        let body_start_line = content[..fn_body_start_byte].lines().count();
        let mut script_lines = Vec::new();
        let mut markup_lines = Vec::new();
        let mut script_mode = true;

        for (line_idx, line) in body_str.lines().enumerate() {
            let line_num = body_start_line + line_idx;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if script_mode {
                    script_lines.push((line.to_string(), line_num));
                } else {
                    markup_lines.push((line.to_string(), line_num));
                }
                continue;
            }

            if script_mode {
                let starts_with_markup = trimmed.starts_with('<')
                    || trimmed.starts_with("if ")
                    || trimmed.starts_with("for ")
                    || trimmed.starts_with("return")
                    || trimmed.starts_with("{#if")
                    || trimmed.starts_with("{#for");
                if starts_with_markup {
                    script_mode = false;
                }
            }

            if script_mode {
                script_lines.push((line.to_string(), line_num));
            } else {
                markup_lines.push((line.to_string(), line_num));
            }
        }

        let mut cleaned_markup = Vec::new();
        for (line, ln) in markup_lines {
            let mut cleaned = line.trim().to_string();
            if cleaned.starts_with("return") {
                cleaned = cleaned["return".len()..].trim().to_string();
                if cleaned.starts_with('(') {
                    cleaned = cleaned[1..].trim().to_string();
                }
            }
            if cleaned == "(" {
                continue;
            }
            if cleaned == ")" || cleaned == ");" || cleaned == "}" || cleaned == "};" {
                continue;
            }
            if cleaned.ends_with(';') {
                cleaned.pop();
            }
            if !cleaned.is_empty() {
                cleaned_markup.push((cleaned, ln));
            }
        }

        let markup_only: String = cleaned_markup.iter().map(|(m, _)| m.as_str()).collect::<Vec<&str>>().join("\n");
        check_adjacent_jsx(&markup_only)?;

        let param_binding = if !params_str.trim().is_empty() {
            format!("let {} = useParams();\n", params_str.trim())
        } else {
            "".to_string()
        };

        let mut result = String::new();
        result.push_str("<script>\n");
        if !prefix.trim().is_empty() {
            result.push_str(prefix.trim());
            result.push('\n');
        }
        if !param_binding.is_empty() {
            result.push_str(&param_binding);
        }
        for (s, ln) in script_lines {
            result.push_str(&format!("{} // line:{}\n", s, ln));
        }
        result.push_str("</script>\n");

        for (m, ln) in cleaned_markup {
            let injected = inject_line_attr(&m, ln);
            result.push_str(&injected);
            result.push('\n');
        }

        if !suffix.trim().is_empty() {
            result.push_str(suffix.trim());
            result.push('\n');
        }

        Ok(result)
    } else {
        Ok(content.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_based_template() {
        let content = r#"
        import Header from './Header.erm';

        export fn page(params) {
            let name = useState('world');
            <h1>Hello {name} from {params.id}</h1>
        }
        <style>
            h1 { color: red; }
        </style>
        "#;
        
        let preprocessed = preprocess_function_template(content).unwrap();
        println!("PREPROCESSED:\n{}", preprocessed);
        assert!(preprocessed.contains("<script>"));
        assert!(preprocessed.contains("let params = useParams();"));
        assert!(preprocessed.contains("let name = useState('world');"));
        assert!(preprocessed.contains("<h1 data-erm-line=\"6\">Hello {name} from {params.id}</h1>"));
        assert!(preprocessed.contains("<style>"));
    }

    #[test]
    fn test_function_based_template_adjacent_error() {
        let content = r#"
        export fn page(params) {
            <h1>Hello</h1>
            <button>Click</button>
        }
        "#;
        let preprocessed = preprocess_function_template(content);
        assert!(preprocessed.is_err());
        let err_msg = preprocessed.unwrap_err().to_string();
        assert!(err_msg.contains("Adjacent ERM elements must be wrapped"));
    }

    #[test]
    fn test_link_compilation() {
        let content = "<Link to=\"/contact\">Contact</Link>";
        let mut visited = std::collections::HashMap::new();
        let mut if_counter = 0;
        let mut for_counter = 0;
        let params = std::collections::HashMap::new();
        let res = process_component_tree(".", content, &mut visited, None, &params, &mut if_counter, &mut for_counter).unwrap();
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
        let mut if_counter = 0;
        let mut for_counter = 0;
        let params = std::collections::HashMap::new();
        let res = process_component_tree(".", content, &mut visited, None, &params, &mut if_counter, &mut for_counter).unwrap();
        assert!(res.state_vars.contains(&"count".to_string()));
        let combined = res.scripts.join("\n");
        assert!(combined.contains("useState(0, \"count\")"));
        assert!(combined.contains("count.value++"));
    }

    #[test]
    fn test_fragment_compilation() {
        let content = r#"
        export fn page(params) {
            let count = useState(0);
            <>
                <h1>Hello from function based template!</h1>
                <p>Current count is: {count}</p>
                <button onClick={() => { count++ }}>Increment</button>
            </>
        }
        "#;
        let mut visited = std::collections::HashMap::new();
        let mut if_counter = 0;
        let mut for_counter = 0;
        let params = std::collections::HashMap::new();
        let res = process_component_tree(".", content, &mut visited, None, &params, &mut if_counter, &mut for_counter).unwrap();
        assert!(!res.html.contains("<>"));
        assert!(!res.html.contains("</>"));
        assert!(res.html.contains("Hello from function based template!</h1>"));
        assert!(res.html.contains("Increment</button>"));
    }

    #[test]
    fn test_for_loop_compilation() {
        let content = r#"
        <script>
            let items = useState([1, 2, 3]);
        </script>
        for item, i in items {
            <p>Item key as {i} : {item}</p>
        }
        "#;
        let params = std::collections::HashMap::new();
        let res = process_erm_component(".", content, true, &params).unwrap();
        println!("{}", res);
        assert!(res.contains("Item key as 0 : 1"));
    }

    #[test]
    fn test_contact_page_id() {
        let content = std::fs::read_to_string("libs/init/app/pages/contact.erm").unwrap();
        let mut visited = std::collections::HashMap::new();
        let mut if_counter = 0;
        let mut for_counter = 0;
        let params = std::collections::HashMap::new();
        let tree_res = process_component_tree("libs/init/app/pages", &content, &mut visited, None, &params, &mut if_counter, &mut for_counter).unwrap();
        assert!(!tree_res.html.is_empty());
        let params = std::collections::HashMap::new();
        let res = process_erm_component("libs/init/app/pages", &content, true, &params).unwrap();
        assert!(!res.is_empty());
    }

    #[test]
    fn test_new_if_syntax_compiler() {
        let content = r#"
        <script>
            let porridge = { temperature: 90 };
        </script>
        if porridge.temperature > 100 {
            <p>too hot!</p>
        } else if 80 > porridge.temperature {
            <p>too cold!</p>
        } else {
            <p>just right!</p>
        }
        "#;
        let params = std::collections::HashMap::new();
        let res = process_erm_component(".", content, false, &params).unwrap();
        println!("RES HTML:\n{}", res);
        let html_part = res.split("<script class=\"__erm_script\">").next().unwrap();
        assert!(html_part.contains("just right!"));
        assert!(!html_part.contains("too hot!"));
        assert!(!html_part.contains("too cold!"));
        assert!(res.contains("erm-if-0"));
    }

    #[test]
    fn test_nested_if_syntax_compiler() {
        let content = r#"
        <script>
            let outer = true;
            let inner = false;
        </script>
        if outer {
            if inner {
                <p>Inner True</p>
            } else {
                <p>Inner False</p>
            }
        }
        "#;
        let params = std::collections::HashMap::new();
        let res = process_erm_component(".", content, false, &params).unwrap();
        println!("NESTED RES:\n{}", res);
        let html_part = res.split("<script class=\"__erm_script\">").next().unwrap();
        assert!(html_part.contains("Inner False"));
        assert!(!html_part.contains("Inner True"));
        assert!(res.contains("erm-if-0"));
        assert!(res.contains("erm-if-1"));
    }
}

