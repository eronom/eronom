use std::fmt;

#[derive(Debug, Clone)]
pub struct CompileError {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub hint: Option<String>,
    pub frame: String,
}

impl CompileError {
    pub fn new(
        file: impl Into<String>,
        source: &str,
        pos: usize,
        highlight_len: usize,
        message: impl Into<String>,
        hint: Option<String>,
    ) -> Self {
        let file_raw = file.into();
        let file = if let Ok(cwd) = std::env::current_dir() {
            if let Ok(rel) = std::path::Path::new(&file_raw).strip_prefix(&cwd) {
                rel.to_string_lossy().into_owned()
            } else {
                file_raw
            }
        } else {
            file_raw
        };
        let (line, col) = calculate_line_col(source, pos);
        let frame = generate_code_frame(source, line, col, highlight_len);
        Self {
            file,
            line,
            col,
            message: message.into(),
            hint,
            frame,
        }
    }
}

pub fn calculate_line_col(source: &str, pos: usize) -> (usize, usize) {
    let safe_pos = pos.min(source.len());
    let line = source[..safe_pos].chars().filter(|&c| c == '\n').count() + 1;
    let line_start = source[..safe_pos].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let col = source[line_start..safe_pos].chars().count() + 1;
    (line, col)
}

pub fn generate_code_frame(source: &str, target_line: usize, target_col: usize, highlight_len: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() || target_line == 0 {
        return String::new();
    }

    let line_idx = target_line - 1;
    let start_line = line_idx.saturating_sub(2);
    let end_line = (line_idx + 3).min(lines.len());

    let max_line_num = end_line;
    let gutter_width = max_line_num.to_string().len();

    let mut frame = String::new();

    for curr_idx in start_line..end_line {
        let curr_line_num = curr_idx + 1;
        let line_content = lines[curr_idx];

        if curr_idx == line_idx {
            // Highlight error line
            frame.push_str(&format!(
                "> {:>width$} | {}\n",
                curr_line_num,
                line_content,
                width = gutter_width
            ));
            // Underline caret pointer with matching whitespace
            let indent_chars: String = line_content
                .chars()
                .take(target_col.saturating_sub(1))
                .map(|ch| if ch == '\t' { '\t' } else { ' ' })
                .collect();
            let carets = if highlight_len > 0 { highlight_len } else { 1 };
            frame.push_str(&format!(
                "  {:>width$} | {}{}\n",
                "",
                indent_chars,
                "^".repeat(carets),
                width = gutter_width
            ));
        } else {
            // Surrounding context line
            frame.push_str(&format!(
                "  {:>width$} | {}\n",
                curr_line_num,
                line_content,
                width = gutter_width
            ));
        }
    }

    if frame.ends_with('\n') {
        frame.pop();
    }

    frame
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SyntaxError in {} ({}:{}): {}\n\n{}\n",
            self.file, self.line, self.col, self.message, self.frame
        )?;
        if let Some(ref hint) = self.hint {
            write!(f, "\n{}\n", hint)?;
        }
        Ok(())
    }
}

impl std::error::Error for CompileError {}
