use crate::eval;
use super::utils::*;
use super::errors::CompileError;
use super::transform::is_bare_script_template;
use super::css::scope_html;
use super::template_js::{compile_template_to_js, ScriptStyleRanges};

pub fn find_tag_end(content: &str, start_pos: usize) -> Option<usize> {
    let mut brace_depth = 0;
    let mut in_quotes = false;
    let mut in_squotes = false;
    for (idx, c) in content[start_pos..].char_indices() {
        if c == '"' && !in_squotes {
            in_quotes = !in_quotes;
        } else if c == '\'' && !in_quotes {
            in_squotes = !in_squotes;
        } else if !in_quotes && !in_squotes {
            if c == '{' {
                brace_depth += 1;
            } else if c == '}' {
                if brace_depth > 0 { brace_depth -= 1; }
            } else if c == '>' && brace_depth == 0 {
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
    if !content[start_pos..].starts_with("@if") {
        return None;
    }
    let after_if = start_pos + 3;
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
            let mut cond = content[after_if..scan].trim().to_string();
            if cond.starts_with('(') && cond.ends_with(')') {
                cond = cond[1..cond.len() - 1].trim().to_string();
            }
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
    if content[j..].starts_with("@elseif") {
        let after_elseif = j + 7;
        let mut scan = after_elseif;
        while scan < content.len() {
            let c = content[scan..].chars().next().unwrap();
            if c == '{' {
                let mut cond = content[after_elseif..scan].trim().to_string();
                if cond.starts_with('(') && cond.ends_with(')') {
                    cond = cond[1..cond.len() - 1].trim().to_string();
                }
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
    } else if content[j..].starts_with("@else") {
        let after_else = j + 5;
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
        let (is_if, if_len) = if content[k..].starts_with("@if") {
            (true, 3)
        } else if content[k..].starts_with("if") {
            (true, 2)
        } else {
            (false, 0)
        };
        if is_if {
            let after_if = k + if_len;
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
                    let mut cond = content[after_if..scan].trim().to_string();
                    if cond.starts_with('(') && cond.ends_with(')') {
                        cond = cond[1..cond.len() - 1].trim().to_string();
                    }
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
    if !content[start_pos..].starts_with("@for") {
        return None;
    }
    let after_for = start_pos + 4;
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
    let mut collection_expr_raw = header[in_idx + 4..].trim();
    // Support item keying syntax: collection (item.id)
    if let Some(open_paren) = collection_expr_raw.rfind(" (") {
        if collection_expr_raw.ends_with(')') {
            collection_expr_raw = collection_expr_raw[..open_paren].trim();
        }
    }
    let collection_expr_raw = if collection_expr_raw.starts_with('(') && collection_expr_raw.ends_with(')') {
        collection_expr_raw[1..collection_expr_raw.len() - 1].trim()
    } else {
        collection_expr_raw
    };

    // Check for @empty { ... } or @else { ... }
    let mut actual_end_idx = absolute_close_idx + 1;
    let mut empty_body = None;
    let mut scan_empty = absolute_close_idx + 1;
    while scan_empty < res_html.len() {
        let c = res_html[scan_empty..].chars().next().unwrap();
        if c.is_whitespace() {
            scan_empty += c.len_utf8();
        } else {
            break;
        }
    }
    if scan_empty < res_html.len() {
        let rest = &res_html[scan_empty..];
        let empty_kw_len = if rest.starts_with("@empty") {
            Some(6)
        } else if rest.starts_with("@else") {
            Some(5)
        } else {
            None
        };
        if let Some(kw_len) = empty_kw_len {
            let after_kw = scan_empty + kw_len;
            let mut scan_brace = after_kw;
            let mut found_brace = false;
            while scan_brace < res_html.len() {
                let c = res_html[scan_brace..].chars().next().unwrap();
                if c == '{' {
                    found_brace = true;
                    scan_brace += 1;
                    break;
                } else if !c.is_whitespace() {
                    break;
                }
                scan_brace += c.len_utf8();
            }
            if found_brace && scan_brace < res_html.len() {
                if let Some(empty_close) = find_matching_close_brace(&res_html[scan_brace..]) {
                    let eb = res_html[scan_brace..scan_brace + empty_close].to_string();
                    let eb = wrap_unquoted_braces(&eb);
                    empty_body = Some(eb);
                    actual_end_idx = scan_brace + empty_close + 1;
                }
            }
        }
    }

    let collection_expr = crate::frontend::transpile_expr_reactivity(collection_expr_raw, state_vars)
        .unwrap_or_else(|| collection_expr_raw.to_string());
    let (item_name, index_name) = if let Some(comma_idx) = vars_part.find(',') {
        (vars_part[0..comma_idx].trim(), vars_part[comma_idx + 1..].trim())
    } else {
        (vars_part, "")
    };
    let anchor_id = format!("erm-for-{}", for_counter);
    *for_counter += 1;
    let mut ssr_html = String::new();
    let mut is_empty = true;
    let collection_val = ev.eval(&collection_expr).unwrap_or(eval::Value::Null);
    if let eval::Value::List(items) = collection_val {
        if !items.is_empty() {
            is_empty = false;
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
    }
    if is_empty {
        if let Some(ref eb) = empty_body {
            let ssr_empty = evaluate_braces_in_html(eb, ev, state_vars);
            ssr_html.push_str(&ssr_empty);
        }
    }
    let js_params = if !index_name.is_empty() { format!("{}, {}", item_name, index_name) } else { item_name.to_string() };
    let scoped_body = scope_html(&body, scope_id)?;
    let compiled_body = compile_template_to_js(&scoped_body, state_vars);
    let logic = if let Some(ref eb) = empty_body {
        let scoped_empty = scope_html(eb, scope_id)?;
        let compiled_empty = compile_template_to_js(&scoped_empty, state_vars);
        format!(
            r#"renderFor("{}", () => ({}), ({}) => {}, () => {});"#,
            anchor_id, collection_expr, js_params, compiled_body, compiled_empty
        )
    } else {
        format!(
            r#"renderFor("{}", () => ({}), ({}) => {});"#,
            anchor_id, collection_expr, js_params, compiled_body
        )
    };
    block_logic.push(logic);
    let anchor_html = format!("<span id=\"{}\" style=\"display:contents;\">{}</span>", anchor_id, ssr_html);
    res_html.replace_range(start_idx..actual_end_idx, &anchor_html);
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

    for (cond_expr_raw, body) in branches {
        let cond_expr = crate::frontend::transpile_expr_reactivity(&cond_expr_raw, state_vars)
            .unwrap_or_else(|| cond_expr_raw);
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

pub fn find_bare_script_markup_start(content: &str) -> Option<usize> {
    if !is_bare_script_template(content) {
        return None;
    }
    let mut paren_depth = 0;
    let mut bracket_depth = 0;
    let mut offset = 0;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            offset += line.len() + 1;
            continue;
        }

        let is_expression_markup = trimmed.starts_with('{') && (trimmed.contains(".map(") || trimmed.contains('<'));
        let starts_with_markup = paren_depth == 0 && bracket_depth == 0 && (
            super::utils::starts_with_html_tag(trimmed)
            || trimmed.starts_with("@for")
            || trimmed.starts_with("@if")
            || is_expression_markup
        );

        if starts_with_markup {
            return Some(offset);
        }

        let mut in_str = false;
        let mut str_char = ' ';
        for c in line.chars() {
            if in_str {
                if c == str_char { in_str = false; }
            } else if c == '"' || c == '\'' || c == '`' {
                in_str = true;
                str_char = c;
            } else if c == '(' {
                paren_depth += 1;
            } else if c == ')' {
                if paren_depth > 0 { paren_depth -= 1; }
            } else if c == '[' {
                bracket_depth += 1;
            } else if c == ']' {
                if bracket_depth > 0 { bracket_depth -= 1; }
            }
        }
        offset += line.len() + 1;
    }
    None
}

pub fn validate_template_directives(content: &str, file_path: &str) -> anyhow::Result<()> {
    let ranges = ScriptStyleRanges::new(content);
    let markup_start = find_bare_script_markup_start(content).unwrap_or(0);
    let mut i = 0;
    let mut brace_depth: usize = 0;

    while i < content.len() {
        if let Some(skip_to) = ranges.get_skip_pos(i) {
            i = skip_to;
            continue;
        }

        let c = content[i..].chars().next().unwrap();

        // Skip script preamble in bare-script templates
        if i < markup_start {
            i += c.len_utf8();
            continue;
        }

        // Track template expression brace depth
        if c == '{' {
            brace_depth += 1;
            i += 1;
            continue;
        } else if c == '}' {
            if brace_depth > 0 {
                brace_depth -= 1;
            }
            i += 1;
            continue;
        }

        // Only validate control flow keywords outside of { ... } expressions
        if brace_depth == 0 {
            // Check bare 'if'
            if content[i..].starts_with("if") {
                let is_preceded_by_at_else = {
                    let mut check = i;
                    while check > 0 {
                        let bc = content[..check].chars().next_back().unwrap();
                        if bc.is_whitespace() {
                            check -= bc.len_utf8();
                        } else {
                            break;
                        }
                    }
                    check >= 5 && &content[check - 5..check] == "@else"
                };

                let prev_ok = if is_preceded_by_at_else {
                    false
                } else if i == 0 {
                    true
                } else {
                    let prev_c = content[..i].chars().next_back().unwrap();
                    !prev_c.is_alphanumeric() && prev_c != '_' && prev_c != '@' && prev_c != '.' && prev_c != '-'
                };
                let after_if = i + 2;
                let next_ok = if after_if < content.len() {
                    let next_c = content[after_if..].chars().next().unwrap();
                    next_c.is_whitespace() || next_c == '('
                } else {
                    false
                };

                if prev_ok && next_ok {
                    let mut scan = after_if;
                    let mut found_brace = None;
                    while scan < content.len() {
                        let sc = content[scan..].chars().next().unwrap();
                        if sc == '{' {
                            found_brace = Some(scan);
                            break;
                        } else if content[scan..].starts_with("</") || sc == ';' || (sc == '\n' && scan > after_if + 120) {
                            break;
                        }
                        scan += sc.len_utf8();
                    }

                    if let Some(brace_pos) = found_brace {
                        let cond = content[after_if..brace_pos].trim();
                        let after_brace = brace_pos + 1;
                        if let Some(cb) = find_matching_close_brace(&content[after_brace..]) {
                            let body = &content[after_brace..after_brace + cb];
                            let has_markup = body.contains('<') || body.contains('@') || body.contains("class=");
                            let rest = &content[after_brace + cb + 1..];
                            let rest_trimmed = rest.trim_start();
                            let has_else = rest_trimmed.starts_with("@else") || rest_trimmed.starts_with("else");
                            let looks_like_cond = cond.contains('>') || cond.contains('<') || cond.contains("==")
                                || cond.contains("!=") || cond.contains("&&") || cond.contains("||")
                                || cond.contains(".value") || cond.starts_with('(');

                            if has_markup || has_else || looks_like_cond {
                                return Err(CompileError::new(
                                    file_path,
                                    content,
                                    i,
                                    2,
                                    format!("Bare 'if' block detected in template markup: 'if {}'", cond),
                                    Some(format!("Directives in ERM templates must begin with '@'.\nFix: Change 'if {}' to '@if {}'.", cond, cond)),
                                ).into());
                            }
                        }
                    }
                }
            }

            // Check bare 'for'
            if content[i..].starts_with("for") {
                let prev_ok = if i == 0 {
                    true
                } else {
                    let prev_c = content[..i].chars().next_back().unwrap();
                    !prev_c.is_alphanumeric() && prev_c != '_' && prev_c != '@' && prev_c != '.' && prev_c != '-'
                };
                let after_for = i + 3;
                let next_ok = if after_for < content.len() {
                    let next_c = content[after_for..].chars().next().unwrap();
                    next_c.is_whitespace() || next_c == '('
                } else {
                    false
                };

                if prev_ok && next_ok {
                    let mut scan = after_for;
                    let mut found_brace = None;
                    while scan < content.len() {
                        let sc = content[scan..].chars().next().unwrap();
                        if sc == '{' {
                            found_brace = Some(scan);
                            break;
                        } else if content[scan..].starts_with("</") || sc == ';' || (sc == '\n' && scan > after_for + 120) {
                            break;
                        }
                        scan += sc.len_utf8();
                    }

                    if let Some(brace_pos) = found_brace {
                        let header = content[after_for..brace_pos].trim();
                        if header.contains(" in ") || header.contains(" of ") || header.contains("let ") {
                            return Err(CompileError::new(
                                file_path,
                                content,
                                i,
                                3,
                                format!("Bare 'for' loop detected in template markup: 'for {}'", header),
                                Some(format!("Directives in ERM templates must begin with '@'.\nFix: Change 'for {}' to '@for {}' or use expression mapping '{{{}.map(...)}}'.", header, header, header)),
                            ).into());
                        }
                    }
                }
            }

            // Check bare 'else'
            if content[i..].starts_with("else") {
                let prev_ok = if i == 0 {
                    true
                } else {
                    let prev_c = content[..i].chars().next_back().unwrap();
                    !prev_c.is_alphanumeric() && prev_c != '_' && prev_c != '@' && prev_c != '.'
                };
                let after_else = i + 4;
                let next_ok = if after_else < content.len() {
                    let next_c = content[after_else..].chars().next().unwrap();
                    next_c.is_whitespace() || next_c == '{'
                } else {
                    false
                };

                if prev_ok && next_ok {
                    let mut before = i;
                    let mut preceded_by_close = false;
                    while before > 0 {
                        let bc = content[..before].chars().next_back().unwrap();
                        if bc.is_whitespace() {
                            before -= bc.len_utf8();
                        } else {
                            if bc == '}' {
                                preceded_by_close = true;
                            }
                            break;
                        }
                    }

                    if preceded_by_close {
                        return Err(CompileError::new(
                            file_path,
                            content,
                            i,
                            4,
                            "Bare 'else' detected in template markup".to_string(),
                            Some("Transition directives in ERM templates must use '@else' or '@else if'.\nFix: Change 'else' to '@else'.".to_string()),
                        ).into());
                    }
                }
            }

            // Check bare '.map(' outside braces
            if content[i..].starts_with(".map(") {
                let mut before = i;
                let mut id_chars = Vec::new();
                while before > 0 {
                    let bc = content[..before].chars().next_back().unwrap();
                    if bc.is_alphanumeric() || bc == '_' || bc == '$' {
                        id_chars.push(bc);
                        before -= bc.len_utf8();
                    } else {
                        break;
                    }
                }
                if !id_chars.is_empty() {
                    id_chars.reverse();
                    let coll_name: String = id_chars.into_iter().collect();
                    return Err(CompileError::new(
                        file_path,
                        content,
                        before,
                        i + 5 - before,
                        format!("Array mapping '{}.map(...)' was found outside expression braces", coll_name),
                        Some(format!("In ERM templates, functional expressions must be enclosed in braces.\nFix: Wrap the mapping in braces: '{{{}.map(...)}}'.", coll_name)),
                    ).into());
                }
            }

            // Check bare function declaration in template markup
            let is_fn = content[i..].starts_with("function ") || content[i..].starts_with("fn ") || content[i..].starts_with("async function ");
            if is_fn {
                let prev_ok = if i == 0 {
                    true
                } else {
                    let prev_c = content[..i].chars().next_back().unwrap();
                    !prev_c.is_alphanumeric() && prev_c != '_' && prev_c != '@' && prev_c != '.' && prev_c != '-'
                };
                if prev_ok {
                    // Check if preceded by 'export'
                    let is_exported = {
                        let mut before = i;
                        while before > 0 && content[..before].chars().next_back().unwrap().is_whitespace() {
                            before -= content[..before].chars().next_back().unwrap().len_utf8();
                        }
                        before >= 6 && &content[before - 6..before] == "export"
                    };

                    let kw_len = if content[i..].starts_with("async function ") { 15 } else if content[i..].starts_with("function ") { 9 } else { 3 };
                    if is_exported {
                        i += kw_len;
                        continue;
                    }

                    let after_kw = i + kw_len;
                    let mut found_brace = None;
                    if let Some(open_paren) = content[after_kw..].find('(') {
                        let paren_start = after_kw + open_paren;
                        let mut p_depth = 1;
                        let mut p_scan = paren_start + 1;
                        while p_scan < content.len() && p_depth > 0 {
                            let pc = content[p_scan..].chars().next().unwrap();
                            if pc == '(' { p_depth += 1; }
                            else if pc == ')' { p_depth -= 1; }
                            p_scan += pc.len_utf8();
                        }

                        let mut scan = p_scan;
                        while scan < content.len() {
                            let sc = content[scan..].chars().next().unwrap();
                            if sc == '{' {
                                found_brace = Some(scan);
                                break;
                            } else if content[scan..].starts_with("</") || sc == ';' || sc == '<' {
                                break;
                            }
                            scan += sc.len_utf8();
                        }
                    }

                    if let Some(brace_pos) = found_brace {
                        let fn_name = content[after_kw..brace_pos].split('(').next().unwrap_or("").trim();
                        let after_brace = brace_pos + 1;
                        if let Some(cb) = find_matching_close_brace(&content[after_brace..]) {
                            let body = &content[after_brace..after_brace + cb];
                            let is_component = body.contains('<') || body.contains('@');
                            if !is_component {
                                return Err(CompileError::new(
                                    file_path,
                                    content,
                                    i,
                                    kw_len + fn_name.len(),
                                    format!("Bare function declaration detected in template markup: 'function {}'", fn_name),
                                    Some(format!("JavaScript functions and helper logic must be placed inside a <script> block or component function.\nFix: Wrap the function in a <script> tag:\n<script>\nfunction {}(...) {{ ... }}\n</script>", fn_name)),
                                ).into());
                            }
                        }
                    }
                }
            }

            // Check bare variable declaration in template markup
            let is_var_decl = content[i..].starts_with("const ") || content[i..].starts_with("let ") || content[i..].starts_with("var ");
            if is_var_decl {
                let prev_ok = if i == 0 {
                    true
                } else {
                    let prev_c = content[..i].chars().next_back().unwrap();
                    !prev_c.is_alphanumeric() && prev_c != '_' && prev_c != '@' && prev_c != '.' && prev_c != '-'
                };
                if prev_ok {
                    let kw_len = if content[i..].starts_with("const ") { 6 } else if content[i..].starts_with("let ") { 4 } else { 4 };
                    let after_kw = i + kw_len;
                    let mut scan = after_kw;
                    let mut found_eq = None;
                    while scan < content.len() {
                        let sc = content[scan..].chars().next().unwrap();
                        if sc == '=' {
                            found_eq = Some(scan);
                            break;
                        } else if sc == ';' || sc == '\n' || content[scan..].starts_with("</") || sc == '<' {
                            break;
                        }
                        scan += sc.len_utf8();
                    }

                    if let Some(eq_pos) = found_eq {
                        let var_name = content[after_kw..eq_pos].trim();
                        if !var_name.is_empty() && var_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                            let kw = &content[i..i + kw_len - 1];
                            return Err(CompileError::new(
                                file_path,
                                content,
                                i,
                                kw_len + var_name.len(),
                                format!("Bare variable declaration detected in template markup: '{} {}'", kw, var_name),
                                Some(format!("JavaScript variable declarations and reactive state must be placed inside a <script> block or component function.\nFix: Move declarations into a <script> tag:\n<script>\n{} {} = ...;\n</script>", kw, var_name)),
                            ).into());
                        }
                    }
                }
            }
        }

        i += c.len_utf8();
    }

    Ok(())
}
