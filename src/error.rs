use std::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The font could not be parsed or cannot be embedded.
    Font(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Font(msg) => write!(f, "font: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
