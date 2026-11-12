use super::utils::find_matching_close_brace;
use super::reactivity::get_event_attribute_name;

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
        } else if c == '{' {
            if let Some(close_idx) = find_matching_close_brace(&body[i + 1..]) {
                let brace_end = i + 1 + close_idx;
                let raw_expr = &body[i + 1..brace_end];
                let sub_expr = crate::frontend::transpile_expr_reactivity(raw_expr, state_vars)
                    .unwrap_or_else(|| raw_expr.to_string());
                
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
                    js_expr.push_str(&format!("${{registerEvent('{}', (event) => {{ ({})(event); }})}}", event_type, sub_expr));
                } else {
                    js_expr.push_str("${escapeHtml(");
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

pub struct ScriptStyleRanges {
    pub ranges: Vec<std::ops::Range<usize>>,
}

pub fn find_real_tag_ranges(content: &str, tag_name: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let open_tag = format!("<{}", tag_name);
    let close_tag = format!("</{}>", tag_name);
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut skip_until = 0;

    for (idx, ch) in content.char_indices() {
        if idx < skip_until {
            continue;
        }
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            continue;
        }
        if in_block_comment {
            if ch == '*' && content[idx..].starts_with("*/") {
                in_block_comment = false;
                skip_until = idx + 2;
            }
            continue;
        }

        if ch == '/' {
            if content[idx..].starts_with("//") {
                in_line_comment = true;
                skip_until = idx + 2;
                continue;
            } else if content[idx..].starts_with("/*") {
                in_block_comment = true;
                skip_until = idx + 2;
                continue;
            }
        }

        if content[idx..].starts_with(&open_tag) {
            let after_tag = idx + open_tag.len();
            if after_tag < content.len() {
                let next_ch = content[after_tag..].chars().next().unwrap_or(' ');
                if next_ch == '>' || next_ch.is_whitespace() {
                    if let Some(close_pos) = content[after_tag..].find(&close_tag) {
                        let close_idx = after_tag + close_pos + close_tag.len();
                        ranges.push(idx..close_idx);
                        skip_until = close_idx;
                        continue;
                    }
                }
            }
        }
    }
    ranges
}

impl ScriptStyleRanges {
    pub fn new(content: &str) -> Self {
        let mut ranges = find_real_tag_ranges(content, "script");
        ranges.extend(find_real_tag_ranges(content, "style"));
        Self { ranges }
    }

    pub fn get_skip_pos(&self, pos: usize) -> Option<usize> {
        for range in &self.ranges {
            if range.contains(&pos) {
                return Some(range.end);
            }
        }
        None
    }
}
