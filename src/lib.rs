//! HTML + CSS to paginated PDF.

mod error;
mod font;
pub mod pdf;

pub use error::{Error, Result};
pub use font::Font;
