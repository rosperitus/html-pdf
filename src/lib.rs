//! HTML + CSS to paginated PDF.
//!
//! ```no_run
//! let data = std::fs::read("NotoSans-Regular.ttf")?;
//! let opts = html_pdf::Options::new(vec![html_pdf::Font::new(&data)?]);
//! let pdf = html_pdf::render("<h1>Привет</h1><p>Текст</p>", &opts)?;
//! std::fs::write("out.pdf", pdf)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod css;
mod error;
mod font;
mod html;
mod layout;
mod pdf;
mod style;

pub use error::{Error, Result};
pub use font::Font;

use css::Length;
use layout::Item;

pub struct Options<'a> {
    /// Fonts to lay out with. CSS `font-family` picks among them by family
    /// name; families not found here, including generic ones like
    /// `sans-serif`, fall back to the family of the first font.
    pub fonts: Vec<Font<'a>>,
    /// Page width and height in points, unless the document sets `@page { size }`.
    pub page_size: (f32, f32),
    /// Top, right, bottom, left page margins in points, unless the document sets `@page { margin }`.
    pub margin: [f32; 4],
}

impl<'a> Options<'a> {
    /// A4 with 1 cm margins.
    pub fn new(fonts: Vec<Font<'a>>) -> Self {
        let cm = 72.0 / 2.54;
        Options {
            fonts,
            page_size: (595.28, 841.89),
            margin: [cm; 4],
        }
    }
}

pub fn render(html: &str, opts: &Options) -> Result<Vec<u8>> {
    if opts.fonts.is_empty() {
        return Err(Error::Font("no fonts given".into()));
    }
    let dom = html::parse(html);
    let styled = style::cascade(&dom);
    let (pw, ph) = styled.page.size.unwrap_or(opts.page_size);
    let [mt, mr, mb, ml] = std::array::from_fn(|i| {
        let along = if i % 2 == 0 { ph } else { pw };
        match styled.page.margin[i] {
            Some(Length::Pt(v)) => v,
            Some(Length::Em(v) | Length::Rem(v)) => v * 12.0,
            Some(Length::Percent(p)) => p * along / 100.0,
            Some(Length::Auto) | None => opts.margin[i],
        }
    });
    let lay = layout::layout(&dom, &styled.styles, &opts.fonts, (pw - ml - mr).max(0.0));

    let mut doc = pdf::Document::new(&opts.fonts);
    // ponytail: one page as tall as the content; pagination is phase 4
    doc.add_page(pw, (lay.height + mt + mb).max(ph));
    for item in &lay.items {
        match item {
            Item::Rect { x, y, w, h, color } if color.a > 0 => {
                doc.rect(x + ml, y + mt, *w, *h, *color)
            }
            Item::Text {
                x,
                y,
                font,
                size,
                color,
                word_spacing,
                text,
            } if color.a > 0 => doc.text(
                x + ml,
                y + mt,
                &pdf::Text {
                    font: *font,
                    size: *size,
                    color: *color,
                    word_spacing: *word_spacing,
                    text,
                },
            ),
            _ => {}
        }
    }
    Ok(doc.finish())
}
