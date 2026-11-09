//! LaTeX formulas in chat messages.
//!
//! Messages are ordinary Markdown, and formulas arrive as `$...$`, `$$...$$`, `\(...\)` or
//! `\[...\]`. [`prepare`] brings them all to the `$` forms the Markdown parser understands and
//! escapes every other `$` (prices, shell variables). [`show`] renders the message, with RaTeX
//! typesetting each formula into a texture that is kept between frames.

use eframe::egui::{
    self, emath::GuiRounding, pos2, vec2, Color32, ColorImage, Id, Pos2, Rect, Response, RichText, Sense, TextStyle,
    TextureHandle, TextureId, TextureOptions, Ui, Vec2,
};
use egui_commonmark::{math_line_height, CommonMarkCache, CommonMarkViewer, MathExtent};
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
use ratex_layout::{layout_with_fallback_metrics, to_display_list, FallbackGlyphMetrics};
use ratex_render::{render_to_png, RenderOptions};
use ratex_types::{color::Color, math_style::MathStyle};
use std::{
    borrow::Cow,
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    ops::Range,
    sync::{Arc, LazyLock, Mutex, Once},
};

/// Computer Modern with Cyrillic: the KaTeX fonts have none, and this one matches their look.
const FALLBACK_FONT: &[u8] = include_bytes!("../assets/cmunrm.ttf");

/// Formula size relative to the surrounding text. Computer Modern has a small x-height, so this
/// is the same scale KaTeX uses on the web.
const MATH_SCALE: f32 = 1.21;
/// Transparent margin of the image: glyphs may overshoot the box TeX computes for them.
const PADDING_EM: f32 = 0.3;
/// Space above and below a formula on its own row.
const BLOCK_MARGIN: f32 = 6.0;
/// An inline formula taller than this many text rows moves to a row of its own.
const MAX_INLINE_ROWS: f32 = 2.0;
const MAX_TEX_BYTES: usize = 8 * 1024;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_PREPARED: usize = 512;

/// Show a chat message as Markdown with its formulas typeset.
pub fn show(ui: &mut Ui, cache: &mut CommonMarkCache, text: &str) {
    let prepared = has_math_markers(text).then(|| prepared(ui.ctx(), text));
    CommonMarkViewer::new()
        .render_math_fn(Some(&paint))
        .render_math_extent_fn(Some(&extent))
        .show(ui, cache, prepared.as_deref().unwrap_or(text));
}

// ---------------------------------------------------------------------------------------------
// Finding formulas
// ---------------------------------------------------------------------------------------------

fn prepared(ctx: &egui::Context, text: &str) -> Arc<str> {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    let hash = hasher.finish();
    let shared = store(ctx);
    let mut store = shared.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(done) = store.prepared.get(&hash) {
        return done.clone();
    }
    if store.prepared.len() >= MAX_PREPARED {
        store.prepared.clear();
    }
    let done: Arc<str> = prepare(text).into_owned().into();
    store.prepared.insert(hash, done.clone());
    done
}

fn has_math_markers(text: &str) -> bool {
    text.contains('$') || text.contains("\\(") || text.contains("\\[") || text.contains("\\begin{")
}

/// Environments Claude writes without any `$`: they are formulas when they start a line.
const BARE_ENVIRONMENTS: [&str; 22] = [
    "equation", "equation*", "align", "align*", "aligned", "gather", "gather*", "gathered", "multline",
    "multline*", "eqnarray", "eqnarray*", "split", "cases", "matrix", "pmatrix", "bmatrix", "Bmatrix",
    "vmatrix", "Vmatrix", "array", "alignat",
];

fn bare_environment(text: &str, start: usize, limit: usize) -> Option<(usize, String)> {
    let line_start = text[..start].rfind('\n').map_or(0, |n| n + 1);
    if !text[line_start..start].trim().is_empty() {
        return None;
    }
    let rest = &text[start + "\\begin{".len()..limit];
    let name = &rest[..rest.find('}')?];
    if !BARE_ENVIRONMENTS.contains(&name) {
        return None;
    }
    let end_tag = format!("\\end{{{name}}}");
    let end = start + text[start..limit].find(&end_tag)? + end_tag.len();
    let body_start = start + "\\begin{".len() + name.len() + 1;
    let body = &text[body_start..end - end_tag.len()];
    // Numbered display environments become their unnumbered inline forms.
    let wrapped = match name.trim_end_matches('*') {
        "equation" => body.to_owned(),
        "align" | "eqnarray" | "split" => format!("\\begin{{aligned}}{}\\end{{aligned}}", body.replace("&=&", "&=")),
        "gather" | "multline" => format!("\\begin{{gathered}}{body}\\end{{gathered}}"),
        _ => text[start..end].to_owned(),
    };
    let mut content = wrapped.replace("\\nonumber", "").replace("\\notag", "");
    while let Some(at) = content.find("\\label{") {
        let close = content[at..].find('}').map_or(content.len(), |c| at + c + 1);
        content.replace_range(at..close, "");
    }
    let content = one_line(&content, is_quoted(text, start));
    if content.is_empty() || !braces_balanced(&content) {
        return None;
    }
    Some((end, format!("$${}$$", for_row(text, start, &content))))
}

/// Rewrite Markdown so that every formula is `$...$` or `$$...$$` on one line and every other `$`
/// is escaped. Code and link targets stay untouched.
pub fn prepare(text: &str) -> Cow<'_, str> {
    if !has_math_markers(text) {
        return Cow::Borrowed(text);
    }
    let protected = protected_ranges(text);
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 16);
    let mut copied = 0;
    let mut i = 0;
    let mut next = 0;
    while i < bytes.len() {
        while protected.get(next).is_some_and(|range| range.end <= i) {
            next += 1;
        }
        let limit = match protected.get(next) {
            Some(range) if range.start <= i => {
                i = range.end;
                continue;
            }
            Some(range) => range.start,
            None => bytes.len(),
        };
        let found = match bytes[i] {
            b'\\' => match bytes.get(i + 1) {
                Some(b'(') => paren_formula(text, i, limit),
                Some(b'[') => bracket_formula(text, i, limit),
                Some(b'b') if text[i..limit].starts_with("\\begin{") => match bare_environment(text, i, limit) {
                    Some(found) => Some(found),
                    None => {
                        i += 2;
                        continue;
                    }
                },
                Some(c) if c.is_ascii() => {
                    i += 2;
                    continue;
                }
                _ => {
                    i += 1;
                    continue;
                }
            },
            b'$' if bytes.get(i + 1) == Some(&b'$') => {
                Some(display_formula(text, i, limit).unwrap_or((i + 2, r"\$\$".to_owned())))
            }
            b'$' => Some(inline_formula(text, i, limit).unwrap_or((i + 1, r"\$".to_owned()))),
            _ => {
                i += 1;
                continue;
            }
        };
        match found {
            Some((end, replacement)) => {
                out.push_str(&text[copied..i]);
                out.push_str(&replacement);
                copied = end;
                i = end;
            }
            None => i += 2,
        }
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

/// Byte ranges where a `$` or `\(` means nothing: code, raw HTML and link targets.
fn protected_ranges(text: &str) -> Vec<Range<usize>> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST;
    let mut ranges = Vec::new();
    for (event, span) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::Image { .. })
            | Event::Code(_)
            | Event::Html(_)
            | Event::InlineHtml(_) => ranges.push(span),
            Event::Start(Tag::Link { link_type, .. }) => match link_type {
                LinkType::Autolink | LinkType::Email => ranges.push(span),
                _ => {
                    if let Some(at) = text[span.clone()].rfind("](") {
                        ranges.push(span.start + at..span.end);
                    }
                }
            },
            _ => {}
        }
    }
    ranges.sort_by_key(|range| range.start);
    ranges
}

/// `\(...\)` on one line. Markdown also uses `\(` for a literal bracket, so plain words stay text.
fn paren_formula(text: &str, start: usize, limit: usize) -> Option<(usize, String)> {
    let close = find_escaped(text, start + 2, limit, b')', false)?;
    let content = text[start + 2..close].trim();
    let plain_word = content.chars().all(char::is_alphabetic) && content.chars().count() > 2;
    let plain = !content.contains('\\');
    if content.is_empty() || plain_word || plain && (is_prose(content) || has_cyrillic(content)) {
        return None;
    }
    if !braces_balanced(content) {
        return None;
    }
    Some((close + 2, format!("${}$", for_row(text, start, content))))
}

/// `\[...\]`, possibly over several lines. `\[note\]` and `\[1\]` are literal brackets in Markdown,
/// so the content has to show some sign of being a formula.
fn bracket_formula(text: &str, start: usize, limit: usize) -> Option<(usize, String)> {
    let close = find_escaped(text, start + 2, limit, b']', true)?;
    let content = &text[start + 2..close];
    if !content.contains(['\\', '^', '_', '=', '+', '{', '<', '>', '\n']) {
        return None;
    }
    let content = one_line(content, is_quoted(text, start));
    if content.is_empty() || !braces_balanced(&content) {
        return None;
    }
    Some((close + 2, format!("$${}$$", for_row(text, start, &content))))
}

fn display_formula(text: &str, start: usize, limit: usize) -> Option<(usize, String)> {
    let bytes = text.as_bytes();
    let mut i = start + 2;
    while i < limit {
        match bytes[i] {
            b'\\' => i += 2,
            b'$' if bytes.get(i + 1) == Some(&b'$') => {
                let content = one_line(&text[start + 2..i], is_quoted(text, start));
                if content.is_empty() {
                    return None;
                }
                // The Markdown parser pairs `$$` by brace depth, so broken braces would tear this
                // and the next formula apart. TeX rejects them anyway: show the source as code.
                if !braces_balanced(&content) {
                    return (!content.contains('`')).then(|| (i + 2, format!("`$${content}$$`")));
                }
                return Some((i + 2, format!("$${}$$", for_row(text, start, &content))));
            }
            b'$' => return None,
            b'\n' if blank_line_follows(bytes, i) => return None,
            _ => i += 1,
        }
    }
    None
}

fn inline_formula(text: &str, start: usize, limit: usize) -> Option<(usize, String)> {
    let bytes = text.as_bytes();
    if text[..start]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '$')
    {
        return None;
    }
    let mut i = start + 1;
    while i < limit {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' => return None,
            b'$' => {
                let after = text[i + 1..].chars().next();
                let content = &text[start + 1..i];
                if after.is_some_and(char::is_alphanumeric) || !plausible_inline(content) {
                    return None;
                }
                return Some((i + 1, format!("${}$", for_row(text, start, content))));
            }
            _ => i += 1,
        }
    }
    None
}

/// Position of the next `\<close>`, skipping other escapes and staying within one paragraph.
fn find_escaped(
    text: &str,
    mut i: usize,
    limit: usize,
    close: u8,
    multiline: bool,
) -> Option<usize> {
    let bytes = text.as_bytes();
    while i < limit {
        match bytes[i] {
            b'\\' if bytes.get(i + 1) == Some(&close) => return Some(i),
            b'\\' => i += 2,
            b'\n' if !multiline || blank_line_follows(bytes, i) => return None,
            _ => i += 1,
        }
    }
    None
}

fn blank_line_follows(bytes: &[u8], newline: usize) -> bool {
    let mut i = newline + 1;
    while let Some(b' ' | b'\t' | b'\r') = bytes.get(i) {
        i += 1;
    }
    matches!(bytes.get(i), Some(b'\n') | None)
}

/// Is this text between two `$` a formula, and not two prices or two shell variables?
fn plausible_inline(content: &str) -> bool {
    if content.is_empty() || content.starts_with(char::is_whitespace) {
        return false;
    }
    if content.ends_with(char::is_whitespace) {
        return false;
    }
    let Some((0, brackets)) = nesting(content) else {
        return false;
    };
    // A backslash command is TeX beyond doubt. Everything below guards plain text.
    if content.contains('\\') {
        return true;
    }
    if brackets != 0 || has_cyrillic(content) {
        return false;
    }
    !is_prose(content)
}

fn braces_balanced(content: &str) -> bool {
    nesting(content).is_some_and(|(braces, _)| braces == 0)
}

/// Depth left open at the end by `{}` and by brackets. `None` if a `}` closes nothing or there is
/// a bare `$`, which the Markdown parser would take for a delimiter.
fn nesting(content: &str) -> Option<(i32, i32)> {
    let (mut braces, mut brackets) = (0i32, 0i32);
    let mut chars = content.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '$' => return None,
            '{' => braces += 1,
            '}' => braces -= 1,
            '(' | '[' => brackets += 1,
            ')' | ']' => brackets -= 1,
            _ => {}
        }
        if braces < 0 {
            return None;
        }
    }
    Some((braces, brackets))
}

/// Several words in a row, or a number followed by a word: "5 and 10", "or more than 6".
fn is_prose(content: &str) -> bool {
    if content.contains(['=', '+', '<', '>', '^', '_']) || content.contains(" - ") {
        return false;
    }
    let words = content
        .split_whitespace()
        .filter(|word| word.chars().count() >= 2 && word.chars().all(char::is_alphabetic))
        .count();
    words >= 2 || words == 1 && content.starts_with(|c: char| c.is_ascii_digit())
}

fn has_cyrillic(content: &str) -> bool {
    content.chars().any(|c| matches!(c, '\u{400}'..='\u{52F}'))
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn is_quoted(text: &str, at: usize) -> bool {
    text[line_start(text, at)..at].trim_start().starts_with('>')
}

/// A `|` would end the cell inside a table row, so a formula there spells it `\vert`.
fn for_row<'a>(text: &str, at: usize, content: &'a str) -> Cow<'a, str> {
    if !text[line_start(text, at)..at].trim_start().starts_with('|') || !content.contains('|') {
        return Cow::Borrowed(content);
    }
    let mut out = String::with_capacity(content.len() + 12);
    let mut chars = content.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '|' => out.push_str(r"\vert{}"),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// TeX treats a line break as a space, but in Markdown a line starting with `-` or `+` would begin
/// a list and tear the formula apart.
fn one_line(content: &str, quoted: bool) -> String {
    let mut lines = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let mut line = line.trim();
        if quoted && index > 0 {
            line = line.trim_start_matches(['>', ' ']);
        }
        let line = strip_comment(line).trim();
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines.join(" ")
}

fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'%' => return &line[..i],
            _ => i += 1,
        }
    }
    line
}

// ---------------------------------------------------------------------------------------------
// Typesetting
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Typeset {
    texture: TextureId,
    /// Image size, padding included.
    size: Vec2,
    padding: f32,
    width: f32,
    above: f32,
    below: f32,
    /// Rendered in white, and takes the text colour when painted.
    tinted: bool,
}

struct Entry {
    result: Result<Typeset, String>,
    _texture: Option<TextureHandle>,
    bytes: usize,
    used: u64,
}

#[derive(PartialEq, Eq, Hash)]
struct Key {
    tex: String,
    display: bool,
    em: u32,
    pixels: u32,
    color: Option<[u8; 4]>,
}

#[derive(Default)]
struct Store {
    formulas: HashMap<Key, Entry>,
    prepared: HashMap<u64, Arc<str>>,
    bytes: usize,
    clock: u64,
}

impl Store {
    fn evict(&mut self) {
        if self.bytes <= MAX_CACHE_BYTES {
            return;
        }
        let mut order: Vec<(u64, usize)> = self
            .formulas
            .values()
            .map(|entry| (entry.used, entry.bytes))
            .collect();
        order.sort_unstable();
        let mut freed = 0;
        let mut cutoff = 0;
        for (used, bytes) in order {
            if self.bytes - freed <= MAX_CACHE_BYTES / 4 * 3 {
                break;
            }
            freed += bytes;
            cutoff = used;
        }
        self.formulas.retain(|_, entry| entry.used > cutoff);
        self.bytes = self.formulas.values().map(|entry| entry.bytes).sum();
    }
}

fn store(ctx: &egui::Context) -> Arc<Mutex<Store>> {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Store>>>(Id::new("relay-math"))
            .clone()
    })
}

fn lookup(ui: &Ui, tex: &str, display: bool) -> Result<Typeset, String> {
    let ctx = ui.ctx();
    let em = TextStyle::Body.resolve(ui.style()).size * MATH_SCALE;
    let pixels = ctx.pixels_per_point();
    let text_color = ui.visuals().text_color();
    // A formula with its own colours has to be drawn in the real text colour. All others are
    // drawn once in white, so that changing the theme does not typeset them again.
    let colored = tex.contains("color");
    let key = Key {
        tex: tex.to_owned(),
        display,
        em: em.to_bits(),
        pixels: pixels.to_bits(),
        color: colored.then(|| text_color.to_array()),
    };
    let shared = store(ctx);
    let mut store = shared.lock().unwrap_or_else(|e| e.into_inner());
    store.clock += 1;
    let clock = store.clock;
    if let Some(entry) = store.formulas.get_mut(&key) {
        entry.used = clock;
        return entry.result.clone();
    }
    let base = if colored { text_color } else { Color32::WHITE };
    let rendered = render(ctx, tex, display, em, pixels, base);
    let entry = match rendered {
        Ok((texture, mut typeset, bytes)) => {
            typeset.tinted = !colored;
            Entry {
                result: Ok(typeset),
                _texture: Some(texture),
                bytes,
                used: clock,
            }
        }
        Err(message) => Entry {
            result: Err(message),
            _texture: None,
            bytes: 0,
            used: clock,
        },
    };
    let result = entry.result.clone();
    store.bytes += entry.bytes;
    store.formulas.insert(key, entry);
    store.evict();
    result
}

fn render(
    ctx: &egui::Context,
    tex: &str,
    display: bool,
    em: f32,
    pixels: f32,
    color: Color32,
) -> Result<(TextureHandle, Typeset, usize), String> {
    if tex.len() > MAX_TEX_BYTES {
        return Err("формула слишком длинная".into());
    }
    let (list, padding) = typeset(tex, display, em, color)?;
    let png = render_to_png(
        &list,
        &RenderOptions {
            font_size: em,
            padding,
            background_color: Color {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.,
            },
            font_dir: String::new(),
            device_pixel_ratio: pixels,
        },
    )?;
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let (width, height) = (image.width() as usize, image.height() as usize);
    if width.max(height) > ctx.input(|input| input.max_texture_side) {
        return Err("формула слишком большая".into());
    }
    let texture = ctx.load_texture(
        "math",
        ColorImage::from_rgba_unmultiplied([width, height], image.as_raw()),
        TextureOptions::LINEAR,
    );
    let typeset = Typeset {
        texture: texture.id(),
        size: vec2(width as f32, height as f32) / pixels,
        padding,
        width: list.width as f32 * em,
        above: list.height as f32 * em,
        below: list.depth as f32 * em,
        tinted: false,
    };
    Ok((texture, typeset, width * height * 4))
}

fn typeset(
    tex: &str,
    display: bool,
    em: f32,
    color: Color32,
) -> Result<(ratex_types::DisplayList, f32), String> {
    static FONT: Once = Once::new();
    FONT.call_once(|| {
        ratex_unicode_font::set_unicode_font(FALLBACK_FONT.to_vec());
    });
    let nodes = ratex_parser::parse(tex).map_err(|error| error.message.clone())?;
    let [r, g, b, a] = color.to_normalized_gamma_f32();
    let options = ratex_layout::LayoutOptions {
        style: if display {
            MathStyle::Display
        } else {
            MathStyle::Text
        },
        color: Color { r, g, b, a },
        ..Default::default()
    };
    let boxed = layout_with_fallback_metrics(&nodes, &options, FALLBACK_METRICS.clone());
    Ok((to_display_list(&boxed), (em * PADDING_EM).ceil()))
}

/// Real sizes of the Cyrillic letters. RaTeX would otherwise lay them out with the widths of
/// similar Latin letters, and the real glyphs would collide.
static FALLBACK_METRICS: LazyLock<Arc<HashMap<char, FallbackGlyphMetrics>>> =
    LazyLock::new(|| Arc::new(measure_fallback_font()));

fn measure_fallback_font() -> HashMap<char, FallbackGlyphMetrics> {
    use ab_glyph::Font;
    let Ok(font) = ab_glyph::FontRef::try_from_slice(FALLBACK_FONT) else {
        return HashMap::new();
    };
    let units = f64::from(font.units_per_em().unwrap_or(1000.));
    ('\u{400}'..='\u{52F}')
        .chain(['№'])
        .filter_map(|ch| {
            let id = font.glyph_id(ch);
            if id.0 == 0 {
                return None;
            }
            let bounds = font.outline(id)?.bounds;
            let top = f64::from(bounds.min.y.max(bounds.max.y));
            let bottom = f64::from(bounds.min.y.min(bounds.max.y));
            Some((
                ch,
                FallbackGlyphMetrics {
                    width: f64::from(font.h_advance_unscaled(id)) / units,
                    height: top.max(0.) / units,
                    depth: (-bottom).max(0.) / units,
                },
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Placing formulas in the text
// ---------------------------------------------------------------------------------------------

/// Ascent and height of a row of body text.
fn text_row(ui: &Ui) -> (f32, f32) {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap("x".into(), font, Color32::WHITE));
    let ascent = galley.rows[0]
        .glyphs
        .first()
        .map_or(galley.size().y * 0.8, |glyph| glyph.pos.y);
    (ascent, galley.size().y)
}

fn fits_inline(ui: &Ui, formula: &Typeset) -> bool {
    let (_, row) = text_row(ui);
    formula.above + formula.below <= MAX_INLINE_ROWS * row && formula.width <= ui.max_rect().width()
}

fn extent(ui: &Ui, tex: &str) -> MathExtent {
    match lookup(ui, tex, false) {
        Ok(formula) => MathExtent {
            width: formula.width,
            above: formula.above,
            below: formula.below,
            inline: fits_inline(ui, &formula),
        },
        Err(_) => MathExtent {
            width: source_width(ui, tex),
            ..Default::default()
        },
    }
}

fn paint(ui: &mut Ui, tex: &str, inline: bool) {
    if tex.trim().is_empty() {
        return;
    }
    match lookup(ui, tex, !inline) {
        Err(message) => paint_source(ui, tex, inline, &message),
        Ok(formula) if inline && fits_inline(ui, &formula) => paint_inline(ui, tex, &formula),
        Ok(formula) => paint_block(ui, tex, &formula),
    }
}

/// Stand in the text row without changing its height, with the baseline on the baseline of the
/// text. Rows of a paragraph with tall formulas were already spaced to make room.
fn paint_inline(ui: &mut Ui, tex: &str, formula: &Typeset) {
    let (ascent, row) = text_row(ui);
    let line = math_line_height(ui).unwrap_or(row);
    let height = ui.cursor().height().max(line);
    let (rect, response) = ui.allocate_exact_size(vec2(formula.width, height), Sense::click());
    // Where a label in this row puts its text: see `valign` in epaint's text layout.
    let valign = ui.text_valign().to_factor();
    let baseline = rect.top() + ascent + valign * (rect.height() - line);
    paint_at(ui, formula, pos2(rect.left(), baseline));
    copy_menu(ui, response, tex);
}

fn paint_block(ui: &mut Ui, tex: &str, formula: &Typeset) {
    end_row(ui);
    let available = ui.available_size_before_wrap().x.max(1.);
    let height = formula.above + formula.below + 2. * BLOCK_MARGIN;
    if formula.width <= available {
        let (rect, response) = ui.allocate_exact_size(vec2(available, height), Sense::click());
        let left = rect.center().x - formula.width / 2.;
        let baseline = rect.top() + BLOCK_MARGIN + formula.above;
        paint_at(ui, formula, pos2(left, baseline));
        copy_menu(ui, response, tex);
    } else {
        // Too wide even for a row of its own: scroll sideways, like a wide table.
        egui::ScrollArea::horizontal()
            .id_salt(("math-scroll", tex))
            .auto_shrink([false, true])
            .max_width(available)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                let size = vec2(formula.width + 2. * formula.padding, height);
                let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                let left = rect.left() + formula.padding;
                let baseline = rect.top() + BLOCK_MARGIN + formula.above;
                paint_at(ui, formula, pos2(left, baseline));
                copy_menu(ui, response, tex);
            });
    }
    finish_row(ui);
}

fn paint_at(ui: &Ui, formula: &Typeset, baseline_start: Pos2) {
    let corner = baseline_start - vec2(formula.padding, formula.above + formula.padding);
    let corner = corner.round_to_pixels(ui.painter().pixels_per_point());
    let rect = Rect::from_min_size(corner, formula.size);
    let tint = if formula.tinted {
        ui.visuals().text_color()
    } else {
        Color32::WHITE
    };
    let uv = Rect::from_min_max(pos2(0., 0.), pos2(1., 1.));
    ui.painter().image(formula.texture, rect, uv, tint);
}

/// A formula that does not parse stays readable: its source, the way it was written.
fn paint_source(ui: &mut Ui, tex: &str, inline: bool, message: &str) {
    if !inline {
        end_row(ui);
    }
    ui.add(egui::Label::new(source_text(tex, inline)).wrap())
        .on_hover_text(format!("Не удалось отрисовать формулу: {message}"));
    if !inline {
        finish_row(ui);
    }
}

fn source_text(tex: &str, inline: bool) -> RichText {
    let fence = if inline { "$" } else { "$$" };
    RichText::new(format!("{fence}{tex}{fence}")).code()
}

fn source_width(ui: &Ui, tex: &str) -> f32 {
    let galley = egui::WidgetText::from(source_text(tex, true)).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Body,
    );
    galley.size().x
}

fn copy_menu(ui: &Ui, response: Response, tex: &str) {
    if ui.is_visible() {
        response.context_menu(|ui| {
            if ui.button("Скопировать LaTeX").clicked() {
                ui.ctx().copy_text(tex.to_owned());
                ui.close_menu();
            }
        });
    }
}

fn end_row(ui: &mut Ui) {
    if ui.cursor().left() > ui.max_rect().left() + 0.5 {
        ui.end_row();
    }
}

fn finish_row(ui: &mut Ui) {
    ui.end_row();
    ui.set_row_height(ui.text_style_height(&TextStyle::Body));
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{
        epaint::{Shape, TextShape},
        FontData, FontDefinitions, FontFamily,
    };

    fn same(input: &str) {
        assert_eq!(prepare(input), input, "{input}");
    }

    #[test]
    fn delimiters_are_brought_to_dollars() {
        same("Пусть $x^2$ и $\\frac{a}{b}$, а также ($y$).");
        same("$$x^2$$");
        assert_eq!(prepare(r"где \(x^2\) и \( y \)"), "где $x^2$ и $y$");
        assert_eq!(prepare(r"\[x^2 + 1\]"), "$$x^2 + 1$$");
        assert_eq!(prepare("\\[\n a = b \\\\\n c = d\n\\]"), r"$$a = b \\ c = d$$");
        assert_eq!(prepare("$$\n a\n - b\n + c\n$$"), "$$a - b + c$$");
        assert_eq!(prepare("$$a % comment\n+ b$$"), "$$a + b$$");
        assert_eq!(prepare("> $$\n> a\n> $$"), "> $$a$$");
        assert_eq!(prepare(r"\(x\)"), "$x$");
    }

    #[test]
    fn ordinary_formulas_with_spaces_are_formulas() {
        for input in [
            "Пусть $f(x) = x^x$ при $x > 0$.",
            "$a + b = c$",
            "$E = mc^2$ и $y = kx + b$",
            "$x and y$",
            "$\\int_0^1 x e^x \\, dx = 1$",
            "$\\text{при } x > 0$",
            "$ABC$ и $AB = CD$",
            "$sin x + cos x$",
            "$[0, 1)$",
        ] {
            same(input);
        }
        assert_eq!(
            prepare(r"\(\int_0^1 x e^x \, dx = 1\)"),
            r"$\int_0^1 x e^x \, dx = 1$"
        );
        assert_eq!(prepare(r"\(f(x) = x^x\) и \(a + b\)"), "$f(x) = x^x$ и $a + b$");
    }

    #[test]
    fn code_and_links_stay_untouched() {
        same("Команда `echo $x$` и `\\(y\\)`.");
        same("```\n$x$ \\(y\\) $5\n```");
        same("Текст\n\n    $x$ и \\(y\\)\n");
        same("[файл](https://example.com/$a$b/c)");
        same("<https://example.com/$a$b>");
        assert_eq!(prepare("`$a` и $b$"), "`$a` и $b$");
    }

    #[test]
    fn other_dollars_are_escaped() {
        for (input, expected) in [
            ("Цена $5 или $10", r"Цена \$5 или \$10"),
            ("$5/$10", r"\$5/\$10"),
            ("$HOME/$USER", r"\$HOME/\$USER"),
            ("PATH=$HOME:$PATH", r"PATH=\$HOME:\$PATH"),
            ("\"$HOME\" и \"$USER\"", "\"\\$HOME\" и \"\\$USER\""),
            ("цена $5, а потом 10$", r"цена \$5, а потом 10\$"),
            ("US$5 и a$b$c", r"US\$5 и a\$b\$c"),
            ("$5 and 10$", r"\$5 and 10\$"),
            ("$5 (or 6$)", r"\$5 (or 6\$)"),
            ("$ x $", r"\$ x \$"),
            ("pid $$ и дальше", r"pid \$\$ и дальше"),
            ("$$a\n\nb$$", "\\$\\$a\n\nb\\$\\$"),
            ("одна $ строка\nи $ другая", "одна \\$ строка\nи \\$ другая"),
        ] {
            assert_eq!(prepare(input), expected, "{input}");
        }
        same(r"Уже \$5 и \$6");
    }

    #[test]
    fn broken_braces_in_a_display_formula_show_the_source() {
        assert_eq!(prepare("$$a}$$ и $$b$$"), "`$$a}$$` и $$b$$");
        assert_eq!(prepare("$$\\frac{a}{$$"), "`$$\\frac{a}{$$`");
        for literal in [
            r"\[a}^2\] и $$b$$",
            r"\(a}\) и $$b$$",
            r"\(a$\) и $$b$$",
        ] {
            let (math, _) = parsed(literal);
            assert_eq!(math, ["b"], "{literal}");
        }
        let (math, plain) = parsed("$$a}$$ и $$b$$");
        assert_eq!(math, ["b"]);
        assert_eq!(plain, "$$a}$$ и ");
    }

    #[test]
    fn literal_brackets_stay_text() {
        for input in [
            r"\[ERROR\] всё плохо",
            r"\[1\] Кнут",
            r"\[a-z\]",
            r"\(см. выше и ниже\)",
            r"\(подробно\)",
            r"\(",
            r"\[",
            r"\\(x\\)",
        ] {
            same(input);
        }
    }

    #[test]
    fn a_pipe_inside_a_table_row_cannot_end_the_cell() {
        assert_eq!(
            prepare("| a | b |\n|---|---|\n| $|x|$ | 1 |"),
            "| a | b |\n|---|---|\n| $\\vert{}x\\vert{}$ | 1 |"
        );
        same("Вне таблицы $|x|$");
    }

    #[test]
    fn non_ascii_text_is_kept_whole() {
        assert_eq!(prepare("Привет $ мир 🙂 \\я $x$"), "Привет \\$ мир 🙂 \\я $x$");
    }

    /// Formulas found by the Markdown parser after `prepare`, with what stayed as text.
    fn parsed(input: &str) -> (Vec<String>, String) {
        let text = prepare(input);
        let options = Options::ENABLE_TABLES | Options::ENABLE_MATH;
        let (mut math, mut plain) = (vec![], String::new());
        for event in Parser::new_ext(&text, options) {
            match event {
                Event::InlineMath(tex) | Event::DisplayMath(tex) => math.push(tex.to_string()),
                Event::Text(t) | Event::Code(t) => plain.push_str(&t),
                _ => {}
            }
        }
        (math, plain)
    }

    #[test]
    fn the_markdown_parser_agrees_with_prepare() {
        let (math, plain) = parsed("Цена $5, $x^2$ и `$y$` за $10, потом \\(z\\) и $HOME/$USER.");
        assert_eq!(math, ["x^2", "z"]);
        assert_eq!(plain, "Цена $5,  и $y$ за $10, потом  и $HOME/$USER.");
        let (math, _) = parsed("| a |\n|---|\n| $|x|_{2}^*$ |");
        assert_eq!(math, [r"\vert{}x\vert{}_{2}^*"]);
        let (math, _) = parsed("$$\n\\begin{aligned}\nx &= a \\\\\n  + b\n\\end{aligned}\n$$");
        assert_eq!(math, [r"\begin{aligned} x &= a \\ + b \end{aligned}"]);
        let (math, plain) = parsed("$*a*$ и $_b_$ *c*");
        assert_eq!(math, ["*a*", "_b_"]);
        assert_eq!(plain, " и  c");
    }

    #[test]
    fn typesetting_reports_size_and_errors() {
        let (list, _) = typeset(r"\frac{a}{b}", false, 17., Color32::WHITE).unwrap();
        assert!(list.width > 0. && list.height > 0. && list.depth > 0.);
        assert!(typeset(r"\nosuchcommand{x}", false, 17., Color32::WHITE).is_err());
        assert!(typeset(r"\frac{1}{", false, 17., Color32::WHITE).is_err());
    }

    #[test]
    fn cyrillic_letters_get_their_real_width() {
        // In KaTeX's table `я` borrows the narrow `r`, and `ш` the narrower `w`.
        let metrics = measure_fallback_font();
        assert!(metrics[&'я'].width > 0.43, "{:?}", metrics[&'я']);
        assert!(metrics[&'ш'].width > 0.75, "{:?}", metrics[&'ш']);
        assert!(metrics[&'щ'].depth > 0.1, "{:?}", metrics[&'щ']);
        let width = |tex: &str| typeset(tex, false, 17., Color32::WHITE).unwrap().0.width;
        let expected: f64 = "яяяя".chars().map(|c| metrics[&c].width).sum();
        assert!((width(r"\text{яяяя}") - expected).abs() < 0.01);
    }

    #[test]
    fn extreme_formulas_fail_without_a_crash() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            let long = "x".repeat(MAX_TEX_BYTES + 1);
            assert!(render(ctx, &long, true, 17., 1., Color32::WHITE).is_err());
            let wide = r"x\kern100000em y";
            assert!(render(ctx, wide, true, 17., 1., Color32::WHITE).is_err());
            let nested = format!("{}x{}", "{".repeat(5000), "}".repeat(5000));
            assert!(render(ctx, &nested, true, 17., 1., Color32::WHITE).is_err());
            let bomb = r"\def\a{\a\a}\a";
            assert!(render(ctx, bomb, true, 17., 1., Color32::WHITE).is_err());
        });
    }

    fn render_message(markdown: &str, width: f32, pixels: f32) -> Vec<Shape> {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(pixels);
        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            "inter".into(),
            Arc::new(FontData::from_static(include_bytes!(
                "../assets/InterVariable.ttf"
            ))),
        );
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "inter".into());
        ctx.set_fonts(fonts);
        ctx.style_mut(|style| {
            style
                .text_styles
                .insert(TextStyle::Body, egui::FontId::proportional(14.));
        });
        let mut cache = CommonMarkCache::default();
        let run = |ctx: &egui::Context, cache: &mut CommonMarkCache| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    vec2(width, 1800.) / pixels,
                )),
                max_texture_side: Some(8192),
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, cache, markdown));
            })
        };
        let _ = run(&ctx, &mut cache);
        let output = run(&ctx, &mut cache);
        fn flatten(shape: Shape, out: &mut Vec<Shape>) {
            match shape {
                Shape::Vec(shapes) => shapes.into_iter().for_each(|shape| flatten(shape, out)),
                shape => out.push(shape),
            }
        }
        let mut shapes = Vec::new();
        for clipped in output.shapes {
            flatten(clipped.shape, &mut shapes);
        }
        shapes
    }

    fn text_shape<'a>(shapes: &'a [Shape], prefix: &str) -> &'a TextShape {
        shapes
            .iter()
            .find_map(|shape| match shape {
                Shape::Text(text) if text.galley.text().starts_with(prefix) => Some(text),
                _ => None,
            })
            .expect(prefix)
    }

    fn formula_rects(shapes: &[Shape]) -> Vec<Rect> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Rect(rect) if rect.brush.is_some() => Some(rect.rect),
                Shape::Mesh(mesh) if mesh.texture_id != TextureId::default() => {
                    Some(mesh.calc_bounds())
                }
                _ => None,
            })
            .collect()
    }

    /// A window that lives for several frames and receives real input.
    struct Session {
        ctx: egui::Context,
        cache: CommonMarkCache,
        markdown: String,
        width: f32,
        time: f64,
    }

    impl Session {
        fn new(markdown: &str, width: f32) -> Self {
            let ctx = egui::Context::default();
            ctx.style_mut(|style| {
                style
                    .text_styles
                    .insert(TextStyle::Body, egui::FontId::proportional(14.));
            });
            Self {
                ctx,
                cache: CommonMarkCache::default(),
                markdown: markdown.to_owned(),
                width,
                time: 0.,
            }
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            self.time += 1. / 30.;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, 900.))),
                max_texture_side: Some(8192),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let (cache, markdown) = (&mut self.cache, &self.markdown);
            self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, cache, markdown));
            })
        }

        fn click(&mut self, at: Pos2, button: egui::PointerButton) -> Vec<egui::FullOutput> {
            let press = |pressed| egui::Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: Default::default(),
            };
            vec![
                self.frame(vec![egui::Event::PointerMoved(at)]),
                self.frame(vec![press(true)]),
                self.frame(vec![press(false)]),
                self.frame(vec![]),
            ]
        }
    }

    fn output_shapes(output: egui::FullOutput) -> Vec<Shape> {
        fn flatten(shape: Shape, out: &mut Vec<Shape>) {
            match shape {
                Shape::Vec(shapes) => shapes.into_iter().for_each(|shape| flatten(shape, out)),
                shape => out.push(shape),
            }
        }
        let mut shapes = Vec::new();
        for clipped in output.shapes {
            flatten(clipped.shape, &mut shapes);
        }
        shapes
    }

    #[test]
    fn right_click_copies_the_latex_source() {
        let tex = r"\frac{a}{b}";
        let mut session = Session::new(&format!("Дробь ${tex}$ в тексте"), 800.);
        session.frame(vec![]);
        let rect = formula_rects(&output_shapes(session.frame(vec![])))[0];
        let mut outputs = session.click(rect.center(), egui::PointerButton::Secondary);
        let shapes = output_shapes(outputs.pop().unwrap());
        let item = text_shape(&shapes, "Скопировать LaTeX");
        let at = item.pos + item.galley.size() / 2.;
        let copied: Vec<_> = session
            .click(at, egui::PointerButton::Primary)
            .into_iter()
            .flat_map(|output| output.platform_output.commands)
            .collect();
        assert!(
            copied.iter().any(
                |command| matches!(command, egui::OutputCommand::CopyText(text) if text == tex)
            ),
            "{copied:?}"
        );
    }

    #[test]
    fn a_wide_formula_scrolls_sideways() {
        let wide = format!("$${}$$", "x + y + ".repeat(80));
        let mut session = Session::new(&wide, 500.);
        session.frame(vec![]);
        let before = formula_rects(&output_shapes(session.frame(vec![])))[0];
        assert!(before.width() > 500.);
        let at = pos2(250., before.center().y);
        session.frame(vec![egui::Event::PointerMoved(at)]);
        session.frame(vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(-400., 0.),
            modifiers: Default::default(),
        }]);
        let mut left = before.left();
        for _ in 0..60 {
            left = formula_rects(&output_shapes(session.frame(vec![])))[0].left();
        }
        assert!(left < before.left() - 100., "{} -> {left}", before.left());
    }

    #[test]
    fn every_prefix_of_a_streamed_message_renders() {
        let message = concat!(
            "Пусть $f(x) = x^x$, цена $5 🙂.\n\n$$\\frac{a}{b}$$\n\n\\[x^2 + 1\\] и \\(y\\)\n\n",
            "| a | $|x|$ |\n|---|---|\n| 1 | $\\sqrt{2}$ |\n\n",
            "- я $z$\n- $\\begin{cases} a \\\\ b \\end{cases}$\n\n",
            "> $$q$$\n\n```\n$code$\n```\n\n",
            "$$\n\\begin{aligned}\nx &= 1 \\\\\n  + y\n\\end{aligned}\n$$\n",
        );
        let mut session = Session::new("", 600.);
        let ends: Vec<usize> = message
            .char_indices()
            .map(|(at, _)| at)
            .chain([message.len()])
            .collect();
        for end in ends {
            session.markdown = message[..end].to_owned();
            session.frame(vec![]);
        }
    }

    #[test]
    fn random_text_never_breaks_prepare_or_the_parser() {
        const PIECES: &[&str] = &[
            "$", "$$", "\\", "\\(", "\\)", "\\[", "\\]", "(", ")", "[", "]", "{", "}", "`", "```",
            "\n", "\n\n", " ", "    ", "a", "x^2", "я", "щ", "🙂", "|", "| a |\n|---|\n", ">",
            "> ", "- ", "1. ", "%", "^", "_", "*", "\\frac{a}{b}", "\\text{я}", "<b>", "](", "\r\n",
        ];
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n as u64) as usize
        };
        let options = Options::ENABLE_TABLES | Options::ENABLE_MATH;
        for _ in 0..30_000 {
            let input: String = (0..next(24)).map(|_| PIECES[next(PIECES.len())]).collect();
            let output = prepare(&input);
            for event in Parser::new_ext(&output, options) {
                if let Event::InlineMath(tex) | Event::DisplayMath(tex) = event {
                    assert!(!tex.is_empty(), "{input:?} -> {output:?}");
                }
            }
        }
    }

    #[test]
    fn inline_formula_sits_on_the_text_baseline() {
        // `x` has no depth, so the bottom of its image is the baseline plus the padding.
        for pixels in [1., 1.25, 1.5, 2.] {
            for markdown in [
                "Слово $x$ дальше",
                "Первая $x$ и высокая $\\frac{a}{b}$ формула",
                "1. Пункт $x$ дальше\n2. Пункт $\\frac{a}{b}$",
                "> Цитата $x$ дальше",
            ] {
                let shapes = render_message(markdown, 900., pixels);
                let rects = formula_rects(&shapes);
                let first = rects.first().unwrap_or_else(|| panic!("{markdown}: no formula"));
                let word = markdown.trim_start_matches(['1', '.', ' ', '>']);
                let word: String = word.chars().take(3).collect();
                let text = text_shape(&shapes, &word);
                let baseline = text.pos.y + text.galley.rows[0].glyphs[0].pos.y;
                let em = 14. * MATH_SCALE;
                let padding = (em * PADDING_EM).ceil();
                let (list, _) = typeset("x", false, em, Color32::WHITE).unwrap();
                let image_baseline = first.top() + padding + list.height as f32 * em;
                assert!(
                    (image_baseline - baseline).abs() <= 1.0,
                    "{markdown} at {pixels}: formula {image_baseline}, text {baseline}"
                );
            }
        }
    }

    #[test]
    fn rows_around_a_tall_formula_make_room_and_the_next_paragraph_is_normal() {
        let long = "слово ".repeat(60);
        let tall = format!("Начало {long} $\\sum_{{i=1}}^n \\frac{{a_i}}{{b_i}}$ конец {long}\n\nСледующий абзац {long}");
        let shapes = render_message(&tall, 500., 1.);
        let first = text_shape(&shapes, "Начало");
        let rows = &first.galley.rows;
        assert!(rows.len() > 3);
        let pitch = rows[2].rect.top() - rows[1].rect.top();
        let next = text_shape(&shapes, "Следующий");
        let normal = next.galley.rows[2].rect.top() - next.galley.rows[1].rect.top();
        assert!(pitch > normal + 3., "tall {pitch}, normal {normal}");
        let (ascent, row) = (next.galley.rows[0].glyphs[0].pos.y, normal);
        let em = 14. * MATH_SCALE;
        let (list, _) = typeset(r"\sum_{i=1}^n \frac{a_i}{b_i}", false, em, Color32::WHITE).unwrap();
        let above = list.height as f32 * em;
        let below = list.depth as f32 * em;
        assert!(pitch + 1. >= above.max(ascent) + below.max(row - ascent));
        // Plain short formulas keep the ordinary rhythm.
        let plain = format!("Начало {long} $x$ конец {long}\n\nСледующий {long}");
        let shapes = render_message(&plain, 500., 1.);
        let first = text_shape(&shapes, "Начало");
        let pitch = first.galley.rows[2].rect.top() - first.galley.rows[1].rect.top();
        assert!((pitch - normal).abs() < 0.5, "{pitch} vs {normal}");
    }

    #[test]
    fn formulas_in_a_table_are_drawn_and_fit_their_cells() {
        let markdown = "| Что | Формула |\n|---|---|\n| Предел | $\\lim_{x\\to0}\\frac{\\sin x}{x}$ и слова |\n| Ряд | $\\sum_{n=1}^{\\infty} \\frac{1}{n^2} = \\frac{\\pi^2}{6}$ |\n\nПосле таблицы";
        let shapes = render_message(markdown, 900., 1.);
        let rects = formula_rects(&shapes);
        assert_eq!(rects.len(), 2, "both formulas are painted once");
        assert!(rects[0].bottom() <= rects[1].top() + 1.);
        let after = text_shape(&shapes, "После");
        assert!(after.pos.y >= rects[1].bottom() - 1.);
    }

    #[test]
    fn display_formula_is_centered_on_its_own_row_and_wide_ones_stay_inside() {
        let shapes = render_message("До\n\n$$\\frac{a}{b}$$\n\nПосле", 800., 1.);
        let rect = formula_rects(&shapes)[0];
        let before = text_shape(&shapes, "До");
        let after = text_shape(&shapes, "После");
        assert!(before.pos.y + 10. < rect.top() + 5.);
        assert!(rect.bottom() - 5. < after.pos.y + 5.);
        assert!((rect.center().x - 400.).abs() < 16., "{rect:?}");

        let wide = format!("$${}$$", "x + y + ".repeat(60));
        let shapes = render_message(&wide, 500., 1.);
        let rect = formula_rects(&shapes)[0];
        assert!(rect.width() > 500. && rect.left() < 40., "{rect:?}");
    }

    #[test]
    fn a_broken_formula_shows_its_source() {
        let shapes = render_message("До $\\nosuch{x}$ после", 800., 1.);
        assert!(formula_rects(&shapes).is_empty());
        assert!(shapes.iter().any(|shape| matches!(
            shape,
            Shape::Text(text) if text.galley.text().contains("\\nosuch{x}")
        )));
    }

    #[test]
    fn words_after_a_formula_on_its_own_row_have_no_stray_space() {
        let shapes = render_message("Текст\n$$\\frac{a}{b}$$\nдальше", 800., 1.);
        assert_eq!(text_shape(&shapes, "дальше").galley.text(), "дальше");
    }

    #[test]
    fn tall_inline_formula_moves_to_its_own_row() {
        let matrix = r"\begin{pmatrix} a \\ b \\ c \\ d \end{pmatrix}";
        let shapes = render_message(&format!("Матрица ${matrix}$ дальше"), 800., 1.);
        let rect = formula_rects(&shapes)[0];
        let before = text_shape(&shapes, "Матрица");
        let after = text_shape(&shapes, "дальше");
        let padding = (14. * MATH_SCALE * PADDING_EM).ceil();
        assert!(before.pos.y + before.galley.size().y <= rect.top() + padding + 1.);
        assert!(after.pos.y >= rect.bottom() - padding - 1.);
    }
}

#[cfg(test)]
mod coverage {
    use super::*;
    #[test]
    fn common_formulas_all_typeset() {
        let cases = [
            r"E = mc^2", r"\frac{-b \pm \sqrt{b^2-4ac}}{2a}", r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}",
            r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}", r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
            r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}", r"\begin{cases} x & x>0 \\ -x & x \le 0 \end{cases}",
            r"\begin{aligned} a &= b + c \\ d &= e \end{aligned}", r"\begin{align} a &= b \end{align}",
            r"\mathbb{R}^n", r"\mathcal{O}(n \log n)", r"\operatorname{argmax}_x f(x)", r"\text{скорость} = \frac{s}{t}",
            r"\lim_{x \to 0} \frac{\sin x}{x} = 1", r"\vec{v} \cdot \hat{n}", r"\overline{AB}", r"\underbrace{a+b}_{c}",
            r"\binom{n}{k}", r"\left( \frac{a}{b} \right)", r"x \in \{1,2,3\}", r"\forall \epsilon > 0\ \exists \delta",
            r"\nabla \times \mathbf{B} = \mu_0 \mathbf{J}", r"\partial_t u = \Delta u", r"a \equiv b \pmod{n}",
            r"\boxed{x=1}", r"\color{red} x", r"\textbf{bold}", r"\mathrm{d}x", r"\ce{H2O}", r"\tag{1} x",
            r"\begin{array}{cc} 1 & 2 \\ 3 & 4 \end{array}", r"\dfrac{1}{2}", r"\tfrac{1}{2}", r"\sqrt[3]{x}",
            r"\langle x, y \rangle", r"\lVert x \rVert", r"\iint_D f\,dA", r"\oint", r"x \mapsto x^2", r"\xrightarrow{f}",
            r"\begin{matrix} a \end{matrix}", r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}", r"\displaystyle\sum_n",
            r"\stackrel{?}{=}", r"\overset{def}{=}", r"\underset{x}{\min}", r"\not\subset", r"\cancel{x}", r"\hbar",
        ];
        let mut failed = vec![];
        for tex in cases {
            if let Err(e) = typeset(tex, true, 17., Color32::WHITE) {
                failed.push(format!("{tex}  →  {e}"));
            }
        }
        assert!(failed.is_empty(), "{} of {} failed:\n{}", failed.len(), cases.len(), failed.join("\n"));
    }

    #[test]
    fn bare_environments_become_display_formulas() {
        let math = |m: &str| -> Vec<String> {
            let prepared = prepare(m);
            Parser::new_ext(&prepared, Options::all())
                .filter_map(|e| match e {
                    Event::DisplayMath(t) => Some(t.to_string()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(math("\\begin{equation}\nE = mc^2 \\label{eq:1}\n\\end{equation}"), vec!["E = mc^2"]);
        let aligned = math("Итак:\n\\begin{align}\nx &= 1 \\nonumber \\\\\ny &= 2\n\\end{align}\nконец");
        assert_eq!(aligned.len(), 1);
        assert!(aligned[0].starts_with("\\begin{aligned}") && !aligned[0].contains("nonumber"), "{aligned:?}");
        assert!(typeset(&aligned[0], true, 17., Color32::WHITE).is_ok());
        // Mid-sentence or inside code it stays text.
        assert!(math("слово \\begin{equation}x\\end{equation}").is_empty());
        assert!(math("```\n\\begin{equation}x\n\\end{equation}\n```").is_empty());
        // Unknown environments are not formulas.
        assert!(math("\\begin{itemize}\n\\item x\n\\end{itemize}").is_empty());
    }

}
