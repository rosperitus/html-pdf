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
mod image;
mod layout;
mod pdf;
mod style;
mod subset;

pub use error::{Error, Result};
pub use font::Font;

use css::Length;
use layout::Item;

/// Guards against runaway output from absurd lengths in the input.
const MAX_PAGES: usize = 100_000;

/// Returns the bytes behind a resource URL, or `None` if it is unavailable.
pub type Loader<'a> = Box<dyn Fn(&str) -> Option<Vec<u8>> + 'a>;

pub struct Options<'a> {
    /// Fonts to lay out with. CSS `font-family` picks among them by family
    /// name; families not found here, including generic ones like
    /// `sans-serif`, fall back to the family of the first font.
    pub fonts: Vec<Font<'a>>,
    /// Page width and height in points, unless the document sets `@page { size }`.
    pub page_size: (f32, f32),
    /// Top, right, bottom, left page margins in points, unless the document sets `@page { margin }`.
    pub margin: [f32; 4],
    /// Loads `<img src>` values other than `data:` URIs, for example from
    /// disk. JPEG and PNG are supported; images that fail to load or decode
    /// are left out.
    pub resources: Option<Loader<'a>>,
}

impl<'a> Options<'a> {
    /// A4 with 1 cm margins.
    pub fn new(fonts: Vec<Font<'a>>) -> Self {
        let cm = 72.0 / 2.54;
        Options {
            fonts,
            page_size: (595.28, 841.89),
            margin: [cm; 4],
            resources: None,
        }
    }
}

/// Text of a page-margin box on page `page` of `pages`.
fn margin_text(content: &[css::Content], page: usize, pages: usize) -> String {
    content
        .iter()
        .map(|c| match c {
            css::Content::Text(t) => t.clone(),
            css::Content::Page => page.to_string(),
            css::Content::Pages => pages.to_string(),
        })
        .collect()
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
    let none = |_: &str| None;
    let resolve: layout::Resolve = opts.resources.as_deref().unwrap_or(&none);
    let lay = layout::layout(
        &dom,
        &styled.styles,
        &opts.fonts,
        (pw - ml - mr).max(0.0),
        page_h,
        resolve,
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
            list.push(item.shifted(ml, mt - p as f32 * page_h));
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
                            y: y0,
                            w,
                            h: y1 - y0,
                            color,
                        },
                    )?;
                }
            }
            Item::Text { y, color, .. } if color.a > 0 => put(page(y), item)?,
            Item::Image { y, .. } => put(page(y), item)?,
            _ => {}
        }
    }
    if pages.is_empty() {
        pages.push(Vec::new());
    }

    let mut doc = pdf::Document::new(&opts.fonts);
    for img in lay.images {
        doc.add_image(img);
    }
    let faces: Vec<_> = opts
        .fonts
        .iter()
        .map(|f| (f.family.as_str(), f.weight, f.italic))
        .collect();
    let root = styled
        .styles
        .get(html::HTML)
        .cloned()
        .unwrap_or_else(style::Style::initial);
    let boxes: Vec<_> = styled
        .page
        .boxes
        .iter()
        .map(|b| {
            let s = style::margin_box(&root, &b.decls);
            let font = layout::pick(&faces, &s.font_family, s.font_weight, s.italic);
            (b, s, font)
        })
        .collect();
    let count = pages.len();
    for (index, items) in pages.iter().enumerate() {
        doc.add_page(pw, ph);
        for (b, s, font) in &boxes {
            let text = margin_text(&b.content, index + 1, count);
            let Some(f) = opts
                .fonts
                .get(*font)
                .filter(|_| !text.is_empty() && s.color.a > 0)
            else {
                continue;
            };
            let w = f.measure(&text, s.font_size);
            let x = match b.at % 3 {
                0 => ml,
                1 => (pw - w) / 2.0,
                _ => pw - mr - w,
            };
            // Centered vertically in the top or bottom margin.
            let (asc, desc, _) = f.vmetrics();
            let center = if b.at < 3 { mt / 2.0 } else { ph - mb / 2.0 };
            let baseline = center + (asc - desc) * s.font_size / 2000.0;
            let run = pdf::Text {
                font: *font,
                size: s.font_size,
                color: s.color,
                word_spacing: 0.0,
                text: &text,
            };
            doc.text(x, baseline, &run);
        }
        for item in items {
            match item {
                Item::Rect { x, y, w, h, color } => doc.rect(*x, *y, *w, *h, *color),
                Item::Image { x, y, w, h, image } => doc.image(*x, *y, *w, *h, *image),
                Item::Text {
                    x,
                    y,
                    font,
                    size,
                    color,
                    word_spacing,
                    text,
                } => doc.text(
                    *x,
                    *y,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margin_box_text() {
        let mut sheet = css::Sheet::default();
        css::parse_sheet(
            r#"@page { margin: 2cm; @bottom-right { content: "Стр. " counter(page) " из " counter(pages); font-size: 8pt }
               @top-left { content: 'Отчёт \2116 5' } @top-left { content: "Итог" } @left-middle { content: "x" } }"#,
            &mut sheet,
        );
        let boxes = &sheet.page.boxes;
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0].at, 5);
        assert_eq!(margin_text(&boxes[0].content, 2, 7), "Стр. 2 из 7");
        assert_eq!(boxes[0].decls.len(), 1);
        assert_eq!(margin_text(&boxes[1].content, 1, 1), "Итог");
        assert_eq!(sheet.page.margin[0], Some(Length::Pt(72.0 * 2.0 / 2.54)));
        let mut escaped = css::Sheet::default();
        css::parse_sheet(
            r#"@page { @top-left { content: 'Отчёт \2116 5 \"x\"' } }"#,
            &mut escaped,
        );
        assert_eq!(
            margin_text(&escaped.page.boxes[0].content, 1, 1),
            "Отчёт №5 \"x\""
        );
    }
}
