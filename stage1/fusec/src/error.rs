use std::fmt;

#[derive(Debug, Clone)]
pub struct FuseError {
    pub message: String,
    pub hint: Option<String>,
    pub file: String,
    pub line: usize,
    pub col: usize,
}

impl FuseError {
    pub fn new(message: impl Into<String>, file: impl Into<String>, line: usize, col: usize) -> Self {
        Self { message: message.into(), hint: None, file: file.into(), line, col }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl fmt::Display for FuseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error: {}", self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, "\n       {hint}")?;
        }
        write!(f, "\n  --> {}:{}:{}", self.file, self.line, self.col)
    }
}

impl std::error::Error for FuseError {}

pub type Result<T> = std::result::Result<T, FuseError>;
