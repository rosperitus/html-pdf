//! TrueType subsetting for embedding.
//!
//! Glyph ids stay as they are, so the PDF's CIDs and widths need no
//! remapping: unused glyphs become empty in `loca` and drop out of `glyf`.
//! Only the tables a PDF viewer needs to render TrueType outlines are kept
//! (ISO 32000-1 §9.9).

use std::collections::BTreeSet;

use ttf_parser::{RawFace, Tag};

/// Tables kept in the subset; everything else (cmap, layout, names) is
/// unused when text is drawn by glyph id.
const KEEP: [&[u8; 4]; 9] = [
    b"OS/2", b"cvt ", b"fpgm", b"glyf", b"head", b"hhea", b"hmtx", b"maxp", b"prep",
];

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        b.get(i..i.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        b.get(i..i.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// A font file holding only `glyphs` (plus `.notdef` and the components of
/// composite glyphs), or `None` if the font's tables are malformed.
pub fn subset(data: &[u8], glyphs: impl IntoIterator<Item = u16>) -> Option<Vec<u8>> {
    let raw = RawFace::parse(data, 0).ok()?;
    let table = |tag: &[u8; 4]| raw.table(Tag::from_bytes(tag));
    let head = table(b"head")?;
    let glyf = table(b"glyf")?;
    let loca = table(b"loca")?;
    let num_glyphs = usize::from(u16_at(table(b"maxp")?, 4)?);
    let long = u16_at(head, 50)? == 1;
    let glyph = |g: usize| glyph_data(glyf, loca, long, g);

    // Used glyphs and, transitively, the components of composite glyphs.
    let mut keep = BTreeSet::new();
    let mut todo: Vec<usize> = glyphs.into_iter().map(usize::from).chain([0]).collect();
    while let Some(g) = todo.pop() {
        if g < num_glyphs && keep.insert(g) {
            todo.extend(components(glyph(g)));
        }
    }

    let mut new_glyf = Vec::new();
    let mut new_loca = Vec::with_capacity((num_glyphs + 1) * 4);
    for g in 0..num_glyphs {
        new_loca.extend_from_slice(&u32::try_from(new_glyf.len()).ok()?.to_be_bytes());
        if keep.contains(&g) {
            new_glyf.extend_from_slice(glyph(g));
            new_glyf.resize(new_glyf.len().next_multiple_of(4), 0);
        }
    }
    new_loca.extend_from_slice(&u32::try_from(new_glyf.len()).ok()?.to_be_bytes());

    let mut new_head = head.to_vec();
    // Long loca offsets; checkSumAdjustment is filled in at the end.
    new_head
        .get_mut(50..52)?
        .copy_from_slice(&1u16.to_be_bytes());
    new_head.get_mut(8..12)?.fill(0);

    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = KEEP
        .iter()
        .filter_map(|&tag| {
            let data = match tag {
                b"glyf" => std::mem::take(&mut new_glyf),
                b"head" => std::mem::take(&mut new_head),
                _ => table(tag)?.to_vec(),
            };
            Some((tag, data))
        })
        .collect();
    tables.push((b"loca", new_loca));
    tables.sort_by_key(|(tag, _)| **tag);
    Some(assemble(&tables))
}

/// Outline data of glyph `g`; empty when `loca` points outside `glyf`.
fn glyph_data<'a>(glyf: &'a [u8], loca: &[u8], long: bool, g: usize) -> &'a [u8] {
    let offset = |g: usize| -> Option<usize> {
        if long {
            usize::try_from(u32_at(loca, g.checked_mul(4)?)?).ok()
        } else {
            Some(usize::from(u16_at(loca, g.checked_mul(2)?)?) * 2)
        }
    };
    let range = offset(g)
        .zip(offset(g.saturating_add(1)))
        .filter(|(a, b)| a <= b);
    range.and_then(|(a, b)| glyf.get(a..b)).unwrap_or(&[])
}

/// Glyph ids referenced by a composite glyph (OpenType `glyf` §Composite).
fn components(glyph: &[u8]) -> Vec<usize> {
    const WORDS: u16 = 0x0001;
    const SCALE: u16 = 0x0008;
    const MORE: u16 = 0x0020;
    const XY_SCALE: u16 = 0x0040;
    const TWO_BY_TWO: u16 = 0x0080;
    let mut out = Vec::new();
    // Simple glyphs have a non-negative contour count.
    if u16_at(glyph, 0).is_none_or(|n| (n as i16) >= 0) {
        return out;
    }
    let mut i = 10;
    while let (Some(flags), Some(id)) = (u16_at(glyph, i), u16_at(glyph, i + 2)) {
        out.push(usize::from(id));
        i += 4 + if flags & WORDS != 0 { 4 } else { 2 };
        i += match flags {
            f if f & SCALE != 0 => 2,
            f if f & XY_SCALE != 0 => 4,
            f if f & TWO_BY_TWO != 0 => 8,
            _ => 0,
        };
        if flags & MORE == 0 {
            break;
        }
    }
    out
}

/// Writes an sfnt file from tables sorted by tag.
fn assemble(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let n = tables.len() as u16;
    let pow = if n == 0 {
        0
    } else {
        15 - n.leading_zeros() as u16
    };
    let search = 16u16 << pow;
    let mut out = Vec::new();
    for v in [n, search, pow, n * 16 - search] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.splice(0..0, 0x0001_0000u32.to_be_bytes());
    let mut offset = 12 + 16 * tables.len();
    let mut head_at = None;
    for (tag, data) in tables {
        if **tag == *b"head" {
            head_at = Some(offset);
        }
        out.extend_from_slice(*tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    // head's checkSumAdjustment is zero here, as the spec's formula wants.
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    if let Some(slot) = head_at.and_then(|h| out.get_mut(h + 8..h + 12)) {
        slot.copy_from_slice(&adjust.to_be_bytes());
    }
    out
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut word = [0u8; 4];
        word[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttf_parser::{Face, GlyphId};

    const NOTO: &[u8] = include_bytes!("../tests/fixtures/NotoSans-Regular.ttf");

    #[test]
    fn keeps_used_glyphs_only() {
        let face = Face::parse(NOTO, 0).unwrap();
        let used: Vec<u16> = "Привет, мир ё"
            .chars()
            .map(|c| face.glyph_index(c).unwrap().0)
            .collect();
        let out = subset(NOTO, used.iter().copied()).unwrap();
        assert!(
            out.len() < NOTO.len() / 10,
            "{} of {}",
            out.len(),
            NOTO.len()
        );

        let sub = Face::parse(&out, 0).unwrap();
        assert_eq!(sub.number_of_glyphs(), face.number_of_glyphs());
        let bbox = |f: &Face, g: u16| f.glyph_bounding_box(GlyphId(g));
        for &g in &used {
            assert_eq!(bbox(&sub, g), bbox(&face, g), "glyph {g}");
            assert_eq!(
                sub.glyph_hor_advance(GlyphId(g)),
                face.glyph_hor_advance(GlyphId(g))
            );
        }
        let unused = face.glyph_index('Z').unwrap().0;
        assert!(bbox(&face, unused).is_some() && bbox(&sub, unused).is_none());
        // Whole-file checksum per the OpenType spec.
        assert_eq!(checksum(&out), 0xB1B0_AFBA);
    }

    /// Counts outline segments; drawing a composite needs all its components.
    struct Count(usize);

    impl ttf_parser::OutlineBuilder for Count {
        fn move_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn line_to(&mut self, _: f32, _: f32) {
            self.0 += 1;
        }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
            self.0 += 1;
        }
        fn close(&mut self) {}
    }

    #[test]
    fn composite_components_are_kept() {
        let face = Face::parse(NOTO, 0).unwrap();
        let out = subset(NOTO, "ёйÅ".chars().map(|c| face.glyph_index(c).unwrap().0)).unwrap();
        let sub = Face::parse(&out, 0).unwrap();
        for c in "ёйÅ".chars() {
            let g = face.glyph_index(c).unwrap();
            let (mut a, mut b) = (Count(0), Count(0));
            face.outline_glyph(g, &mut a);
            sub.outline_glyph(g, &mut b);
            assert!(a.0 > 0 && a.0 == b.0, "{c}: {} vs {}", a.0, b.0);
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(subset(b"junk", [1]).is_none());
        for end in (0..NOTO.len()).step_by(4099) {
            let _ = subset(&NOTO[..end], [1, 2, 3]);
        }
        subset(NOTO, [u16::MAX]).unwrap();
    }
}
