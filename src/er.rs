use std::collections::HashMap;
use regex::Regex;

#[derive(Clone, Debug)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub handler_lines: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Variable {
    pub value: String,
    pub is_mutable: bool,
    pub decl_line: usize,
    pub decl_path: String,
}

pub fn evaluate_file(
    path: &str,
    variables: &mut HashMap<String, Variable>,
    routes: &mut Vec<Route>,
) -> anyhow::Result<()> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    let lines: Vec<&str> = content.lines().collect();
    let mut if_was_executed = false;
    execute_statements(&lines, variables, &mut if_was_executed, routes, path, 0)
}

pub fn run_file(path: &str) -> anyhow::Result<()> {
    let mut variables = HashMap::new();
    let mut routes = Vec::new();
    evaluate_file(path, &mut variables, &mut routes)
}

pub fn execute_statements(
    lines: &[&str],
    variables: &mut HashMap<String, Variable>,
    if_was_executed: &mut bool,
    routes: &mut Vec<Route>,
    path: &str,
    line_offset: usize,
) -> anyhow::Result<()> {
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let mut line_trimmed = line.trim();
        if line_trimmed.is_empty() || line_trimmed.starts_with("//") {
            i += 1;
            continue;
        }
        if line_trimmed.ends_with(';') {
            line_trimmed = line_trimmed[..line_trimmed.len() - 1].trim();
        }
        let trimmed = line_trimmed;

        if trimmed.starts_with("for ") {
            if let Some(in_idx) = trimmed.find(" in ") {
                if let Some(dotdot_idx) = trimmed.find("..") {
                    if let Some(brace_idx) = trimmed.find('{') {
                        let var_name = trimmed[4..in_idx].trim();
                        let start_expr = trimmed[in_idx + 4..dotdot_idx].trim();
                        let end_expr = trimmed[dotdot_idx + 2..brace_idx].trim();

                        let start_val_s = evaluate_expression(start_expr, variables);
                        let end_val_s = evaluate_expression(end_expr, variables);

                        let start_val = start_val_s.parse::<i64>().unwrap_or(0);
                        let end_val = end_val_s.parse::<i64>().unwrap_or(0);

                        let block_end = find_closing_brace(lines, i);
                        let block_lines = &lines[i + 1..block_end];

                        for loop_val in start_val..=end_val {
                            variables.insert(
                                var_name.to_string(),
                                Variable {
                                    value: loop_val.to_string(),
                                    is_mutable: true,
                                    decl_line: line_offset + i,
                                    decl_path: path.to_string(),
                                },
                            );
                            let mut dummy_if = false;
                            execute_statements(
                                block_lines,
                                variables,
                                &mut dummy_if,
                                routes,
                                path,
                                line_offset + i + 1,
                            )?;
                        }
                        i = block_end;
                        *if_was_executed = false;
                    }
                }
            }
        } else if trimmed.starts_with("if") {
            if let Some(start_p) = trimmed.find('(') {
                if let Some(end_p) = trimmed.rfind(')') {
                    let cond_str = &trimmed[start_p + 1..end_p];
                    let cond_result = evaluate_condition(cond_str, variables);

                    let block_end = find_closing_brace(lines, i);
                    let block_lines = &lines[i + 1..block_end];

                    if cond_result {
                        let mut dummy_if = false;
                        execute_statements(
                            block_lines,
                            variables,
                            &mut dummy_if,
                            routes,
                            path,
                            line_offset + i + 1,
                        )?;
                        *if_was_executed = true;
                    } else {
                        *if_was_executed = false;
                    }
                    i = block_end;
                }
            }
        } else if trimmed.starts_with("else") {
            let block_end = find_closing_brace(lines, i);
            let block_lines = &lines[i + 1..block_end];

            if !*if_was_executed {
                let mut dummy_if = false;
                execute_statements(
                    block_lines,
                    variables,
                    &mut dummy_if,
                    routes,
                    path,
                    line_offset + i + 1,
                )?;
            }
            i = block_end;
            *if_was_executed = false;
        } else if trimmed.starts_with("print(") {
            if let Some(open_p) = trimmed.find('(') {
                if let Some(close_p) = trimmed.rfind(')') {
                    let arg = trimmed[open_p + 1..close_p].trim();
                    if arg.starts_with('"') && arg.ends_with('"') {
                        let content = &arg[1..arg.len() - 1];
                        let mut output = String::new();
                        let mut pos = 0;
                        while let Some(start) = content[pos..].find('{') {
                            output.push_str(&content[pos..pos + start]);
                            let open_brace = pos + start;
                            if let Some(end) = content[open_brace..].find('}') {
                                let close_brace = open_brace + end;
                                let expr = &content[open_brace + 1..close_brace];
                                output.push_str(&evaluate_expression(expr, variables));
                                pos = close_brace + 1;
                            } else {
                                break;
                            }
                        }
                        output.push_str(&content[pos..]);
                        println!("{}", output);
                    } else {
                        println!("{}", evaluate_expression(arg, variables));
                    }
                }
            }
            *if_was_executed = false;
        } else if trimmed.starts_with("return") {
            return Ok(());
        } else if let Some(dot_idx) = trimmed.find('.') {
            if let Some(open_p) = trimmed[dot_idx..].find('(') {
                let actual_open_p = dot_idx + open_p;
                let method_name = trimmed[dot_idx + 1..actual_open_p].trim();
                let methods = ["get", "post", "put", "delete", "patch"];
                
                if methods.contains(&method_name) {
                    let first_line = trimmed;
                    let comma_idx = first_line.find(',').unwrap_or(first_line.len());
                    let path_raw = first_line[actual_open_p + 1..comma_idx].trim().trim_matches(|c| c == '\'' || c == '"');
                    
                    let block_end = find_closing_brace(lines, i);
                    let handler_lines = lines[i + 1..block_end].iter().map(|s| s.to_string()).collect();
                    
                    routes.push(Route {
                        method: method_name.to_uppercase(),
                        path: path_raw.to_string(),
                        handler_lines,
                    });
                    i = block_end;
                }
            }
        } else if let Some(index) = trimmed.find('=') {
            // Assignment logic
            let mut is_decl = false;
            let mut current_is_mutable = true;

            let mut decl_part = trimmed[..index].trim();
            if decl_part.starts_with("let ") {
                decl_part = decl_part[4..].trim();
                is_decl = true;
                current_is_mutable = true;
            } else if decl_part.starts_with("const ") {
                decl_part = decl_part[6..].trim();
                is_decl = true;
                current_is_mutable = false;
            }

            let var_name = if let Some(colon_idx) = decl_part.find(':') {
                decl_part[..colon_idx].trim()
            } else {
                decl_part
            };

            if !is_decl {
                if let Some(old_v) = variables.get(var_name) {
                    if !old_v.is_mutable {
                        anyhow::bail!("Cannot assign to \"{}\" because it is a constant", var_name);
                    }
                    current_is_mutable = old_v.is_mutable;
                }
            }

            let val_raw = trimmed[index + 1..].trim();
            
            if val_raw.starts_with('{') || val_raw.starts_with('[') {
                let block_end = find_closing_brace(lines, i);
                let mut full_val = val_raw.to_string();
                for j in i + 1..=block_end {
                    full_val.push('\n');
                    full_val.push_str(lines[j]);
                }
                i = block_end;
                
                // Try to parse as JSON to flatten it
                if let Ok(json_val) = parse_json(&full_val) {
                    flatten_json(var_name, &json_val, variables, line_offset + i, path);
                } else {
                    variables.insert(
                        var_name.to_string(),
                        Variable {
                            value: full_val,
                            is_mutable: current_is_mutable,
                            decl_line: line_offset + i,
                            decl_path: path.to_string(),
                        },
                    );
                }
            } else {
                let val = evaluate_expression(val_raw, variables);
                variables.insert(
                    var_name.to_string(),
                    Variable {
                        value: val,
                        is_mutable: current_is_mutable,
                        decl_line: line_offset + i,
                        decl_path: path.to_string(),
                    },
                );
            }
        }
        i += 1;
    }
    Ok(())
}

fn parse_json(s: &str) -> anyhow::Result<serde_json::Value> {
    let mut hack = s.to_string();
    
    // 1. Quote unquoted keys: { id: 1 } -> { "id": 1 }
    // We look for a word followed by a colon, preceded by {, comma, or start of string
    let re_keys = Regex::new(r"([{,]\s*)(\w+)\s*:").unwrap();
    hack = re_keys.replace_all(&hack, "$1\"$2\":").to_string();
    
    // 2. Handle first key in an object if it's at the start of the string
    let re_first_key = Regex::new(r"^\s*(\w+)\s*:").unwrap();
    hack = re_first_key.replace_all(&hack, "\"$1\":").to_string();

    // 3. Remove trailing commas: [1, 2, ] -> [1, 2]
    let re_comma = Regex::new(r",\s*([\]}])").unwrap();
    hack = re_comma.replace_all(&hack, "$1").to_string();
    
    // 4. Convert single quotes to double quotes (naive but helpful)
    // Only if not already valid JSON
    if let Ok(v) = serde_json::from_str(&hack) {
        return Ok(v);
    }
    
    let hack2 = hack.replace('\'', "\"");
    if let Ok(v) = serde_json::from_str(&hack2) {
        return Ok(v);
    }

    // Fallback to original
    serde_json::from_str(s).map_err(|e| e.into())
}

fn flatten_json(prefix: &str, val: &serde_json::Value, variables: &mut HashMap<String, Variable>, line: usize, path: &str) {
    match val {
        serde_json::Value::Object(map) => {
            variables.insert(prefix.to_string(), Variable {
                value: serde_json::to_string(val).unwrap_or_default(),
                is_mutable: true,
                decl_line: line,
                decl_path: path.to_string(),
            });
            for (k, v) in map {
                let new_prefix = format!("{}.{}", prefix, k);
                flatten_json(&new_prefix, v, variables, line, path);
            }
        }
        serde_json::Value::Array(list) => {
            variables.insert(prefix.to_string(), Variable {
                value: serde_json::to_string(val).unwrap_or_default(),
                is_mutable: true,
                decl_line: line,
                decl_path: path.to_string(),
            });
            for (i, v) in list.iter().enumerate() {
                let new_prefix = format!("{}.{}", prefix, i);
                flatten_json(&new_prefix, v, variables, line, path);
            }
        }
        _ => {
            variables.insert(prefix.to_string(), Variable {
                value: val.to_string().trim_matches('"').to_string(),
                is_mutable: true,
                decl_line: line,
                decl_path: path.to_string(),
            });
        }
    }
}

pub fn handle_api_request(
    request: &mut tiny_http::Request,
    api_file_path: &str,
    base_path: &str,
) -> anyhow::Result<Option<tiny_http::Response<std::io::Cursor<Vec<u8>>>>> {
    let mut variables = HashMap::new();
    let mut routes = Vec::new();
    
    // Handle POST body if present
    if request.method() == &tiny_http::Method::Post {
        let mut body = String::new();
        // Note: this consumes the body reader
        request.as_reader().read_to_string(&mut body).ok();
        if let Ok(json_body) = serde_json::from_str::<serde_json::Value>(&body) {
            flatten_json("body", &json_body, &mut variables, 0, api_file_path);
        }
    }
    
    evaluate_file(api_file_path, &mut variables, &mut routes)?;

    let target = request.url();
    let mut clean_target = target;
    if let Some(idx) = clean_target.find('?') { clean_target = &clean_target[..idx]; }
    if let Some(idx) = clean_target.find('#') { clean_target = &clean_target[..idx]; }
    
    let mut clean_target_str = clean_target.to_string();
    if clean_target_str.len() > 1 && clean_target_str.ends_with('/') {
        clean_target_str.pop();
    }
    let clean_target = clean_target_str.as_str();

    for route in routes {
        if route.method == request.method().to_string() {
            let mut full_route_path = base_path.to_string();
            if !full_route_path.ends_with('/') && !route.path.starts_with('/') {
                full_route_path.push('/');
            }
            if full_route_path.ends_with('/') && route.path.starts_with('/') {
                full_route_path.push_str(&route.path[1..]);
            } else {
                full_route_path.push_str(&route.path);
            }

            // Normalize: remove trailing slash if not root
            if full_route_path.len() > 1 && full_route_path.ends_with('/') {
                full_route_path.pop();
            }
            
            let mut match_route = full_route_path == clean_target;
            if !match_route {
                if full_route_path == "/" && (clean_target == "" || clean_target == "/") {
                    match_route = true;
                }
            }

            if match_route {
                for h_line in route.handler_lines {
                    let h_trimmed = h_line.trim();
                    if let Some(json_idx) = h_trimmed.find("c.json(") {
                        if let Some(o_p) = h_trimmed[json_idx..].find('(') {
                            let actual_o_p = json_idx + o_p;
                            if let Some(c_p) = h_trimmed.rfind(')') {
                                let data_expr = &h_trimmed[actual_o_p + 1..c_p];
                                let data_val = evaluate_expression(data_expr, &variables);
                                
                                let response = tiny_http::Response::from_string(data_val)
                                    .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
                                    .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                                return Ok(Some(response));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

fn find_closing_brace(lines: &[&str], start_idx: usize) -> usize {
    let mut depth = 0;
    for (i, line) in lines.iter().enumerate().skip(start_idx) {
        for c in line.chars() {
            if c == '{' || c == '[' {
                depth += 1;
            }
            if c == '}' || c == ']' {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        return i;
                    }
                }
            }
        }
    }
    lines.len()
}

fn evaluate_expression(expr: &str, variables: &HashMap<String, Variable>) -> String {
    let trimmed = expr.trim();
    if trimmed.is_empty() { return String::new(); }
    
    if trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return trimmed.to_string();
    }
    
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return trimmed.to_string();
    }

    if let Some(v) = variables.get(trimmed) {
        return v.value.clone();
    }
    
    // Basic math
    if let Some(plus_idx) = trimmed.find('+') {
        let left = evaluate_expression(&trimmed[..plus_idx], variables);
        let right = evaluate_expression(&trimmed[plus_idx + 1..], variables);
        if let (Ok(l), Ok(r)) = (left.parse::<i64>(), right.parse::<i64>()) {
            return (l + r).to_string();
        }
    }
    trimmed.to_string()
}

fn evaluate_condition(condition: &str, variables: &HashMap<String, Variable>) -> bool {
    let ops = ["==", "!=", ">=", "<=", ">", "<"];
    for op in ops {
        if let Some(idx) = condition.find(op) {
            let left = evaluate_expression(&condition[..idx], variables);
            let right = evaluate_expression(&condition[idx + op.len()..], variables);
            return match op {
                "==" => left == right,
                "!=" => left != right,
                ">" => left.parse::<i64>().unwrap_or(0) > right.parse::<i64>().unwrap_or(0),
                "<" => left.parse::<i64>().unwrap_or(0) < right.parse::<i64>().unwrap_or(0),
                ">=" => left.parse::<i64>().unwrap_or(0) >= right.parse::<i64>().unwrap_or(0),
                "<=" => left.parse::<i64>().unwrap_or(0) <= right.parse::<i64>().unwrap_or(0),
                _ => false,
            };
        }
    }
    false
}
