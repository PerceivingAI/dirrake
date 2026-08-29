use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ErrorKind {
    Usage = 2,
    Root = 3,
    Output = 4,
    Internal = 5,
}

#[derive(Debug)]
pub struct AppError {
    kind: ErrorKind,
    source: anyhow::Error,
}

impl AppError {
    pub fn usage(error: impl Into<anyhow::Error>) -> Self {
        Self::new(ErrorKind::Usage, error)
    }

    pub fn root(error: impl Into<anyhow::Error>) -> Self {
        Self::new(ErrorKind::Root, error)
    }

    pub fn output(error: impl Into<anyhow::Error>) -> Self {
        Self::new(ErrorKind::Output, error)
    }

    pub fn internal(error: impl Into<anyhow::Error>) -> Self {
        Self::new(ErrorKind::Internal, error)
    }

    pub fn code(&self) -> u8 {
        self.kind as u8
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    fn new(kind: ErrorKind, source: impl Into<anyhow::Error>) -> Self {
        Self {
            kind,
            source: source.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.source)
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.source()
    }
}

pub type AppResult<T> = Result<T, AppError>;
