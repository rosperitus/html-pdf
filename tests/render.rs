use html_pdf::{Font, Options, render};

const NOTO: &[u8] = include_bytes!("fixtures/NotoSans-Regular.ttf");

fn find(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn renders_document() {
    let opts = Options::new(vec![Font::new(NOTO).unwrap()]);
    let pdf = render(
        "<h1>Заголовок</h1><p style='background: #eee'>Текст</p>",
        &opts,
    )
    .unwrap();
    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    assert!(find(&pdf, b"/BaseFont /NotoSans-Regular"));
    assert!(find(&pdf, b"/MediaBox [0 0 595.28 841.89]"));
}

#[test]
fn page_rule_sets_size() {
    let opts = Options::new(vec![Font::new(NOTO).unwrap()]);
    let pdf = render("<style>@page { size: letter landscape }</style>x", &opts).unwrap();
    assert!(find(&pdf, b"/MediaBox [0 0 792 612]"));
}

#[test]
fn requires_a_font() {
    assert!(render("x", &Options::new(Vec::new())).is_err());
}

#[test]
fn survives_hostile_input() {
    let opts = Options::new(vec![Font::new(NOTO).unwrap()]);
    let deep = "<div style='padding: 1pt; border: 1pt solid'>".repeat(2000);
    for src in [
        deep.as_str(),
        "<p style='width: -1e30pt; margin: 1e30pt; font-size: 1e30pt; line-height: -5'>x</p>",
        "<p style='font-size: 0'>x y z</p>",
        "<div style='width: 1pt'>abc def</div>",
        "<pre>\t\t\r\n\r\n</pre><br><br>",
    ] {
        render(src, &opts).unwrap();
    }
}
