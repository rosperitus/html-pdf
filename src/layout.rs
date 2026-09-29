//! Block and inline layout (CSS 2.1 §9–10) into a flat display list.
//!
//! Coordinates are points from the top-left of the content area, y downward.

use crate::css::{Break, Color, Display, Length, LineHeight, TextAlign, WhiteSpace};
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
}

/// Lays out the document `width` wide on pages `page_height` tall; pages
/// follow each other at multiples of `page_height`. Pass `f32::INFINITY` for
/// one endless page.
pub fn layout(dom: &Dom, styles: &[Style], fonts: &[Font], width: f32, page_height: f32) -> Layout {
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
        retrying: false,
    };
    let mut flow = Flow {
        page_h: page_height,
        ..Flow::default()
    };
    cx.children(0, 0.0, width, &mut flow);
    Layout { items: cx.items }
}

/// Tolerance for float comparisons against page boundaries.
const EPS: f32 = 0.01;

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
    page_h: f32,
    /// Top of every line placed so far, in order.
    line_tops: Vec<f32>,
}

/// Flow and output state to roll back to when content moves to the next page.
struct Mark {
    y: f32,
    pos: f32,
    neg: f32,
    tops: usize,
    waiting: Vec<usize>,
    lines: usize,
    items: usize,
}

impl Flow {
    fn paged(&self) -> bool {
        self.page_h.is_finite() && self.page_h > 0.0
    }

    fn page_top(&self, y: f32) -> f32 {
        if !self.paged() {
            return 0.0;
        }
        ((y + EPS) / self.page_h).floor() * self.page_h
    }

    fn next_page(&self, y: f32) -> f32 {
        self.page_top(y) + self.page_h
    }

    fn at_page_top(&self, y: f32) -> bool {
        y - self.page_top(y) < EPS
    }

    /// Whether a box of height `h` at `top` runs past the end of its page.
    /// A box already at the top of a page never does: moving it gains nothing.
    fn crosses(&self, top: f32, h: f32) -> bool {
        self.paged() && !self.at_page_top(top) && top + h > self.next_page(top) + EPS
    }

    /// Forced break: continue on the next page, dropping margins before it.
    fn force_break(&mut self) {
        if self.paged() && !self.at_page_top(self.y) {
            self.y = self.next_page(self.y);
            self.pos = 0.0;
            self.neg = 0.0;
        }
    }

    /// Unforced break before content that was at `top`: margins are truncated.
    fn move_to_next_page(&mut self, top: f32) {
        self.y = self.next_page(top);
        self.pos = 0.0;
        self.neg = 0.0;
    }

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
    /// Inside a re-layout that moved content to the next page. Nested moves
    /// are skipped so that nested `avoid` boxes cannot cost exponential time.
    retrying: bool,
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

    fn mark(&self, flow: &Flow) -> Mark {
        Mark {
            y: flow.y,
            pos: flow.pos,
            neg: flow.neg,
            tops: flow.tops.len(),
            waiting: flow.waiting.clone(),
            lines: flow.line_tops.len(),
            items: self.items.len(),
        }
    }

    fn restore(&mut self, flow: &mut Flow, m: &Mark) {
        flow.y = m.y;
        flow.pos = m.pos;
        flow.neg = m.neg;
        flow.tops.truncate(m.tops);
        flow.waiting.clone_from(&m.waiting);
        flow.line_tops.truncate(m.lines);
        self.items.truncate(m.items);
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
        // The previous block sibling when it has `break-after: avoid`, with the state before it.
        let mut keep: Option<(Mark, usize)> = None;
        for &c in &node.children {
            if self.is_block(c) {
                if !run.is_empty() {
                    if self.inline(id, &run, x, w, flow) {
                        keep = None;
                    }
                    run.clear();
                }
                let mut mark = self.mark(flow);
                let (lines, prev_end) = (flow.line_tops.len(), flow.y);
                self.block(c, x, w, flow);
                if let Some((m, prev)) = keep.take()
                    && !self.retrying
                    && self.style(c).break_before != Break::Page
                    && !flow.at_page_top(m.y)
                    && let Some(&first) = flow.line_tops.get(lines)
                    && flow.page_top(first) > flow.page_top(prev_end - EPS)
                {
                    // A page break fell between `prev` and `c`: move `prev` along.
                    self.restore(flow, &m);
                    flow.move_to_next_page(m.y);
                    self.retrying = true;
                    self.block(prev, x, w, flow);
                    mark = self.mark(flow);
                    self.block(c, x, w, flow);
                    self.retrying = false;
                }
                keep = (self.style(c).break_after == Break::Avoid).then_some((mark, c));
            } else if self.style(c).display != Display::None {
                run.push(c);
            }
        }
        if !run.is_empty() {
            self.inline(id, &run, x, w, flow);
        }
    }

    /// A block with its forced breaks and `break-inside: avoid`.
    fn block(&mut self, id: usize, x: f32, cw: f32, flow: &mut Flow) {
        let s = self.style(id);
        if s.break_before == Break::Page {
            flow.force_break();
        }
        if s.break_inside == Break::Avoid && flow.paged() && !self.retrying {
            let m = self.mark(flow);
            let (top, bottom) = self.block_box(id, x, cw, flow);
            if bottom - top <= flow.page_h
                && flow.page_top(top) != flow.page_top(bottom - EPS)
                && !flow.at_page_top(top)
            {
                self.restore(flow, &m);
                flow.move_to_next_page(top);
                self.retrying = true;
                self.block_box(id, x, cw, flow);
                self.retrying = false;
            }
        } else {
            self.block_box(id, x, cw, flow);
        }
        if s.break_after == Break::Page {
            flow.force_break();
        }
    }

    // ponytail: tables and list items lay out as plain blocks until phase 5
    /// Lays out the block box; returns its border-box top and bottom.
    fn block_box(&mut self, id: usize, x: f32, cw: f32, flow: &mut Flow) -> (f32, f32) {
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
        (top, bottom)
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

    /// Lays out a run of inline-level siblings as line boxes; false if there were none.
    fn inline(&mut self, container: usize, run: &[usize], x: f32, w: f32, flow: &mut Flow) -> bool {
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
            return false;
        }

        let strut = self.metrics(self.font_for(cs), cs.font_size, cs.line_height);
        let heights: Vec<(f32, f32)> = lines
            .iter()
            .map(|line| {
                line.atoms
                    .iter()
                    .filter_map(|a| pieces.get(a.piece))
                    .fold(strut, |(ab, be), p| {
                        let s = self.style(p.node);
                        let (a, b) = self.metrics(p.font, s.font_size, s.line_height);
                        (ab.max(a), be.max(b))
                    })
            })
            .collect();
        let start = flow.y + flow.collapsed();
        let tops = place_lines(
            flow,
            start,
            &heights,
            cs.orphans as usize,
            cs.widows as usize,
        );
        // Pushing the first line to the next page moves the enclosing boxes' tops with it.
        if let Some(&first) = tops.first()
            && first != start
        {
            flow.move_to_next_page(start);
        }
        flow.resolve();

        for (i, line) in lines.iter().enumerate() {
            let (Some(&top), Some(&(above, below))) = (tops.get(i), heights.get(i)) else {
                continue;
            };
            flow.line_tops.push(top);
            let baseline = top + above;
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
        true
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

/// Tops of lines with `heights` (above, below the baseline) starting at
/// `start`, breaking pages between lines with orphans and widows honored
/// when that is possible (CSS Fragmentation §4.4).
fn place_lines(
    flow: &Flow,
    start: f32,
    heights: &[(f32, f32)],
    orphans: usize,
    widows: usize,
) -> Vec<f32> {
    let n = heights.len();
    let h = |i: usize| heights.get(i).map_or(0.0, |(a, b)| a + b);
    let mut tops = vec![start; n];
    // First line of the paragraph on the current page.
    let mut seg = 0;
    let mut y = start;
    let mut i = 0;
    while i < n {
        if flow.crosses(y, h(i)) {
            let mut b = i.min(n.saturating_sub(widows)).max(seg);
            if b < seg + orphans {
                b = seg;
            }
            if b == seg && flow.at_page_top(tops[seg]) {
                b = i;
            }
            y = flow.next_page(if b == i { y } else { tops[b] });
            i = b;
            seg = b;
        }
        tops[i] = y;
        y += h(i);
        i += 1;
    }
    tops
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
        paged(src, width, f32::INFINITY)
    }

    fn paged(src: &str, width: f32, page_h: f32) -> Layout {
        let fonts = [Font::new(NOTO).unwrap()];
        let dom = html::parse(src);
        let styled = style::cascade(&dom);
        layout(&dom, &styled.styles, &fonts, width, page_h)
    }

    /// Line tops (baseline minus the 12pt line's ascent share) grouped by page.
    fn pages(l: &Layout, page_h: f32) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = Vec::new();
        for (_, y, text) in texts(l) {
            let p = (y / page_h) as usize;
            out.resize_with(out.len().max(p + 1), Vec::new);
            out[p].push(text.trim().to_string());
        }
        out
    }

    fn para(n: usize) -> String {
        (1..=n)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("<br>")
    }

    #[test]
    fn lines_flow_onto_pages() {
        // 12pt lines, 50pt pages: four lines fit on a page.
        let l = paged(
            &format!("{BODY}<p style='orphans: 1; widows: 1'>{}</p>", para(9)),
            100.0,
            50.0,
        );
        assert_eq!(
            pages(&l, 50.0),
            [
                vec!["l1", "l2", "l3", "l4"],
                vec!["l5", "l6", "l7", "l8"],
                vec!["l9"]
            ]
        );
        // The first line on a new page starts right at its top.
        let t = texts(&l);
        assert!(t[4].1 > 50.0 && t[4].1 < 62.0);
    }

    #[test]
    fn widows_and_orphans() {
        let l = paged(&format!("{BODY}<p>{}</p>", para(5)), 100.0, 50.0);
        assert_eq!(pages(&l, 50.0), [vec!["l1", "l2", "l3"], vec!["l4", "l5"]]);
        // Only one line would stay on the first page: the paragraph moves whole.
        let l = paged(
            &format!("{BODY}<div style='height: 30pt'></div><p>{}</p>", para(3)),
            100.0,
            50.0,
        );
        assert_eq!(pages(&l, 50.0), [vec![], vec!["l1", "l2", "l3"]]);
    }

    #[test]
    fn forced_breaks() {
        let l = paged(
            &format!(
                "{BODY}<p style='break-before: page'>a</p><p style='page-break-after: always'>b</p><p>c</p>"
            ),
            100.0,
            50.0,
        );
        // No blank first page for a break before the very first content.
        assert_eq!(pages(&l, 50.0), [vec!["a", "b"], vec!["c"]]);
    }

    #[test]
    fn avoid_inside_and_keep_heading() {
        let src = format!(
            "{BODY}<p>a</p><p>b</p><div style='break-inside: avoid'>{}</div>",
            para(3)
        );
        let l = paged(&src, 100.0, 50.0);
        assert_eq!(pages(&l, 50.0), [vec!["a", "b"], vec!["l1", "l2", "l3"]]);
        // H would be the last line of page 1 with its paragraph on page 2.
        let src = format!(
            "{BODY}<p>a</p><p>b</p><p>c</p><h4 style='margin: 0; line-height: 12pt'>H</h4><p>{}</p>",
            para(3)
        );
        let l = paged(&src, 100.0, 50.0);
        assert_eq!(
            pages(&l, 50.0),
            [vec!["a", "b", "c"], vec!["H", "l1", "l2", "l3"]]
        );
        let src = src.replace("margin: 0;", "margin: 0; break-after: auto;");
        let l = paged(&src, 100.0, 50.0);
        assert_eq!(
            pages(&l, 50.0),
            [vec!["a", "b", "c", "H"], vec!["l1", "l2", "l3"]]
        );
    }

    #[test]
    fn oversized_content_still_progresses() {
        let l = paged(
            &format!("{BODY}<p style='font-size: 100pt; line-height: 120pt'>a b c</p>"),
            50.0,
            50.0,
        );
        assert_eq!(texts(&l).len(), 3);
        paged(
            &format!("{BODY}<div style='break-inside: avoid; height: 500pt'>x</div>"),
            50.0,
            50.0,
        );
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
        lay("<p>x</p>", 0.0);
        lay("<p>x</p>", -10.0);
    }
}
