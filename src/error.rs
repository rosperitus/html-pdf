use std::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The font could not be parsed or cannot be embedded.
    Font(String),
    /// The document would need more pages than the renderer allows.
    TooManyPages,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Font(msg) => write!(f, "font: {msg}"),
            Error::TooManyPages => f.write_str("document needs too many pages"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
