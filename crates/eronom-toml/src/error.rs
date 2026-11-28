use std::fmt;

/// Error returned when parsing TOML input fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl Error {
    pub(crate) fn new(line: usize, col: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            col,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TOML parse error at {}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for Error {}

