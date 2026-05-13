use std::collections::HashMap;
use crate::eval::{self, ErmEval};
use fnv::FnvHasher;
use std::hash::Hasher;

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
    pub atom_vars: Vec<String>,
}

pub fn replace_word(input: &str, word: &str, suffix: &str) -> String {
    if word.len() <= 1 { return input.to_string(); }
    let mut res = String::new();
    let mut i = 0;
    let mut in_string: Option<char> = None;
    let bytes = input.as_bytes();
    while i < input.len() {
        if let Some(quote) = in_string {
            if bytes[i] as char == quote && (i == 0 || (i > 0 && bytes[i - 1] != b'\\')) {
                in_string = None;
            }
            res.push(bytes[i] as char);
            i += 1;
            continue;
        }

        if bytes[i] == b'"' || bytes[i] == b'\'' {
            in_string = Some(bytes[i] as char);
            res.push(bytes[i] as char);
            i += 1;
            continue;
        }

        if input[i..].starts_with(word) {
            let end = i + word.len();
            let before_ok = i == 0 || (!bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_' && bytes[i - 1] != b'$');
            let after_ok = end == input.len() || (!bytes[end].is_ascii_alphanumeric() && bytes[end] != b'_' && bytes[end] != b'$');

            if before_ok && after_ok {
                let mut is_decl = false;
                for kw in ["let", "const", "var"] {
                    if i >= kw.len() + 1 {
                        let start = i - kw.len() - 1;
                        if &input[start..i - 1] == kw && bytes[i - 1].is_ascii_whitespace() {
                            let pre_kw_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
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
        res.push(bytes[i] as char);
        i += 1;
    }
    res
}

pub fn inject_atom_name(input: &str, name: &str) -> String {
    let mut res = String::new();
    let mut i = 0;
    while i < input.len() {
        if input[i..].starts_with(name) {
            let end = i + name.len();
            let mut k = end;
            while k < input.len() && (input.as_bytes()[k].is_ascii_whitespace() || input.as_bytes()[k] == b':') {
                k += 1;
            }
            if k < input.len() && input.as_bytes()[k] == b'=' {
                k += 1;
                while k < input.len() && input.as_bytes()[k].is_ascii_whitespace() {
                    k += 1;
                }
                if input[k..].starts_with("atom(") {
                    let mut depth = 1;
                    let mut j = k + 5;
                    while j < input.len() && depth > 0 {
                        if input.as_bytes()[j] == b'(' { depth += 1; }
                        else if input.as_bytes()[j] == b')' { depth -= 1; }
                        j += 1;
                    }
                    if depth == 0 {
                        res.push_str(&input[i..j - 1]);
                        res.push_str(", \"");
                        res.push_str(name);
                        res.push_str("\")");
                        res.push_str(&inject_atom_name(&input[j..], name));
                        return res;
                    }
                }
            }
        }
        res.push(input.as_bytes()[i] as char);
        i += 1;
    }
    res
}

pub fn process_component_tree(base_dir: &str, content: &str, visited: &mut HashMap<String, bool>) -> anyhow::Result<ProcessResult> {
    let mut scripts = Vec::new();
    let mut styles = Vec::new();
    let mut atom_vars = Vec::new();

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

            // Find atom vars in script
            let mut aj = 0;
            while let Some(idx) = script_content[aj..].find("atom(") {
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
                        if !atom_vars.contains(&name.to_string()) {
                            atom_vars.push(name.to_string());
                        }
                    }
                }
                aj = call_pos + 5;
            }

            scripts.push(script_content.to_string());
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
            if !tag_content.is_empty() && !tag_content.starts_with('/') {
                let mut parts = tag_content.split_whitespace();
                let tag_name = parts.next().unwrap_or("");
                if !tag_name.is_empty() && tag_name.chars().next().unwrap().is_ascii_uppercase() {
                    let comp_filename = format!("{}.erm", tag_name);
                    let comp_path = std::path::Path::new(base_dir).join(&comp_filename);
                    if comp_path.exists() {
                        let comp_path_str = comp_path.to_string_lossy().into_owned();
                        if !visited.contains_key(&comp_path_str) {
                            visited.insert(comp_path_str.clone(), true);
                            let comp_content = std::fs::read_to_string(&comp_path)?;
                            let mut sub_res = process_component_tree(base_dir, &comp_content, visited)?;
                            html_buf.push_str(&sub_res.html);
                            scripts.append(&mut sub_res.scripts);
                            styles.append(&mut sub_res.styles);
                            for v in sub_res.atom_vars {
                                if !atom_vars.contains(&v) { atom_vars.push(v); }
                            }
                            i += tag_end + 1;
                            continue;
                        }
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
        for sig in &atom_vars {
            transformed = inject_atom_name(&transformed, sig);
        }
        for sig in &atom_vars {
            transformed = replace_word(&transformed, sig, ".value");
        }
        *s = transformed;
    }

    let mut bindings = Vec::new();
    let mut events = Vec::new();
    let reactive_html = parse_reactivity(&html_buf, &mut bindings, &mut events, &atom_vars);
    let scoped_html = scope_html(&reactive_html, &scope_id)?;

    scripts.append(&mut bindings);
    scripts.append(&mut events);

    Ok(ProcessResult {
        html: scoped_html,
        scripts,
        styles,
        atom_vars,
    })
}

fn parse_reactivity(html: &str, bindings: &mut Vec<String>, events: &mut Vec<String>, atoms: &[String]) -> String {
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
                        for sig in atoms {
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
                            for sig in atoms {
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

pub fn process_erm_component(base_dir: &str, content: &str, is_prod: bool) -> anyhow::Result<String> {
    let mut visited = HashMap::new();
    let result = process_component_tree(base_dir, content, &mut visited)?;

    let mut script_all = String::new();
    script_all.push_str("window.__erm_bindings = []; window.__erm_events = [];");
    for s in &result.scripts {
        script_all.push_str(s);
        script_all.push('\n');
    }
    
    // Add the framework runtime
    script_all.push_str(r#"
        function __erm_update() {
            window.__erm_bindings.forEach(b => {
                const el = document.getElementById(b.id);
                if (el) el.innerText = b.get();
            });
        }
        window.__erm_update = __erm_update;
        document.addEventListener('DOMContentLoaded', () => {
            window.__erm_events.forEach(e => {
                const el = document.getElementById(e.id);
                if (el) el.addEventListener(e.event, e.handler);
            });
            __erm_update();
        });
    "#);

    if !is_prod {
        script_all.push_str("(function(){ if(window.__hmr_initialized) return; window.__hmr_initialized=true; console.log('[HMR] Initialized'); })();");
    }

    let res_html = result.html.clone();
    
    let mut output = String::new();
    output.push_str("<!DOCTYPE html><html><head>");
    for s in &result.styles {
        output.push_str(&format!("<style>{}</style>", s));
    }
    output.push_str("</head><body>");
    output.push_str(&res_html);
    output.push_str("<script>");
    output.push_str(&script_all);
    output.push_str("</script></body></html>");

    Ok(output)
}
