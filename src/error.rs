use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl Error {
    pub fn new(message: impl Into<String>, line: usize, col: usize) -> Self {
        Self { message: message.into(), line, col }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self { message: message.into(), line: 0, col: 0 }
    }

    /// A runtime error anchored to a source line (e.g. the instruction that
    /// raised it). Kept distinct from [`Error::runtime`] so callers that only
    /// produce line-less errors stay unchanged.
    pub fn runtime_at(message: impl Into<String>, line: usize) -> Self {
        Self { message: message.into(), line, col: 0 }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "goscript: {}", self.message)
        } else {
            write!(f, "goscript: {} (line {}, col {})", self.message, self.line, self.col)
        }
    }
}

impl std::error::Error for Error {}