//! `cargo run --example render -- in.html out.pdf font.ttf [more fonts...]`
use html_pdf::{Font, Options, render};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, fonts @ ..] = args.as_slice() else {
        return Err("usage: render <in.html> <out.pdf> <font.ttf>...".into());
    };
    let data: Vec<Vec<u8>> = fonts.iter().map(std::fs::read).collect::<Result<_, _>>()?;
    let fonts = data
        .iter()
        .map(|d| Font::new(d))
        .collect::<Result<_, _>>()?;
    let html = std::fs::read_to_string(input)?;
    std::fs::write(output, render(&html, &Options::new(fonts))?)?;
    Ok(())
}
