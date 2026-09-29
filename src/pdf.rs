//! Minimal PDF 1.7 writer: pages, text in embedded TrueType fonts.
//!
//! Fonts are written as Type0 / CIDFontType2 with Identity-H encoding, so text
//! is a sequence of 2-byte glyph ids. A ToUnicode CMap maps them back to
//! characters for search and copy.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use miniz_oxide::deflate::compress_to_vec_zlib;

use crate::css::Color;
use crate::font::Font;
use crate::image::Image;
use crate::subset;

pub struct Document<'f> {
    fonts: Vec<FontUse<'f>>,
    images: Vec<Image>,
    pages: Vec<Page>,
}

struct FontUse<'f> {
    font: &'f Font<'f>,
    /// Glyphs drawn with this font and the character each one came from.
    used: BTreeMap<u16, char>,
}

struct Page {
    width: f32,
    height: f32,
    content: String,
    fill: Option<Color>,
}

impl Page {
    fn set_fill(&mut self, c: Color) {
        if self.fill != Some(c) {
            let ch = |v: u8| num(f32::from(v) / 255.0);
            let _ = writeln!(self.content, "{} {} {} rg", ch(c.r), ch(c.g), ch(c.b));
            self.fill = Some(c);
        }
    }
}

/// A run of text in one font, size and color.
pub struct Text<'a> {
    /// Index into the fonts in the order they were added.
    pub font: usize,
    pub size: f32,
    pub color: Color,
    /// Extra advance after each U+0020, in points.
    pub word_spacing: f32,
    pub text: &'a str,
}

impl<'f> Document<'f> {
    pub fn new(fonts: &'f [Font<'f>]) -> Self {
        Document {
            fonts: fonts
                .iter()
                .map(|font| FontUse {
                    font,
                    used: BTreeMap::new(),
                })
                .collect(),
            images: Vec::new(),
            pages: Vec::new(),
        }
    }

    /// Starts a new page; size in points.
    pub fn add_page(&mut self, width: f32, height: f32) {
        self.pages.push(Page {
            width,
            height,
            content: String::new(),
            fill: None,
        });
    }

    /// Draws a run on the last page with its baseline starting at (`x`, `y`),
    /// measured in points from the top-left corner.
    pub fn text(&mut self, x: f32, y: f32, run: &Text) {
        let (Some(page), Some(f)) = (self.pages.last_mut(), self.fonts.get_mut(run.font)) else {
            return;
        };
        page.set_fill(run.color);
        let _ = write!(
            page.content,
            "BT /F{} {} Tf {} {} Td [<",
            run.font,
            num(run.size),
            num(x),
            num(page.height - y)
        );
        // TJ adjustments are in thousandths of text space, positive moves left.
        let adjust = num(-run.word_spacing * 1000.0 / run.size);
        for c in run.text.chars() {
            let gid = f.font.glyph(c);
            if gid != 0 {
                f.used.entry(gid).or_insert(c);
            }
            let _ = write!(page.content, "{gid:04X}");
            if c == ' ' && adjust != "0" {
                let _ = write!(page.content, "> {adjust} <");
            }
        }
        page.content.push_str(">] TJ ET\n");
    }

    /// Fills a rectangle given by its top-left corner.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let Some(page) = self.pages.last_mut() else {
            return;
        };
        page.set_fill(color);
        let _ = writeln!(
            page.content,
            "{} {} {} {} re f",
            num(x),
            num(page.height - y - h),
            num(w),
            num(h)
        );
    }

    /// Registers an image; returns its index for [`image`](Self::image).
    pub fn add_image(&mut self, image: Image) -> usize {
        self.images.push(image);
        self.images.len() - 1
    }

    /// Draws image `index` into the rectangle given by its top-left corner.
    pub fn image(&mut self, x: f32, y: f32, w: f32, h: f32, index: usize) {
        let Some(page) = self.pages.last_mut() else {
            return;
        };
        if index < self.images.len() {
            let _ = writeln!(
                page.content,
                "q {} 0 0 {} {} {} cm /Im{index} Do Q",
                num(w),
                num(h),
                num(x),
                num(page.height - y - h)
            );
        }
    }

    pub fn finish(self) -> Vec<u8> {
        let mut w = Writer::default();
        w.out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");

        // Fixed object numbers: 1 catalog, 2 page tree, 3 shared resources,
        // then 5 per font, 2 per image (image, soft mask), 2 per page.
        let font_base = 4;
        let image_base = font_base + 5 * self.fonts.len();
        let page_base = image_base + 2 * self.images.len();
        let page_id = |i: usize| page_base + 2 * i;

        w.obj(1, "<< /Type /Catalog /Pages 2 0 R >>");

        let kids: String = (0..self.pages.len())
            .map(|i| format!("{} 0 R ", page_id(i)))
            .collect();
        w.obj(
            2,
            &format!(
                "<< /Type /Pages /Kids [{kids}] /Count {} >>",
                self.pages.len()
            ),
        );

        let fonts: String = (0..self.fonts.len())
            .map(|i| format!("/F{i} {} 0 R ", font_base + 5 * i))
            .collect();
        let images: String = (0..self.images.len())
            .map(|i| format!("/Im{i} {} 0 R ", image_base + 2 * i))
            .collect();
        w.obj(
            3,
            &format!("<< /Font << {fonts}>> /XObject << {images}>> >>"),
        );

        for (i, f) in self.fonts.iter().enumerate() {
            write_font(&mut w, font_base + 5 * i, i, f);
        }

        for (i, img) in self.images.iter().enumerate() {
            let id = image_base + 2 * i;
            let mut dict = format!(
                "/Type /XObject /Subtype /Image /Width {} /Height {} {}",
                img.width, img.height, img.dict
            );
            if let Some((mask_dict, mask)) = &img.smask {
                let _ = write!(dict, " /SMask {} 0 R", id + 1);
                w.raw_stream(
                    id + 1,
                    &format!(
                        "/Type /XObject /Subtype /Image /Width {} /Height {} {mask_dict}",
                        img.width, img.height
                    ),
                    mask,
                );
            }
            w.raw_stream(id, &dict, &img.data);
        }

        for (i, page) in self.pages.iter().enumerate() {
            let id = page_id(i);
            w.obj(
                id,
                &format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources 3 0 R /Contents {} 0 R >>",
                    num(page.width),
                    num(page.height),
                    id + 1
                ),
            );
            w.stream(id + 1, "", page.content.as_bytes());
        }

        w.finish()
    }
}

/// Writes the five objects of one font starting at object `id`.
fn write_font(w: &mut Writer, id: usize, index: usize, f: &FontUse) {
    let font = f.font;
    let face = &font.face;
    let base = font
        .postscript_name()
        .unwrap_or_else(|| format!("Font{index}"));
    // A subset's name carries a six-letter tag unique to its glyph set (§9.6.4).
    let subset = face
        .is_subsetting_allowed()
        .then(|| subset::subset(font.data, f.used.keys().copied()))
        .flatten();
    let name = match subset {
        Some(_) => format!("{}+{base}", subset_tag(index, f.used.keys().copied())),
        None => base,
    };
    let data = subset.as_deref().unwrap_or(font.data);
    let [cid, descriptor, file, cmap] = [id + 1, id + 2, id + 3, id + 4];

    w.obj(
        id,
        &format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
             /DescendantFonts [{cid} 0 R] /ToUnicode {cmap} 0 R >>"
        ),
    );
    w.obj(
        cid,
        &format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor {descriptor} 0 R /CIDToGIDMap /Identity /W {} >>",
            widths(f)
        ),
    );

    let s = |v: i16| num(font.scale(v.into()));
    let bbox = face.global_bounding_box();
    let ascent = face.ascender();
    w.obj(
        descriptor,
        &format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags 4 /FontBBox [{} {} {} {}] \
             /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {file} 0 R >>",
            s(bbox.x_min),
            s(bbox.y_min),
            s(bbox.x_max),
            s(bbox.y_max),
            num(face.italic_angle()),
            s(ascent),
            s(face.descender()),
            s(face.capital_height().unwrap_or(ascent)),
        ),
    );
    w.stream(file, &format!("/Length1 {}", data.len()), data);
    w.stream(cmap, "", to_unicode(&f.used).as_bytes());
}

/// Six uppercase letters from an FNV-1a hash of the font index and glyphs.
fn subset_tag(index: usize, glyphs: impl Iterator<Item = u16>) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in std::iter::once(index as u64).chain(glyphs.map(u64::from)) {
        h = (h ^ v).wrapping_mul(0x0100_0000_01b3);
    }
    (0..6)
        .map(|i| char::from(b'A' + ((h >> (i * 8)) % 26) as u8))
        .collect()
}

/// `/W` array for the used glyphs, consecutive ids grouped: `[0 [600] 3 [500 600]]`.
/// `.notdef` (0) is always listed: it is drawn for missing characters, and
/// without an entry viewers give it the default width of 1000, not the one
/// layout measured.
fn widths(f: &FontUse) -> String {
    let mut out = String::from("[");
    let mut prev: Option<u16> = None;
    for &gid in std::iter::once(&0).chain(f.used.keys().filter(|&&g| g != 0)) {
        if prev.and_then(|p| p.checked_add(1)) == Some(gid) {
            out.push(' ');
        } else {
            if prev.is_some() {
                out.push_str("] ");
            }
            let _ = write!(out, "{gid} [");
        }
        out.push_str(&num(f.font.advance(gid)));
        prev = Some(gid);
    }
    if prev.is_some() {
        out.push(']');
    }
    out.push(']');
    out
}

fn to_unicode(used: &BTreeMap<u16, char>) -> String {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<_> = used.iter().collect();
    // The CMap format caps a bfchar block at 100 entries.
    for chunk in entries.chunks(100) {
        let _ = writeln!(out, "{} beginbfchar", chunk.len());
        for (gid, c) in chunk {
            let _ = write!(out, "<{gid:04X}> <");
            for unit in c.encode_utf16(&mut [0; 2]) {
                let _ = write!(out, "{unit:04X}");
            }
            out.push_str(">\n");
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

/// PDF number: at most two decimals, no exponent.
fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[derive(Default)]
struct Writer {
    out: Vec<u8>,
    /// Byte offset of each object; index is object number - 1.
    offsets: Vec<usize>,
}

impl Writer {
    fn start(&mut self, id: usize) {
        if self.offsets.len() < id {
            self.offsets.resize(id, 0);
        }
        self.offsets[id - 1] = self.out.len();
        self.out
            .extend_from_slice(format!("{id} 0 obj\n").as_bytes());
    }

    fn obj(&mut self, id: usize, body: &str) {
        self.start(id);
        self.out.extend_from_slice(body.as_bytes());
        self.out.extend_from_slice(b"\nendobj\n");
    }

    /// Flate-compressed stream; `extra` goes into its dictionary.
    fn stream(&mut self, id: usize, extra: &str, data: &[u8]) {
        let data = compress_to_vec_zlib(data, 6);
        self.start(id);
        self.out.extend_from_slice(
            format!(
                "<< /Length {} /Filter /FlateDecode {extra} >>\nstream\n",
                data.len()
            )
            .as_bytes(),
        );
        self.out.extend_from_slice(&data);
        self.out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    /// Stream whose data is already encoded as `dict` says.
    fn raw_stream(&mut self, id: usize, dict: &str, data: &[u8]) {
        self.start(id);
        self.out
            .extend_from_slice(format!("<< {dict} /Length {} >>\nstream\n", data.len()).as_bytes());
        self.out.extend_from_slice(data);
        self.out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    fn finish(mut self) -> Vec<u8> {
        let xref = self.out.len();
        let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", self.offsets.len() + 1);
        for off in &self.offsets {
            // Offset 0 is the header, so it marks an unused object number.
            if *off == 0 {
                table.push_str("0000000000 65535 f \n");
            } else {
                let _ = writeln!(table, "{off:010} 00000 n ");
            }
        }
        let _ = write!(
            table,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            self.offsets.len() + 1
        );
        self.out.extend_from_slice(table.as_bytes());
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTO: &[u8] = include_bytes!("../tests/fixtures/NotoSans-Regular.ttf");

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn xref_points_at_objects() {
        let fonts = [Font::new(NOTO).unwrap()];
        let mut doc = Document::new(&fonts);
        let run = |text| Text {
            font: 0,
            size: 12.0,
            color: Color::BLACK,
            word_spacing: 0.0,
            text,
        };
        doc.add_page(595.0, 842.0);
        doc.text(72.0, 72.0, &run("Привет, world"));
        doc.rect(72.0, 80.0, 100.0, 1.0, Color::BLACK);
        doc.add_page(595.0, 842.0);
        doc.text(72.0, 72.0, &run("second"));
        let pdf = doc.finish();

        let tail = &pdf[find(&pdf, b"startxref\n").unwrap() + 10..];
        let xref: usize = std::str::from_utf8(tail)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let table = std::str::from_utf8(&pdf[xref..]).unwrap();
        assert!(table.starts_with("xref\n0 13\n"));
        for (i, line) in table.lines().skip(3).take(12).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(
                pdf[off..].starts_with(format!("{} 0 obj\n", i + 1).as_bytes()),
                "object {}",
                i + 1
            );
        }
    }

    #[test]
    fn to_unicode_maps_glyphs() {
        let used = BTreeMap::from([(0x12, 'Я'), (0x13, '😀')]);
        let cmap = to_unicode(&used);
        assert!(cmap.contains("2 beginbfchar\n<0012> <042F>\n<0013> <D83DDE00>\n"));
    }

    #[test]
    fn widths_group_consecutive_ids() {
        let font = Font::new(NOTO).unwrap();
        let used = BTreeMap::from([(3, ' '), (4, '!'), (10, 'x')]);
        let w = widths(&FontUse { font: &font, used });
        assert!(
            w.starts_with("[0 [")
                && w.contains("] 3 [")
                && w.contains("] 10 [")
                && w.ends_with("]]"),
            "{w}"
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(num(12.0), "12");
        assert_eq!(num(0.126), "0.13");
        assert_eq!(num(-0.001), "0");
        assert_eq!(num(f32::NAN), "0");
        assert_eq!(num(1e20), "100000002004087734272");
    }
}
