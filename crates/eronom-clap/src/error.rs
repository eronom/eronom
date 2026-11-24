#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NoParamFound,
    InvalidShortParam,
    TrailingComma,
    DoesntTakeValue(&'static str),
    MissingValue(&'static str),
    InvalidArgument(&'static str),
    UnknownOption(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoParamFound => write!(f, "No parameter found"),
            Self::InvalidShortParam => write!(f, "Invalid short parameter"),
            Self::TrailingComma => write!(f, "Trailing comma in parameter specification"),
            Self::DoesntTakeValue(name) => write!(f, "Option '{}' does not take a value", name),
            Self::MissingValue(name) => write!(f, "Option '{}' requires a value", name),
            Self::InvalidArgument(msg) => write!(f, "Invalid argument: {}", msg),
            Self::UnknownOption(name) => write!(f, "Unknown option '{}'", name),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

