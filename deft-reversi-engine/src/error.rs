use std::fmt;
use std::io;

#[derive(Debug)]
pub enum EngineError {
    Io(io::Error),
    InvalidData(String),
    IllegalMove { move_str: String },
    InvalidPosition(String),
    InvalidRecord { reason: String, offset: usize },
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::InvalidData(reason) => write!(f, "invalid data: {reason}"),
            Self::IllegalMove { move_str } => write!(f, "illegal move: {move_str}"),
            Self::InvalidPosition(pos) => write!(f, "invalid position: {pos}"),
            Self::InvalidRecord { reason, offset } => {
                write!(f, "invalid record at offset {offset}: {reason}")
            }
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for EngineError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
