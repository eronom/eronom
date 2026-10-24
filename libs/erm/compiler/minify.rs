/// Native minifier utilities for Eronom production builds.

/// Minifies CSS by stripping comments and compressing whitespace while preserving strings.
pub fn minify_css(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let chars: Vec<char> = css.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < len {
        let c = chars[i];

        // Handle string literals
        if c == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            out.push(c);
            i += 1;
            continue;
        }

        if in_single_quote || in_double_quote {
            out.push(c);
            i += 1;
            continue;
        }

        // Handle CSS comments: /* ... */
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2; // skip */
            continue;
        }

        // Collapse whitespace
        if c.is_whitespace() {
            let prev = out.chars().last();
            let mut next_non_ws = None;
            let mut j = i + 1;
            while j < len {
                if !chars[j].is_whitespace() {
                    next_non_ws = Some(chars[j]);
                    break;
                }
                j += 1;
            }

            if let (Some(p), Some(n)) = (prev, next_non_ws) {
                let p_is_ident = p.is_ascii_alphanumeric() || p == '-' || p == '_' || p == '%';
                let n_is_ident = n.is_ascii_alphanumeric() || n == '-' || n == '_' || n == '%';
                if p_is_ident && n_is_ident {
                    out.push(' ');
                }
            }
            i = j;
            continue;
        }

        // Remove redundant space around punctuation: '{', '}', ':', ';', ','
        if c == ';' {
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            if j < len && chars[j] == '}' {
                i = j;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }

    out
}

/// Minifies JavaScript by removing comments and compressing whitespace outside of string literals.
pub fn minify_js(js: &str) -> String {
    let mut out = String::with_capacity(js.len());
    let chars: Vec<char> = js.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_template = false;
    let mut escaped = false;

    while i < len {
        let c = chars[i];

        if escaped {
            out.push(c);
            escaped = false;
            i += 1;
            continue;
        }

        if c == '\\' && (in_single_quote || in_double_quote || in_template) {
            escaped = true;
            out.push(c);
            i += 1;
            continue;
        }

        // String & template literal state machine
        if c == '\'' && !in_double_quote && !in_template {
            in_single_quote = !in_single_quote;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '"' && !in_single_quote && !in_template {
            in_double_quote = !in_double_quote;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '`' && !in_single_quote && !in_double_quote {
            in_template = !in_template;
            out.push(c);
            i += 1;
            continue;
        }

        if in_single_quote || in_double_quote || in_template {
            out.push(c);
            i += 1;
            continue;
        }

        // Single-line comment: //
        if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            i += 2;
            while i < len && chars[i] != '\n' && chars[i] != '\r' {
                i += 1;
            }
            continue;
        }

        // Multi-line comment: /* ... */
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }

        // Whitespace handling
        if c.is_whitespace() {
            let prev = out.chars().last();
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            let next_char = if j < len { Some(chars[j]) } else { None };

            if let (Some(p), Some(n)) = (prev, next_char) {
                let p_is_ident = p.is_ascii_alphanumeric() || p == '_' || p == '$';
                let n_is_ident = n.is_ascii_alphanumeric() || n == '_' || n == '$';
                if (p_is_ident && n_is_ident)
                    || (p == ')' && n == '=')
                    || (p == '>' && (n == '(' || n_is_ident))
                    || (p == '+' && n == '+')
                    || (p == '-' && n == '-')
                {
                    out.push(' ');
                }
            }
            i = j;
            continue;
        }

        out.push(c);
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minify_css() {
        let css = r#"
            /* Main Header Style */
            .header {
                color: #ffffff;
                margin: 0px 10px;
                background: url("/* not a comment */");
            }
            .btn {
                padding: 10px;
            }
        "#;
        let min = minify_css(css);
        assert!(!min.contains("Main Header Style"));
        assert!(min.contains(".header{color:#ffffff;margin:0px 10px;background:url(\"/* not a comment */\")}"));
        assert!(min.contains(".btn{padding:10px}"));
    }

    #[test]
    fn test_minify_js() {
        let js = r#"
            // Setup counter
            let count = 0;
            /* Update function */
            function increment() {
                const url = "http://example.com/api"; // URL
                count += 1;
                return `Count: ${count} // ok`;
            }
        "#;
        let min = minify_js(js);
        assert!(!min.contains("Setup counter"));
        assert!(!min.contains("Update function"));
        assert!(min.contains("let count=0;"));
        assert!(min.contains("function increment()"));
        assert!(min.contains("\"http://example.com/api\""));
        assert!(min.contains("`Count: ${count} // ok`"));
    }
}
