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
    let nested_avoid = format!(
        "{}{}",
        "<div style='break-inside: avoid; padding-top: 3pt'>".repeat(300),
        "<p>x</p>".repeat(100)
    );
    let keep_chain = "<h2>h</h2>".repeat(3000);
    for src in [
        deep.as_str(),
        nested_avoid.as_str(),
        keep_chain.as_str(),
        "<p style='font-size: 0'>x y z</p>",
        "<div style='width: 1pt'>abc def</div>",
        "<pre>\t\t\r\n\r\n</pre><br><br>",
    ] {
        render(src, &opts).unwrap();
    }
    let huge = "100000000000000000000000000000pt";
    let src = format!("<p style='margin-top: {huge}; font-size: {huge}; width: -{huge}'>x</p>");
    assert!(matches!(
        render(&src, &opts),
        Err(html_pdf::Error::TooManyPages)
    ));
}

#[test]
fn paginates() {
    let opts = Options::new(vec![Font::new(NOTO).unwrap()]);
    let body = "<p>Строка текста</p>".repeat(200);
    let pdf = render(&body, &opts).unwrap();
    let pages = pdf.windows(12).filter(|w| w == b"/Type /Page ").count();
    assert!(pages > 3, "{pages}");
    let pdf = render(
        "<style>@page { size: A5 }</style>a<div style='break-before: page'>b</div>",
        &opts,
    )
    .unwrap();
    assert_eq!(pdf.windows(12).filter(|w| w == b"/Type /Page ").count(), 2);
}
