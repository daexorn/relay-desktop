use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Never let model output write terminal control sequences (including OSC 52).
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') | Some('P') | Some('^') | Some('_') => {
                    let mut esc = false;
                    for c in chars.by_ref() {
                        if c == '\u{7}' || esc && c == '\\' {
                            break;
                        }
                        esc = c == '\u{1b}';
                    }
                }
                _ => {}
            }
        } else if c == '\r' {
            out.push('\n');
        } else if c == '\t' {
            out.push_str("    ");
        } else if !c.is_control() || c == '\n' {
            out.push(c);
        }
    }
    out
}

pub fn inline(text: &str, base: Style, accent: Color, muted: Color) -> Line<'static> {
    let mut spans = vec![];
    let mut style = base;
    let mut stack = vec![];
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Strong) => {
                stack.push(style);
                style = style.add_modifier(Modifier::BOLD);
            }
            Event::Start(Tag::Emphasis) => {
                stack.push(style);
                style = style.add_modifier(Modifier::ITALIC);
            }
            Event::Start(Tag::Link { .. }) => {
                stack.push(style);
                style = style.fg(accent).add_modifier(Modifier::UNDERLINED);
            }
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link) => {
                style = stack.pop().unwrap_or(base)
            }
            Event::Text(text) => spans.push(Span::styled(text.into_string(), style)),
            Event::Code(text) => spans.push(Span::styled(text.into_string(), base.fg(accent))),
            Event::Html(text) | Event::InlineHtml(text) => {
                spans.push(Span::styled(text.into_string(), base.fg(muted)))
            }
            Event::SoftBreak | Event::HardBreak => spans.push(Span::raw(" ")),
            _ => {}
        }
    }
    Line::from(spans)
}

/// Word wrap while preserving span styles. Long paths wrap by grapheme, never byte.
pub fn wrap(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines = vec![];
    let mut spans = vec![];
    let mut used = 0;
    for span in line.spans {
        for word in span.content.split_word_bounds() {
            let word_width = word.width();
            if used > 0 && used + word_width > width && word_width <= width {
                lines.push(Line::from(std::mem::take(&mut spans)));
                used = 0;
                if word.chars().all(char::is_whitespace) {
                    continue;
                }
            }
            let mut part = String::new();
            for g in word.graphemes(true) {
                let w = g.width();
                if used + w > width && used > 0 {
                    if !part.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut part), span.style));
                    }
                    lines.push(Line::from(std::mem::take(&mut spans)));
                    used = 0;
                }
                part.push_str(g);
                used += w;
            }
            if !part.is_empty() {
                spans.push(Span::styled(part, span.style));
            }
        }
    }
    lines.push(Line::from(spans));
    lines
}

pub fn render(
    text: &str,
    width: usize,
    text_color: Color,
    accent: Color,
    muted: Color,
) -> Vec<Line<'static>> {
    let text = clean(text);
    let rows: Vec<_> = text.lines().collect();
    let mut result = vec![];
    let mut i = 0;
    let mut code = false;
    while i < rows.len() {
        let row = rows[i];
        if row.trim_start().starts_with("```") || row.trim_start().starts_with("~~~") {
            code = !code;
            if code {
                let label = row
                    .trim_start()
                    .trim_start_matches('`')
                    .trim_start_matches('~');
                result.push(Line::styled(
                    format!("┌─ {}", if label.is_empty() { "код" } else { label }),
                    Style::default().fg(muted),
                ));
            } else {
                result.push(Line::styled("└─", Style::default().fg(muted)));
            }
            i += 1;
            continue;
        }
        if code {
            for line in wrap(
                Line::styled(row.to_owned(), Style::default().fg(text_color)),
                width.saturating_sub(2),
            ) {
                let mut spans = vec![Span::styled("│ ", Style::default().fg(muted))];
                spans.extend(line.spans);
                result.push(Line::from(spans));
            }
            i += 1;
            continue;
        }
        if i + 1 < rows.len() && row.contains('|') && table_separator(rows[i + 1]) {
            let mut table = vec![cells(row)];
            i += 2;
            while i < rows.len() && rows[i].contains('|') && !rows[i].trim().is_empty() {
                table.push(cells(rows[i]));
                i += 1;
            }
            result.extend(render_table(&table, width, text_color, accent, muted));
            continue;
        }
        let trim = row.trim_start();
        let mut style = Style::default().fg(text_color);
        let row = if trim.starts_with('#') && trim.chars().take_while(|c| *c == '#').count() <= 6 {
            style = style.fg(accent).add_modifier(Modifier::BOLD);
            trim.trim_start_matches('#').trim_start().to_owned()
        } else if let Some(value) = trim.strip_prefix("- ").or_else(|| trim.strip_prefix("* ")) {
            format!("{}• {value}", " ".repeat(row.len() - trim.len()))
        } else if let Some(value) = trim.strip_prefix("> ") {
            style = style.fg(muted);
            format!("│ {value}")
        } else {
            row.to_owned()
        };
        let (prefix, body) = ordered(&row);
        let mut rendered = inline(body, style, accent, muted);
        if !prefix.is_empty() {
            rendered
                .spans
                .insert(0, Span::styled(prefix.to_owned(), style));
        }
        result.extend(wrap(rendered, width));
        i += 1;
    }
    result
}
fn ordered(row: &str) -> (&str, &str) {
    let digits = row.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && digits < 10
        && (row[digits..].starts_with(". ") || row[digits..].starts_with(") "))
    {
        row.split_at(digits + 2)
    } else {
        ("", row)
    }
}
fn cells(row: &str) -> Vec<String> {
    row.trim()
        .trim_matches('|')
        .split('|')
        .map(|s| s.trim().to_owned())
        .collect()
}
fn table_separator(row: &str) -> bool {
    let cells = cells(row);
    !cells.is_empty()
        && cells
            .iter()
            .all(|c| c.contains('-') && c.chars().all(|c| matches!(c, '-' | ':' | ' ')))
}
fn plain(text: &str) -> String {
    Parser::new(text)
        .filter_map(|e| match e {
            Event::Text(s) | Event::Code(s) => Some(s.into_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}
fn render_table(
    rows: &[Vec<String>],
    width: usize,
    text: Color,
    accent: Color,
    muted: Color,
) -> Vec<Line<'static>> {
    let n = rows.first().map_or(0, Vec::len).min(16);
    if n == 0 {
        return vec![];
    }
    if width < n * 9 + 1 {
        let mut lines = vec![];
        for row in rows.iter().skip(1) {
            for (i, cell) in row.iter().take(n).enumerate() {
                let title = rows[0].get(i).cloned().unwrap_or_default();
                lines.extend(wrap(
                    Line::from(vec![
                        Span::styled(format!("{}: ", plain(&title)), Style::default().fg(accent)),
                        Span::styled(plain(cell), Style::default().fg(text)),
                    ]),
                    width,
                ));
            }
            lines.push(Line::default());
        }
        return lines;
    }
    let available = width.saturating_sub(n * 3 + 1);
    let mut widths = vec![6usize; n];
    let desired: Vec<_> = (0..n)
        .map(|i| {
            rows.iter()
                .filter_map(|r| r.get(i))
                .map(|s| plain(s).width())
                .max()
                .unwrap_or(6)
                .max(6)
                .min(60)
        })
        .collect();
    while widths.iter().sum::<usize>() < available {
        let candidate = (0..n)
            .filter(|i| widths[*i] < desired[*i])
            .max_by_key(|i| desired[*i] - widths[*i]);
        if let Some(i) = candidate {
            widths[i] += 1;
        } else {
            break;
        }
    }
    let rule = |left: &str, middle: &str, right: &str| {
        Line::styled(
            format!(
                "{left}{}{right}",
                widths
                    .iter()
                    .map(|w| "─".repeat(w + 2))
                    .collect::<Vec<_>>()
                    .join(middle)
            ),
            Style::default().fg(muted),
        )
    };
    let mut lines = vec![rule("┌", "┬", "┐")];
    for (row_index, row) in rows.iter().enumerate() {
        let wrapped: Vec<_> = (0..n)
            .map(|i| {
                wrap(
                    Line::raw(plain(row.get(i).map(String::as_str).unwrap_or(""))),
                    widths[i],
                )
            })
            .collect();
        for line_index in 0..wrapped.iter().map(Vec::len).max().unwrap_or(1) {
            let mut spans = vec![Span::styled("│", Style::default().fg(muted))];
            for i in 0..n {
                let value = wrapped[i]
                    .get(line_index)
                    .map(|l| l.to_string())
                    .unwrap_or_default();
                let padding = widths[i].saturating_sub(value.width());
                spans.push(Span::styled(
                    format!(" {value}{} ", " ".repeat(padding)),
                    Style::default()
                        .fg(if row_index == 0 { accent } else { text })
                        .add_modifier(if row_index == 0 {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ));
                spans.push(Span::styled("│", Style::default().fg(muted)));
            }
            lines.push(Line::from(spans));
        }
        if row_index == 0 {
            lines.push(rule("├", "┼", "┤"));
        }
    }
    lines.push(rule("└", "┴", "┘"));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_output_cannot_change_title_clipboard_or_styles() {
        assert_eq!(
            clean("a\u{1b}]52;c;secret\u{7}b\u{1b}[31mc\u{1b}[0m\u{9b}d"),
            "abcd"
        );
    }
    #[test]
    fn tables_wrap_as_cells_and_switch_to_records_when_narrow() {
        let table =
            "| Название | Результат |\n|---|---|\n| Длинное название проверки | Всё хорошо |";
        for width in [12, 30, 70] {
            let rows = render(table, width, Color::White, Color::Green, Color::Gray);
            assert!(rows.iter().all(|l| l.width() <= width));
            let text = rows
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("хорошо"));
            if width == 12 {
                assert!(text.contains("Название:"));
            } else {
                assert!(text.contains('┌'));
            }
        }
    }
    #[test]
    fn markdown_lists_code_and_long_paths_stay_inside_view() {
        for width in [8, 30, 80] {
            let rows=render("## Заголовок\n1. Первый пункт\n2. Второй пункт\n\n```rust\nlet path = \"/very/long/path/без/пробелов\";\n```\n**Жирный** и `код`",width,Color::White,Color::Green,Color::Gray);
            assert!(rows.iter().all(|l| l.width() <= width));
            let text = rows
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("1."));
            assert!(text.contains("2."));
        }
    }
}
