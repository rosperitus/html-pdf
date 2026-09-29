# html_pdf

Renders HTML and CSS to a paginated PDF without a browser. The Rust library has two
dependencies, `ttf-parser` and `miniz_oxide`. The same code ships to npm as `@rosperitus/html-pdf`,
a 424 KB WebAssembly module (170 KB gzipped) for Node.js and browsers.

It targets documents: reports, invoices, articles. Text breaks into lines and pages, tables
repeat their header on every page, and fonts are embedded as subsets, so a four-font document
comes out at about 55 KB. On one core the native build renders a 792-page report (3.3 MB of
HTML) in 360 ms, about 2200 pages per second (`examples/bench.rs`).

It is not a browser engine. There is no JavaScript, no flexbox, grid, floats or positioning,
and nothing is fetched from the network. See [Limitations](#limitations).

## Rust

The crate is not on crates.io yet; add it by path or git:

```toml
[dependencies]
html_pdf = { path = "../html-pdf" }
```

```rust,no_run
use html_pdf::{Font, Options, render};

let regular = std::fs::read("NotoSans-Regular.ttf")?;
let bold = std::fs::read("NotoSans-Bold.ttf")?;
let opts = Options::new(vec![Font::new(&regular)?, Font::new(&bold)?]);

let pdf = render("<h1>Invoice 42</h1><p>Total: <b>1 250 ₽</b></p>", &opts)?;
std::fs::write("invoice.pdf", pdf)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Font` borrows the font bytes, so they have to outlive the `Options`. The caller always
supplies fonts: the library does not read system fonts.

### Page size, margins and images

`Options::new` gives A4 with 1 cm margins. Sizes are in points (1/72 inch). A `@page` rule in
the document overrides both.

```rust,no_run
use html_pdf::{Font, Options, render};

let font = std::fs::read("NotoSans-Regular.ttf")?;
let mut opts = Options::new(vec![Font::new(&font)?]);
opts.page_size = (612.0, 792.0); // US Letter
opts.margin = [36.0; 4]; // top, right, bottom, left: half an inch

// `<img src>` values other than data: URIs go through this loader.
let dir = std::path::PathBuf::from("assets");
opts.resources = Some(Box::new(move |src| std::fs::read(dir.join(src)).ok()));

let pdf = render(r#"<img src="logo.png" width="120"><p>Report</p>"#, &opts)?;
std::fs::write("report.pdf", pdf)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

An image that the loader cannot provide, or that is not a JPEG or PNG, is left out, the way a
browser leaves out a broken image.

### Errors

`render` returns `Err(Error::Font)` when no fonts are given, and `Font::new` returns it for a
file that is not a TrueType font, uses CFF outlines (`.otf`), or forbids embedding in its
license flags. `Err(Error::TooManyPages)` stops documents that would exceed 100 000 pages,
which only happens with absurd lengths such as a 30-digit `margin-top`. Malformed HTML and CSS
never fail: they are recovered from the way a browser recovers.

## Node.js

```sh
npm install @rosperitus/html-pdf
```

```js
import { readFileSync, writeFileSync } from 'node:fs';
import { render } from '@rosperitus/html-pdf';

const pdf = render('<h1>Hello</h1><p>From Node.js.</p>', {
  fonts: [readFileSync('NotoSans-Regular.ttf'), readFileSync('NotoSans-Bold.ttf')],
});
writeFileSync('hello.pdf', pdf);
```

The Node.js build loads the WebAssembly module synchronously on import, so `render` is ready
to call. `require('@rosperitus/html-pdf')` works the same way from CommonJS.

All options:

```js
const pdf = render(html, {
  fonts: [regular, bold, italic],   // Uint8Array, Buffer or ArrayBuffer; required
  pageSize: [595.28, 841.89],       // points; A4 by default
  margin: 28.35,                    // points; one number or [top, right, bottom, left]
  resources: (src) => images.get(src), // bytes for <img src>, or undefined to skip
});
```

`render` returns a `Uint8Array` and throws an `Error` with a message such as
`font: no fonts given` or `options.margin must be a number or [top, right, bottom, left]`.

## Browser

The browser build is an ES module. Call its default export once to load the `.wasm` file. By
default it is fetched from next to the JavaScript file through
`new URL('html_pdf_bg.wasm', import.meta.url)`; if your bundler does not copy it, serve the file
yourself and pass its URL: `await init({ module_or_path: '/assets/html_pdf_bg.wasm' })`.

```js
import init, { render } from '@rosperitus/html-pdf';

await init();
const font = await (await fetch('/fonts/NotoSans-Regular.ttf')).arrayBuffer();

const pdf = render(document.querySelector('#invoice').outerHTML, { fonts: [font] });

const link = document.createElement('a');
link.href = URL.createObjectURL(new Blob([pdf], { type: 'application/pdf' }));
link.download = 'invoice.pdf';
link.click();
```

`resources` is called synchronously, so fetch the images first:

```js
const sources = [...doc.querySelectorAll('img')].map((img) => img.getAttribute('src'));
const images = new Map(
  await Promise.all(sources.map(async (src) => [src, await (await fetch(src)).arrayBuffer()])),
);
const pdf = render(html, { fonts: [font], resources: (src) => images.get(src) });
```

`examples/browser/index.html` is a live editor: HTML on the left, the PDF on the right,
re-rendered on each keystroke. Serve the repository root (`python3 -m http.server`) and open
`/examples/browser/`.

## Building the npm package

Requires `wasm-pack` and the `wasm32-unknown-unknown` target.

```sh
sh npm/build.sh   # writes npm/node (CommonJS) and npm/web (ES module)
cd npm && npm test
npm publish       # from npm/
```

`package.json` sends Node.js to `node/` and everything else, bundlers and browsers, to `web/`
through its `exports` field.

## What is supported

**HTML.** Any tag parses; unknown tags lay out by their CSS `display`. Default styles follow
browsers for `h1`–`h6`, `p`, `div`, `span`, `b`, `strong`, `i`, `em`, `u`, `s`, `del`, `ins`,
`a`, `code`, `pre`, `kbd`, `blockquote`, `hr`, `br`, `ul`, `ol`, `li`, `dl`, `dt`, `dd`,
`table` and its parts, `img`, `figure`, and the HTML5 sectioning elements. The parser
implies `html`, `body`, `tbody` and `tr`, closes `p`, `li` and table cells the way browsers do,
and decodes numeric and common named character references.

**CSS sources.** `<style>` elements and `style` attributes. `@media print` and `@media all`
apply, other media do not. `<link rel="stylesheet">` is not loaded.

**Selectors.** Type, `.class`, `#id`, `*`, descendant and child (`>`) combinators, comma
groups. Specificity, `!important` and `inherit` work as in CSS 2.1. A rule with any other
selector (`:hover`, `[attr]`, `+`, `~`) is dropped whole, as the CSS spec requires.

**Properties.** `display`, `margin`, `padding`, `border` (every style draws solid), `width`,
`height`, `box-sizing`, `color`, `background-color`, `font` and `font-family/size/weight/style`,
`line-height`, `text-align` including `justify`, `text-indent`, `text-decoration` (underline,
line-through), `white-space`, `vertical-align` in table cells, `list-style-type`,
`border-collapse`, `border-spacing`. Units: `px pt pc in cm mm Q em rem ex ch %`.

**Pages.** `@page { size; margin }` with named sizes (A3, A4, A5, B4, B5, letter, legal,
ledger) and `landscape`. Headers and footers go in margin boxes:

```css
@page {
  margin: 2cm;
  @top-right { content: "Quarterly report"; font-size: 8pt; color: gray }
  @bottom-center { content: "Page " counter(page) " of " counter(pages) }
}
```

`break-before`, `break-after` and `break-inside` (and the older `page-break-*`) take `page`
and `avoid`. `orphans` and `widows` default to 2. Headings carry `break-after: avoid`: a heading
that would end a page moves to the next one together with the block after it. Table rows never split, and rows from `thead`
repeat at the top of each page.

**Tables.** Column widths follow the content, as in browsers' automatic table layout; cell
widths in points or percent are respected. `colspan`, `rowspan` and `caption` work.

**Images.** JPEG goes into the PDF unchanged. PNG without transparency is copied without
decoding; PNG with an alpha channel is decoded to split out a soft mask. Images are sized by
CSS, `width`/`height` attributes or their pixel size (1 px = 0.75 pt), and scaled down to fit
the line and the page.

**Fonts.** TrueType (`.ttf`) only. `font-family` picks a font by the family name stored in
the file, then by italic and closest weight. Families that match nothing, generic ones such
as `monospace` included, use the first font. Each font is embedded as a subset of the glyphs
the document uses, with a ToUnicode map, so text can be searched and copied.

## Limitations

- No flexbox, grid, floats, `position`, transforms or JavaScript. Page layouts built on
  flexbox fall back to stacked blocks.
- `<link rel="stylesheet">` and network URLs are not loaded; pass CSS in `<style>` and images
  through `resources`.
- A character missing from the chosen font draws as an empty box: there is no per-glyph
  fallback to another font yet, so emoji and arrows need a font that contains them.
- No text shaping: no ligatures, kerning or complex scripts (Arabic, Devanagari). Latin,
  Cyrillic and Greek render correctly. Lines break at spaces only; there is no hyphenation.
- Alpha in colors is ignored (`rgba(0 0 0 / 40%)` paints black); dashed and dotted borders
  draw solid.
- GIF, SVG, WebP and interlaced PNG images are skipped.
- Form controls (`input`, `select`) are not drawn; their text content is.

## Repository

| Path | Contents |
|---|---|
| `src/` | The library: `html.rs`, `css.rs`, `style.rs`, `layout.rs`, `layout/table.rs`, `pdf.rs`, `subset.rs`, `image.rs`, `font.rs` |
| `wasm/` | WebAssembly bindings (`wasm-bindgen`) |
| `npm/` | npm package: `package.json`, build script, Node.js test |
| `examples/render.rs` | `cargo run --release --example render -- in.html out.pdf font.ttf...` |
| `examples/bench.rs` | Synthetic report benchmark |
| `examples/browser/` | In-browser live editor |
| `tests/showcase.html` | A page that uses every supported feature, with its PDF |

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

## License

MIT, see [LICENSE](LICENSE).
