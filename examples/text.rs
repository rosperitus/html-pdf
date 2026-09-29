//! `cargo run --example text -- font.ttf out.pdf`
use html_pdf::{Font, pdf::Document};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, font, out] = args.as_slice() else {
        return Err("usage: text <font.ttf> <out.pdf>".into());
    };
    let data = std::fs::read(font)?;
    let font = Font::new(&data)?;
    let mut doc = Document::new();
    let f = doc.add_font(&font);
    doc.add_page(595.0, 842.0);
    doc.text(
        f,
        20.0,
        72.0,
        100.0,
        "Съешь же ещё этих мягких французских булок",
    );
    doc.text(
        f,
        12.0,
        72.0,
        130.0,
        "The quick brown fox jumps over the lazy dog. 1234567890",
    );
    doc.add_page(842.0, 595.0);
    doc.text(f, 18.0, 72.0, 100.0, "Вторая страница, альбомная");
    std::fs::write(out, doc.finish())?;
    Ok(())
}
