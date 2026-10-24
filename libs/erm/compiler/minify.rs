/// Native minifier and asset hashing utilities for Eronom production builds.

/// Computes a fast 8-character hex hash from binary data using FNV-1a.
pub fn compute_asset_hash(data: &[u8]) -> String {
    let mut hasher = 0xcbf29ce484222325u64;
    for &byte in data {
        hasher ^= byte as u64;
        hasher = hasher.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", hasher as u32)
}

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
            // Check if we need a space: only between alphanumeric / identifier characters
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
            // Look ahead: if next non-whitespace is '}', skip semicolon
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

pub fn inline_public_env_vars(code: &str) -> String {
    // Replaces references to (?:process\.)?env\.(ER_PUBLIC_[A-Za-z0-9_]+) and bare ER_PUBLIC_* variables
    let re_env = regex::Regex::new(r#"(?:process\.)?env\.(ER_PUBLIC_[A-Za-z0-9_]+)"#).unwrap();
    let replaced_env = re_env.replace_all(code, |caps: &regex::Captures| {
        let var_name = &caps[1];
        if let Ok(val) = std::env::var(var_name) {
            serde_json::to_string(&val).unwrap_or_else(|_| format!("\"{}\"", val.replace('"', "\\\"")))
        } else {
            "\"\"".to_string()
        }
    }).into_owned();

    let re_bare = regex::Regex::new(r#"\b(ER_PUBLIC_[A-Za-z0-9_]+)\b"#).unwrap();
    re_bare.replace_all(&replaced_env, |caps: &regex::Captures| {
        let var_name = &caps[1];
        if let Ok(val) = std::env::var(var_name) {
            serde_json::to_string(&val).unwrap_or_else(|_| format!("\"{}\"", val.replace('"', "\\\"")))
        } else {
            "\"\"".to_string()
        }
    }).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_asset_hash() {
        let h1 = compute_asset_hash(b"console.log('hello');");
        let h2 = compute_asset_hash(b"console.log('hello');");
        let h3 = compute_asset_hash(b"console.log('world');");

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 8);
    }

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

    #[test]
    fn test_inline_public_env_vars() {
        unsafe {
            std::env::set_var("ER_PUBLIC_API_URL", "https://api.eronom.dev");
            std::env::set_var("SECRET_DB_PASS", "supersecret");
        }

        let code = r#"
            const endpoint = process.env.ER_PUBLIC_API_URL;
            const fallback = env.ER_PUBLIC_API_URL;
            const bare = ER_PUBLIC_API_URL;
            const secret = process.env.SECRET_DB_PASS;
        "#;
        let inlined = inline_public_env_vars(code);
        assert!(inlined.contains("const endpoint = \"https://api.eronom.dev\";"));
        assert!(inlined.contains("const fallback = \"https://api.eronom.dev\";"));
        assert!(inlined.contains("const bare = \"https://api.eronom.dev\";"));
        // Secret must NOT be inlined!
        assert!(inlined.contains("process.env.SECRET_DB_PASS"));
        assert!(!inlined.contains("supersecret"));
    }
}
