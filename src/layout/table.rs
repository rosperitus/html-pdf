//! Tables: automatic column widths (CSS 2.1 §17.5.2.2), rows that never
//! split across pages, and header rows repeated on every page.

use super::{Ctx, Flow, Item, Kind};
use crate::css::{Display, VerticalAlign};
use crate::style::{Size, Style};

/// Columns beyond this are dropped; guards memory against absurd `colspan`.
const MAX_COLS: usize = 1000;

struct Cell {
    node: usize,
    col: usize,
    span: usize,
    rows: usize,
}

struct Row {
    node: usize,
    cells: Vec<Cell>,
}

struct Grid {
    rows: Vec<Row>,
    cols: usize,
    /// Leading rows from the first `thead`.
    header: usize,
    /// Captions and stray children, laid out as blocks above the rows.
    other: Vec<usize>,
}

/// A cell laid out off-page, waiting for its row's position.
struct Laid {
    node: usize,
    col: usize,
    rows: usize,
    x: f32,
    w: f32,
    /// Border-box height of the content.
    h: f32,
    /// Offset of the content box inside the border box.
    inset: (f32, f32),
    items: Vec<Item>,
}

fn spacing(s: &Style) -> (f32, f32) {
    if s.border_collapse {
        (0.0, 0.0)
    } else {
        s.border_spacing
    }
}

/// Column widths for `avail`: max widths plus a share of the extra when
/// there is room, else interpolated between min and max, never below min.
fn distribute(min: &[f32], max: &[f32], avail: f32) -> Vec<f32> {
    let (smin, smax): (f32, f32) = (min.iter().sum(), max.iter().sum());
    if avail >= smax {
        let extra = avail - smax;
        if smax > 0.0 {
            max.iter().map(|m| m + extra * m / smax).collect()
        } else {
            vec![avail / max.len().max(1) as f32; max.len()]
        }
    } else if avail > smin && smax > smin {
        let t = (avail - smin) / (smax - smin);
        min.iter().zip(max).map(|(a, b)| a + (b - a) * t).collect()
    } else {
        min.to_vec()
    }
}

impl Ctx<'_> {
    fn children_with(&self, id: usize, display: Display) -> Vec<usize> {
        self.dom.nodes.get(id).map_or_else(Vec::new, |n| {
            n.children
                .iter()
                .copied()
                .filter(|&c| self.dom.element(c).is_some() && self.style(c).display == display)
                .collect()
        })
    }

    fn grid(&self, table: usize) -> Grid {
        let dom = self.dom;
        let (mut header, mut body, mut footer, mut other) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for &c in dom
            .nodes
            .get(table)
            .map_or(&[][..], |n| n.children.as_slice())
        {
            if dom.element(c).is_none() {
                continue;
            }
            match self.style(c).display {
                Display::TableHeaderGroup if header.is_empty() => {
                    header = self.children_with(c, Display::TableRow)
                }
                Display::TableHeaderGroup | Display::TableRowGroup => {
                    body.extend(self.children_with(c, Display::TableRow))
                }
                Display::TableFooterGroup => {
                    footer.extend(self.children_with(c, Display::TableRow))
                }
                Display::TableRow => body.push(c),
                Display::None => {}
                _ => other.push(c),
            }
        }
        let n_header = header.len();
        let row_nodes: Vec<usize> = header.into_iter().chain(body).chain(footer).collect();
        let n = row_nodes.len();
        // Slots taken by cells from earlier rows (rowspan) or earlier in the row.
        let mut taken: Vec<Vec<bool>> = vec![Vec::new(); n];
        let mut rows = Vec::with_capacity(n);
        let mut cols = 0;
        for (r, &row) in row_nodes.iter().enumerate() {
            let mut cells = Vec::new();
            let mut col = 0;
            for c in self.children_with(row, Display::TableCell) {
                let attr = |name: &str| {
                    dom.element(c)
                        .and_then(|e| e.attr(name))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .filter(|v| *v > 0)
                        .unwrap_or(1)
                };
                let span = attr("colspan").min(MAX_COLS);
                let rowspan = attr("rowspan").min(n - r);
                while taken[r].get(col) == Some(&true) {
                    col += 1;
                }
                if col + span > MAX_COLS {
                    break;
                }
                for t in &mut taken[r..r + rowspan] {
                    if t.len() < col + span {
                        t.resize(col + span, false);
                    }
                    t[col..col + span].fill(true);
                }
                cells.push(Cell {
                    node: c,
                    col,
                    span,
                    rows: rowspan,
                });
                col += span;
                cols = cols.max(col);
            }
            rows.push(Row { node: row, cells });
        }
        Grid {
            rows,
            cols,
            header: n_header,
            other,
        }
    }

    /// Min- and max-content widths of the content of a block container.
    fn content_widths(&mut self, id: usize) -> (f32, f32) {
        let dom = self.dom;
        let Some(node) = dom.nodes.get(id) else {
            return (0.0, 0.0);
        };
        let (mut min, mut max) = (0.0f32, 0.0f32);
        let mut run = Vec::new();
        for &c in &node.children {
            if self.is_block(c) {
                let (a, b) = self.inline_widths(&std::mem::take(&mut run));
                let (c, d) = self.block_widths(c);
                (min, max) = (min.max(a).max(c), max.max(b).max(d));
            } else if self.style(c).display != Display::None {
                run.push(c);
            }
        }
        let (a, b) = self.inline_widths(&run);
        (min.max(a), max.max(b))
    }

    // ponytail: words glued across elements ("a<b>b</b>") count as separate for min width
    fn inline_widths(&mut self, run: &[usize]) -> (f32, f32) {
        let mut pieces = Vec::new();
        let mut prev_space = true;
        for &id in run {
            self.collect(id, None, f32::INFINITY, &mut prev_space, &mut pieces);
        }
        let (mut min, mut max, mut line) = (0.0f32, 0.0f32, 0.0f32);
        for a in self.atoms(&pieces) {
            match a.kind {
                Kind::Break => {
                    max = max.max(line);
                    line = 0.0;
                }
                Kind::Space => line += a.width,
                Kind::Word | Kind::Image => {
                    min = min.max(a.width);
                    line += a.width;
                }
            }
        }
        (min, max.max(line))
    }

    fn block_widths(&mut self, id: usize) -> (f32, f32) {
        let s = self.style(id);
        let fixed = |v: Size| if let Size::Pt(p) = v { p } else { 0.0 };
        let edges =
            s.border_width[1] + s.border_width[3] + fixed(s.padding[1]) + fixed(s.padding[3]);
        let margins = fixed(s.margin[1]) + fixed(s.margin[3]);
        let (min, max) = if let Size::Pt(w) = s.width {
            let w = if s.border_box { w } else { w + edges };
            (w, w)
        } else if self.dom.element(id).is_some_and(|e| e.tag == "img") {
            let w = self.image_box(id, f32::INFINITY).map_or(0.0, |(_, w, _)| w) + edges;
            (w, w)
        } else if s.display == Display::Table {
            let (a, b) = self.table_widths(id);
            (a + edges, b + edges)
        } else {
            let (a, b) = self.content_widths(id);
            (a + edges, b + edges)
        };
        (min + margins, max + margins)
    }

    /// Border-box min and max widths of a cell. Percentages resolve against
    /// `table_w` when it is known (positive).
    fn cell_widths(&mut self, cell: usize, table_w: f32) -> (f32, f32) {
        let s = self.style(cell);
        let pct = |v: Size| match v {
            Size::Pt(p) => p,
            Size::Percent(p) => p * table_w.max(0.0) / 100.0,
            Size::Auto => 0.0,
        };
        let edges = s.border_width[1] + s.border_width[3] + pct(s.padding[1]) + pct(s.padding[3]);
        let (a, b) = self.content_widths(cell);
        let (a, b) = (a + edges, b + edges);
        match s.width {
            Size::Pt(w) => {
                let w = a.max(if s.border_box { w } else { w + edges });
                (w, w)
            }
            Size::Percent(p) if table_w > 0.0 => (a, a.max(p * table_w / 100.0)),
            _ => (a, b),
        }
    }

    fn column_widths(&mut self, grid: &Grid, sh: f32, table_w: f32) -> (Vec<f32>, Vec<f32>) {
        let mut min = vec![0.0f32; grid.cols];
        let mut max = vec![0.0f32; grid.cols];
        let mut spanning = Vec::new();
        for cell in grid.rows.iter().flat_map(|r| &r.cells) {
            let (a, b) = self.cell_widths(cell.node, table_w);
            if cell.span == 1 {
                if let (Some(m), Some(x)) = (min.get_mut(cell.col), max.get_mut(cell.col)) {
                    *m = m.max(a);
                    *x = x.max(b);
                }
            } else {
                spanning.push((cell.col, cell.span, a, b));
            }
        }
        // Spanning cells widen their columns evenly where they need more room.
        for (col, span, a, b) in spanning {
            let gaps = sh * (span - 1) as f32;
            for (widths, need) in [(&mut min, a), (&mut max, b)] {
                let Some(range) = widths.get_mut(col..col + span) else {
                    continue;
                };
                let have = range.iter().sum::<f32>() + gaps;
                if need > have {
                    let add = (need - have) / span as f32;
                    range.iter_mut().for_each(|w| *w += add);
                }
            }
        }
        for (x, m) in max.iter_mut().zip(&min) {
            *x = x.max(*m);
        }
        (min, max)
    }

    /// Min and max content-box widths of a table.
    fn table_widths(&mut self, id: usize) -> (f32, f32) {
        let (sh, _) = spacing(self.style(id));
        let grid = self.grid(id);
        let (min, max) = self.column_widths(&grid, sh, 0.0);
        let gaps = if grid.cols > 0 {
            sh * (grid.cols + 1) as f32
        } else {
            0.0
        };
        (
            min.iter().sum::<f32>() + gaps,
            max.iter().sum::<f32>() + gaps,
        )
    }

    /// Shrink-to-fit content width of an auto-width table.
    pub(super) fn table_width(&mut self, id: usize, avail: f32) -> f32 {
        let (min, max) = self.table_widths(id);
        max.min(avail).max(min)
    }

    /// Lays out the rows of table `id` in a content box at `x`, `w` wide.
    pub(super) fn table(&mut self, id: usize, x: f32, w: f32, flow: &mut Flow) {
        let s = self.style(id);
        let (sh, sv) = spacing(s);
        let grid = self.grid(id);
        for &o in &grid.other {
            self.block(o, x, w, flow);
        }
        if grid.rows.is_empty() || grid.cols == 0 {
            return;
        }
        let (min, max) = self.column_widths(&grid, sh, w);
        let widths = distribute(&min, &max, (w - sh * (grid.cols + 1) as f32).max(0.0));
        let mut col_x = Vec::with_capacity(grid.cols + 1);
        let mut cx = x + sh;
        for wd in &widths {
            col_x.push(cx);
            cx += wd + sh;
        }
        col_x.push(cx);
        let span_x = |col: usize, span: usize| {
            let start = col_x.get(col).copied().unwrap_or(cx);
            let end = col_x.get(col + span).copied().unwrap_or(cx) - sh;
            (start, (end - start).max(0.0))
        };

        // Every cell is laid out on an endless page, then placed with its row.
        let mut laid: Vec<Vec<Laid>> = Vec::with_capacity(grid.rows.len());
        let mut heights = Vec::with_capacity(grid.rows.len());
        for row in &grid.rows {
            let mut cells = Vec::with_capacity(row.cells.len());
            let mut height = match self.style(row.node).height {
                Size::Pt(h) => h,
                _ => 0.0,
            };
            for cell in &row.cells {
                let (cx, cw) = span_x(cell.col, cell.span);
                let l = self.lay_cell(cell, cx, cw, w);
                if cell.rows == 1 {
                    height = height.max(l.h);
                }
                cells.push(l);
            }
            laid.push(cells);
            heights.push(height);
        }
        // Row-spanning cells stretch the last row they cover.
        for (r, cells) in laid.iter().enumerate() {
            for l in cells.iter().filter(|l| l.rows > 1) {
                let covered = heights
                    .get(r..r + l.rows)
                    .map_or(0.0, |h| h.iter().sum::<f32>());
                let need = l.h - covered - sv * (l.rows - 1) as f32;
                if need > 0.0
                    && let Some(last) = heights.get_mut(r + l.rows - 1)
                {
                    *last += need;
                }
            }
        }

        let row_span = (x + sh, (cx - sh - (x + sh)).max(0.0));
        let nh = grid.header;
        let header_h: f32 = heights.iter().take(nh).map(|h| h + sv).sum();
        let h_at = |r: usize| heights.get(r).copied().unwrap_or(0.0);
        // The header and the first body row stay together.
        let first_unit = header_h + h_at(nh) + sv;
        let start = flow.y + flow.collapsed();
        if flow.crosses(start, first_unit + sv) {
            flow.move_to_next_page(start);
        }
        flow.resolve();
        let mut cursor = flow.y;
        let mut page_first = true;
        for r in 0..grid.rows.len() {
            let h = h_at(r);
            let mut top = cursor + sv;
            if r > nh && flow.crosses(top, h) && h + header_h <= flow.page_h {
                cursor = flow.next_page(top);
                page_first = true;
                for hr in 0..nh {
                    let t = cursor + sv;
                    self.emit_row(
                        &grid,
                        &laid,
                        &heights,
                        hr,
                        t,
                        page_first,
                        s.border_collapse,
                        row_span,
                        sv,
                    );
                    page_first = false;
                    cursor = t + h_at(hr);
                }
                top = cursor + sv;
            }
            self.emit_row(
                &grid,
                &laid,
                &heights,
                r,
                top,
                page_first,
                s.border_collapse,
                row_span,
                sv,
            );
            flow.line_tops.push((top, top + h));
            page_first = false;
            cursor = top + h;
        }
        flow.y = cursor + sv;
    }

    /// Lays out a cell's content off-page at x = 0 of its content box.
    fn lay_cell(&mut self, cell: &Cell, x: f32, w: f32, table_w: f32) -> Laid {
        let s = self.style(cell.node);
        let pct = |v: Size| match v {
            Size::Pt(p) => p,
            Size::Percent(p) => p * table_w / 100.0,
            Size::Auto => 0.0,
        };
        let [bt, br, bb, bl] = s.border_width;
        let [pt, pr, pb, pl] = s.padding.map(pct);
        let inner = (w - bl - br - pl - pr).max(0.0);
        let saved = std::mem::take(&mut self.items);
        let mut sub = Flow {
            page_h: f32::INFINITY,
            ..Flow::default()
        };
        self.children(cell.node, 0.0, inner, &mut sub);
        let content = sub.y + sub.collapsed().max(0.0);
        let items = std::mem::replace(&mut self.items, saved);
        let edges = bt + pt + pb + bb;
        let h = match s.height {
            Size::Pt(v) => (content + edges).max(if s.border_box { v } else { v + edges }),
            _ => content + edges,
        };
        let inset = (bl + pl, bt + pt);
        Laid {
            node: cell.node,
            col: cell.col,
            rows: cell.rows,
            x,
            w,
            h,
            inset,
            items,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_row(
        &mut self,
        grid: &Grid,
        laid: &[Vec<Laid>],
        heights: &[f32],
        r: usize,
        top: f32,
        page_first: bool,
        collapse: bool,
        (row_x, row_w): (f32, f32),
        sv: f32,
    ) {
        let (Some(row), Some(cells)) = (grid.rows.get(r), laid.get(r)) else {
            return;
        };
        let h = heights.get(r).copied().unwrap_or(0.0);
        if let Some(color) = self.style(row.node).background {
            self.items.push(Item::Rect {
                x: row_x,
                y: top,
                w: row_w,
                h,
                color,
            });
        }
        for l in cells {
            let s = self.style(l.node);
            let cell_h = heights
                .get(r..r + l.rows)
                .map_or(h, |h| h.iter().sum::<f32>())
                + sv * (l.rows - 1) as f32;
            if let Some(color) = s.background {
                self.items.push(Item::Rect {
                    x: l.x,
                    y: top,
                    w: l.w,
                    h: cell_h,
                    color,
                });
            }
            let [bt, br, bb, bl] = s.border_width;
            // Collapsed borders: each cell draws right and bottom, plus top on
            // a page's first row and left in the first column.
            let drawn = [!collapse || page_first, true, true, !collapse || l.col == 0];
            for side in (0..4).filter(|&i| drawn[i] && s.border_width[i] > 0.0) {
                let color = s.border_color[side].unwrap_or(s.color);
                let (x, y, w, h) = match side {
                    0 => (l.x, top, l.w, bt),
                    1 => (l.x + l.w - br, top, br, cell_h),
                    2 => (l.x, top + cell_h - bb, l.w, bb),
                    _ => (l.x, top, bl, cell_h),
                };
                self.items.push(Item::Rect { x, y, w, h, color });
            }
            let offset = match s.vertical_align {
                VerticalAlign::Middle => (cell_h - l.h) / 2.0,
                VerticalAlign::Bottom => cell_h - l.h,
                VerticalAlign::Top | VerticalAlign::Baseline => 0.0,
            }
            .max(0.0);
            let (dx, dy) = (l.x + l.inset.0, top + l.inset.1 + offset);
            self.items.extend(l.items.iter().map(|i| i.shifted(dx, dy)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution() {
        assert_eq!(distribute(&[10.0, 10.0], &[30.0, 10.0], 80.0), [60.0, 20.0]);
        assert_eq!(distribute(&[10.0, 10.0], &[30.0, 10.0], 30.0), [20.0, 10.0]);
        assert_eq!(distribute(&[10.0, 10.0], &[30.0, 10.0], 5.0), [10.0, 10.0]);
        assert_eq!(distribute(&[0.0, 0.0], &[0.0, 0.0], 10.0), [5.0, 5.0]);
    }
}
