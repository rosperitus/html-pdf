//! `cargo run --release --example bench -- font.ttf [sections]`
//!
//! Renders a synthetic report (headings, justified paragraphs, a table per
//! section) and prints the average time per run and the output size.
use std::time::Instant;

use html_pdf::{Font, Options, render};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [font, rest @ ..] = args.as_slice() else {
        return Err("usage: bench <font.ttf> [sections]".into());
    };
    let sections: usize = rest.first().map_or(Ok(100), |s| s.parse())?;
    let para = "Съешь же ещё этих мягких французских булок, да выпей чаю. ".repeat(8);
    let mut html = String::from(
        "<style>body { font-size: 10pt } p { text-align: justify } \
         table { width: 100%; border-collapse: collapse } td, th { border: 1px solid #999; padding: 2pt 4pt }</style>",
    );
    for i in 0..sections {
        html += &format!("<h2>Раздел {i}</h2><p>{para}</p><p>{para}</p>");
        html += "<table><thead><tr><th>№</th><th>Наименование</th><th>Сумма</th></tr></thead>";
        for r in 0..20 {
            html += &format!(
                "<tr><td>{r}</td><td>Позиция {r} раздела {i}</td><td>{} ₽</td></tr>",
                r * 100
            );
        }
        html += "</table>";
    }

    let data = std::fs::read(font)?;
    let opts = Options::new(vec![Font::new(&data)?]);
    let pdf = render(&html, &opts)?; // warm-up
    let pages = pdf.windows(12).filter(|w| w == b"/Type /Page ").count();
    let runs = 5;
    let start = Instant::now();
    for _ in 0..runs {
        render(&html, &opts)?;
    }
    let per = start.elapsed() / runs;
    println!(
        "{} KB of HTML -> {pages} pages, {} KB of PDF in {per:?} per run ({:.0} pages/s)",
        html.len() / 1024,
        pdf.len() / 1024,
        pages as f64 / per.as_secs_f64()
    );
    Ok(())
}
