//! CSS syntax: style sheets, selectors and declarations with typed values.
//!
//! Follows CSS 2.1 error recovery: a rule with an unsupported selector is
//! dropped, a declaration with an unsupported value is ignored. Shorthands are
//! expanded to longhands here, so the cascade only sees longhands.

use std::rc::Rc;

#[derive(Default)]
pub struct Sheet {
    pub rules: Vec<Rule>,
    pub page: PageRule,
}

pub struct Rule {
    pub selectors: Vec<Selector>,
    /// Declaration and its `!important` flag.
    pub decls: Vec<(Decl, bool)>,
}

/// Declarations from plain `@page` rules; later rules override earlier ones.
#[derive(Default, Debug, PartialEq)]
pub struct PageRule {
    /// Width and height in points.
    pub size: Option<(f32, f32)>,
    pub margin: [Option<Length>; 4],
}

pub struct Selector {
    /// Compounds left to right, each with the combinator that joins it to the previous one.
    pub parts: Vec<(Combinator, Compound)>,
    pub specificity: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
}

#[derive(Default, Debug, PartialEq)]
pub struct Compound {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Pt(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0);

    const fn rgb(v: u32) -> Color {
        let [_, r, g, b] = v.to_be_bytes();
        Color { r, g, b, a: 255 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Inline,
    Block,
    InlineBlock,
    ListItem,
    Table,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    TableCaption,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    Auto,
    Page,
    Avoid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStyle {
    None,
    Disc,
    Circle,
    Square,
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontWeight {
    Abs(u16),
    Bolder,
    Lighter,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    Normal,
    /// Multiple of the element's font size; inherited as a factor.
    Factor(f32),
    Len(Length),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decoration {
    pub underline: bool,
    pub line_through: bool,
}

/// A longhand declaration. Sides are indexed top, right, bottom, left.
#[derive(Clone, Debug, PartialEq)]
pub enum Decl {
    Display(Display),
    Margin(usize, Length),
    Padding(usize, Length),
    BorderWidth(usize, Length),
    /// `true` when a border is drawn (any style but none/hidden).
    BorderStyle(usize, bool),
    /// `None` is `currentColor`.
    BorderColor(usize, Option<Color>),
    Width(Length),
    Height(Length),
    Color(Color),
    Background(Color),
    /// Lowercase family names in preference order.
    FontFamily(Rc<[String]>),
    /// `Em` and `Percent` are relative to the parent's font size.
    FontSize(Length),
    FontWeight(FontWeight),
    Italic(bool),
    LineHeight(LineHeight),
    TextAlign(TextAlign),
    TextIndent(Length),
    Decoration(Decoration),
    WhiteSpace(WhiteSpace),
    BreakBefore(Break),
    BreakAfter(Break),
    BreakInside(Break),
    Orphans(u32),
    Widows(u32),
    ListStyle(ListStyle),
    /// `true` for `border-box`.
    BorderBox(bool),
    /// `inherit` for the named longhand.
    Inherit(&'static str),
}

/// Every longhand the cascade knows, for expanding `inherit` on shorthands.
pub const LONGHANDS: &[&str] = &[
    "display",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-style",
    "border-right-style",
    "border-bottom-style",
    "border-left-style",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "width",
    "height",
    "color",
    "background-color",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "text-align",
    "text-indent",
    "text-decoration",
    "white-space",
    "break-before",
    "break-after",
    "break-inside",
    "orphans",
    "widows",
    "list-style-type",
    "box-sizing",
];

const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];
const MAX_NESTING: u32 = 8;

pub fn parse_sheet(src: &str, sheet: &mut Sheet) {
    parse_rules(&strip_comments(src), sheet, 0);
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..]
            .find("*/")
            .map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out.push_str(rest);
    out
}

fn parse_rules(src: &str, sheet: &mut Sheet, depth: u32) {
    let mut rest = src;
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest
            .strip_prefix("<!--")
            .or_else(|| rest.strip_prefix("-->"))
        {
            rest = r;
            continue;
        }
        if rest.is_empty() {
            break;
        }
        let at = rest.starts_with('@');
        let Some(open) = find_top(rest, |c| c == '{' || (at && c == ';')) else {
            break;
        };
        let prelude = rest[..open].trim();
        if rest[open..].starts_with(';') {
            rest = &rest[open + 1..];
            continue;
        }
        let close = block_end(rest, open);
        let body = &rest[open + 1..close];
        rest = rest.get(close + 1..).unwrap_or("");

        if let Some(at) = prelude.strip_prefix('@') {
            let name_len = at.find(|c: char| !is_ident(c)).unwrap_or(at.len());
            let (name, cond) = at.split_at(name_len);
            match name.to_ascii_lowercase().as_str() {
                "media" if depth < MAX_NESTING && media_matches(cond) => {
                    parse_rules(body, sheet, depth + 1)
                }
                // ponytail: :first/:left/:right pages ignored, add with per-page styles in phase 4
                "page" if cond.trim().is_empty() => parse_page(body, &mut sheet.page),
                _ => {}
            }
            continue;
        }
        let selectors: Option<Vec<_>> = split_top(prelude, |c| c == ',')
            .into_iter()
            .map(parse_selector)
            .collect();
        if let Some(selectors) = selectors {
            let mut decls = Vec::new();
            parse_decls(body, &mut decls);
            sheet.rules.push(Rule { selectors, decls });
        }
    }
}

/// ponytail: media features are not evaluated; only `print` queries may carry them
fn media_matches(query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    query.trim().is_empty()
        || split_top(&query, |c| c == ',').iter().any(|q| {
            let mut words = q.split_whitespace().skip_while(|w| *w == "only");
            match words.next() {
                Some("print") => true,
                Some("all") => !q.contains('('),
                _ => false,
            }
        })
}

fn parse_page(body: &str, page: &mut PageRule) {
    // Drop nested margin-box rules like `@top-center { ... }`.
    let mut flat = String::new();
    let mut rest = body;
    while let Some(at) = rest.find('@') {
        flat.push_str(&rest[..at]);
        let open = rest[at..].find('{').map_or(rest.len(), |o| at + o);
        rest = rest.get(block_end(rest, open) + 1..).unwrap_or("");
    }
    flat.push_str(rest);

    for d in split_top(&flat, |c| c == ';') {
        let Some((name, value)) = d.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        if name == "size" {
            if let Some(size) = page_size(&value) {
                page.size = size;
            }
        } else if name == "margin" || name.starts_with("margin-") {
            for decl in expand(&name, &value).unwrap_or_default() {
                if let Decl::Margin(side, len) = decl
                    && let Some(slot) = page.margin.get_mut(side)
                {
                    *slot = Some(len);
                }
            }
        }
    }
}

/// `Some(None)` for `auto`.
fn page_size(value: &str) -> Option<Option<(f32, f32)>> {
    let mut size = None;
    let mut landscape = None;
    let mut lengths = Vec::new();
    for t in split_ws(value) {
        match t {
            "auto" => return Some(None),
            "portrait" => landscape = Some(false),
            "landscape" => landscape = Some(true),
            "a5" => size = Some((419.53, 595.28)),
            "a4" => size = Some((595.28, 841.89)),
            "a3" => size = Some((841.89, 1190.55)),
            "b5" => size = Some((498.9, 708.66)),
            "b4" => size = Some((708.66, 1000.63)),
            "letter" => size = Some((612.0, 792.0)),
            "legal" => size = Some((612.0, 1008.0)),
            "ledger" => size = Some((792.0, 1224.0)),
            t => match length(t)? {
                Length::Pt(v) if v > 0.0 => lengths.push(v),
                _ => return None,
            },
        }
    }
    let (w, h) = match (size, lengths.as_slice()) {
        (Some(s), []) => s,
        (None, [side]) => (*side, *side),
        (None, [w, h]) => (*w, *h),
        (None, []) if landscape.is_some() => (595.28, 841.89),
        _ => return None,
    };
    Some(Some(match landscape {
        Some(true) => (w.max(h), w.min(h)),
        Some(false) => (w.min(h), w.max(h)),
        None => (w, h),
    }))
}

fn parse_selector(s: &str) -> Option<Selector> {
    let mut parts = Vec::new();
    let mut comb = Combinator::Descendant;
    let mut cur: Option<Compound> = None;
    let mut chars = s.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '>' | ' ' | '\t' | '\n' | '\r' | '\x0c' => {
                if let Some(k) = cur.take() {
                    parts.push((comb, k));
                    comb = Combinator::Descendant;
                }
                if c == '>' {
                    if parts.is_empty() || comb == Combinator::Child {
                        return None;
                    }
                    comb = Combinator::Child;
                }
            }
            '*' => {
                cur.get_or_insert_default();
            }
            '.' | '#' => {
                let name = ident(&mut chars);
                if name.is_empty() {
                    return None;
                }
                let k = cur.get_or_insert_default();
                if c == '.' {
                    k.classes.push(name);
                } else {
                    k.id = Some(name);
                }
            }
            c if is_ident(c) => {
                let mut name = c.to_string();
                name.push_str(&ident(&mut chars));
                let k = cur.get_or_insert_default();
                if k.tag.is_some() || k.id.is_some() || !k.classes.is_empty() {
                    return None;
                }
                k.tag = Some(name.to_ascii_lowercase());
            }
            // ponytail: attribute selectors, pseudo-classes, `+` and `~` drop the rule; add on demand
            _ => return None,
        }
    }
    match cur {
        Some(k) => parts.push((comb, k)),
        None if comb == Combinator::Child => return None,
        None => {}
    }
    if parts.is_empty() {
        return None;
    }
    let (mut ids, mut classes, mut tags) = (0u32, 0u32, 0u32);
    for (_, k) in &parts {
        ids += u32::from(k.id.is_some());
        classes += k.classes.len() as u32;
        tags += u32::from(k.tag.is_some());
    }
    let specificity = ids.min(255) << 16 | classes.min(255) << 8 | tags.min(255);
    Some(Selector { parts, specificity })
}

fn ident(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut s = String::new();
    while let Some(&c) = chars.peek().filter(|c| is_ident(**c)) {
        s.push(c);
        chars.next();
    }
    s
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

/// Parses a declaration block (the inside of `{}` or a `style` attribute).
pub fn parse_decls(block: &str, out: &mut Vec<(Decl, bool)>) {
    for d in split_top(block, |c| c == ';') {
        let Some((name, value)) = d.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let mut value = value.trim().to_ascii_lowercase();
        let mut important = false;
        if let Some(bang) = value.rfind('!')
            && value[bang + 1..].trim() == "important"
        {
            important = true;
            value.truncate(bang);
            value.truncate(value.trim_end().len());
        }
        if let Some(decls) = expand(&name, &value) {
            out.extend(decls.into_iter().map(|d| (d, important)));
        }
    }
}

/// Expands one declaration into longhands. `value` is lowercase and trimmed.
/// `None` means the declaration is invalid and must be ignored.
fn expand(name: &str, value: &str) -> Option<Vec<Decl>> {
    if value == "inherit" {
        let names = longhands(name);
        return (!names.is_empty()).then(|| names.into_iter().map(Decl::Inherit).collect());
    }
    let tokens = split_ws(value);
    let one = |d: Decl| Some(vec![d]);
    if let Some((prop, side)) = side_prop(name) {
        return one(side_decl(prop, side, value)?);
    }
    if let Some(side) = name
        .strip_prefix("border-")
        .and_then(|s| SIDES.iter().position(|x| *x == s))
    {
        let (w, s, c) = border(&tokens)?;
        return Some(border_decls(side, w, s, c).into());
    }
    match name {
        "margin" | "padding" | "border-width" | "border-style" | "border-color" => {
            let [t, r, b, l] = match tokens.as_slice() {
                [a] => [a, a, a, a],
                [a, b] => [a, b, a, b],
                [a, b, c] => [a, b, c, b],
                [a, b, c, d] => [a, b, c, d],
                _ => return None,
            };
            [t, r, b, l]
                .iter()
                .enumerate()
                .map(|(side, v)| side_decl(name, side, v))
                .collect()
        }
        "border" => {
            let (w, s, c) = border(&tokens)?;
            Some((0..4).flat_map(|i| border_decls(i, w, s, c)).collect())
        }
        "display" => one(Decl::Display(display(value)?)),
        "width" => one(Decl::Width(non_negative(value)?)),
        "height" => one(Decl::Height(non_negative(value)?)),
        "color" if value == "currentcolor" => one(Decl::Inherit("color")),
        "color" => one(Decl::Color(color(value)?)),
        "background-color" => one(Decl::Background(color(value)?)),
        "background" => {
            // Only the color part is supported; the other components reset it.
            let c = tokens.iter().find_map(|t| color(t));
            one(Decl::Background(c.unwrap_or(Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            })))
        }
        "font-family" => one(Decl::FontFamily(families(value)?)),
        "font-size" => one(Decl::FontSize(font_size(value)?)),
        "font-weight" => one(Decl::FontWeight(font_weight(value)?)),
        "font-style" => match value {
            "normal" => one(Decl::Italic(false)),
            "italic" | "oblique" => one(Decl::Italic(true)),
            _ => None,
        },
        "font" => font(&tokens),
        "line-height" => one(Decl::LineHeight(line_height(value)?)),
        "text-align" => one(Decl::TextAlign(match value {
            "left" | "start" => TextAlign::Left,
            "right" | "end" => TextAlign::Right,
            "center" => TextAlign::Center,
            "justify" => TextAlign::Justify,
            _ => return None,
        })),
        "text-indent" => match length(value)? {
            Length::Auto => None,
            l => one(Decl::TextIndent(l)),
        },
        "text-decoration" | "text-decoration-line" => {
            let mut d = Decoration::default();
            for t in &tokens {
                match *t {
                    "underline" => d.underline = true,
                    "line-through" => d.line_through = true,
                    _ => {}
                }
            }
            one(Decl::Decoration(d))
        }
        "white-space" => one(Decl::WhiteSpace(match value {
            "normal" => WhiteSpace::Normal,
            "nowrap" => WhiteSpace::NoWrap,
            "pre" => WhiteSpace::Pre,
            "pre-wrap" | "break-spaces" => WhiteSpace::PreWrap,
            "pre-line" => WhiteSpace::PreLine,
            _ => return None,
        })),
        "page-break-before" | "break-before" => one(Decl::BreakBefore(break_value(value)?)),
        "page-break-after" | "break-after" => one(Decl::BreakAfter(break_value(value)?)),
        "page-break-inside" | "break-inside" => match value {
            "auto" => one(Decl::BreakInside(Break::Auto)),
            "avoid" | "avoid-page" => one(Decl::BreakInside(Break::Avoid)),
            _ => None,
        },
        "orphans" => one(Decl::Orphans(value.parse().ok().filter(|n| *n > 0)?)),
        "widows" => one(Decl::Widows(value.parse().ok().filter(|n| *n > 0)?)),
        "list-style-type" => one(Decl::ListStyle(list_style(value)?)),
        "box-sizing" => match value {
            "content-box" => one(Decl::BorderBox(false)),
            "border-box" => one(Decl::BorderBox(true)),
            _ => None,
        },
        "list-style" => {
            let t = tokens
                .iter()
                .find_map(|t| list_style(t))
                .unwrap_or(ListStyle::Disc);
            one(Decl::ListStyle(t))
        }
        _ => None,
    }
}

/// Longhands covered by a property name, for `inherit`.
fn longhands(name: &str) -> Vec<&'static str> {
    let matches = |f: &dyn Fn(&str) -> bool| LONGHANDS.iter().copied().filter(|l| f(l)).collect();
    match name {
        "margin" | "padding" | "border" => matches(&|l| l.starts_with(name) && l != name),
        "border-width" | "border-style" | "border-color" => {
            let suffix = &name[6..];
            matches(&|l| l.starts_with("border-") && l.ends_with(suffix))
        }
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            matches(&|l| l.starts_with(name) && l.len() > name.len())
        }
        "font" => vec![
            "font-style",
            "font-weight",
            "font-size",
            "line-height",
            "font-family",
        ],
        "background" => vec!["background-color"],
        "list-style" => vec!["list-style-type"],
        "page-break-before" => vec!["break-before"],
        "page-break-after" => vec!["break-after"],
        "page-break-inside" => vec!["break-inside"],
        _ => matches(&|l| l == name),
    }
}

/// Splits `margin-top`, `border-left-color` and the like into the
/// four-sided property name and the side index.
pub fn side_prop(name: &str) -> Option<(&'static str, usize)> {
    let side = |s: &str| SIDES.iter().position(|x| *x == s);
    if let Some(s) = name.strip_prefix("margin-") {
        return Some(("margin", side(s)?));
    }
    if let Some(s) = name.strip_prefix("padding-") {
        return Some(("padding", side(s)?));
    }
    let (s, what) = name.strip_prefix("border-")?.split_once('-')?;
    let prop = match what {
        "width" => "border-width",
        "style" => "border-style",
        "color" => "border-color",
        _ => return None,
    };
    Some((prop, side(s)?))
}

fn side_decl(prop: &str, side: usize, value: &str) -> Option<Decl> {
    Some(match prop {
        "margin" => Decl::Margin(side, length(value)?),
        "padding" => match non_negative(value)? {
            Length::Auto => return None,
            l => Decl::Padding(side, l),
        },
        "border-width" => Decl::BorderWidth(side, border_width(value)?),
        "border-style" => Decl::BorderStyle(side, border_style(value)?),
        "border-color" => Decl::BorderColor(side, border_color(value)?),
        _ => return None,
    })
}

/// `border` / `border-top` components: width, drawn, color. Missing ones reset to initial.
fn border(tokens: &[&str]) -> Option<(Length, bool, Option<Color>)> {
    let (mut w, mut s, mut c) = (None, None, None);
    for t in tokens {
        if let (None, Some(v)) = (s, border_style(t)) {
            s = Some(v);
        } else if let (None, Some(v)) = (w, border_width(t)) {
            w = Some(v);
        } else if let (None, Some(v)) = (c, border_color(t)) {
            c = Some(v);
        } else {
            return None;
        }
    }
    Some((
        w.unwrap_or(Length::Pt(2.25)),
        s.unwrap_or(false),
        c.flatten(),
    ))
}

fn border_decls(side: usize, w: Length, s: bool, c: Option<Color>) -> [Decl; 3] {
    [
        Decl::BorderWidth(side, w),
        Decl::BorderStyle(side, s),
        Decl::BorderColor(side, c),
    ]
}

fn border_width(v: &str) -> Option<Length> {
    match v {
        "thin" => Some(Length::Pt(0.75)),
        "medium" => Some(Length::Pt(2.25)),
        "thick" => Some(Length::Pt(3.75)),
        _ => match non_negative(v)? {
            l @ (Length::Pt(_) | Length::Em(_) | Length::Rem(_)) => Some(l),
            _ => None,
        },
    }
}

// ponytail: every visible style draws as solid
fn border_style(v: &str) -> Option<bool> {
    match v {
        "none" | "hidden" => Some(false),
        "solid" | "dotted" | "dashed" | "double" | "groove" | "ridge" | "inset" | "outset" => {
            Some(true)
        }
        _ => None,
    }
}

fn border_color(v: &str) -> Option<Option<Color>> {
    if v == "currentcolor" {
        Some(None)
    } else {
        color(v).map(Some)
    }
}

fn display(v: &str) -> Option<Display> {
    Some(match v {
        "inline" => Display::Inline,
        // ponytail: flex and grid lay out as block
        "block" | "flow-root" | "flex" | "grid" => Display::Block,
        "inline-block" | "inline-flex" | "inline-grid" | "inline-table" => Display::InlineBlock,
        "list-item" => Display::ListItem,
        "table" => Display::Table,
        "table-row-group" => Display::TableRowGroup,
        "table-header-group" => Display::TableHeaderGroup,
        "table-footer-group" => Display::TableFooterGroup,
        "table-row" => Display::TableRow,
        "table-cell" => Display::TableCell,
        "table-caption" => Display::TableCaption,
        "none" | "table-column" | "table-column-group" => Display::None,
        _ => return None,
    })
}

fn break_value(v: &str) -> Option<Break> {
    match v {
        "auto" => Some(Break::Auto),
        "always" | "page" | "left" | "right" | "recto" | "verso" => Some(Break::Page),
        "avoid" | "avoid-page" => Some(Break::Avoid),
        "column" | "avoid-column" => Some(Break::Auto),
        _ => None,
    }
}

fn list_style(v: &str) -> Option<ListStyle> {
    Some(match v {
        "none" => ListStyle::None,
        "disc" => ListStyle::Disc,
        "circle" => ListStyle::Circle,
        "square" => ListStyle::Square,
        "decimal" | "decimal-leading-zero" => ListStyle::Decimal,
        "lower-alpha" | "lower-latin" => ListStyle::LowerAlpha,
        "upper-alpha" | "upper-latin" => ListStyle::UpperAlpha,
        "lower-roman" => ListStyle::LowerRoman,
        "upper-roman" => ListStyle::UpperRoman,
        _ => return None,
    })
}

fn font(tokens: &[&str]) -> Option<Vec<Decl>> {
    let mut italic = false;
    let mut weight = FontWeight::Abs(400);
    let mut i = 0;
    while let Some(t) = tokens.get(i) {
        match *t {
            "italic" | "oblique" => italic = true,
            "normal" | "small-caps" => {}
            t => match font_weight(t) {
                Some(w) => weight = w,
                None => break,
            },
        }
        i += 1;
    }
    // size[/line-height] family, with optional spaces around the slash.
    let rest = tokens.get(i..)?.join(" ");
    let end = rest
        .find(|c: char| c == '/' || c.is_whitespace())
        .unwrap_or(rest.len());
    let size = font_size(&rest[..end])?;
    let mut rest = rest[end..].trim_start();
    let mut lh = LineHeight::Normal;
    if let Some(r) = rest.strip_prefix('/') {
        let r = r.trim_start();
        let end = r.find(char::is_whitespace).unwrap_or(r.len());
        lh = line_height(&r[..end])?;
        rest = r[end..].trim_start();
    }
    Some(vec![
        Decl::Italic(italic),
        Decl::FontWeight(weight),
        Decl::FontSize(size),
        Decl::LineHeight(lh),
        Decl::FontFamily(families(rest)?),
    ])
}

fn families(v: &str) -> Option<Rc<[String]>> {
    let list: Vec<String> = split_top(v, |c| c == ',')
        .into_iter()
        .map(|f| {
            f.trim()
                .trim_matches(['"', '\''])
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    (!list.is_empty() && list.iter().all(|f| !f.is_empty())).then(|| list.into())
}

fn font_size(v: &str) -> Option<Length> {
    let px = |n: f32| Some(Length::Pt(n * 0.75));
    match v {
        "xx-small" => px(9.0),
        "x-small" => px(10.0),
        "small" => px(13.0),
        "medium" => px(16.0),
        "large" => px(18.0),
        "x-large" => px(24.0),
        "xx-large" => px(32.0),
        "xxx-large" => px(48.0),
        "smaller" => Some(Length::Em(1.0 / 1.2)),
        "larger" => Some(Length::Em(1.2)),
        _ => match non_negative(v)? {
            Length::Auto => None,
            Length::Percent(p) => Some(Length::Em(p / 100.0)),
            l => Some(l),
        },
    }
}

fn font_weight(v: &str) -> Option<FontWeight> {
    Some(match v {
        "normal" => FontWeight::Abs(400),
        "bold" => FontWeight::Abs(700),
        "bolder" => FontWeight::Bolder,
        "lighter" => FontWeight::Lighter,
        n => match n.parse::<u16>() {
            Ok(n @ 1..=1000) => FontWeight::Abs(n),
            _ => return None,
        },
    })
}

fn line_height(v: &str) -> Option<LineHeight> {
    if v == "normal" {
        return Some(LineHeight::Normal);
    }
    if let Some((n, "")) = number(v) {
        return (n >= 0.0).then_some(LineHeight::Factor(n));
    }
    match non_negative(v)? {
        Length::Auto => None,
        Length::Percent(p) => Some(LineHeight::Len(Length::Em(p / 100.0))),
        l => Some(LineHeight::Len(l)),
    }
}

fn non_negative(v: &str) -> Option<Length> {
    match length(v)? {
        Length::Pt(n) | Length::Em(n) | Length::Rem(n) | Length::Percent(n) if n < 0.0 => None,
        l => Some(l),
    }
}

/// Parses a length, percentage or `auto`. `v` is lowercase.
pub fn length(v: &str) -> Option<Length> {
    if v == "auto" {
        return Some(Length::Auto);
    }
    let (n, unit) = number(v)?;
    Some(match unit {
        "" if n == 0.0 => Length::Pt(0.0),
        "px" => Length::Pt(n * 0.75),
        "pt" => Length::Pt(n),
        "pc" => Length::Pt(n * 12.0),
        "in" => Length::Pt(n * 72.0),
        "cm" => Length::Pt(n * 72.0 / 2.54),
        "mm" => Length::Pt(n * 72.0 / 25.4),
        "q" => Length::Pt(n * 18.0 / 25.4),
        "em" => Length::Em(n),
        "rem" => Length::Rem(n),
        "ex" | "ch" => Length::Em(n / 2.0),
        "%" => Length::Percent(n),
        _ => return None,
    })
}

/// Leading number and the rest of the string.
fn number(v: &str) -> Option<(f32, &str)> {
    let end = v
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+'))))
        .map_or(v.len(), |(i, _)| i);
    let n: f32 = v[..end].parse().ok()?;
    n.is_finite().then_some((n, &v[end..]))
}

/// Parses a color. `v` is lowercase.
pub fn color(v: &str) -> Option<Color> {
    if let Some(hex) = v.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()?;
        let [r, g, b, a] = match digits.as_slice() {
            [r, g, b] => [r * 17, g * 17, b * 17, 255],
            [r, g, b, a] => [r * 17, g * 17, b * 17, a * 17],
            [r1, r2, g1, g2, b1, b2] => [r1 << 4 | r2, g1 << 4 | g2, b1 << 4 | b2, 255],
            [r1, r2, g1, g2, b1, b2, a1, a2] => {
                [r1 << 4 | r2, g1 << 4 | g2, b1 << 4 | b2, a1 << 4 | a2]
            }
            _ => return None,
        };
        return Some(Color { r, g, b, a });
    }
    if let Some(args) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
        let args = args.strip_suffix(')')?;
        let parts: Vec<&str> = args
            .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .collect();
        let channel = |s: &str, max: f32| -> Option<u8> {
            let (n, unit) = number(s)?;
            let n = match unit {
                "%" => n / 100.0 * 255.0,
                "" => n * 255.0 / max,
                _ => return None,
            };
            Some(n.round().clamp(0.0, 255.0) as u8)
        };
        let [r, g, b, a] = match parts.as_slice() {
            [r, g, b] => [*r, *g, *b, "1"],
            [r, g, b, a] => [*r, *g, *b, *a],
            _ => return None,
        };
        return Some(Color {
            r: channel(r, 255.0)?,
            g: channel(g, 255.0)?,
            b: channel(b, 255.0)?,
            a: channel(a, 1.0)?,
        });
    }
    // ponytail: hsl() unsupported
    if v == "transparent" {
        return Some(Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        });
    }
    NAMED_COLORS
        .iter()
        .find(|(n, _)| *n == v)
        .map(|&(_, c)| Color::rgb(c))
}

fn split_ws(v: &str) -> Vec<&str> {
    split_top(v, char::is_whitespace)
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
}

/// Splits on characters matching `sep` outside quotes and brackets.
fn split_top(s: &str, sep: impl Fn(char) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut scan = Scanner::default();
    for (i, c) in s.char_indices() {
        if scan.top(c) && sep(c) {
            out.push(&s[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&s[start..]);
    out
}

/// Index of the first top-level character matching `pred`.
fn find_top(s: &str, pred: impl Fn(char) -> bool) -> Option<usize> {
    let mut scan = Scanner::default();
    s.char_indices()
        .find(|&(_, c)| scan.top(c) && pred(c))
        .map(|(i, _)| i)
}

/// Index of the `}` closing the `{` at `open`, or the end of `s`.
fn block_end(s: &str, open: usize) -> usize {
    let mut scan = Scanner::default();
    let mut depth = 0usize;
    for (i, c) in s.char_indices().skip_while(|&(i, _)| i < open) {
        if !scan.top(c) {
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    s.len()
}

/// Tracks strings, escapes and `()`/`[]` nesting while scanning CSS.
#[derive(Default)]
struct Scanner {
    quote: Option<char>,
    escaped: bool,
    parens: usize,
}

impl Scanner {
    /// Feeds `c`; true when `c` is outside strings and parentheses.
    fn top(&mut self, c: char) -> bool {
        if self.escaped {
            self.escaped = false;
            return false;
        }
        match (self.quote, c) {
            (_, '\\') => {
                self.escaped = true;
                false
            }
            (Some(q), c) => {
                if c == q {
                    self.quote = None;
                }
                false
            }
            (None, '"' | '\'') => {
                self.quote = Some(c);
                false
            }
            (None, '(' | '[') => {
                self.parens += 1;
                false
            }
            (None, ')' | ']') => {
                self.parens = self.parens.saturating_sub(1);
                false
            }
            (None, _) => self.parens == 0,
        }
    }
}

const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn decls(block: &str) -> Vec<Decl> {
        let mut out = Vec::new();
        parse_decls(block, &mut out);
        out.into_iter().map(|(d, _)| d).collect()
    }

    fn sheet(src: &str) -> Sheet {
        let mut s = Sheet::default();
        parse_sheet(src, &mut s);
        s
    }

    #[test]
    fn selectors() {
        let s = parse_selector("div.a.b > p #x").unwrap();
        assert_eq!(s.specificity, 1 << 16 | 2 << 8 | 2);
        assert_eq!(s.parts.len(), 3);
        assert_eq!(s.parts[1].0, Combinator::Child);
        assert_eq!(s.parts[2].0, Combinator::Descendant);
        assert_eq!(s.parts[0].1.classes, ["a", "b"]);
        assert_eq!(parse_selector("*").unwrap().specificity, 0);
        for bad in [
            "", ">", "a >", "a > > b", "a:hover", "a[href]", "a + b", ".", "p.a p",
        ] {
            assert_eq!(parse_selector(bad).is_some(), bad == "p.a p", "{bad:?}");
        }
    }

    #[test]
    fn rules_and_recovery() {
        let s = sheet(
            "/* c */ p, h1 { color: red; bogus: 1; margin: 1px 2px 3px } \
             a:hover, p { color: blue } \
             @import url(x.css); @font-face { src: url(a;b) } \
             @media screen { p { color: green } } @media print, x { i { color: red } } \
             @media (min-width: 1px) { b {} } \
             div { width: 10px ; } q { color: red",
        );
        let tags: Vec<_> = s
            .rules
            .iter()
            .map(|r| r.selectors[0].parts[0].1.tag.clone().unwrap_or_default())
            .collect();
        assert_eq!(tags, ["p", "i", "div", "q"]);
        assert_eq!(s.rules[0].decls.len(), 5);
        assert_eq!(s.rules[0].decls[3].0, Decl::Margin(2, Length::Pt(2.25)));
        assert_eq!(s.rules[0].decls[4].0, Decl::Margin(3, Length::Pt(1.5)));
    }

    #[test]
    fn shorthands() {
        assert_eq!(
            decls("border: 2pt dashed #f00"),
            (0..4)
                .flat_map(|i| {
                    border_decls(
                        i,
                        Length::Pt(2.0),
                        true,
                        Some(Color {
                            r: 255,
                            g: 0,
                            b: 0,
                            a: 255,
                        }),
                    )
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(decls("border-left: none")[1], Decl::BorderStyle(3, false));
        assert_eq!(decls("border: 1px solid red blue"), []);
        assert_eq!(
            decls("font: italic bold 12px / 1.5 'Times New Roman', serif"),
            [
                Decl::Italic(true),
                Decl::FontWeight(FontWeight::Abs(700)),
                Decl::FontSize(Length::Pt(9.0)),
                Decl::LineHeight(LineHeight::Factor(1.5)),
                Decl::FontFamily(Rc::from([
                    "times new roman".to_string(),
                    "serif".to_string()
                ])),
            ]
        );
        assert_eq!(
            decls("font: 10pt/12pt Arial")[3],
            Decl::LineHeight(LineHeight::Len(Length::Pt(12.0)))
        );
        assert_eq!(decls("font: bold"), []);
        // Negative padding and width are invalid, negative margins are fine.
        assert_eq!(
            decls("padding: -1px; width: -5%; margin: -1px"),
            (0..4)
                .map(|i| Decl::Margin(i, Length::Pt(-0.75)))
                .collect::<Vec<_>>()
        );
        assert_eq!(decls("margin: inherit").len(), 4);
        assert_eq!(
            decls("color: red !important; x: y"),
            [Decl::Color(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            })]
        );
        let mut out = Vec::new();
        parse_decls("color: red ! IMPORTANT", &mut out);
        assert!(out[0].1);
    }

    #[test]
    fn values() {
        assert_eq!(length("1in"), Some(Length::Pt(72.0)));
        assert_eq!(length("25.4mm"), Some(Length::Pt(72.0)));
        assert_eq!(length("-.5em"), Some(Length::Em(-0.5)));
        assert_eq!(length("12"), None);
        assert_eq!(length("1e3px"), None);
        let c = |r, g, b, a| Some(Color { r, g, b, a });
        assert_eq!(color("#abc"), c(0xaa, 0xbb, 0xcc, 255));
        assert_eq!(color("#11223380"), c(0x11, 0x22, 0x33, 0x80));
        assert_eq!(color("rgb(255, 0, 50%)"), c(255, 0, 128, 255));
        assert_eq!(color("rgba(0 0 0 / 0.5)"), c(0, 0, 0, 128));
        assert_eq!(color("rebeccapurple"), c(0x66, 0x33, 0x99, 255));
        assert_eq!(color("#ab"), None);
        assert_eq!(color("#ффф"), None);
        assert_eq!(page_size("letter landscape"), Some(Some((792.0, 612.0))));
        assert_eq!(
            page_size("100mm 50mm"),
            Some(Some((100.0 * 72.0 / 25.4, 50.0 * 72.0 / 25.4)))
        );
        assert_eq!(page_size("a4 a5"), Some(Some((419.53, 595.28))));
    }

    #[test]
    fn never_panics() {
        let src = "@media print { p > .a, #b { margin: 1px 2em; font: bold 12px/1.2 \"Я\", serif } } \
                   @page { size: A4; @top-center { content: 'x' } margin: 1cm } \
                   td { border: 1px solid rgb(1,2,3); color: #fff !important; background: url('a;b') }";
        for end in (0..=src.len()).filter(|&e| src.is_char_boundary(e)) {
            sheet(&src[..end]);
        }
        for junk in [
            "{", "}", "@", "@media", "@media {", "a{b:", "a{:}", "\\", "'", "(", "/*",
        ] {
            sheet(junk);
            decls(junk);
        }
        let deep = "@media print {".repeat(10_000);
        sheet(&deep);
    }
}
