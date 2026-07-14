use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Map(HashMap<String, Value>),
    List(Vec<Value>),
}

impl Value {
    pub fn to_bool(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Map(_) => true,
            Value::List(_) => true,
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::Boolean(b) => if *b { 1.0 } else { 0.0 },
            Value::String(s) => s.parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Map(m) => {
                write!(f, "{{ ")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "\"{}\": {}", k, v)?;
                }
                write!(f, " }}")
            }
            Value::List(l) => {
                write!(f, "[ ")?;
                for (i, v) in l.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, " ]")
            }
        }
    }
}

#[derive(Clone)]
pub struct ErmEval {
    pub vars: HashMap<String, Value>,
}

impl ErmEval {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }

    pub fn set(&mut self, name: &str, val: Value) {
        self.vars.insert(name.to_string(), val);
    }

    pub fn eval(&mut self, expr: &str) -> anyhow::Result<Value> {
        let mut parser = ExprParser { input: expr, pos: 0, ev: self };
        parser.parse_expr()
    }

    pub fn eval_bool(&mut self, expr: &str) -> anyhow::Result<bool> {
        Ok(self.eval(expr)?.to_bool())
    }

    pub fn parse_script_vars(&mut self, script: &str) -> anyhow::Result<()> {
        let mut i = 0;
        let keywords = ["let", "const", "var"];
        while i < script.len() {
            let mut found = false;
            for kw in keywords {
                if script[i..].starts_with(kw) && i + kw.len() < script.len() && script.as_bytes()[i + kw.len()].is_ascii_whitespace() {
                    let mut j = i + kw.len();
                    while j < script.len() && script.as_bytes()[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let name_start = j;
                    while j < script.len() && (script.as_bytes()[j].is_ascii_alphanumeric() || script.as_bytes()[j] == b'_' || script.as_bytes()[j] == b'$') {
                        j += 1;
                    }
                    let name = &script[name_start..j];

                    while j < script.len() && script.as_bytes()[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < script.len() && script.as_bytes()[j] == b'=' {
                        j += 1;
                        let mut end_pos = j;
                        while end_pos < script.len() && script.as_bytes()[end_pos] != b';' && script.as_bytes()[end_pos] != b'\n' {
                            end_pos += 1;
                        }
                        let expr_str = script[j..end_pos].trim();
                        let (val, _) = parse_js_value(expr_str, Some(&self.vars))?;
                        if let Some(v) = val {
                            self.set(name, v);
                        } else if let Ok(v) = self.eval(expr_str) {
                            self.set(name, v);
                        }
                        i = end_pos;
                        if i < script.len() && script.as_bytes()[i] == b';' {
                            i += 1;
                        }
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                let c = script[i..].chars().next().unwrap();
                i += c.len_utf8();
            }
        }
        Ok(())
    }
}

struct ExprParser<'a> {
    input: &'a str,
    pos: usize,
    ev: &'a mut ErmEval,
}

impl<'a> ExprParser<'a> {
    fn skip(&mut self) {
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn parse_expr(&mut self) -> anyhow::Result<Value> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> anyhow::Result<Value> {
        let mut left = self.parse_and()?;
        loop {
            self.skip();
            if self.pos + 2 <= self.input.len() && &self.input[self.pos..self.pos + 2] == "||" {
                self.pos += 2;
                let right = self.parse_and()?;
                if !left.to_bool() {
                    left = right;
                }
            } else { break; }
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> anyhow::Result<Value> {
        let mut left = self.parse_comparison()?;
        loop {
            self.skip();
            if self.pos + 2 <= self.input.len() && &self.input[self.pos..self.pos + 2] == "&&" {
                self.pos += 2;
                let right = self.parse_comparison()?;
                if left.to_bool() {
                    left = right;
                }
            } else { break; }
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> anyhow::Result<Value> {
        let left = self.parse_add_sub()?;
        self.skip();
        if self.pos >= self.input.len() { return Ok(left); }

        let ops = ["===", "!==", "==", "!=", ">=", "<=", ">", "<"];
        let mut found_op = None;
        for op in ops {
            if self.input[self.pos..].starts_with(op) {
                found_op = Some(op);
                break;
            }
        }

        if let Some(op) = found_op {
            self.pos += op.len();
            let right = self.parse_add_sub()?;
            let lf = left.to_number();
            let rf = right.to_number();

            let res = match op {
                ">" => Value::Boolean(lf > rf),
                "<" => Value::Boolean(lf < rf),
                ">=" => Value::Boolean(lf >= rf),
                "<=" => Value::Boolean(lf <= rf),
                "==" | "===" => Value::Boolean(left == right),
                "!=" | "!==" => Value::Boolean(left != right),
                _ => left,
            };
            return Ok(res);
        }
        Ok(left)
    }

    fn parse_add_sub(&mut self) -> anyhow::Result<Value> {
        let mut left = self.parse_mul_div()?;
        loop {
            self.skip();
            if self.pos >= self.input.len() { break; }
            let c = self.input.as_bytes()[self.pos] as char;
            if c == '+' || c == '-' {
                self.pos += 1;
                let right = self.parse_mul_div()?;
                if c == '+' {
                    if let (Value::String(s1), _) = (&left, &right) {
                        left = Value::String(format!("{}{}", s1, right));
                    } else if let (_, Value::String(s2)) = (&left, &right) {
                        left = Value::String(format!("{}{}", left, s2));
                    } else {
                        left = Value::Number(left.to_number() + right.to_number());
                    }
                } else {
                    left = Value::Number(left.to_number() - right.to_number());
                }
            } else { break; }
        }
        Ok(left)
    }

    fn parse_mul_div(&mut self) -> anyhow::Result<Value> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip();
            if self.pos >= self.input.len() { break; }
            let c = self.input.as_bytes()[self.pos] as char;
            if c == '*' || c == '/' || c == '%' {
                self.pos += 1;
                let right = self.parse_unary()?;
                let lf = left.to_number();
                let rf = right.to_number();
                left = match c {
                    '*' => Value::Number(lf * rf),
                    '/' => Value::Number(if rf == 0.0 { 0.0 } else { lf / rf }),
                    '%' => Value::Number(if rf == 0.0 { 0.0 } else { lf % rf }),
                    _ => unreachable!(),
                };
            } else { break; }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> anyhow::Result<Value> {
        self.skip();
        if self.pos < self.input.len() {
            let c = self.input.as_bytes()[self.pos] as char;
            if c == '!' {
                self.pos += 1;
                let val = self.parse_unary()?;
                return Ok(Value::Boolean(!val.to_bool()));
            }
            if c == '-' {
                self.pos += 1;
                let val = self.parse_unary()?;
                return Ok(Value::Number(-val.to_number()));
            }
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> anyhow::Result<Value> {
        self.skip();
        if self.pos >= self.input.len() { anyhow::bail!("Unexpected end of input"); }
        let c = self.input.as_bytes()[self.pos] as char;

        if c == '(' {
            self.pos += 1;
            let val = self.parse_expr()?;
            self.skip();
            if self.pos < self.input.len() && self.input.as_bytes()[self.pos] == b')' {
                self.pos += 1;
            }
            return Ok(val);
        }

        if c == '[' || c == '{' {
            let (val, next_p) = parse_js_value(&self.input[self.pos..], Some(&self.ev.vars))?;
            self.pos += next_p;
            return Ok(val.unwrap_or(Value::Null));
        }

        if c.is_ascii_digit() || c == '.' {
            let start = self.pos;
            while self.pos < self.input.len() && (self.input.as_bytes()[self.pos].is_ascii_digit() || self.input.as_bytes()[self.pos] == b'.') {
                self.pos += 1;
            }
            let n = self.input[start..self.pos].parse::<f64>()?;
            return Ok(Value::Number(n));
        }

        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            self.pos += 1;
            let mut s = String::new();
            while self.pos < self.input.len() && self.input.as_bytes()[self.pos] as char != quote {
                if self.input.as_bytes()[self.pos] == b'\\' && self.pos + 1 < self.input.len() {
                    self.pos += 1;
                    match self.input.as_bytes()[self.pos] as char {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        c => s.push(c),
                    }
                } else {
                    s.push(self.input.as_bytes()[self.pos] as char);
                }
                self.pos += 1;
            }
            if self.pos < self.input.len() { self.pos += 1; }
            return Ok(Value::String(s));
        }

        if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let start = self.pos;
            while self.pos < self.input.len() && (self.input.as_bytes()[self.pos].is_ascii_alphanumeric() || self.input.as_bytes()[self.pos] == b'_' || self.input.as_bytes()[self.pos] == b'$') {
                self.pos += 1;
            }
            let name = &self.input[start..self.pos];

            if name == "true" { return Ok(Value::Boolean(true)); }
            if name == "false" { return Ok(Value::Boolean(false)); }
            if name == "null" || name == "undefined" { return Ok(Value::Null); }

            let mut val = self.ev.vars.get(name).cloned().unwrap_or(Value::Null);
            while self.pos < self.input.len() && self.input.as_bytes()[self.pos] == b'.' {
                self.pos += 1;
                let p_start = self.pos;
                while self.pos < self.input.len() && (self.input.as_bytes()[self.pos].is_ascii_alphanumeric() || self.input.as_bytes()[self.pos] == b'_' || self.input.as_bytes()[self.pos] == b'$') {
                    self.pos += 1;
                }
                let prop = &self.input[p_start..self.pos];
                if let Value::Map(ref m) = val {
                    val = m.get(prop).cloned().unwrap_or(Value::Null);
                } else {
                    val = Value::Null;
                }
            }
            return Ok(val);
        }

        anyhow::bail!("Unexpected character: {}", c)
    }
}

pub fn parse_js_value(s: &str, vars: Option<&HashMap<String, Value>>) -> anyhow::Result<(Option<Value>, usize)> {
    let mut p = 0;
    while p < s.len() && s.as_bytes()[p].is_ascii_whitespace() {
        p += 1;
    }
    if p >= s.len() { return Ok((None, p)); }

    if s[p..].starts_with("useState(") {
        p += 9;
        let mut depth = 1;
        let start = p;
        while p < s.len() && depth > 0 {
            let c = s.as_bytes()[p];
            if c == b'(' { depth += 1; }
            else if c == b')' { depth -= 1; }
            p += 1;
        }
        let full_arg_expr = s[start..p-1].trim();
        let mut arg_expr = full_arg_expr;
        let mut brace_depth = 0;
        let mut in_quote = None;
        for (idx, c) in full_arg_expr.chars().enumerate() {
            if let Some(q) = in_quote {
                if c == q { in_quote = None; }
            } else if c == '"' || c == '\'' || c == '`' {
                in_quote = Some(c);
            } else if c == '(' || c == '[' || c == '{' {
                brace_depth += 1;
            } else if c == ')' || c == ']' || c == '}' {
                brace_depth -= 1;
            } else if c == ',' && brace_depth == 0 {
                arg_expr = &full_arg_expr[..idx];
                break;
            }
        }
        let arg_expr = arg_expr.trim();
        let mut ev_temp = ErmEval::new();
        if let Some(v_map) = vars {
            ev_temp.vars = v_map.clone();
        }
        let inner = ev_temp.eval(arg_expr).ok();
        let mut m = HashMap::new();
        if let Some(v) = inner {
            m.insert("value".to_string(), v);
        }
        return Ok((Some(Value::Map(m)), p));
    }

    if s.as_bytes()[p] == b'"' || s.as_bytes()[p] == b'\'' || s.as_bytes()[p] == b'`' {
        let quote = s.as_bytes()[p];
        p += 1;
        let start = p;
        while p < s.len() && s.as_bytes()[p] != quote { p += 1; }
        let val = s[start..p].to_string();
        if p < s.len() { p += 1; }
        return Ok((Some(Value::String(val)), p));
    }

    if s.as_bytes()[p].is_ascii_digit() || s.as_bytes()[p] == b'-' {
        let start = p;
        if s.as_bytes()[p] == b'-' { p += 1; }
        while p < s.len() && (s.as_bytes()[p].is_ascii_digit() || s.as_bytes()[p] == b'.') { p += 1; }
        let n = s[start..p].parse::<f64>().unwrap_or(0.0);
        return Ok((Some(Value::Number(n)), p));
    }

    if s.as_bytes()[p] == b'[' {
        p += 1;
        let mut list = Vec::new();
        while p < s.len() && s.as_bytes()[p] != b']' {
            let (inner, next_p) = parse_js_value(&s[p..], vars)?;
            if let Some(v) = inner {
                list.push(v);
            }
            p += next_p;
            while p < s.len() && (s.as_bytes()[p] == b',' || s.as_bytes()[p].is_ascii_whitespace()) { p += 1; }
        }
        if p < s.len() { p += 1; }
        return Ok((Some(Value::List(list)), p));
    }

    if s.as_bytes()[p] == b'{' {
        p += 1;
        let mut map = HashMap::new();
        while p < s.len() && s.as_bytes()[p] != b'}' {
            while p < s.len() && s.as_bytes()[p].is_ascii_whitespace() { p += 1; }
            let key = if s.as_bytes()[p] == b'"' || s.as_bytes()[p] == b'\'' {
                let quote = s.as_bytes()[p];
                p += 1;
                let start = p;
                while p < s.len() && s.as_bytes()[p] != quote { p += 1; }
                let k_val = s[start..p].to_string();
                if p < s.len() { p += 1; }
                k_val
            } else {
                let start = p;
                while p < s.len() && (s.as_bytes()[p].is_ascii_alphanumeric() || s.as_bytes()[p] == b'_' || s.as_bytes()[p] == b'$') { p += 1; }
                s[start..p].to_string()
            };
            while p < s.len() && (s.as_bytes()[p].is_ascii_whitespace() || s.as_bytes()[p] == b':') { p += 1; }
            let (inner, next_p) = parse_js_value(&s[p..], vars)?;
            if let Some(v) = inner {
                map.insert(key, v);
            }
            p += next_p;
            while p < s.len() && (s.as_bytes()[p] == b',' || s.as_bytes()[p].is_ascii_whitespace()) { p += 1; }
        }
        if p < s.len() { p += 1; }
        return Ok((Some(Value::Map(map)), p));
    }

    if s[p..].starts_with("true") { return Ok((Some(Value::Boolean(true)), p + 4)); }
    if s[p..].starts_with("false") { return Ok((Some(Value::Boolean(false)), p + 5)); }
    if s[p..].starts_with("null") { return Ok((Some(Value::Null), p + 4)); }
    if s[p..].starts_with("undefined") { return Ok((Some(Value::Null), p + 9)); }

    Ok((None, p))
}
