//! HTML + CSS to paginated PDF.

// Not wired into the public API until layout lands in phase 3.
#[allow(dead_code)]
mod css;
mod error;
mod font;
#[allow(dead_code)]
mod html;
pub mod pdf;
#[allow(dead_code)]
mod style;

pub use error::{Error, Result};
pub use font::Font;
