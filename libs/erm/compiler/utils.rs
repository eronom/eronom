use std::borrow::Cow;
use std::collections::HashMap;
use crate::eval;

#[inline]
fn is_js_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[inline]
fn is_attr_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

#[inline]
fn skip_ws(s: &str, pos: usize) -> usize {
    for (off, ch) in s[pos..].char_indices() {
        if !ch.is_whitespace() {
            return pos + off;
        }
    }
    s.len()
}

#[inline]
fn skip_horiz_ws(s: &str, mut pos: usize) -> usize {
    let bytes = s.as_bytes();
    while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
        pos += 1;
    }
    pos
}

#[inline]
pub fn match_attr_brace(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_attr_name_byte(bytes[i]) {
        i += 1;
    }
    if i > 0 && bytes.get(i) == Some(&b'=') && bytes.get(i + 1) == Some(&b'{') {
        Some(&s[..i])
    } else {
        None
    }
}

pub struct StateDecl<'a> {
    pub var_name: &'a str,
    pub is_fetch: bool,
    pub open_paren_end: usize,
}

pub fn scan_state_decls(script: &str) -> Vec<StateDecl<'_>> {
    let mut out = Vec::new();
    let bytes = script.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if !script.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let kw_len = if i == 0 || !is_js_ident_byte(bytes[i - 1]) {
            if script[i..].starts_with("const") {
                5
            } else if script[i..].starts_with("let") || script[i..].starts_with("var") {
                3
            } else {
                0
            }
        } else {
            0
        };
        if kw_len == 0 {
            i += 1;
            continue;
        }
        let after_kw = i + kw_len;
        let name_start = skip_ws(script, after_kw);
        if name_start == after_kw {
            i += kw_len;
            continue;
        }
        let mut name_end = name_start;
        while name_end < bytes.len() && is_js_ident_byte(bytes[name_end]) {
            name_end += 1;
        }
        if name_end == name_start {
            i += kw_len;
            continue;
        }
        let var_name = &script[name_start..name_end];
        let eq_pos = skip_ws(script, name_end);
        if bytes.get(eq_pos) != Some(&b'=') {
            i = name_end;
            continue;
        }
        let mut rhs = skip_ws(script, eq_pos + 1);
        if script[rhs..].starts_with("await") {
            let after_await = skip_ws(script, rhs + 5);
            if after_await > rhs + 5 {
                rhs = after_await;
            }
        }
        let (fn_len, is_fetch) = if script[rhs..].starts_with("useFetch") {
            (8, true)
        } else if script[rhs..].starts_with("useState") || script[rhs..].starts_with("erm.init") {
            (8, false)
        } else {
            i = name_end;
            continue;
        };
        let paren_pos = skip_ws(script, rhs + fn_len);
        if bytes.get(paren_pos) == Some(&b'(') {
            out.push(StateDecl {
                var_name,
                is_fetch,
                open_paren_end: paren_pos + 1,
            });
            i = paren_pos + 1;
        } else {
            i = rhs + fn_len;
        }
    }
    out
}

pub fn parse_import_named(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut pos = skip_ws(line, 0);
    if !line[pos..].starts_with("import") {
        return None;
    }
    pos += 6;
    pos = skip_ws(line, pos);
    if bytes.get(pos) != Some(&b'{') {
        return None;
    }
    pos += 1;
    let names_raw_start = pos;
    while pos < bytes.len() && bytes[pos] != b'}' {
        let ch = line[pos..].chars().next()?;
        if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == ',' || ch.is_whitespace()) {
            return None;
        }
        pos += ch.len_utf8();
    }
    if bytes.get(pos) != Some(&b'}') {
        return None;
    }
    let names = line[names_raw_start..pos].trim();
    if names.is_empty() {
        return None;
    }
    pos = skip_ws(line, pos + 1);
    if !line[pos..].starts_with("from") {
        return None;
    }
    pos += 4;
    let quote_pos = skip_ws(line, pos);
    if quote_pos == pos {
        return None;
    }
    let q = *bytes.get(quote_pos)?;
    if q != b'\'' && q != b'"' {
        return None;
    }
    let path_start = quote_pos + 1;
    let mut path_end = path_start;
    while path_end < bytes.len() && bytes[path_end] != b'\'' && bytes[path_end] != b'"' {
        path_end += 1;
    }
    let close_q = *bytes.get(path_end)?;
    if close_q != b'\'' && close_q != b'"' {
        return None;
    }
    Some((names, &line[path_start..path_end]))
}

pub fn parse_import_default(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut pos = skip_ws(line, 0);
    if !line[pos..].starts_with("import") {
        return None;
    }
    pos += 6;
    let name_start = skip_ws(line, pos);
    if name_start == pos {
        return None;
    }
    let mut name_end = name_start;
    while name_end < bytes.len() && is_js_ident_byte(bytes[name_end]) {
        name_end += 1;
    }
    if name_end == name_start {
        return None;
    }
    let ident = &line[name_start..name_end];
    let from_pos = skip_ws(line, name_end);
    if from_pos == name_end || !line[from_pos..].starts_with("from") {
        return None;
    }
    let after_from = from_pos + 4;
    let quote_pos = skip_ws(line, after_from);
    if quote_pos == after_from {
        return None;
    }
    let q = *bytes.get(quote_pos)?;
    if q != b'\'' && q != b'"' {
        return None;
    }
    let path_start = quote_pos + 1;
    let mut path_end = path_start;
    while path_end < bytes.len() && bytes[path_end] != b'\'' && bytes[path_end] != b'"' {
        path_end += 1;
    }
    let close_q = *bytes.get(path_end)?;
    if close_q != b'\'' && close_q != b'"' {
        return None;
    }
    Some((ident, &line[path_start..path_end]))
}

pub fn strip_leading_export(line: &str) -> Cow<'_, str> {
    let ws1 = skip_ws(line, 0);
    if line[ws1..].starts_with("export") {
        let after = ws1 + 6;
        let ws2 = skip_ws(line, after);
        if ws2 > after {
            return Cow::Owned(format!("{}{}", &line[..ws1], &line[ws2..]));
        }
    }
    Cow::Borrowed(line)
}

pub struct FnHeader<'a> {
    pub start: usize,
    pub body_start: usize,
    pub name: &'a str,
    pub params: &'a str,
}

pub fn find_fn_headers(content: &str, require_export: bool) -> Vec<FnHeader<'_>> {
    let mut headers = Vec::new();
    let bytes = content.as_bytes();
    let mut line_start = 0usize;

    while line_start <= bytes.len() {
        let line_end = content[line_start..]
            .find('\n')
            .map_or(bytes.len(), |off| line_start + off);

        let mut pos = skip_horiz_ws(content, line_start);
        let mut saw_export = false;
        if content[pos..].starts_with("export") {
            let after_exp = pos + 6;
            let next = skip_ws(content, after_exp);
            if next > after_exp {
                saw_export = true;
                pos = next;
                if content[pos..].starts_with("default") {
                    let after_def = pos + 7;
                    let next_def = skip_ws(content, after_def);
                    if next_def > after_def {
                        pos = next_def;
                    }
                }
            }
        }

        if !require_export || saw_export {
            let kw_len = if content[pos..].starts_with("function") {
                8
            } else if content[pos..].starts_with("fn") {
                2
            } else {
                0
            };
            if kw_len > 0 {
                let after_kw = pos + kw_len;
                let name_start = skip_ws(content, after_kw);
                if name_start > after_kw {
                    let mut name_end = name_start;
                    while name_end < bytes.len() && is_js_ident_byte(bytes[name_end]) {
                        name_end += 1;
                    }
                    if name_end > name_start {
                        let lparen = skip_ws(content, name_end);
                        if bytes.get(lparen) == Some(&b'(') {
                            let params_start = lparen + 1;
                            let mut p = params_start;
                            let mut depth = 1usize;
                            let mut quote: Option<u8> = None;
                            let mut escaped = false;
                            while p < bytes.len() && depth > 0 {
                                let b = bytes[p];
                                if escaped {
                                    escaped = false;
                                    p += 1;
                                    continue;
                                }
                                if let Some(q) = quote {
                                    if b == b'\\' {
                                        escaped = true;
                                    } else if b == q {
                                        quote = None;
                                    }
                                    p += 1;
                                    continue;
                                }
                                match b {
                                    b'\'' | b'"' | b'`' => quote = Some(b),
                                    b'(' => depth += 1,
                                    b')' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                                p += 1;
                            }
                            if depth == 0 && p < bytes.len() {
                                let params_end = p;
                                let mut after_paren = skip_ws(content, params_end + 1);
                                if bytes.get(after_paren) == Some(&b':') {
                                    after_paren = skip_ws(content, after_paren + 1);
                                    let ret_start = after_paren;
                                    while after_paren < bytes.len() {
                                        let ch = content[after_paren..].chars().next().unwrap();
                                        if ch.is_ascii_alphanumeric()
                                            || matches!(ch, '_' | '<' | '>' | '[' | ']' | ',')
                                            || ch.is_whitespace()
                                        {
                                            after_paren += ch.len_utf8();
                                        } else {
                                            break;
                                        }
                                    }
                                    if after_paren > ret_start {
                                        after_paren = skip_ws(content, after_paren);
                                    } else {
                                        after_paren = usize::MAX;
                                    }
                                }
                                if after_paren < bytes.len() && bytes.get(after_paren) == Some(&b'{')
                                {
                                    headers.push(FnHeader {
                                        start: line_start,
                                        body_start: after_paren + 1,
                                        name: &content[name_start..name_end],
                                        params: &content[params_start..params_end],
                                    });
                                    line_start = content[after_paren + 1..]
                                        .find('\n')
                                        .map_or(bytes.len() + 1, |off| after_paren + 2 + off);
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }

        if line_end >= bytes.len() {
            break;
        }
        line_start = line_end + 1;
    }

    headers
}

pub fn is_js_var_declared(script: &str, ident: &str) -> bool {
    if ident.is_empty() {
        return false;
    }
    for stmt in script.split(';') {
        let bytes = stmt.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if !stmt.is_char_boundary(i) {
                i += 1;
                continue;
            }
            let kw_len = if i == 0 || !is_js_ident_byte(bytes[i - 1]) {
                if stmt[i..].starts_with("const") {
                    5
                } else if stmt[i..].starts_with("let") || stmt[i..].starts_with("var") {
                    3
                } else {
                    0
                }
            } else {
                0
            };
            if kw_len > 0 {
                let after_kw = i + kw_len;
                let rest_start = skip_ws(stmt, after_kw);
                if rest_start > after_kw && contains_whole_word(&stmt[rest_start..], ident) {
                    return true;
                }
                i = after_kw;
            } else {
                i += 1;
            }
        }
    }
    false
}

fn contains_whole_word(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut start = 0usize;
    while let Some(off) = haystack[start..].find(needle) {
        let idx = start + off;
        let end = idx + needle.len();
        let left_ok = idx == 0 || !is_js_ident_byte(bytes[idx - 1]);
        let right_ok = end == bytes.len() || !is_js_ident_byte(bytes[end]);
        if left_ok && right_ok {
            return true;
        }
        start = idx + 1;
    }
    false
}

pub fn file_exists_or_vfs(path: &std::path::Path) -> bool {
    path.exists()
}

pub fn read_file_or_vfs(path: &std::path::Path) -> anyhow::Result<String> {
    if path.exists() && path.is_file() {
        return Ok(std::fs::read_to_string(path)?);
    }
    anyhow::bail!("File not found on disk: {}", path.display())
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
    } else if import_path.starts_with("@stores/") {
        let mut curr = std::path::PathBuf::from(base_dir);
        let mut resolved = None;
        loop {
            let store_dir = curr.join("app").join("stores");
            if store_dir.exists() {
                resolved = Some(store_dir.join(&import_path["@stores/".len()..]));
                break;
            }
            let store_dir_alt = curr.join("stores");
            if store_dir_alt.exists() && curr.join("eronom.toml").exists() {
                resolved = Some(store_dir_alt.join(&import_path["@stores/".len()..]));
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
    let bytes = tag_content.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    while i < len {
        i = skip_ws(tag_content, i);
        if i >= len {
            break;
        }
        if !is_attr_name_byte(bytes[i]) {
            let ch = tag_content[i..].chars().next().unwrap();
            i += ch.len_utf8();
            continue;
        }
        let name_start = i;
        while i < len && is_attr_name_byte(bytes[i]) {
            i += 1;
        }
        let name = &tag_content[name_start..i];
        let eq_pos = skip_ws(tag_content, i);
        if bytes.get(eq_pos) != Some(&b'=') {
            i = eq_pos;
            continue;
        }
        let val_start = skip_ws(tag_content, eq_pos + 1);
        if val_start >= len {
            break;
        }
        let val = match bytes[val_start] {
            b'{' => {
                let expr_start = val_start + 1;
                let mut end = expr_start;
                while end < len && bytes[end] != b'{' && bytes[end] != b'}' {
                    end += 1;
                }
                if end > expr_start && bytes.get(end) == Some(&b'}') {
                    i = end + 1;
                    format!("() => ({})", &tag_content[expr_start..end])
                } else {
                    i = val_start + 1;
                    continue;
                }
            }
            b'"' => {
                let s_start = val_start + 1;
                let mut end = s_start;
                while end < len && bytes[end] != b'"' {
                    end += 1;
                }
                if bytes.get(end) == Some(&b'"') {
                    i = end + 1;
                    format!("() => \"{}\"", &tag_content[s_start..end])
                } else {
                    i = val_start + 1;
                    continue;
                }
            }
            b'\'' => {
                let s_start = val_start + 1;
                let mut end = s_start;
                while end < len && bytes[end] != b'\'' {
                    end += 1;
                }
                if bytes.get(end) == Some(&b'\'') {
                    i = end + 1;
                    format!("() => '{}'", &tag_content[s_start..end])
                } else {
                    i = val_start + 1;
                    continue;
                }
            }
            _ => {
                let mut end = val_start;
                while end < len {
                    let ch = tag_content[end..].chars().next().unwrap();
                    if ch.is_whitespace() || ch == '>' {
                        break;
                    }
                    end += ch.len_utf8();
                }
                if end > val_start {
                    i = end;
                    format!("() => ({})", &tag_content[val_start..end])
                } else {
                    i = val_start + 1;
                    continue;
                }
            }
        };
        attrs.insert(name.to_string(), val);
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

fn extract_html_id_attrs(html: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0usize;
    while let Some(off) = html[i..].find("id") {
        let idx = i + off;
        let left_ok = idx == 0 || !is_attr_name_byte(bytes[idx - 1]);
        if !left_ok {
            i = idx + 2;
            continue;
        }
        let eq_pos = skip_ws(html, idx + 2);
        if bytes.get(eq_pos) != Some(&b'=') {
            i = idx + 2;
            continue;
        }
        let q_pos = skip_ws(html, eq_pos + 1);
        let Some(&q) = bytes.get(q_pos) else {
            break;
        };
        if q != b'"' && q != b'\'' {
            i = q_pos;
            continue;
        }
        let val_start = q_pos + 1;
        let mut val_end = val_start;
        while val_end < bytes.len() && bytes[val_end] != b'"' && bytes[val_end] != b'\'' {
            val_end += 1;
        }
        if val_end > val_start && bytes.get(val_end).is_some_and(|&c| c == b'"' || c == b'\'') {
            let id_val = html[val_start..val_end].to_string();
            if !ids.contains(&id_val) {
                ids.push(id_val);
            }
            i = val_end + 1;
        } else {
            i = val_start;
        }
    }
    ids
}

fn rewrite_scoped_ids(
    html: &mut String,
    scripts: &mut Vec<String>,
    html_prefix: &str,
    script_inside_prefix: &str,
    script_outside_prefix: &str,
) {
    let ids = extract_html_id_attrs(html);
    for id in &ids {
        let old_pattern_dq = format!("id=\"{}\"", id);
        let new_pattern_dq = format!("id=\"{}{}\"", html_prefix, id);
        *html = html.replace(&old_pattern_dq, &new_pattern_dq);
        let old_pattern_sq = format!("id='{}'", id);
        let new_pattern_sq = format!("id='{}{}'", html_prefix, id);
        *html = html.replace(&old_pattern_sq, &new_pattern_sq);
    }
    for id in &ids {
        let dq_pattern = format!("\"{}\"", id);
        let dq_replace = format!("{}\"{}{}\"", script_outside_prefix, script_inside_prefix, id);
        let sq_pattern = format!("'{}'", id);
        let sq_replace = format!("{}'{}{}'", script_outside_prefix, script_inside_prefix, id);
        for s in scripts.iter_mut() {
            *s = s.replace(&dq_pattern, &dq_replace).replace(&sq_pattern, &sq_replace);
        }
    }
}

pub fn scope_component_ids(html: &mut String, scripts: &mut Vec<String>) {
    rewrite_scoped_ids(html, scripts, "__erm_anchor_id_prefix__", "", "anchorId + ");
}

pub fn scope_instance_ids(html: &mut String, scripts: &mut Vec<String>, prefix: &str) {
    rewrite_scoped_ids(html, scripts, prefix, prefix, "");
}

pub fn inject_state_name(input: &str, name: &str, scoped_name: &str) -> String {
    let mut result = String::new();
    let mut i = 0;
    let name_pattern = format!("let {} = useState(", name);
    let name_pattern_const = format!("const {} = useState(", name);
    let name_pattern_var = format!("var {} = useState(", name);
    let name_pattern_erm = format!("let {} = erm.init(", name);
    let name_pattern_erm_const = format!("const {} = erm.init(", name);
    let name_pattern_erm_var = format!("var {} = erm.init(", name);
    let name_pattern_fetch = format!("let {} = useFetch(", name);
    let name_pattern_fetch_const = format!("const {} = useFetch(", name);
    let name_pattern_fetch_var = format!("var {} = useFetch(", name);
    let name_pattern_await_fetch = format!("let {} = await useFetch(", name);
    let name_pattern_await_fetch_const = format!("const {} = await useFetch(", name);
    let name_pattern_await_fetch_var = format!("var {} = await useFetch(", name);

    while i < input.len() {
        let matched = if input[i..].starts_with(&name_pattern) {
            Some((name_pattern.len(), name_pattern.as_str()))
        } else if input[i..].starts_with(&name_pattern_const) {
            Some((name_pattern_const.len(), name_pattern_const.as_str()))
        } else if input[i..].starts_with(&name_pattern_var) {
            Some((name_pattern_var.len(), name_pattern_var.as_str()))
        } else if input[i..].starts_with(&name_pattern_erm) {
            Some((name_pattern_erm.len(), name_pattern_erm.as_str()))
        } else if input[i..].starts_with(&name_pattern_erm_const) {
            Some((name_pattern_erm_const.len(), name_pattern_erm_const.as_str()))
        } else if input[i..].starts_with(&name_pattern_erm_var) {
            Some((name_pattern_erm_var.len(), name_pattern_erm_var.as_str()))
        } else if input[i..].starts_with(&name_pattern_await_fetch) {
            Some((name_pattern_await_fetch.len(), name_pattern_await_fetch.as_str()))
        } else if input[i..].starts_with(&name_pattern_await_fetch_const) {
            Some((name_pattern_await_fetch_const.len(), name_pattern_await_fetch_const.as_str()))
        } else if input[i..].starts_with(&name_pattern_await_fetch_var) {
            Some((name_pattern_await_fetch_var.len(), name_pattern_await_fetch_var.as_str()))
        } else if input[i..].starts_with(&name_pattern_fetch) {
            Some((name_pattern_fetch.len(), name_pattern_await_fetch.as_str()))
        } else if input[i..].starts_with(&name_pattern_fetch_const) {
            Some((name_pattern_fetch_const.len(), name_pattern_await_fetch_const.as_str()))
        } else if input[i..].starts_with(&name_pattern_fetch_var) {
            Some((name_pattern_fetch_var.len(), name_pattern_await_fetch_var.as_str()))
        } else {
            None
        };

        if let Some((pat_len, replacement)) = matched {
            result.push_str(replacement);
            i += pat_len;
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

pub fn extract_state_variable_init(content: &str, var_name: &str) -> Option<String> {
    for decl in scan_state_decls(content) {
        if decl.var_name != var_name {
            continue;
        }
        let is_fetch = decl.is_fetch;
        let start = decl.open_paren_end;
        let mut depth = 1;
        let mut in_single = false;
        let mut in_double = false;
        let mut in_backtick = false;
        let mut escaped = false;
        let chars: Vec<(usize, char)> = content[start..].char_indices().collect();
        for (idx, c) in chars {
            if escaped {
                escaped = false;
                continue;
            }
            if c == '\\' {
                escaped = true;
                continue;
            }
            if !in_double && !in_backtick && c == '\'' { in_single = !in_single; }
            else if !in_single && !in_backtick && c == '"' { in_double = !in_double; }
            else if !in_single && !in_double && c == '`' { in_backtick = !in_backtick; }
            else if !in_single && !in_double && !in_backtick {
                if c == '(' || c == '{' || c == '[' { depth += 1; }
                else if c == ')' || c == '}' || c == ']' {
                    depth -= 1;
                    if depth == 0 {
                        let init_raw = content[start..start + idx].trim();
                        if is_fetch {
                            return Some(format!("await useFetch({})", init_raw));
                        } else {
                            return Some(init_raw.to_string());
                        }
                    }
                } else if !is_fetch && c == ',' && depth == 1 {
                    let init_raw = content[start..start + idx].trim();
                    return Some(init_raw.to_string());
                }
            }
        }
    }
    None
}

pub fn extract_attribute(tag_content: &str, attr_name: &str) -> Option<String> {
    let target = format!("{}=", attr_name);
    let mut search_from = 0;
    while search_from < tag_content.len() {
        if let Some(pos) = tag_content[search_from..].find(&target) {
            let match_idx = search_from + pos;
            if match_idx == 0 || tag_content[..match_idx].chars().next_back().map_or(false, |c| c.is_ascii_whitespace()) {
                let val_part = tag_content[match_idx + target.len()..].trim_start();
                if val_part.starts_with('{') {
                    let mut depth = 0;
                    for (i, c) in val_part.char_indices() {
                        if c == '{' { depth += 1; }
                        else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                return Some(val_part[1..i].trim().to_string());
                            }
                        }
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
            search_from = match_idx + target.len();
        } else {
            break;
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
                if let Some(attr_name) = match_attr_brace(&html[i..]) {
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

pub fn starts_with_html_tag(s: &str) -> bool {
    let trimmed = s.trim();
    if !trimmed.starts_with('<') {
        return false;
    }
    if trimmed.starts_with("<style") || trimmed.starts_with("</style") {
        return false;
    }
    let rest = &trimmed[1..];
    if rest.is_empty() {
        return false;
    }
    if rest.starts_with('/') || rest.starts_with('>') || rest.starts_with('!') || rest.starts_with('@') {
        return true;
    }
    let first_char = rest.chars().next().unwrap();
    if !first_char.is_ascii_alphabetic() && first_char != '_' {
        return false;
    }
    if trimmed.ends_with(';') && !trimmed.contains('>') {
        return false;
    }
    true
}
