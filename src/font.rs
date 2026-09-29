use ttf_parser::{Face, GlyphId, Permissions, name_id};

use crate::error::{Error, Result};

/// A TrueType font borrowed from caller-owned bytes.
pub struct Font<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) face: Face<'a>,
}

impl<'a> Font<'a> {
    /// Parses the first face in `data`. Only TrueType (`glyf`) outlines are accepted.
    pub fn new(data: &'a [u8]) -> Result<Self> {
        let face = Face::parse(data, 0).map_err(|e| Error::Font(e.to_string()))?;
        if face.tables().glyf.is_none() {
            // ponytail: CFF outlines need FontFile3 + CIDFontType0, add when someone passes an .otf
            return Err(Error::Font("only TrueType outlines are supported".into()));
        }
        if face.permissions() == Some(Permissions::Restricted) {
            return Err(Error::Font("font license forbids embedding".into()));
        }
        Ok(Font { data, face })
    }

    /// Glyph for `c`, or `.notdef` (0) when the font has none.
    pub(crate) fn glyph(&self, c: char) -> u16 {
        self.face.glyph_index(c).map_or(0, |g| g.0)
    }

    /// Advance width in 1/1000 em, the unit PDF font metrics use.
    pub(crate) fn advance(&self, gid: u16) -> f32 {
        let adv = self.face.glyph_hor_advance(GlyphId(gid)).unwrap_or(0);
        self.scale(adv.into())
    }

    /// Converts font units to 1/1000 em.
    pub(crate) fn scale(&self, v: f32) -> f32 {
        v * 1000.0 / f32::from(self.face.units_per_em())
    }

    /// PostScript name reduced to characters allowed in a PDF name.
    pub(crate) fn postscript_name(&self) -> Option<String> {
        let name = self
            .face
            .names()
            .into_iter()
            .filter(|n| n.name_id == name_id::POST_SCRIPT_NAME)
            .find_map(|n| n.to_string())?;
        let name: String = name
            .chars()
            .filter(|c| c.is_ascii_graphic() && !"()<>[]{}/%#".contains(*c))
            .collect();
        (!name.is_empty()).then_some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTO: &[u8] = include_bytes!("../tests/fixtures/NotoSans-Regular.ttf");

    #[test]
    fn rejects_garbage() {
        assert!(Font::new(b"not a font").is_err());
        assert!(Font::new(&NOTO[..100]).is_err());
    }

    #[test]
    fn reads_metrics() {
        let font = Font::new(NOTO).unwrap();
        assert_ne!(font.glyph('Я'), 0);
        assert_eq!(font.glyph('\u{E000}'), 0);
        assert!(font.advance(font.glyph('M')) > font.advance(font.glyph('i')));
        assert_eq!(font.postscript_name().as_deref(), Some("NotoSans-Regular"));
    }
}
