use crate::eval;
use super::utils::*;
use super::css::scope_html;
use super::template_js::{compile_template_to_js, ScriptStyleRanges};

pub fn find_tag_end(content: &str, start_pos: usize) -> Option<usize> {
    let mut brace_depth = 0;
    let mut in_quotes = false;
    let mut in_squotes = false;
    let chars: Vec<char> = content[start_pos..].chars().collect();
    for (idx, c) in chars.iter().enumerate() {
        if *c == '"' && !in_squotes {
            in_quotes = !in_quotes;
        } else if *c == '\'' && !in_quotes {
            in_squotes = !in_squotes;
        } else if !in_quotes && !in_squotes {
            if *c == '{' {
                brace_depth += 1;
            } else if *c == '}' {
                if brace_depth > 0 { brace_depth -= 1; }
            } else if *c == '>' && brace_depth == 0 {
                return Some(start_pos + idx);
            }
        }
    }
    None
}

pub enum ElseTransition {
    ElseIf(String, usize),
    Else(usize),
}

pub fn try_parse_if_header(content: &str, start_pos: usize) -> Option<(String, usize)> {
    if !content[start_pos..].starts_with("if") {
        return None;
    }
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

pub fn find_else_transition(content: &str, start_pos: usize) -> Option<ElseTransition> {
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

pub fn try_parse_for_header(content: &str, start_pos: usize) -> Option<(String, usize)> {
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

pub fn find_last_for_block(res_html: &str) -> Option<(usize, String, usize)> {
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

pub fn find_last_if_block(res_html: &str) -> Option<(usize, String, usize)> {
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

pub fn process_for_block_at(
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
    let body = wrap_unquoted_braces(&body);

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
        r#"renderFor("{}", () => ({}), ({}) => {});"#,
        anchor_id, collection_expr, js_params, compiled_body
    );
    block_logic.push(logic);
    let anchor_html = format!("<span id=\"{}\" style=\"display:contents;\">{}</span>", anchor_id, ssr_html);
    res_html.replace_range(start_idx..absolute_close_idx + 1, &anchor_html);
    Ok(())
}

pub fn process_if_block_at(
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
            let body = wrap_unquoted_braces(&body);
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
            let body = remaining.to_string();
            let body = wrap_unquoted_braces(&body);
            branches.push((current_cond.clone(), body));
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
        r#"renderIf("{}", () => {{ let __erm_new = ""; {}; return __erm_new; }});"#,
        anchor_id, branches_js
    );
    block_logic.push(logic);
    let anchor_html = format!("<span id=\"{}\" style=\"display:contents;\">{}</span>", anchor_id, ssr_html_res);
    res_html.replace_range(start_idx..block_end_idx, &anchor_html);
    Ok(())
}
