//! Block and inline layout (CSS 2.1 §9–10) into a flat display list.
//!
//! Coordinates are points from the top-left of the content area, y downward.

use crate::css::{Color, Display, Length, LineHeight, TextAlign, WhiteSpace};
use crate::font::Font;
use crate::html::{Dom, NodeData};
use crate::style::{Size, Style};

#[derive(Debug, PartialEq)]
pub enum Item {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
    },
    /// `y` is the baseline.
    Text {
        x: f32,
        y: f32,
        font: usize,
        size: f32,
        color: Color,
        word_spacing: f32,
        text: String,
    },
}

pub struct Layout {
    pub items: Vec<Item>,
    pub height: f32,
}

pub fn layout(dom: &Dom, styles: &[Style], fonts: &[Font], width: f32) -> Layout {
    let faces: Vec<_> = fonts
        .iter()
        .map(|f| (f.family.as_str(), f.weight, f.italic))
        .collect();
    let mut cx = Ctx {
        dom,
        styles,
        fonts,
        faces,
        items: Vec::new(),
    };
    let mut flow = Flow::default();
    cx.children(0, 0.0, width, &mut flow);
    let height = flow.y + flow.collapsed().max(0.0);
    Layout {
        items: cx.items,
        height,
    }
}

/// Vertical position plus the adjoining margins not yet resolved (CSS 2.1 §8.3.1).
#[derive(Default)]
struct Flow {
    y: f32,
    pos: f32,
    neg: f32,
    /// Top edge of each block, filled in when its margins resolve.
    tops: Vec<f32>,
    /// Blocks whose top edge waits for the pending margins.
    waiting: Vec<usize>,
}

impl Flow {
    fn margin(&mut self, m: f32) {
        if m > 0.0 {
            self.pos = self.pos.max(m);
        } else {
            self.neg = self.neg.min(m);
        }
    }

    fn collapsed(&self) -> f32 {
        self.pos + self.neg
    }

    /// Something solid (border, padding, a line) is placed: margins stop collapsing.
    fn resolve(&mut self) {
        self.y += self.collapsed();
        self.pos = 0.0;
        self.neg = 0.0;
        for slot in self.waiting.drain(..) {
            if let Some(t) = self.tops.get_mut(slot) {
                *t = self.y;
            }
        }
    }

    fn open(&mut self) -> usize {
        self.tops.push(self.y);
        self.waiting.push(self.tops.len() - 1);
        self.tops.len() - 1
    }

    /// Top edge of a block; an empty block's margins collapse through it.
    fn top(&mut self, slot: usize) -> f32 {
        if let Some(i) = self.waiting.iter().position(|&s| s == slot) {
            self.waiting.remove(i);
            let y = self.y + self.collapsed();
            if let Some(t) = self.tops.get_mut(slot) {
                *t = y;
            }
        }
        self.tops.get(slot).copied().unwrap_or(self.y)
    }
}

struct Ctx<'a> {
    dom: &'a Dom,
    styles: &'a [Style],
    fonts: &'a [Font<'a>],
    faces: Vec<(&'a str, u16, bool)>,
    items: Vec<Item>,
}

/// Inline content after white-space processing: a text run or a forced break.
struct Piece {
    node: usize,
    text: String,
    font: usize,
    /// Background inherited from enclosing inline elements.
    bg: Option<Color>,
    brk: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Word,
    Space,
    Break,
}

/// Unit of line breaking: a slice of one piece.
#[derive(Clone, Copy)]
struct Atom {
    piece: usize,
    start: usize,
    end: usize,
    width: f32,
    kind: Kind,
}

struct Line {
    atoms: Vec<Atom>,
    width: f32,
    /// Ends the paragraph or a forced break: never justified.
    last: bool,
}

impl<'a> Ctx<'a> {
    fn style(&self, id: usize) -> &'a Style {
        let styles = self.styles;
        &styles[id]
    }

    fn font_for(&self, s: &Style) -> usize {
        pick(&self.faces, &s.font_family, s.font_weight, s.italic)
    }

    fn is_block(&self, id: usize) -> bool {
        match self.dom.nodes.get(id).map(|n| &n.data) {
            Some(NodeData::Element(_)) => match self.style(id).display {
                Display::None => false,
                // ponytail: an inline containing blocks becomes a block; its inline decorations are lost
                Display::Inline | Display::InlineBlock => self.contains_block(id),
                _ => true,
            },
            _ => false,
        }
    }

    fn contains_block(&self, id: usize) -> bool {
        self.dom
            .nodes
            .get(id)
            .is_some_and(|n| n.children.iter().any(|&c| self.is_block(c)))
    }

    /// Lays out the children of `id` in a content box at `x` of width `w`.
    fn children(&mut self, id: usize, x: f32, w: f32, flow: &mut Flow) {
        let dom = self.dom;
        let Some(node) = dom.nodes.get(id) else {
            return;
        };
        let mut run = Vec::new();
        for &c in &node.children {
            if self.is_block(c) {
                if !run.is_empty() {
                    self.inline(id, &run, x, w, flow);
                    run.clear();
                }
                self.block(c, x, w, flow);
            } else if self.style(c).display != Display::None {
                run.push(c);
            }
        }
        if !run.is_empty() {
            self.inline(id, &run, x, w, flow);
        }
    }

    // ponytail: tables and list items lay out as plain blocks until phase 5
    fn block(&mut self, id: usize, x: f32, cw: f32, flow: &mut Flow) {
        let s = self.style(id);
        let pct = |v: Size| match v {
            Size::Pt(p) => p,
            Size::Percent(p) => p * cw / 100.0,
            Size::Auto => 0.0,
        };
        let [bt, br, bb, bl] = s.border_width;
        let [pt, pr, pb, pl] = s.padding.map(pct);
        let [mt, mr, mb, mut ml] = s.margin.map(pct);
        let edges = bl + pl + pr + br;
        let width = match s.width {
            Size::Auto => (cw - ml - mr - edges).max(0.0),
            w => {
                let w = pct(w);
                let content = if s.border_box { w - edges } else { w }.max(0.0);
                let free = (cw - content - edges - ml - mr).max(0.0);
                // Auto margins absorb the free space; only the left one moves the box.
                match (s.margin[3], s.margin[1]) {
                    (Size::Auto, Size::Auto) => ml = free / 2.0,
                    (Size::Auto, _) => ml = free,
                    _ => {}
                }
                content
            }
        };
        // ponytail: percentage heights need a definite containing height; treated as auto
        let height = match s.height {
            Size::Pt(h) if s.border_box => Some((h - bt - pt - pb - bb).max(0.0)),
            Size::Pt(h) => Some(h.max(0.0)),
            _ => None,
        };

        flow.margin(mt);
        let slot = flow.open();
        if bt > 0.0 || pt > 0.0 || height.is_some() {
            flow.resolve();
        }
        flow.y += bt + pt;
        let content_top = flow.y;

        // Reserve slots so backgrounds and borders paint under the content.
        let deco = self.items.len();
        let bg = s.background;
        let borders: Vec<usize> = (0..4).filter(|&i| s.border_width[i] > 0.0).collect();
        let reserved = usize::from(bg.is_some()) + borders.len();
        for _ in 0..reserved {
            self.items.push(Item::Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
                color: Color::BLACK,
            });
        }

        self.children(id, x + ml + bl + pl, width, flow);

        if let Some(h) = height {
            flow.resolve();
            flow.y = content_top + h;
        } else if bb > 0.0 || pb > 0.0 {
            flow.resolve();
        }
        flow.y += pb + bb;
        let top = flow.top(slot);
        let bottom = flow.y.max(top);

        let (bx, bw, bh) = (x + ml, bl + pl + width + pr + br, bottom - top);
        let mut rects = Vec::with_capacity(reserved);
        if let Some(color) = bg {
            rects.push(Item::Rect {
                x: bx,
                y: top,
                w: bw,
                h: bh,
                color,
            });
        }
        for side in borders {
            let color = s.border_color[side].unwrap_or(s.color);
            let (x, y, w, h) = match side {
                0 => (bx, top, bw, bt),
                1 => (bx + bw - br, top, br, bh),
                2 => (bx, bottom - bb, bw, bb),
                _ => (bx, top, bl, bh),
            };
            rects.push(Item::Rect { x, y, w, h, color });
        }
        for (slot, rect) in self.items[deco..deco + reserved].iter_mut().zip(rects) {
            *slot = rect;
        }
        flow.margin(mb);
    }

    fn collect(&self, id: usize, bg: Option<Color>, prev_space: &mut bool, out: &mut Vec<Piece>) {
        let Some(node) = self.dom.nodes.get(id) else {
            return;
        };
        let s = self.style(id);
        match &node.data {
            NodeData::Text(t) => {
                let font = self.font_for(s);
                let ws = s.white_space;
                let collapse = matches!(
                    ws,
                    WhiteSpace::Normal | WhiteSpace::NoWrap | WhiteSpace::PreLine
                );
                let newlines = matches!(
                    ws,
                    WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine
                );
                let mut text = String::new();
                let piece = |text: String, brk| Piece {
                    node: id,
                    text,
                    font,
                    bg,
                    brk,
                };
                for c in t.chars() {
                    match c {
                        '\n' if newlines => {
                            out.push(piece(std::mem::take(&mut text), false));
                            out.push(piece(String::new(), true));
                            *prev_space = true;
                        }
                        '\r' => {}
                        ' ' | '\t' | '\n' | '\x0c' if collapse => {
                            if !*prev_space {
                                text.push(' ');
                                *prev_space = true;
                            }
                        }
                        // ponytail: tabs are 8 spaces, not tab stops
                        '\t' => text.push_str("        "),
                        c => {
                            text.push(c);
                            *prev_space = false;
                        }
                    }
                }
                out.push(piece(text, false));
            }
            NodeData::Element(el) if s.display != Display::None => {
                if el.tag == "br" {
                    out.push(Piece {
                        node: id,
                        text: String::new(),
                        font: 0,
                        bg,
                        brk: true,
                    });
                    *prev_space = true;
                    return;
                }
                let bg = s.background.or(bg);
                for &c in &node.children {
                    self.collect(c, bg, prev_space, out);
                }
            }
            _ => {}
        }
    }

    /// Lays out a run of inline-level siblings as line boxes.
    fn inline(&mut self, container: usize, run: &[usize], x: f32, w: f32, flow: &mut Flow) {
        let cs = self.style(container);
        let mut pieces = Vec::new();
        let mut prev_space = true;
        for &id in run {
            self.collect(id, None, &mut prev_space, &mut pieces);
        }
        let atoms = self.atoms(&pieces);
        let indent = match cs.text_indent {
            Size::Pt(v) => v,
            Size::Percent(p) => p * w / 100.0,
            Size::Auto => 0.0,
        };
        let lines = self.break_lines(&pieces, atoms, w, indent);
        if lines.is_empty() {
            return;
        }

        flow.resolve();
        let strut = self.metrics(self.font_for(cs), cs.font_size, cs.line_height);
        for (i, line) in lines.iter().enumerate() {
            let (mut above, mut below) = strut;
            for a in &line.atoms {
                if let Some(p) = pieces.get(a.piece) {
                    let s = self.style(p.node);
                    let (ab, be) = self.metrics(p.font, s.font_size, s.line_height);
                    above = above.max(ab);
                    below = below.max(be);
                }
            }
            let baseline = flow.y + above;
            let avail = if i == 0 { w - indent } else { w };
            let free = (avail - line.width).max(0.0);
            let spaces: usize = line
                .atoms
                .iter()
                .filter(|a| a.kind == Kind::Space)
                .filter_map(|a| pieces.get(a.piece)?.text.get(a.start..a.end))
                .map(|t| t.matches(' ').count())
                .sum();
            let (offset, word_spacing) = match cs.text_align {
                TextAlign::Left => (0.0, 0.0),
                TextAlign::Right => (free, 0.0),
                TextAlign::Center => (free / 2.0, 0.0),
                TextAlign::Justify if !line.last && spaces > 0 => (0.0, free / spaces as f32),
                TextAlign::Justify => (0.0, 0.0),
            };
            let mut pen = x + if i == 0 { indent } else { 0.0 } + offset;
            for group in line.atoms.chunk_by(|a, b| a.piece == b.piece) {
                let (Some(first), Some(last)) = (group.first(), group.last()) else {
                    continue;
                };
                let Some(p) = pieces.get(first.piece) else {
                    continue;
                };
                let Some(text) = p.text.get(first.start..last.end) else {
                    continue;
                };
                let width = group.iter().map(|a| a.width).sum::<f32>()
                    + word_spacing * text.matches(' ').count() as f32;
                self.emit_run(p, text, pen, baseline, width, word_spacing);
                pen += width;
            }
            flow.y = baseline + below;
        }
    }

    fn emit_run(
        &mut self,
        p: &Piece,
        text: &str,
        x: f32,
        baseline: f32,
        width: f32,
        word_spacing: f32,
    ) {
        let s = self.style(p.node);
        let Some(font) = self.fonts.get(p.font) else {
            return;
        };
        let size = s.font_size;
        let em = size / 1000.0;
        let (asc, desc, _) = font.vmetrics();
        if let Some(color) = p.bg {
            self.items.push(Item::Rect {
                x,
                y: baseline - asc * em,
                w: width,
                h: (asc + desc) * em,
                color,
            });
        }
        self.items.push(Item::Text {
            x,
            y: baseline,
            font: p.font,
            size,
            color: s.color,
            word_spacing,
            text: text.to_owned(),
        });
        let mut line = |(offset, thickness): (f32, f32), below: bool| {
            let y = if below {
                baseline + offset * em
            } else {
                baseline - offset * em
            };
            self.items.push(Item::Rect {
                x,
                y,
                w: width,
                h: thickness * em,
                color: s.color,
            });
        };
        if s.decoration.underline {
            line(font.underline(), true);
        }
        if s.decoration.line_through {
            line(font.strikeout(), false);
        }
    }

    /// Space above and below the baseline for a font at `size` with `line-height`.
    fn metrics(&self, font: usize, size: f32, lh: LineHeight) -> (f32, f32) {
        let (a, d, gap) = self
            .fonts
            .get(font)
            .map_or((800.0, 200.0, 0.0), Font::vmetrics);
        let (a, d, gap) = (a * size / 1000.0, d * size / 1000.0, gap * size / 1000.0);
        let l = match lh {
            LineHeight::Factor(k) => k * size,
            LineHeight::Len(Length::Pt(v)) => v,
            _ => a + d + gap,
        };
        let half = (l - a - d) / 2.0;
        (a + half, d + half)
    }

    fn atoms(&self, pieces: &[Piece]) -> Vec<Atom> {
        let mut atoms = Vec::new();
        for (i, p) in pieces.iter().enumerate() {
            if p.brk {
                atoms.push(Atom {
                    piece: i,
                    start: 0,
                    end: 0,
                    width: 0.0,
                    kind: Kind::Break,
                });
                continue;
            }
            let (Some(font), s) = (self.fonts.get(p.font), self.style(p.node)) else {
                continue;
            };
            let mut start = 0;
            let is_space = |c: char| c == ' ' || c == '\u{200B}';
            while start < p.text.len() {
                let Some(first) = p.text[start..].chars().next() else {
                    break;
                };
                let space = is_space(first);
                let end = p.text[start..]
                    .find(|c| is_space(c) != space)
                    .map_or(p.text.len(), |e| start + e);
                let width = font.measure(&p.text[start..end], s.font_size);
                let kind = if space { Kind::Space } else { Kind::Word };
                atoms.push(Atom {
                    piece: i,
                    start,
                    end,
                    width,
                    kind,
                });
                start = end;
            }
        }
        atoms
    }

    fn break_lines(&self, pieces: &[Piece], atoms: Vec<Atom>, w: f32, indent: f32) -> Vec<Line> {
        let ws = |a: &Atom| {
            pieces
                .get(a.piece)
                .map_or(WhiteSpace::Normal, |p| self.style(p.node).white_space)
        };
        let wraps = |a: &Atom| {
            matches!(
                ws(a),
                WhiteSpace::Normal | WhiteSpace::PreWrap | WhiteSpace::PreLine
            )
        };
        let collapsible = |a: &Atom| {
            matches!(
                ws(a),
                WhiteSpace::Normal | WhiteSpace::NoWrap | WhiteSpace::PreLine
            )
        };
        let trimmable =
            |a: &Atom| a.kind == Kind::Space && (collapsible(a) || ws(a) == WhiteSpace::PreWrap);

        let mut lines: Vec<Line> = Vec::new();
        let mut cur: Vec<Atom> = Vec::new();
        let finish = |lines: &mut Vec<Line>, mut atoms: Vec<Atom>, last: bool| {
            while atoms.last().is_some_and(trimmable) {
                atoms.pop();
            }
            let width = atoms.iter().map(|a| a.width).sum();
            lines.push(Line { atoms, width, last });
        };
        let avail = |lines: &Vec<Line>| if lines.is_empty() { w - indent } else { w };

        for atom in atoms {
            match atom.kind {
                Kind::Break => finish(&mut lines, std::mem::take(&mut cur), true),
                Kind::Space => {
                    if !(cur.is_empty() && collapsible(&atom)) {
                        cur.push(atom);
                    }
                }
                Kind::Word => {
                    let width: f32 = cur.iter().map(|a| a.width).sum();
                    if width + atom.width > avail(&lines)
                        && wraps(&atom)
                        && let Some(sp) = cur.iter().rposition(|a| a.kind == Kind::Space)
                    {
                        let rest = cur.split_off(sp + 1);
                        finish(&mut lines, std::mem::replace(&mut cur, rest), false);
                    }
                    if cur.is_empty() && atom.width > avail(&lines) && wraps(&atom) {
                        // A word wider than the line: break it between characters.
                        let mut part = atom;
                        for piece in self.split_word(pieces, atom, avail(&lines), w) {
                            if part.start != piece.start {
                                finish(&mut lines, std::mem::take(&mut cur), false);
                            }
                            part = piece;
                            cur.push(piece);
                        }
                    } else {
                        cur.push(atom);
                    }
                }
            }
        }
        if cur.iter().any(|a| a.kind != Kind::Space) {
            finish(&mut lines, cur, true);
        } else if let Some(line) = lines.last_mut() {
            line.last = true;
        }
        lines
    }

    /// Splits an overlong word into parts; the first fits `first`, the rest `w`.
    fn split_word(&self, pieces: &[Piece], atom: Atom, first: f32, w: f32) -> Vec<Atom> {
        let Some(p) = pieces.get(atom.piece) else {
            return vec![atom];
        };
        let (Some(font), size) = (self.fonts.get(p.font), self.style(p.node).font_size) else {
            return vec![atom];
        };
        let Some(text) = p.text.get(atom.start..atom.end) else {
            return vec![atom];
        };
        let mut parts = Vec::new();
        let (mut start, mut width, mut limit) = (atom.start, 0.0, first);
        for (i, c) in text.char_indices() {
            let cw = font.advance(font.glyph(c)) * size / 1000.0;
            let at = atom.start + i;
            if width + cw > limit && at > start {
                parts.push(Atom {
                    start,
                    end: at,
                    width,
                    ..atom
                });
                (start, width, limit) = (at, 0.0, w);
            }
            width += cw;
        }
        parts.push(Atom {
            start,
            end: atom.end,
            width,
            ..atom
        });
        parts
    }
}

/// Best face for a family list: first family present (else the first
/// font's family), then matching style, then closest weight.
fn pick(faces: &[(&str, u16, bool)], families: &[String], weight: u16, italic: bool) -> usize {
    let default = faces.first().map_or("", |f| f.0);
    let family = families
        .iter()
        .map(String::as_str)
        .find(|fam| faces.iter().any(|f| f.0 == *fam))
        .unwrap_or(default);
    faces
        .iter()
        .enumerate()
        .filter(|(_, f)| f.0 == family)
        .min_by_key(|(_, f)| (f.2 != italic, f.1.abs_diff(weight)))
        .map_or(0, |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{html, style};

    const NOTO: &[u8] = include_bytes!("../tests/fixtures/NotoSans-Regular.ttf");

    fn lay(src: &str, width: f32) -> Layout {
        let fonts = [Font::new(NOTO).unwrap()];
        let dom = html::parse(src);
        let styled = style::cascade(&dom);
        layout(&dom, &styled.styles, &fonts, width)
    }

    fn texts(l: &Layout) -> Vec<(f32, f32, &str)> {
        l.items
            .iter()
            .filter_map(|i| match i {
                Item::Text { x, y, text, .. } => Some((*x, *y, text.as_str())),
                _ => None,
            })
            .collect()
    }

    fn rects(l: &Layout) -> Vec<(f32, f32, f32, f32)> {
        l.items
            .iter()
            .filter_map(|i| match i {
                Item::Rect { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
                _ => None,
            })
            .collect()
    }

    const BODY: &str =
        "<style>body { margin: 0; font-size: 10pt; line-height: 12pt } p { margin: 0 }</style>";

    #[test]
    fn whitespace_collapses() {
        let l = lay(&format!("{BODY}<p>  a  \n <b> b </b>  c </p>"), 500.0);
        let t: Vec<_> = texts(&l).into_iter().map(|t| t.2).collect();
        assert_eq!(t.concat(), "a b c");
        let l = lay(&format!("{BODY}<pre>a  b\n c</pre>"), 500.0);
        assert_eq!(
            texts(&l).iter().map(|t| t.2).collect::<Vec<_>>(),
            ["a  b", " c"]
        );
    }

    #[test]
    fn wraps_within_width() {
        let words = "Съешь же ещё этих мягких французских булок да выпей чаю ".repeat(5);
        let l = lay(&format!("{BODY}<p>{words}</p>"), 200.0);
        let t = texts(&l);
        assert!(t.len() > 5);
        let fonts = [Font::new(NOTO).unwrap()];
        for (i, (x, y, text)) in t.iter().enumerate() {
            assert_eq!(*x, 0.0);
            assert!(fonts[0].measure(text, 10.0) <= 200.0, "{text:?}");
            assert!(!text.ends_with(' ') && !text.starts_with(' '));
            if i > 0 {
                assert!((y - t[i - 1].1 - 12.0).abs() < 0.01);
            }
        }
        assert!((l.height - 12.0 * t.len() as f32).abs() < 0.01);
    }

    #[test]
    fn long_word_breaks() {
        let l = lay(&format!("{BODY}<p>{}</p>", "x".repeat(200)), 100.0);
        let t = texts(&l);
        assert!(t.len() > 1);
        assert_eq!(t.iter().map(|t| t.2).collect::<String>(), "x".repeat(200));
    }

    #[test]
    fn alignment() {
        let fonts = [Font::new(NOTO).unwrap()];
        let w = fonts[0].measure("ab", 10.0);
        let l = lay(&format!("{BODY}<p style='text-align: right'>ab</p>"), 100.0);
        assert!((texts(&l)[0].0 - (100.0 - w)).abs() < 0.01);
        let l = lay(
            &format!("{BODY}<p style='text-align: center'>ab</p>"),
            100.0,
        );
        assert!((texts(&l)[0].0 - (100.0 - w) / 2.0).abs() < 0.01);
        let l = lay(
            &format!(
                "{BODY}<p style='text-align: justify'>{}</p>",
                "aa bb cc ".repeat(20)
            ),
            100.0,
        );
        let spacing: Vec<f32> = l
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text { word_spacing, .. } => Some(*word_spacing),
                _ => None,
            })
            .collect();
        assert!(spacing[0] > 0.0);
        assert_eq!(*spacing.last().unwrap(), 0.0);
    }

    #[test]
    fn breaks_and_indent() {
        let l = lay(
            &format!("{BODY}<p style='text-indent: 5pt'>a<br>b<br><br>c</p>"),
            100.0,
        );
        let t = texts(&l);
        assert_eq!(t.iter().map(|t| t.2).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(t[0].0, 5.0);
        assert_eq!(t[1].0, 0.0);
        assert!((t[2].1 - t[1].1 - 24.0).abs() < 0.01);
    }

    #[test]
    fn margins_collapse() {
        let l = lay(
            &format!(
                "{BODY}<div style='margin-bottom: 20pt; height: 10pt; background: red'></div>\
                 <div style='margin-top: 30pt; background: red'><p style='margin-top: 15pt'>x</p></div>"
            ),
            100.0,
        );
        let r = rects(&l);
        assert_eq!(r[0], (0.0, 0.0, 100.0, 10.0));
        // Sibling margins take the max; the child's margin collapses into the parent's.
        assert_eq!(r[1].1, 40.0);
        assert!((r[1].3 - 12.0).abs() < 0.01);
    }

    #[test]
    fn box_model() {
        let l = lay(
            &format!(
                "{BODY}<div style='width: 50pt; margin: 0 auto; padding: 5pt; border: 1pt solid; background: red'>x</div>\
                 <div style='width: 50pt; padding: 5pt; box-sizing: border-box; background: red'>x</div>"
            ),
            100.0,
        );
        let r = rects(&l);
        // Centered: 50 content + 2 * (5 padding + 1 border) = 62 wide.
        assert_eq!((r[0].0, r[0].2), (19.0, 62.0));
        assert!((r[0].3 - (12.0 + 12.0)).abs() < 0.01);
        assert_eq!(texts(&l)[0].0, 25.0);
        let second = r.iter().rev().find(|r| r.2 == 50.0).unwrap();
        assert_eq!(second.0, 0.0);
        assert_eq!(texts(&l)[1].0, 5.0);
    }

    #[test]
    fn decorations() {
        let l = lay(
            &format!("{BODY}<p>a <u>b</u> <span style='background: yellow'>c</span></p>"),
            100.0,
        );
        let r = rects(&l);
        assert_eq!(r.len(), 2);
        assert!(r[0].1 > texts(&l)[1].1, "underline below baseline");
        assert!(r[1].1 < texts(&l)[2].1, "background above baseline");
    }

    #[test]
    fn font_matching() {
        let faces = [
            ("sans", 400, false),
            ("sans", 700, false),
            ("serif", 400, false),
            ("serif", 400, true),
        ];
        let fam = |f: &[&str]| f.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick(&faces, &fam(&["serif"]), 400, false), 2);
        assert_eq!(pick(&faces, &fam(&["x", "serif"]), 700, true), 3);
        assert_eq!(pick(&faces, &fam(&["unknown"]), 600, false), 1);
        assert_eq!(pick(&[], &fam(&["a"]), 400, false), 0);
    }

    #[test]
    fn empty_and_hidden() {
        assert!(lay("", 100.0).items.is_empty());
        let l = lay(
            &format!("{BODY}<p>   </p><div style='display:none'>x</div><p><span></span></p>"),
            100.0,
        );
        assert!(l.items.is_empty());
        assert_eq!(l.height, 0.0);
        lay("<p>x</p>", 0.0);
        lay("<p>x</p>", -10.0);
    }
}
