//! Cascade and computed style (CSS 2.1 §6).

use std::rc::Rc;

use crate::css::{
    self, Break, Color, Combinator, Compound, Decl, Decoration, Display, FontWeight, Length,
    LineHeight, ListStyle, PageRule, Selector, Sheet, TextAlign, VerticalAlign, WhiteSpace,
};
use crate::html::{self, Dom, Element, NodeData};

/// A computed length: absolute, or a percentage the layout resolves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    Auto,
    Pt(f32),
    Percent(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub display: Display,
    pub margin: [Size; 4],
    pub padding: [Size; 4],
    /// Zero where the border style is none.
    pub border_width: [f32; 4],
    /// `None` is `currentColor`.
    pub border_color: [Option<Color>; 4],
    pub width: Size,
    pub height: Size,
    /// `box-sizing: border-box`: width and height include padding and border.
    pub border_box: bool,
    pub color: Color,
    pub background: Option<Color>,
    pub font_family: Rc<[String]>,
    /// Points.
    pub font_size: f32,
    pub font_weight: u16,
    pub italic: bool,
    /// `Len` is always `Length::Pt` after the cascade.
    pub line_height: LineHeight,
    pub text_align: TextAlign,
    pub text_indent: Size,
    pub decoration: Decoration,
    pub white_space: WhiteSpace,
    pub break_before: Break,
    pub break_after: Break,
    pub break_inside: Break,
    pub orphans: u32,
    pub widows: u32,
    pub list_style: ListStyle,
    pub vertical_align: VerticalAlign,
    pub border_collapse: bool,
    /// Horizontal and vertical, points.
    pub border_spacing: (f32, f32),
}

/// 16px, the browser default.
const INITIAL_FONT_SIZE: f32 = 12.0;

impl Style {
    pub fn initial() -> Style {
        Style {
            display: Display::Inline,
            margin: [Size::Pt(0.0); 4],
            padding: [Size::Pt(0.0); 4],
            border_width: [0.0; 4],
            border_color: [None; 4],
            width: Size::Auto,
            height: Size::Auto,
            border_box: false,
            color: Color::BLACK,
            background: None,
            font_family: Rc::from(["serif".to_string()]),
            font_size: INITIAL_FONT_SIZE,
            font_weight: 400,
            italic: false,
            line_height: LineHeight::Normal,
            text_align: TextAlign::Left,
            text_indent: Size::Pt(0.0),
            decoration: Decoration::default(),
            white_space: WhiteSpace::Normal,
            break_before: Break::Auto,
            break_after: Break::Auto,
            break_inside: Break::Auto,
            orphans: 2,
            widows: 2,
            list_style: ListStyle::Disc,
            vertical_align: VerticalAlign::Baseline,
            border_collapse: false,
            border_spacing: (0.0, 0.0),
        }
    }

    /// Starting point for a child: inherited properties copied, the rest initial.
    fn inherit(parent: &Style) -> Style {
        Style {
            display: Display::Inline,
            margin: [Size::Pt(0.0); 4],
            padding: [Size::Pt(0.0); 4],
            border_width: [0.0; 4],
            border_color: [None; 4],
            width: Size::Auto,
            height: Size::Auto,
            border_box: false,
            background: None,
            break_before: Break::Auto,
            break_after: Break::Auto,
            break_inside: Break::Auto,
            vertical_align: VerticalAlign::Baseline,
            ..parent.clone()
        }
    }
}

pub struct Styled {
    /// One per DOM node; text nodes carry their parent's style.
    pub styles: Vec<Style>,
    pub page: PageRule,
}

const UA: &str = "
html, body, div, p, h1, h2, h3, h4, h5, h6, ul, ol, dl, dt, dd, blockquote, pre, hr,
address, article, aside, footer, header, main, nav, section, figure, figcaption, form,
fieldset, center, details, summary, legend { display: block }
head, style, script, title, meta, link, template, base { display: none }
li { display: list-item }
table { display: table; border-spacing: 2px; box-sizing: border-box }
thead, tbody, tfoot { vertical-align: middle }
tr, td, th { vertical-align: inherit }
caption { display: table-caption; text-align: center }
thead { display: table-header-group }
tbody { display: table-row-group }
tfoot { display: table-footer-group }
tr { display: table-row }
td, th { display: table-cell; padding: 1px }
col, colgroup { display: none }
img { display: inline-block }
body { margin: 8px }
p, dl, figure, pre { margin: 1em 0 }
blockquote, figure { margin: 1em 40px }
ul, ol { margin: 1em 0; padding-left: 40px }
ul ul, ul ol, ol ul, ol ol { margin: 0 }
ol { list-style-type: decimal }
ul { list-style-type: disc }
ul ul, ol ul { list-style-type: circle }
ul ul ul, ol ul ul, ul ol ul, ol ol ul { list-style-type: square }
dd { margin-left: 40px }
h1 { font-size: 2em; margin: .67em 0 }
h2 { font-size: 1.5em; margin: .83em 0 }
h3 { font-size: 1.17em; margin: 1em 0 }
h4 { margin: 1.33em 0 }
h5 { font-size: .83em; margin: 1.67em 0 }
h6 { font-size: .67em; margin: 2.33em 0 }
h1, h2, h3, h4, h5, h6, b, strong, th, dt { font-weight: bold }
h1, h2, h3, h4, h5, h6 { break-after: avoid }
i, em, cite, var, dfn, address { font-style: italic }
u, ins, a { text-decoration: underline }
s, strike, del { text-decoration: line-through }
a { color: #0000ee }
pre, code, kbd, samp, tt { font-family: monospace }
pre { white-space: pre }
th { text-align: center }
center { text-align: center }
hr { border: 1px solid gray; margin: .5em 0 }
";

pub fn cascade(dom: &Dom) -> Styled {
    let mut sheet = Sheet::default();
    css::parse_sheet(UA, &mut sheet);
    let ua_rules = sheet.rules.len();
    for node in &dom.nodes {
        if let NodeData::Element(el) = &node.data
            && el.tag == "style"
        {
            let text: String = node
                .children
                .iter()
                .filter_map(|&c| match &dom.nodes.get(c)?.data {
                    NodeData::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            css::parse_sheet(&text, &mut sheet);
        }
    }

    let initial = Style::initial();
    let mut root_font_size = INITIAL_FONT_SIZE;
    let mut styles: Vec<Style> = Vec::with_capacity(dom.nodes.len());
    for (id, node) in dom.nodes.iter().enumerate() {
        // Parents precede children in the arena, so the parent is already styled.
        let parent = node.parent.and_then(|p| styles.get(p)).unwrap_or(&initial);
        let style = match &node.data {
            NodeData::Element(el) => {
                let s = compute(dom, id, el, parent, &sheet, ua_rules, root_font_size);
                if id == html::HTML {
                    root_font_size = s.font_size;
                }
                s
            }
            _ => parent.clone(),
        };
        styles.push(style);
    }
    Styled {
        styles,
        page: sheet.page,
    }
}

fn compute(
    dom: &Dom,
    id: usize,
    el: &Element,
    parent: &Style,
    sheet: &Sheet,
    ua_rules: usize,
    rem: f32,
) -> Style {
    let mut inline = Vec::new();
    if let Some(attr) = el.attr("style") {
        css::parse_decls(attr, &mut inline);
    }

    // (origin, specificity, order): UA < author < author !important.
    let mut matched: Vec<((u8, u32, usize), &Decl)> = Vec::new();
    for (order, rule) in sheet.rules.iter().enumerate() {
        let Some(spec) = rule
            .selectors
            .iter()
            .filter(|s| matches(dom, id, s))
            .map(|s| s.specificity)
            .max()
        else {
            continue;
        };
        let author = order >= ua_rules;
        for (decl, important) in &rule.decls {
            let origin = u8::from(author) + u8::from(author && *important);
            matched.push(((origin, spec, order), decl));
        }
    }
    for (decl, important) in &inline {
        matched.push(((1 + u8::from(*important), u32::MAX, usize::MAX), decl));
    }
    matched.sort_by_key(|(key, _)| *key);

    let decls: Vec<&Decl> = matched.into_iter().map(|(_, d)| d).collect();
    computed(parent, &decls, rem)
}

/// Style of a page-margin box: it inherits from the root element.
pub fn margin_box(root: &Style, decls: &[(Decl, bool)]) -> Style {
    let mut ordered: Vec<&(Decl, bool)> = decls.iter().collect();
    ordered.sort_by_key(|(_, important)| *important);
    let decls: Vec<&Decl> = ordered.into_iter().map(|(d, _)| d).collect();
    computed(root, &decls, root.font_size)
}

/// Applies cascaded declarations, in increasing precedence, on top of `parent`.
fn computed(parent: &Style, decls: &[&Decl], rem: f32) -> Style {
    let mut s = Style::inherit(parent);
    // Font size first: em units in every other property depend on it.
    for decl in decls {
        match decl {
            Decl::FontSize(l) => s.font_size = abs(*l, parent.font_size, rem),
            Decl::Inherit("font-size") => s.font_size = parent.font_size,
            _ => {}
        }
    }
    for decl in decls {
        apply(&mut s, parent, decl, rem);
    }
    for (width, drawn) in s.border_width.iter_mut().zip(border_drawn(decls)) {
        if !drawn {
            *width = 0.0;
        }
    }
    s
}

/// Final border-style per side.
fn border_drawn(decls: &[&Decl]) -> [bool; 4] {
    let mut drawn = [false; 4];
    for decl in decls {
        if let Decl::BorderStyle(side, d) = decl
            && let Some(slot) = drawn.get_mut(*side)
        {
            *slot = *d;
        }
    }
    drawn
}

fn apply(s: &mut Style, parent: &Style, decl: &Decl, rem: f32) {
    let em = s.font_size;
    let size = |l: Length| resolve(l, em, rem);
    let set = |arr: &mut [Size; 4], side: usize, v: Size| {
        if let Some(slot) = arr.get_mut(side) {
            *slot = v;
        }
    };
    match decl {
        Decl::Display(d) => s.display = *d,
        Decl::Margin(side, l) => set(&mut s.margin, *side, size(*l)),
        Decl::Padding(side, l) => set(&mut s.padding, *side, size(*l)),
        Decl::BorderWidth(side, l) => {
            if let Some(w) = s.border_width.get_mut(*side) {
                *w = abs(*l, em, rem);
            }
        }
        Decl::BorderStyle(..) => {}
        Decl::BorderColor(side, c) => {
            if let Some(slot) = s.border_color.get_mut(*side) {
                *slot = *c;
            }
        }
        Decl::Width(l) => s.width = size(*l),
        Decl::Height(l) => s.height = size(*l),
        Decl::Color(c) => s.color = *c,
        Decl::Background(c) => s.background = (c.a > 0).then_some(*c),
        Decl::FontFamily(f) => s.font_family = f.clone(),
        Decl::FontSize(_) => {}
        Decl::FontWeight(w) => {
            s.font_weight = match *w {
                FontWeight::Abs(n) => n,
                FontWeight::Bolder => match parent.font_weight {
                    ..350 => 400,
                    350..550 => 700,
                    _ => 900,
                },
                FontWeight::Lighter => match parent.font_weight {
                    ..550 => 100,
                    550..750 => 400,
                    _ => 700,
                },
            }
        }
        Decl::Italic(i) => s.italic = *i,
        Decl::LineHeight(lh) => {
            s.line_height = match *lh {
                LineHeight::Len(l) => LineHeight::Len(Length::Pt(abs(l, em, rem))),
                other => other,
            }
        }
        Decl::TextAlign(a) => s.text_align = *a,
        Decl::TextIndent(l) => s.text_indent = size(*l),
        Decl::Decoration(d) => s.decoration = *d,
        Decl::WhiteSpace(w) => s.white_space = *w,
        Decl::BreakBefore(b) => s.break_before = *b,
        Decl::BreakAfter(b) => s.break_after = *b,
        Decl::BreakInside(b) => s.break_inside = *b,
        Decl::Orphans(n) => s.orphans = *n,
        Decl::Widows(n) => s.widows = *n,
        Decl::ListStyle(l) => s.list_style = *l,
        Decl::BorderBox(b) => s.border_box = *b,
        Decl::VerticalAlign(v) => s.vertical_align = *v,
        Decl::BorderCollapse(c) => s.border_collapse = *c,
        Decl::BorderSpacing(h, v) => s.border_spacing = (abs(*h, em, rem), abs(*v, em, rem)),
        Decl::Inherit(name) => inherit_one(s, parent, name),
    }
}

fn inherit_one(s: &mut Style, p: &Style, name: &str) {
    if let Some((prop, side)) = css::side_prop(name) {
        let copy = |dst: &mut [Size; 4], src: &[Size; 4]| {
            if let (Some(d), Some(v)) = (dst.get_mut(side), src.get(side)) {
                *d = *v;
            }
        };
        match prop {
            "margin" => copy(&mut s.margin, &p.margin),
            "padding" => copy(&mut s.padding, &p.padding),
            "border-width" => {
                if let (Some(d), Some(v)) = (s.border_width.get_mut(side), p.border_width.get(side))
                {
                    *d = *v;
                }
            }
            "border-color" => {
                if let (Some(d), Some(v)) = (s.border_color.get_mut(side), p.border_color.get(side))
                {
                    *d = *v;
                }
            }
            // ponytail: border-style inherit is ignored; nobody writes it
            _ => {}
        }
        return;
    }
    match name {
        "display" => s.display = p.display,
        "width" => s.width = p.width,
        "height" => s.height = p.height,
        "color" => s.color = p.color,
        "background-color" => s.background = p.background,
        "font-family" => s.font_family = p.font_family.clone(),
        "font-weight" => s.font_weight = p.font_weight,
        "font-style" => s.italic = p.italic,
        "line-height" => s.line_height = p.line_height,
        "text-align" => s.text_align = p.text_align,
        "text-indent" => s.text_indent = p.text_indent,
        "text-decoration" => s.decoration = p.decoration,
        "white-space" => s.white_space = p.white_space,
        "break-before" => s.break_before = p.break_before,
        "break-after" => s.break_after = p.break_after,
        "break-inside" => s.break_inside = p.break_inside,
        "orphans" => s.orphans = p.orphans,
        "widows" => s.widows = p.widows,
        "list-style-type" => s.list_style = p.list_style,
        "box-sizing" => s.border_box = p.border_box,
        "vertical-align" => s.vertical_align = p.vertical_align,
        "border-collapse" => s.border_collapse = p.border_collapse,
        "border-spacing" => s.border_spacing = p.border_spacing,
        _ => {}
    }
}

fn resolve(l: Length, em: f32, rem: f32) -> Size {
    match l {
        Length::Pt(v) => Size::Pt(v),
        Length::Em(v) => Size::Pt(v * em),
        Length::Rem(v) => Size::Pt(v * rem),
        Length::Percent(v) => Size::Percent(v),
        Length::Auto => Size::Auto,
    }
}

/// Absolute points; for font-size `em` is the parent's size and `%` never reaches here.
fn abs(l: Length, em: f32, rem: f32) -> f32 {
    match resolve(l, em, rem) {
        Size::Pt(v) => v,
        _ => 0.0,
    }
}

fn matches(dom: &Dom, id: usize, sel: &Selector) -> bool {
    match_from(dom, id, &sel.parts)
}

/// Matches `parts` right to left with `id` as the subject of the last part.
fn match_from(dom: &Dom, id: usize, parts: &[(Combinator, Compound)]) -> bool {
    let Some(((comb, last), rest)) = parts.split_last() else {
        return true;
    };
    let Some(el) = dom.element(id) else {
        return false;
    };
    if !compound_matches(el, last) {
        return false;
    }
    if rest.is_empty() {
        return true;
    }
    let mut ancestor = dom.nodes.get(id).and_then(|n| n.parent);
    while let Some(a) = ancestor {
        if match_from(dom, a, rest) {
            return true;
        }
        if *comb == Combinator::Child {
            return false;
        }
        ancestor = dom.nodes.get(a).and_then(|n| n.parent);
    }
    false
}

fn compound_matches(el: &Element, k: &Compound) -> bool {
    k.tag.as_ref().is_none_or(|t| *t == el.tag)
        && k.id.as_ref().is_none_or(|id| el.attr("id") == Some(id))
        && (k.classes.is_empty() || {
            let have = el.attr("class").unwrap_or("");
            k.classes
                .iter()
                .all(|c| have.split_ascii_whitespace().any(|h| h == c))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Style of the first element with `id` attribute `target`.
    fn style_of(src: &str, target: &str) -> Style {
        let dom = html::parse(src);
        let styled = cascade(&dom);
        let id = (0..dom.nodes.len())
            .find(|&i| dom.element(i).and_then(|e| e.attr("id")) == Some(target))
            .unwrap();
        styled.styles[id].clone()
    }

    #[test]
    fn specificity_importance_and_order() {
        let css = "<style>
            p { color: red }
            .a { color: green }
            #x { color: blue }
            div > p { color: yellow }
            .c { color: black !important }
            .b { color: white !important }
        </style>";
        let red = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        };
        let blue = Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        };
        assert_eq!(style_of(&format!("{css}<p class=a id=x>"), "x").color, blue);
        assert_eq!(
            style_of(&format!("{css}<p class='a b' id=x>"), "x").color.r,
            255
        );
        assert_eq!(
            style_of(&format!("{css}<p class=c id=x style='color: red'>"), "x").color,
            Color::BLACK
        );
        assert_eq!(
            style_of(
                &format!("{css}<p class=c id=x style='color: red !important'>"),
                "x"
            )
            .color,
            red
        );
    }

    #[test]
    fn combinators() {
        let css = "<style>div span { width: 1pt } div > b { width: 2pt } .a .b > i { width: 3pt }</style>";
        let w = |body: &str, id: &str| style_of(&format!("{css}{body}"), id).width;
        assert_eq!(w("<div><p><span id=t>", "t"), Size::Pt(1.0));
        assert_eq!(w("<span id=t>", "t"), Size::Auto);
        assert_eq!(w("<div><b id=t>", "t"), Size::Pt(2.0));
        assert_eq!(w("<div><p><b id=t>", "t"), Size::Auto);
        assert_eq!(
            w("<p class=a><span><span class=b><i id=t>", "t"),
            Size::Pt(3.0)
        );
        assert_eq!(
            w("<p class=a><span class=b><span><i id=t>", "t"),
            Size::Auto
        );
    }

    #[test]
    fn inheritance_and_units() {
        let src = "<html><style>
            html { font-size: 10pt }
            div { font-size: 2em; margin: 1em 2rem; color: navy; line-height: 1.5; border-top: 2px solid }
            p { font-size: 50%; line-height: 150%; border-bottom-width: 3pt }
        </style><div id=d><p id=p>";
        let d = style_of(src, "d");
        assert_eq!(d.font_size, 20.0);
        assert_eq!(
            d.margin,
            [
                Size::Pt(20.0),
                Size::Pt(20.0),
                Size::Pt(20.0),
                Size::Pt(20.0)
            ]
        );
        assert_eq!(d.border_width, [1.5, 0.0, 0.0, 0.0]);
        assert_eq!(d.border_color[0], None);
        let p = style_of(src, "p");
        assert_eq!(p.font_size, 10.0);
        assert_eq!(p.color, d.color);
        // UA `p { margin: 1em 0 }`, not the div's margins.
        assert_eq!(
            p.margin,
            [Size::Pt(10.0), Size::Pt(0.0), Size::Pt(10.0), Size::Pt(0.0)]
        );
        assert_eq!(p.line_height, LineHeight::Len(Length::Pt(15.0)));
        // Width without a style draws nothing.
        assert_eq!(p.border_width, [0.0; 4]);
        let src = "<style>div { line-height: 1.5; font-size: 10pt } p { font-size: 20pt }</style><div><p id=p>";
        assert_eq!(style_of(src, "p").line_height, LineHeight::Factor(1.5));
    }

    #[test]
    fn user_agent_defaults() {
        let src = "<html><head><title id=t>x</title></head><body id=b><h1 id=h>T</h1><table><tr><td id=c>";
        assert_eq!(style_of(src, "t").display, Display::None);
        assert_eq!(style_of(src, "b").margin, [Size::Pt(6.0); 4]);
        let h = style_of(src, "h");
        assert_eq!(
            (h.display, h.font_size, h.font_weight),
            (Display::Block, 24.0, 700)
        );
        assert_eq!(style_of(src, "c").display, Display::TableCell);
    }

    #[test]
    fn explicit_inherit() {
        let src = "<style>div { margin-left: 5pt; color: red } p { margin: inherit; color: inherit; font: inherit }</style><div><p id=p>";
        let p = style_of(src, "p");
        assert_eq!(p.margin[3], Size::Pt(5.0));
        assert_eq!(p.margin[0], Size::Pt(0.0));
        assert_eq!(p.color.r, 255);
    }

    #[test]
    fn page_rule() {
        let dom = html::parse(
            "<style>@page { size: A4 landscape; margin: 2cm 1in } @page :first { size: A5 }</style>",
        );
        let page = cascade(&dom).page;
        assert_eq!(page.size, Some((841.89, 595.28)));
        assert_eq!(page.margin[0], Some(Length::Pt(72.0 * 2.0 / 2.54)));
        assert_eq!(page.margin[1], Some(Length::Pt(72.0)));
    }
}
