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

/// Guards against runaway output from absurd lengths in the input.
const MAX_PAGES: usize = 100_000;

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
    let page_h = (ph - mt - mb).max(1.0);
    let lay = layout::layout(
        &dom,
        &styled.styles,
        &opts.fonts,
        (pw - ml - mr).max(0.0),
        page_h,
    );

    // Items sorted onto pages; a rect crossing page boundaries is cut into pieces.
    let page = |y: f32| (y / page_h).floor().max(0.0) as usize;
    let mut pages: Vec<Vec<Item>> = Vec::new();
    let mut put = |p: usize, item: Item| -> Result<()> {
        if p >= MAX_PAGES {
            return Err(Error::TooManyPages);
        }
        if pages.len() <= p {
            pages.resize_with(p + 1, Vec::new);
        }
        if let Some(list) = pages.get_mut(p) {
            list.push(item);
        }
        Ok(())
    };
    for item in lay.items {
        match item {
            Item::Rect { x, y, w, h, color } if color.a > 0 && h > 0.0 && w > 0.0 => {
                for p in page(y)..=page(y + h - 0.01) {
                    let top = p as f32 * page_h;
                    let (y0, y1) = (y.max(top), (y + h).min(top + page_h));
                    put(
                        p,
                        Item::Rect {
                            x,
                            y: y0 - top,
                            w,
                            h: y1 - y0,
                            color,
                        },
                    )?;
                }
            }
            Item::Text { y, color, .. } if color.a > 0 => {
                let p = page(y);
                let mut item = item;
                if let Item::Text { y: iy, .. } = &mut item {
                    *iy = y - p as f32 * page_h;
                }
                put(p, item)?;
            }
            _ => {}
        }
    }
    if pages.is_empty() {
        pages.push(Vec::new());
    }

    let mut doc = pdf::Document::new(&opts.fonts);
    for items in &pages {
        doc.add_page(pw, ph);
        for item in items {
            match item {
                Item::Rect { x, y, w, h, color } => doc.rect(x + ml, y + mt, *w, *h, *color),
                Item::Text {
                    x,
                    y,
                    font,
                    size,
                    color,
                    word_spacing,
                    text,
                } => doc.text(
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
            }
        }
    }
    Ok(doc.finish())
}
