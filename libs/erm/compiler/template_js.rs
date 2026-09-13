use super::utils::{find_matching_close_brace, replace_word};
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

impl ScriptStyleRanges {
    pub fn new(content: &str) -> Self {
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

    pub fn get_skip_pos(&self, pos: usize) -> Option<usize> {
        for range in &self.ranges {
            if range.contains(&pos) {
                return Some(range.end);
            }
        }
        None
    }
}
