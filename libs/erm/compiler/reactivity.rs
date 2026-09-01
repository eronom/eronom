use super::utils::{get_re_attr_brace, replace_word};

pub fn parse_reactivity(html: &str, bindings: &mut Vec<String>, events: &mut Vec<String>, states: &[String]) -> String {
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

pub fn get_event_attribute_name(prefix: &str) -> Option<String> {
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
