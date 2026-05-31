use std::collections::HashMap;

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
        if line_trimmed.is_empty() {
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
        i += 1;
    }
    Ok(())
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
    if trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_string();
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
