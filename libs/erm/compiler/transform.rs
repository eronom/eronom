use std::sync::OnceLock;

pub fn is_function_template(content: &str) -> bool {
    if content.contains("<script") || content.contains("<SCRIPT") {
        return false;
    }
    static RE_FN: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE_FN.get_or_init(|| {
        regex::Regex::new(r"(?m)^\s*export\s+(?:default\s+)?(?:fn|function)\s+([A-Za-z0-9_]+)\s*\(([^)]*)\)\s*\{").unwrap()
    });
    re.is_match(content)
}

pub fn inject_line_attr(markup: &str, line_num: usize) -> String {
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

pub fn preprocess_function_template(content: &str) -> anyhow::Result<String> {
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
                    || trimmed.starts_with("return");
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

pub fn transform_use_effect(input: &str) -> String {
    let mut res = String::new();
    let mut i = 0;
    while i < input.len() {
        if input[i..].starts_with("useEffect") {
            let mut j = i + 9;
            while j < input.len() {
                let c = input[j..].chars().next().unwrap();
                if c.is_whitespace() {
                    j += c.len_utf8();
                } else {
                    break;
                }
            }
            if j < input.len() && input[j..].starts_with('(') {
                let mut depth = 0;
                let mut brace_depth = 0;
                let mut bracket_depth = 0;
                let mut in_string: Option<char> = None;
                let mut k = j;
                let mut comma_pos = None;
                let mut array_start_pos = None;
                let mut array_end_pos = None;
                
                while k < input.len() {
                    let c = input[k..].chars().next().unwrap();
                    if let Some(quote) = in_string {
                        if c == quote && (k == 0 || input[..k].chars().last() != Some('\\')) {
                            in_string = None;
                        }
                        k += c.len_utf8();
                        continue;
                    }
                    if c == '"' || c == '\'' || c == '`' {
                        in_string = Some(c);
                        k += 1;
                        continue;
                    }
                    
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if c == '{' {
                        brace_depth += 1;
                    } else if c == '}' {
                        brace_depth -= 1;
                    } else if c == '[' {
                        bracket_depth += 1;
                        if depth == 1 && brace_depth == 0 && bracket_depth == 1 {
                            if comma_pos.is_some() {
                                array_start_pos = Some(k);
                            }
                        }
                    } else if c == ']' {
                        if depth == 1 && brace_depth == 0 && bracket_depth == 1 && array_start_pos.is_some() {
                            array_end_pos = Some(k);
                        }
                        bracket_depth -= 1;
                    } else if c == ',' {
                        if depth == 1 && brace_depth == 0 && bracket_depth == 0 {
                            comma_pos = Some(k);
                        }
                    }
                    k += c.len_utf8();
                }
                
                if let (Some(cp), Some(asp), Some(aep)) = (comma_pos, array_start_pos, array_end_pos) {
                    let callback_part = &input[j + 1..cp];
                    let deps_content = &input[asp + 1..aep];
                    
                    res.push_str("useEffect(");
                    res.push_str(&transform_use_effect(callback_part));
                    res.push_str(", () => [");
                    res.push_str(&transform_use_effect(deps_content));
                    res.push_str("])");
                    
                    i = k + 1;
                    continue;
                }
            }
        }
        let c = input[i..].chars().next().unwrap();
        res.push(c);
        i += c.len_utf8();
    }
    res
}
