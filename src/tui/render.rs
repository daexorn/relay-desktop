use super::{
    app::{App, Field, Hit, Picker, Popup, COMMANDS},
    conversation::tool_name,
    editor::Editor,
    markdown::{self, clean},
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

fn rgb(hex: u32) -> Color {
    Color::Rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

#[derive(Clone, Copy)]
struct Palette {
    base: Color,
    panel: Color,
    surface: Color,
    border: Color,
    text: Color,
    muted: Color,
    accent: Color,
    red: Color,
    green: Color,
}
impl Palette {
    fn new(theme: &str) -> Self {
        let &(_, _, is_light, c) = relay_desktop::model::theme_spec(theme);
        Self {
            base: rgb(c[0]),
            panel: rgb(c[1]),
            surface: rgb(c[3]),
            border: rgb(c[4]),
            text: rgb(c[5]),
            muted: rgb(c[6]),
            accent: rgb(c[7]),
            red: if is_light {
                Color::Rgb(185, 48, 56)
            } else {
                Color::Rgb(235, 118, 128)
            },
            green: if is_light {
                Color::Rgb(34, 134, 86)
            } else {
                Color::Rgb(110, 212, 154)
            },
        }
    }
    fn normal(self) -> Style {
        Style::default().fg(self.text).bg(self.base)
    }
}
#[derive(Default)]
pub struct View {
    cache: Option<(String, u64, usize, u16, bool, String, String)>,
    lines: Vec<Line<'static>>,
}

fn block(title: impl Into<Line<'static>>, p: Palette, active: bool) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if active { p.accent } else { p.border }))
        .style(p.normal())
}
fn label(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(clean(&text.into()), Style::default().fg(color))
}
fn fit(text: &str, width: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    if text.width() <= width {
        return text.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for g in text.graphemes(true) {
        if out.width() + g.width() + 1 > width {
            break;
        }
        out.push_str(g);
    }
    out.push('…');
    out
}
fn put(frame: &mut Frame, area: Rect, text: impl Into<Text<'static>>, p: Palette) {
    let area = area.intersection(frame.area());
    if area.width > 0 && area.height > 0 {
        frame.render_widget(Paragraph::new(text).style(p.normal()), area);
    }
}

pub fn draw(frame: &mut Frame, app: &mut App, view: &mut View) {
    let p = Palette::new(app.theme());
    let area = frame.area();
    app.hits.clear();
    frame.render_widget(Block::default().style(p.normal()), area);
    if area.width < 32 || area.height < 10 {
        put(
            frame,
            area,
            Text::raw("Relay TUI\nУвеличь терминал до 32×10\nCtrl+Q — выйти"),
            p,
        );
        return;
    }
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    let header = layout[0];
    let right_reserve = if area.width >= 76 {
        36
    } else if area.width >= 54 {
        13
    } else {
        2
    };
    let mut head = vec![
        label("  ◈ ", p.accent),
        Span::styled(
            "Relay",
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        label(format!(" TUI  {}", env!("CARGO_PKG_VERSION")), p.muted),
    ];
    if let Some(server) = app.server() {
        head.push(label(
            format!("   {} ", if app.connected() { "●" } else { "○" }),
            if app.connected() { p.green } else { p.muted },
        ));
        let max_srv = (area.width as usize).saturating_sub(26 + right_reserve);
        head.push(Span::styled(
            fit(&clean(&server.name), max_srv),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ));
    }
    if area.width >= 98 {
        if let Some(c) = app.chat() {
            head.push(label("  ", p.muted));
            head.push(Span::styled(
                format!(" {} ", relay_desktop::model::backend_short_label(&c.backend)),
                Style::default()
                    .fg(p.accent)
                    .bg(p.surface)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    put(
        frame,
        Rect {
            x: header.x,
            y: header.y + 1,
            width: header.width,
            height: 1,
        },
        Line::from(head),
        p,
    );
    if area.width >= 76 {
        let controls = "[F2 Хосты] [F3 Сессии] [F5 Меню]";
        let w = controls.width() as u16;
        let r = Rect {
            x: header.right().saturating_sub(w + 2),
            y: header.y + 1,
            width: w,
            height: 1,
        };
        let spans = vec![
            label("[", p.border),
            Span::styled(
                "F2",
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            ),
            label(" Хосты", p.text),
            label("] [", p.border),
            Span::styled(
                "F3",
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            ),
            label(" Сессии", p.text),
            label("] [", p.border),
            Span::styled(
                "F5",
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            ),
            label(" Меню", p.text),
            label("]", p.border),
        ];
        put(frame, r, Line::from(spans), p);
        app.hits.push((Rect { width: 10, ..r }, Hit::Hosts));
        app.hits.push((
            Rect {
                x: r.x + 11,
                width: 11,
                ..r
            },
            Hit::Sessions,
        ));
        app.hits.push((
            Rect {
                x: r.x + 23,
                width: 9,
                ..r
            },
            Hit::Workspace,
        ));
    } else if area.width >= 54 {
        let controls = "[F5 Меню]";
        let w = controls.width() as u16;
        let r = Rect {
            x: header.right().saturating_sub(w + 2),
            y: header.y + 1,
            width: w,
            height: 1,
        };
        put(
            frame,
            r,
            Line::from(vec![
                label("[", p.border),
                Span::styled(
                    "F5",
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                ),
                label(" Меню", p.text),
                label("]", p.border),
            ]),
            p,
        );
        app.hits.push((r, Hit::Workspace));
    }
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(p.border)),
        header,
    );

    let body = layout[1];
    let content = Rect {
        x: body.x + 1,
        y: body.y,
        width: body.width.saturating_sub(2),
        height: body.height,
    };
    let input_rows = app
        .chat()
        .map(|c| {
            c.editor
                .visual(content.width.saturating_sub(4) as usize, false)
                .0
                .len()
        })
        .unwrap_or(1)
        .clamp(2, 6) as u16;
    let notice_height = if app.notice.is_empty() { 0 } else { 2 };
    let sections = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(notice_height),
        Constraint::Length(input_rows + 3),
    ])
    .split(content);

    let mut title_spans = vec![];
    if let Some(c) = app.chat() {
        if c.locked && !c.unlocked {
            title_spans.push(Span::styled(
                "🔒 ",
                Style::default().fg(p.red).add_modifier(Modifier::BOLD),
            ));
        } else if c.locked {
            title_spans.push(Span::styled(
                "🔓 ",
                Style::default().fg(p.green).add_modifier(Modifier::BOLD),
            ));
        }
        title_spans.push(Span::styled(
            fit(&clean(&c.title), content.width.saturating_sub(4) as usize),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ));
    } else {
        title_spans.push(Span::styled(
            "Рабочее пространство",
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ));
    }
    put(
        frame,
        Rect {
            height: 1,
            ..sections[0]
        },
        Line::from(title_spans),
        p,
    );
    if let Some(c) = app.chat() {
        put(
            frame,
            Rect {
                y: sections[0].y + 1,
                height: 1,
                ..sections[0]
            },
            Line::from(vec![
                label("⌁ ", p.muted),
                label(
                    fit(&clean(&c.cwd), content.width.saturating_sub(2) as usize),
                    p.muted,
                ),
            ]),
            p,
        );
    }

    app.chat_rect = sections[1];
    if app.chat().is_some_and(|c| c.locked && !c.unlocked) {
        locked_view(frame, sections[1], p);
    } else if app.chat().is_some_and(|c| !c.messages.is_empty()) {
        transcript(frame, app, view, sections[1], p);
    } else {
        welcome(frame, app, sections[1], p);
    }

    status(frame, app, sections[2], p);
    if notice_height > 0 {
        let notice = clean(&app.notice);
        let mut rows = markdown::wrap(
            Line::styled(notice, Style::default().fg(p.muted)),
            content.width as usize,
        );
        rows.truncate(2);
        put(frame, sections[3], Text::from(rows), p);
    }
    composer(frame, app, sections[4], p);

    let hint = if matches!(app.popup, Some(Popup::Workspace { .. })) {
        " Esc/F5 закрыть   ↑↓ выбор   Tab раздел   Enter открыть   Ctrl+N новая   Ctrl+A сервер "
    } else if app.popup.is_some() {
        " Esc закрыть   ↑↓ выбор   Enter открыть "
    } else if area.width < 84 {
        " Enter отправить · F5 меню · Alt+Enter строка · F1 справка "
    } else {
        " Enter отправить   Alt+Enter строка   F5 меню   Ctrl+N новая   Ctrl+O вывод   PgUp/PgDn история   F1 справка "
    };
    put(
        frame,
        layout[2],
        Line::styled(fit(hint, area.width as usize), Style::default().fg(p.muted)),
        p,
    );
    if let Some(popup) = &app.popup {
        let mut popup_hits = vec![];
        popup_view(frame, app, popup, p, &mut popup_hits);
        if !popup_hits.is_empty() {
            popup_hits.append(&mut app.hits);
            app.hits = popup_hits;
        }
    }
}

fn locked_view(frame: &mut Frame, area: Rect, p: Palette) {
    let width = area.width.saturating_sub(4) as usize;
    let mut rows = vec![
        Line::default(),
        Line::styled(
            fit("🔒 Сессия защищена паролем", width),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Line::default(),
    ];
    rows.extend(markdown::wrap(
        Line::styled(
            "Введи пароль через /unlock или нажми Enter, чтобы открыть историю.",
            Style::default().fg(p.muted),
        ),
        width,
    ));
    rows.push(Line::default());
    rows.extend(markdown::wrap(
        Line::styled(
            "F3 — выбрать другую сессию    Ctrl+N — новая сессия    F5 — рабочая панель",
            Style::default().fg(p.accent),
        ),
        width,
    ));
    let top = (area.height / 3).min(4);
    put(
        frame,
        Rect {
            x: area.x + 2,
            y: area.y + top,
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(top),
        },
        Text::from(rows),
        p,
    );
}

fn welcome(frame: &mut Frame, app: &App, area: Rect, p: Palette) {
    let connected = app.connected();
    let no_hosts = app.servers().is_empty();
    let c = app.chat();
    let title = if c.is_some_and(|c| c.loading) {
        "Загружаю историю…"
    } else if no_hosts {
        "Добро пожаловать в Relay"
    } else if !connected {
        "Подключись к своему серверу"
    } else {
        "Что сделаем?"
    };
    let detail = if no_hosts {
        "F2 → Ctrl+A — добавь сервер. Сохранённые серверы Relay появятся здесь автоматически."
    } else if !connected {
        "F2 — выбрать хост · /connect — подключиться · F5 — рабочая панель. История остаётся на сервере."
    } else {
        "Продолжи сессию через F3, открой панель F5 или напиши новую задачу.\nClaude Code, Codex, AGY и собственный API работают с общей историей."
    };
    let mut rows = vec![
        Line::default(),
        Line::styled(
            title,
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Line::default(),
    ];
    rows.extend(markdown::wrap(
        Line::styled(detail.replace('\n', " "), Style::default().fg(p.muted)),
        area.width.saturating_sub(4) as usize,
    ));
    if app.active_backend_missing() {
        rows.push(Line::default());
        rows.extend(markdown::wrap(
            Line::styled(
                "Агент не установлен на сервере · введи /install",
                Style::default().fg(p.red).add_modifier(Modifier::BOLD),
            ),
            area.width.saturating_sub(4) as usize,
        ));
    }
    if connected {
        rows.push(Line::default());
        rows.extend(markdown::wrap(
            Line::styled(
                "/menu  панель    /backend  агент    /install  CLI    /lock  пароль    /help  команды",
                Style::default().fg(p.accent),
            ),
            area.width.saturating_sub(4) as usize,
        ));
    }
    let top = (area.height / 3).min(4);
    put(
        frame,
        Rect {
            x: area.x + 2,
            y: area.y + top,
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(top),
        },
        Text::from(rows),
        p,
    );
}

fn transcript(frame: &mut Frame, app: &mut App, view: &mut View, area: Rect, p: Palette) {
    let Some(key) = app.key() else {
        return;
    };
    let c = &app.chats[&key];
    let signature = (
        key.clone(),
        c.revision,
        c.messages.len(),
        area.width,
        c.expanded,
        app.theme().to_owned(),
        c.backend.clone(),
    );
    if view.cache.as_ref() != Some(&signature) {
        let mut rows = vec![];
        let width = area.width as usize;
        let mut results = BTreeMap::new();
        for message in &c.messages {
            if message.role == "tool_result" && !message.tool_id.is_empty() {
                results.insert(message.tool_id.as_str(), message);
            }
        }
        for message in &c.messages {
            match message.role.as_str() {
                "user" => {
                    rows.push(Line::from(vec![Span::styled(
                        "› Вы",
                        Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                    )]));
                    rows.extend(markdown::render(
                        &message.text,
                        width,
                        p.text,
                        p.accent,
                        p.muted,
                    ));
                    rows.push(Line::default());
                }
                "assistant" => {
                    if message.text.is_empty() {
                        continue;
                    }
                    rows.push(Line::styled(
                        match c.backend.as_str() {
                            "codex" => "● Codex",
                            "agy" => "● AGY",
                            "api" => "● API",
                            _ => "● Claude",
                        },
                        Style::default().fg(p.muted),
                    ));
                    rows.extend(markdown::render(
                        &message.text,
                        width,
                        p.text,
                        p.accent,
                        p.muted,
                    ));
                    rows.push(Line::default());
                }
                "tool" => {
                    let result = results.get(message.tool_id.as_str());
                    let failed = result.is_some_and(|r| r.failed);
                    let color = if failed { p.red } else { p.accent };
                    rows.extend(markdown::wrap(
                        Line::styled(
                            format!(
                                "╭─ {}  {}",
                                tool_name(&clean(&message.tool)),
                                if result.is_some() {
                                    if failed {
                                        "ошибка"
                                    } else {
                                        "готово"
                                    }
                                } else {
                                    "в работе"
                                }
                            ),
                            Style::default().fg(color),
                        ),
                        width,
                    ));
                    let command = message.input["command"]
                        .as_str()
                        .or_else(|| message.input["file_path"].as_str())
                        .or_else(|| message.input["pattern"].as_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| message.input.to_string());
                    for row in markdown::wrap(
                        Line::styled(clean(&command), Style::default().fg(p.text)),
                        width.saturating_sub(2),
                    ) {
                        let mut spans = vec![label("│ ", p.border)];
                        spans.extend(row.spans);
                        rows.push(Line::from(spans));
                    }
                    if let Some(result) = result {
                        let output = clean(&result.text);
                        let output: Vec<_> = output.lines().collect();
                        let count = if c.expanded {
                            output.len()
                        } else {
                            output.len().min(5)
                        };
                        if count < output.len() {
                            rows.push(Line::styled(
                                format!(
                                    "│ … ещё {} строк · Ctrl+O развернуть",
                                    output.len() - count
                                ),
                                Style::default().fg(p.muted),
                            ));
                        }
                        for line in output.iter().skip(output.len() - count) {
                            for row in markdown::wrap(
                                Line::styled(
                                    (*line).to_owned(),
                                    Style::default().fg(if failed { p.red } else { p.muted }),
                                ),
                                width.saturating_sub(2),
                            ) {
                                let mut spans = vec![label("│ ", p.border)];
                                spans.extend(row.spans);
                                rows.push(Line::from(spans));
                            }
                        }
                    }
                    rows.push(Line::styled("╰─", Style::default().fg(p.border)));
                    rows.push(Line::default());
                }
                "tool_result" => {}
                _ => {}
            }
        }
        view.lines = rows;
        view.cache = Some(signature);
    }
    let c = app.chats.get_mut(&key).unwrap();
    let maximum = view.lines.len().saturating_sub(area.height as usize);
    c.scroll = c.scroll.min(maximum);
    let top = maximum.saturating_sub(c.scroll);
    let visible: Vec<_> = view
        .lines
        .iter()
        .skip(top)
        .take(area.height as usize)
        .cloned()
        .collect();
    put(frame, area, Text::from(visible), p);
    if c.scroll > 0 && area.height > 1 {
        let hint = format!(" ↑ {} строк · Ctrl+End вниз ", c.scroll);
        let hint = fit(&hint, area.width as usize);
        put(
            frame,
            Rect {
                x: area.right() - hint.width() as u16,
                y: area.bottom() - 1,
                width: hint.width() as u16,
                height: 1,
            },
            Line::styled(hint, Style::default().fg(p.accent).bg(p.panel)),
            p,
        );
    }
}

fn status(frame: &mut Frame, app: &App, area: Rect, p: Palette) {
    if let Some(c) = app.chat() {
        let mut spans = vec![];
        if !c.error.is_empty() {
            spans.push(label(fit(&clean(&c.error), area.width as usize), p.red));
        } else if c.busy {
            let chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let i = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 120) as usize;
            spans.push(label(
                format!("{} {}", chars[i % chars.len()], c.status),
                p.accent,
            ));
            spans.push(label("  Ctrl+C остановить", p.muted));
        } else if c.loading {
            spans.push(label("Загрузка истории…", p.muted));
        } else if app.active_backend_missing() {
            spans.push(label(
                fit(
                    "Агент не установлен на сервере · введи /install",
                    area.width as usize,
                ),
                p.red,
            ));
        }
        if !c.queue.is_empty() {
            spans.push(label(
                format!(
                    "  ◷ {} в очереди{} · /queue",
                    c.queue.len(),
                    if c.paused { " · пауза" } else { "" }
                ),
                p.accent,
            ));
        }
        put(frame, area, Line::from(spans), p);
    }
}

fn composer(frame: &mut Frame, app: &App, area: Rect, p: Palette) {
    let title = app
        .chat()
        .map(|c| {
            let lock_tag = if c.locked && !c.unlocked {
                "🔒 закрыта · "
            } else if c.locked {
                "🔓 защищена · "
            } else {
                ""
            };
            format!(
                " {}{} · {} · {} · {} ",
                lock_tag,
                relay_desktop::model::backend_short_label(&c.backend),
                if c.model == "default" {
                    "модель: авто"
                } else {
                    &c.model
                },
                match c.mode.as_str() {
                    "auto" => "авто",
                    "manual" => "подтверждения",
                    "plan" => "план",
                    _ => "правки",
                },
                if c.effort == "default" {
                    "глубина: авто"
                } else {
                    &c.effort
                }
            )
        })
        .unwrap_or_else(|| " Сообщение ".into());
    let outer = block(
        Line::styled(
            fit(&title, area.width.saturating_sub(4) as usize),
            Style::default().fg(if app.popup.is_none() {
                p.accent
            } else {
                p.muted
            }),
        ),
        p,
        app.popup.is_none(),
    );
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let edit_area = Rect {
        x: inner.x + 1,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(1),
    };
    if let Some(c) = app.chat() {
        let placeholder = if c.is_locked_gate() {
            "🔒 Сессия закрыта · нажми Enter или введи /unlock"
        } else {
            "Напиши задачу…  /help  ·  F5 панель"
        };
        editor(
            frame,
            &c.editor,
            edit_area,
            p,
            false,
            app.popup.is_none(),
            placeholder,
        );
        if !c.files.is_empty() {
            let files = c
                .files
                .iter()
                .map(|f| clean(&f.file_name().unwrap_or_default().to_string_lossy()))
                .collect::<Vec<_>>()
                .join(" · ");
            put(
                frame,
                Rect {
                    y: inner.bottom() - 1,
                    height: 1,
                    ..edit_area
                },
                Line::styled(
                    format!(
                        "▧ {} · /clear-files",
                        fit(&files, inner.width.saturating_sub(20) as usize)
                    ),
                    Style::default().fg(p.accent),
                ),
                p,
            );
        }
    } else {
        put(
            frame,
            edit_area,
            Line::styled(
                "Выбери сервер: F2  ·  Панель: F5  ·  Добавить: F2 → Ctrl+A",
                Style::default().fg(p.muted),
            ),
            p,
        );
    }
}

fn editor(
    frame: &mut Frame,
    editor: &Editor,
    area: Rect,
    p: Palette,
    secret: bool,
    focus: bool,
    placeholder: &str,
) {
    let area = area.intersection(frame.area());
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (rows, (x, y)) = editor.visual(area.width as usize, secret);
    let offset = y.saturating_sub(area.height.saturating_sub(1) as usize);
    if editor.text.is_empty() {
        put(
            frame,
            area,
            Line::styled(
                fit(placeholder, area.width as usize),
                Style::default().fg(p.muted),
            ),
            p,
        );
    } else {
        put(
            frame,
            area,
            Text::from(
                rows.into_iter()
                    .skip(offset)
                    .take(area.height as usize)
                    .map(Line::raw)
                    .collect::<Vec<_>>(),
            ),
            p,
        );
    }
    if focus {
        frame.set_cursor_position((
            area.x + (x as u16).min(area.width - 1),
            area.y + (y - offset) as u16,
        ));
    }
}

fn workspace_telemetry(app: &App, width: usize, p: Palette) -> Line<'static> {
    let usage = app
        .active
        .as_ref()
        .and_then(|h| app.remote.get(h))
        .map(|r| &r.usage);
    if let Some(windows) = usage.and_then(|v| v["windows"].as_array()).filter(|w| !w.is_empty()) {
        let mut spans = vec![Span::styled(
            " Лимиты Claude: ",
            Style::default().fg(p.muted).bg(p.panel),
        )];
        let bar_len = if width >= 72 { 10usize } else { 5usize };
        for (idx, window) in windows.iter().take(2).enumerate() {
            if idx > 0 {
                spans.push(Span::styled("   ", Style::default().bg(p.panel)));
            }
            let name = window["label"]
                .as_str()
                .unwrap_or(if window["id"] == "five_hour" {
                    "5 часов"
                } else {
                    "Неделя"
                });
            spans.push(Span::styled(
                format!("{} ", clean(name)),
                Style::default().fg(p.text).bg(p.panel),
            ));
            if let Some(value) = window["utilization"].as_f64().filter(|v| v.is_finite()) {
                let value = value.clamp(0., 100.);
                let filled = ((value / 100.) * bar_len as f64).round() as usize;
                spans.push(Span::styled(
                    "━".repeat(filled),
                    Style::default()
                        .fg(if value >= 95. { p.red } else { p.accent })
                        .bg(p.panel),
                ));
                spans.push(Span::styled(
                    "─".repeat(bar_len.saturating_sub(filled)),
                    Style::default().fg(p.border).bg(p.panel),
                ));
                spans.push(Span::styled(
                    format!(" {value:>3.0}%"),
                    Style::default().fg(p.muted).bg(p.panel),
                ));
            } else {
                spans.push(Span::styled(
                    "нет данных",
                    Style::default().fg(p.muted).bg(p.panel),
                ));
            }
        }
        spans.push(Span::styled(" ", Style::default().bg(p.panel)));
        Line::from(spans)
    } else {
        Line::styled(
            fit(" Лимиты Claude: пока недоступны · /refresh обновить ", width),
            Style::default().fg(p.muted).bg(p.panel),
        )
    }
}

fn popup_view(
    frame: &mut Frame,
    app: &App,
    popup: &Popup,
    p: Palette,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let outer = frame.area();
    let width = outer.width.saturating_sub(4).min(match popup {
        Popup::Workspace { .. } => 96,
        _ => 86,
    });
    let height = outer.height.saturating_sub(2).min(match popup {
        Popup::Workspace { .. } => 34,
        Popup::Picker { .. } | Popup::Help { .. } => 28,
        Popup::Permission { .. } => 24,
        Popup::Queue { .. } => 20,
        _ => 14,
    });
    if width < 6 || height < 4 {
        return;
    }
    let area = Rect {
        x: outer.x + (outer.width - width) / 2,
        y: outer.y + (outer.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, area);
    let title = match popup {
        Popup::Workspace { .. } => "Рабочая панель · Relay",
        Popup::Picker { kind, .. } => match kind {
            Picker::Hosts => "Серверы",
            Picker::Sessions => "Сессии",
            Picker::Models => "Модель",
            Picker::Backends => "Агент сессии",
            Picker::Install => "Установка и обновление CLI",
            Picker::Themes => "Оформление",
            Picker::Modes => "Разрешения",
            Picker::Efforts => "Глубина",
            Picker::Commands => "Команды",
        },
        Popup::Password { .. } => "Пароль SSH",
        Popup::Trust { .. } => "Ключ сервера",
        Popup::Field { kind, .. } => field_title(kind),
        Popup::Confirm { .. } => "Подтверждение",
        Popup::Permission { .. } => "Нужно решение",
        Popup::Queue { .. } => "Очередь сообщений",
        Popup::Help { .. } => "Relay TUI · справка",
    };
    let border = block(format!(" {title} "), p, true);
    let inner = border.inner(area);
    frame.render_widget(border, area);
    let inner = Rect {
        x: inner.x + 1,
        y: inner.y + 1,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(2),
    };
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    match popup {
        Popup::Workspace { section, index } => {
            let items = app.workspace_items();
            let clamped = (*index).min(items.len().saturating_sub(1));
            let active_sec = items.get(clamped).map_or(*section, |it| it.section);
            let mut y_cursor = inner.y;

            if inner.height >= 6 {
                let tabs = [
                    (0usize, " 1 · Серверы "),
                    (1usize, " 2 · Сессии и защита "),
                    (2usize, " 3 · Агенты и CLI "),
                ];
                let mut tab_spans = vec![];
                let mut x_tab = inner.x;
                for (sec_id, tab_label) in tabs {
                    let w = tab_label.width() as u16;
                    if x_tab + w <= inner.right() {
                        hits.push((
                            Rect {
                                x: x_tab,
                                y: y_cursor,
                                width: w,
                                height: 1,
                            },
                            Hit::WorkspaceTab(sec_id),
                        ));
                    }
                    let selected_tab = active_sec == sec_id;
                    tab_spans.push(Span::styled(
                        tab_label,
                        if selected_tab {
                            Style::default()
                                .fg(p.accent)
                                .bg(p.surface)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(p.muted)
                        },
                    ));
                    tab_spans.push(Span::raw("  "));
                    x_tab = x_tab.saturating_add(w + 2);
                }
                put(
                    frame,
                    Rect {
                        x: inner.x,
                        y: y_cursor,
                        width: inner.width,
                        height: 1,
                    },
                    Line::from(tab_spans),
                    p,
                );
                y_cursor += 1;
            }

            if inner.height >= 12 {
                let telem = workspace_telemetry(app, inner.width as usize, p);
                put(
                    frame,
                    Rect {
                        x: inner.x,
                        y: y_cursor,
                        width: inner.width,
                        height: 1,
                    },
                    telem,
                    p,
                );
                y_cursor += 2;
            } else if inner.height >= 8 {
                y_cursor += 1;
            }

            let footer_rows = u16::from(inner.height >= 4);
            let list_height = inner
                .bottom()
                .saturating_sub(y_cursor + footer_rows)
                .max(1) as usize;

            let mut visual: Vec<(Option<usize>, Line<'static>)> = vec![];
            let mut last_sec = None;
            let mut selected_visual = 0usize;
            for (i, item) in items.iter().enumerate() {
                if last_sec != Some(item.section) {
                    if last_sec.is_some() && inner.height >= 16 {
                        visual.push((None, Line::default()));
                    }
                    let header_text = match item.section {
                        0 => "СЕРВЕРЫ  ·  F2  ·  Ctrl+A добавить",
                        1 => "СЕССИИ И ЗАЩИТА  ·  F3  ·  Ctrl+N новая  ·  /lock пароль",
                        _ => "АГЕНТЫ, ЛИМИТЫ И CLI НА СЕРВЕРЕ  ·  /backend  ·  /install",
                    };
                    visual.push((
                        None,
                        Line::styled(
                            fit(header_text, inner.width as usize),
                            Style::default().fg(p.muted).add_modifier(Modifier::BOLD),
                        ),
                    ));
                    last_sec = Some(item.section);
                }
                let selected = i == clamped;
                if selected {
                    selected_visual = visual.len();
                }
                let prefix = if selected {
                    "› "
                } else if item.active {
                    "• "
                } else {
                    "  "
                };
                let max_w = inner.width as usize;
                let clean_label = clean(&item.label);
                let clean_detail = clean(&item.detail);
                let label_budget = max_w.saturating_sub(2).min(42.max(max_w / 2));
                let shown_label = fit(&clean_label, label_budget);
                let used = 2 + shown_label.width();
                let rem = max_w.saturating_sub(used + 2);
                let row_bg = if selected { p.surface } else { p.base };
                let mut spans = vec![
                    Span::styled(
                        prefix,
                        Style::default()
                            .fg(if selected {
                                p.accent
                            } else if item.active {
                                p.green
                            } else {
                                p.muted
                            })
                            .bg(row_bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        shown_label,
                        Style::default()
                            .fg(if selected || item.active {
                                p.accent
                            } else {
                                p.text
                            })
                            .bg(row_bg)
                            .add_modifier(if selected || item.active {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                ];
                if rem >= 6 && !clean_detail.is_empty() {
                    let shown_detail = fit(&clean_detail, rem);
                    let pad = max_w.saturating_sub(used + 2 + shown_detail.width());
                    spans.push(Span::styled(
                        format!("  {}{}", shown_detail, " ".repeat(pad)),
                        Style::default()
                            .fg(if selected { p.text } else { p.muted })
                            .bg(row_bg),
                    ));
                } else if selected {
                    let pad = max_w.saturating_sub(used);
                    spans.push(Span::styled(" ".repeat(pad), Style::default().bg(row_bg)));
                }
                visual.push((Some(i), Line::from(spans)));
            }

            let target_top = if selected_visual > 0 && visual[selected_visual - 1].0.is_none() {
                selected_visual - 1
            } else {
                selected_visual
            };
            let offset = if selected_visual >= list_height {
                (selected_visual + 1).saturating_sub(list_height)
            } else {
                0
            }
            .min(target_top);

            for (row_offset, (item_idx, line)) in visual
                .into_iter()
                .skip(offset)
                .take(list_height)
                .enumerate()
            {
                let row_rect = Rect {
                    x: inner.x,
                    y: y_cursor + row_offset as u16,
                    width: inner.width,
                    height: 1,
                };
                if let Some(idx) = item_idx {
                    hits.push((row_rect, Hit::WorkspaceItem(idx)));
                }
                put(frame, row_rect, line, p);
            }

            if footer_rows > 0 {
                put(
                    frame,
                    Rect {
                        x: inner.x,
                        y: inner.bottom() - 1,
                        width: inner.width,
                        height: 1,
                    },
                    Line::styled(
                        fit(
                            "↑↓ выбор · Tab раздел · Enter выполнить · Ctrl+N новая · Ctrl+A сервер · Esc закрыть",
                            inner.width as usize,
                        ),
                        Style::default().fg(p.muted),
                    ),
                    p,
                );
            }
        }
        Popup::Picker {
            kind,
            search,
            index,
        } => {
            editor(
                frame,
                search,
                Rect { height: 1, ..inner },
                p,
                false,
                true,
                "Поиск…",
            );
            let items = app.items(*kind, &search.text);
            let rows = inner.height.saturating_sub(4) as usize / 2;
            let index = (*index).min(items.len().saturating_sub(1));
            let offset = index.saturating_sub(rows.saturating_sub(1));
            let mut lines = vec![];
            for (i, item) in items.iter().enumerate().skip(offset).take(rows.max(1)) {
                let selected = i == index;
                lines.push(Line::styled(
                    format!(
                        "{} {}",
                        if selected { "›" } else { " " },
                        fit(&clean(&item.label), inner.width as usize - 2)
                    ),
                    Style::default()
                        .fg(if selected { p.accent } else { p.text })
                        .add_modifier(if selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ));
                lines.push(Line::styled(
                    format!("  {}", fit(&clean(&item.detail), inner.width as usize - 2)),
                    Style::default().fg(p.muted),
                ));
            }
            if items.is_empty() {
                lines.push(Line::styled(
                    if *kind == Picker::Hosts {
                        "Серверов пока нет. Ctrl+A — добавить."
                    } else {
                        "Ничего не найдено. /refresh обновит список."
                    },
                    Style::default().fg(p.muted),
                ));
            }
            put(
                frame,
                Rect {
                    y: inner.y + 2,
                    height: inner.height.saturating_sub(3),
                    ..inner
                },
                Text::from(lines),
                p,
            );
            put(
                frame,
                Rect {
                    y: inner.bottom() - 1,
                    height: 1,
                    ..inner
                },
                Line::styled(
                    fit(
                        if *kind == Picker::Hosts {
                            "↑↓ выбрать · Enter подключиться · Ctrl+A добавить · Esc назад"
                        } else if *kind == Picker::Install {
                            "↑↓ выбрать · Enter установить / обновить · Esc назад"
                        } else {
                            "↑↓ выбрать · Enter открыть · Esc назад"
                        },
                        inner.width as usize,
                    ),
                    Style::default().fg(p.muted),
                ),
                p,
            );
        }
        Popup::Password {
            server,
            editor: input,
        } => {
            put(
                frame,
                inner,
                Text::from(vec![
                    Line::styled(clean(&server.name), Style::default().fg(p.text)),
                    Line::styled(
                        format!(
                            "{}@{}:{}",
                            clean(&server.user),
                            clean(&server.host),
                            server.port
                        ),
                        Style::default().fg(p.muted),
                    ),
                    Line::default(),
                    Line::styled(
                        "Пароль не отображается и не сохраняется в TUI-файле.",
                        Style::default().fg(p.muted),
                    ),
                ]),
                p,
            );
            editor(
                frame,
                input,
                Rect {
                    y: inner.bottom().saturating_sub(2),
                    height: 1,
                    ..inner
                },
                p,
                true,
                true,
                "Пароль",
            );
            put(
                frame,
                Rect {
                    y: inner.bottom() - 1,
                    height: 1,
                    ..inner
                },
                Line::styled(
                    "Enter подключиться · Esc отменить",
                    Style::default().fg(p.muted),
                ),
                p,
            );
        }
        Popup::Trust {
            server,
            fingerprint,
            ..
        } => {
            let name = app
                .servers()
                .into_iter()
                .find(|s| s.id == *server)
                .map(|s| s.name)
                .unwrap_or_default();
            let text = format!("Первое подключение к {name}.\n\nОтпечаток SSH:\n{fingerprint}\n\nСверь его с данными сервера. Изменённый сохранённый ключ автоматически не принимается.");
            put(
                frame,
                inner,
                Text::from(markdown::render(
                    &text,
                    inner.width as usize,
                    p.text,
                    p.accent,
                    p.muted,
                )),
                p,
            );
            put(
                frame,
                Rect {
                    y: inner.bottom() - 1,
                    height: 1,
                    ..inner
                },
                Line::styled(
                    "Ctrl+Y доверять · Ctrl+N / Esc отменить",
                    Style::default().fg(p.accent),
                ),
                p,
            );
        }
        Popup::Confirm { message, .. } => {
            put(
                frame,
                inner,
                Text::from(markdown::render(
                    message,
                    inner.width as usize,
                    p.text,
                    p.accent,
                    p.muted,
                )),
                p,
            );
            put(
                frame,
                Rect {
                    y: inner.bottom() - 1,
                    height: 1,
                    ..inner
                },
                Line::styled(
                    "Ctrl+Y подтвердить · Ctrl+N / Esc отменить",
                    Style::default().fg(p.accent),
                ),
                p,
            );
        }
        Popup::Field {
            kind,
            editor: input,
            error,
        } => {
            let description = match kind {
                Field::Attach => "Путь к файлу на этом компьютере. Пробелы допустимы.",
                Field::Cwd => "Папка на сервере. После проверки создастся новая сессия.",
                Field::Prompt => "Инструкции для этой сессии. Alt+Enter добавляет строку.",
                Field::DownloadLocal { .. } => "Куда сохранить файл на этом компьютере.",
                Field::Add { .. } => {
                    "Профиль сохранится в tui-state.json. Пароль вводится отдельно."
                }
                Field::Api { stage: 2, .. } => {
                    "Ключ хранится отдельно от настроек, как в графическом Relay."
                }
                Field::SessionLockSet {
                    has_existing: true,
                    stage: 0,
                    ..
                } => "Введи действующий пароль этой сессии для его смены.",
                Field::SessionLockSet { .. } => {
                    "Пароль хранится в виде PBKDF2-хеша на сервере и никогда не пишется на диск ПК."
                }
                Field::SessionUnlock { .. } => {
                    "Сессия защищена паролем. Введи пароль для загрузки истории."
                }
                Field::SessionLockRemove { .. } => {
                    "Введи текущий пароль, чтобы снять защиту с этой сессии."
                }
                _ => "Enter сохранить · Esc отменить",
            };
            let description = markdown::wrap(
                Line::styled(description, Style::default().fg(p.muted)),
                inner.width as usize,
            );
            let n = description
                .len()
                .min(inner.height.saturating_sub(3) as usize)
                .min(3) as u16;
            put(
                frame,
                Rect { height: n, ..inner },
                Text::from(description),
                p,
            );
            let secret = matches!(
                kind,
                Field::Api { stage: 2, .. }
                    | Field::SessionLockSet { .. }
                    | Field::SessionUnlock { .. }
                    | Field::SessionLockRemove { .. }
            );
            let placeholder = if secret {
                "Пароль…"
            } else {
                "Введи значение…"
            };
            let gap = u16::from(n > 0);
            let field = Rect {
                y: inner.y + n + gap,
                height: inner.height.saturating_sub(n + gap + 2).max(1),
                ..inner
            };
            editor(frame, input, field, p, secret, true, placeholder);
            if !error.is_empty() && inner.height >= 3 {
                put(
                    frame,
                    Rect {
                        y: inner.bottom().saturating_sub(2),
                        height: 1,
                        ..inner
                    },
                    Line::styled(
                        fit(&clean(error), inner.width as usize),
                        Style::default().fg(p.red),
                    ),
                    p,
                );
            }
            put(
                frame,
                Rect {
                    y: inner.bottom() - 1,
                    height: 1,
                    ..inner
                },
                Line::styled(
                    "Enter продолжить · Esc отменить",
                    Style::default().fg(p.accent),
                ),
                p,
            );
        }
        Popup::Permission {
            server,
            sid,
            permission,
            answers,
            selected,
            error,
        } => {
            let title = app
                .chats
                .get(&super::app::chat_key(server, sid))
                .map(|c| c.title.as_str())
                .unwrap_or("");
            let server = app
                .servers()
                .into_iter()
                .find(|s| s.id == *server)
                .map(|s| s.name)
                .unwrap_or_default();
            let mut lines = vec![
                Line::from(vec![label(format!("{server} · {title}"), p.muted)]),
                Line::styled(
                    clean(tool_name(&permission.tool)),
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                ),
                Line::default(),
            ];
            if let Some(questions) = permission.input["questions"].as_array() {
                for (i, question) in questions.iter().enumerate() {
                    let question = question["question"].as_str().unwrap_or("");
                    lines.extend(markdown::wrap(
                        Line::styled(
                            format!(
                                "{} {}",
                                if i == *selected { "›" } else { " " },
                                clean(question)
                            ),
                            Style::default().fg(p.text),
                        ),
                        inner.width as usize,
                    ));
                    if let Some(options) = permission.input["questions"][i]["options"].as_array() {
                        lines.extend(markdown::wrap(
                            Line::styled(
                                options
                                    .iter()
                                    .filter_map(|v| v["label"].as_str())
                                    .map(clean)
                                    .collect::<Vec<_>>()
                                    .join(" · "),
                                Style::default().fg(p.muted),
                            ),
                            inner.width as usize,
                        ));
                    }
                    lines.push(Line::styled(
                        format!(
                            "  {}",
                            answers.get(i).map(|a| a.text.as_str()).unwrap_or("")
                        ),
                        Style::default().fg(p.accent),
                    ));
                }
            } else {
                let input = permission.input["command"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        serde_json::to_string_pretty(&permission.input).unwrap_or_default()
                    });
                lines.extend(markdown::render(
                    &input,
                    inner.width as usize,
                    p.text,
                    p.accent,
                    p.muted,
                ));
            }
            put(
                frame,
                Rect {
                    height: inner.height.saturating_sub(3),
                    ..inner
                },
                Text::from(lines),
                p,
            );
            if !error.is_empty() {
                put(
                    frame,
                    Rect {
                        y: inner.bottom() - 3,
                        height: 1,
                        ..inner
                    },
                    Line::styled(clean(error), Style::default().fg(p.red)),
                    p,
                );
            }
            put(
                frame,
                Rect {
                    y: inner.bottom() - 2,
                    height: 2,
                    ..inner
                },
                Text::from(vec![
                    Line::styled(
                        if answers.is_empty() {
                            "Ctrl+Y разрешить · Ctrl+N отклонить"
                        } else {
                            "Введи ответ · Tab следующий вопрос · Ctrl+Y отправить"
                        },
                        Style::default().fg(p.accent),
                    ),
                    Line::styled(
                        "Esc отклонить. Enter сам по себе ничего не разрешает.",
                        Style::default().fg(p.muted),
                    ),
                ]),
                p,
            );
        }
        Popup::Queue { key, index } => {
            let mut lines = vec![];
            if let Some(c) = app.chats.get(key) {
                lines.push(Line::styled(
                    if c.paused {
                        "Очередь на паузе"
                    } else {
                        "Следующее сообщение уйдёт после ответа"
                    },
                    Style::default().fg(p.accent),
                ));
                lines.push(Line::default());
                let count = inner.height.saturating_sub(5) as usize;
                let offset = index.saturating_sub(count.saturating_sub(1));
                for (i, prompt) in c.queue.iter().enumerate().skip(offset).take(count) {
                    lines.push(Line::styled(
                        format!(
                            "{} {}. {}{}",
                            if i == *index { "›" } else { " " },
                            i + 1,
                            fit(
                                &clean(&prompt.text).replace('\n', " "),
                                inner.width.saturating_sub(13) as usize
                            ),
                            if prompt.files.is_empty() {
                                String::new()
                            } else {
                                format!(" ▧{}", prompt.files.len())
                            }
                        ),
                        Style::default().fg(if i == *index { p.accent } else { p.text }),
                    ));
                }
                if c.queue.is_empty() {
                    lines.push(Line::styled("Очередь пуста", Style::default().fg(p.muted)));
                }
            }
            put(frame, inner, Text::from(lines), p);
            put(
                frame,
                Rect {
                    y: inner.bottom() - 2,
                    height: 2,
                    ..inner
                },
                Text::from(vec![
                    Line::styled(
                        "↑↓ выбрать · E в черновик · D удалить",
                        Style::default().fg(p.muted),
                    ),
                    Line::styled(
                        "P пауза · R продолжить · Esc назад",
                        Style::default().fg(p.accent),
                    ),
                ]),
                p,
            );
        }
        Popup::Help { scroll } => {
            let text = "Enter — отправить или добавить в очередь\nAlt+Enter / Shift+Enter / Ctrl+J — новая строка\nCtrl+C — остановить ответ; Ctrl+Q — выйти\nF2 — серверы; Ctrl+A в списке — добавить сервер\nF3 — сессии; F4 — 24 темы оформления; Ctrl+N — новая сессия\nF5 / Ctrl+B — рабочая панель (серверы, сессии, лимиты, CLI, защита)\nCtrl+O — полный вывод команд\nPgUp/PgDn — история; Ctrl+End — к последнему сообщению\nCtrl+P / Alt+N — предыдущие отправленные сообщения\nСтрелки / Home / End — редактирование; Ctrl+W — убрать слово\nTab — дополнить slash-команду; Shift+мышь — выделение в терминале\n\nСессии, парольная защита и авторизация общие с графическим Relay.\nПри запуске восстановленная очередь всегда на паузе.\nБраузер и управление кластером остаются в графическом Relay.\n";
            let mut lines = markdown::render(text, inner.width as usize, p.text, p.accent, p.muted);
            lines.push(Line::default());
            for (command, description) in COMMANDS {
                lines.extend(markdown::wrap(
                    Line::from(vec![
                        label(format!("{command:<16} "), p.accent),
                        label(*description, p.text),
                    ]),
                    inner.width as usize,
                ));
            }
            let top = (*scroll).min(lines.len().saturating_sub(inner.height as usize));
            put(
                frame,
                inner,
                Text::from(
                    lines
                        .into_iter()
                        .skip(top)
                        .take(inner.height as usize)
                        .collect::<Vec<_>>(),
                ),
                p,
            );
        }
    }
}

pub fn field_title(field: &Field) -> &str {
    match field {
        Field::Attach => "Прикрепить файл",
        Field::Cwd => "Рабочая папка",
        Field::Rename => "Название сессии",
        Field::Prompt => "Инструкции сессии",
        Field::Resume => "ID сессии",
        Field::DownloadRemote => "Путь на сервере",
        Field::DownloadLocal { .. } => "Сохранить на компьютере",
        Field::Add { stage, .. } => match stage {
            0 => "Название сервера",
            1 => "IP или домен сервера",
            2 => "Пользователь SSH",
            3 => "Порт SSH",
            _ => "Рабочая папка на сервере",
        },
        Field::Api { stage, .. } => match stage {
            0 => "Базовый URL API",
            1 => "Модель API",
            _ => "Ключ API",
        },
        Field::SessionLockSet {
            has_existing: true,
            stage: 0,
            ..
        } => "Текущий пароль сессии",
        Field::SessionLockSet { .. } => "Новый пароль сессии",
        Field::SessionUnlock { .. } => "Разблокировать сессию",
        Field::SessionLockRemove { .. } => "Снять пароль с сессии",
    }
}

pub fn snapshot(app: &mut App, width: u16, height: u16) -> anyhow::Result<ratatui::buffer::Buffer> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))?;
    terminal.draw(|f| draw(f, app, &mut View::default()))?;
    Ok(terminal.backend().buffer().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn buffer_text(b: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..b.area.height {
            for x in 0..b.area.width {
                out.push_str(b[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn all_views_render_in_small_and_large_terminals() {
        for (w, h) in [(32, 10), (60, 18), (80, 24), (120, 40), (180, 55)] {
            let mut app = App::new(true).unwrap();
            app.demo();
            let b = snapshot(&mut app, w, h).unwrap();
            assert_eq!(b.area.width, w);
            for (x, y) in [(w - 1, h - 1), (0, 0)] {
                assert!(b.cell((x, y)).is_some());
            }
            for kind in [
                Picker::Hosts,
                Picker::Sessions,
                Picker::Themes,
                Picker::Install,
            ] {
                app.picker(kind);
                snapshot(&mut app, w, h).unwrap();
            }
            app.open_workspace();
            snapshot(&mut app, w, h).unwrap();
            app.popup = Some(Popup::Help { scroll: 0 });
            snapshot(&mut app, w, h).unwrap();
        }
    }

    #[test]
    fn main_screen_has_no_permanent_sidebar_and_workspace_popup_renders_overview() {
        for (w, h) in [(120, 40), (180, 55)] {
            let mut app = App::new(true).unwrap();
            app.demo();
            let main_buf = snapshot(&mut app, w, h).unwrap();
            let main_txt = buffer_text(&main_buf);
            assert!(
                !main_txt.contains("СЕРВЕРЫ"),
                "Permanent sidebar servers header must not appear on main screen ({w}x{h})"
            );
            assert!(
                !main_txt.contains("ЛИМИТЫ"),
                "Permanent sidebar limits must not appear on main screen ({w}x{h})"
            );
            assert!(main_txt.contains("F5 Меню"));

            app.open_workspace();
            let ws_buf = snapshot(&mut app, w, h).unwrap();
            let ws_txt = buffer_text(&ws_buf);
            assert!(ws_txt.contains("Рабочая панель · Relay"));
            assert!(ws_txt.contains("Production"));
            assert!(ws_txt.contains("Новая сессия"));
            assert!(ws_txt.contains("Лимиты Claude"));
            assert!(ws_txt.contains("Antigravity (agy)"));
        }
    }

    #[test]
    fn all_24_shared_themes_render_cleanly_with_distinct_palettes() {
        assert_eq!(relay_desktop::model::THEME_SPECS.len(), 24);
        let mut app = App::new(true).unwrap();
        app.demo();
        for &(id, _, is_light, spec_colors) in &relay_desktop::model::THEME_SPECS {
            app.command(&format!("/theme {id}"));
            assert_eq!(app.theme(), id);
            let p = Palette::new(app.theme());
            assert_eq!(p.base, rgb(spec_colors[0]));
            assert_eq!(p.panel, rgb(spec_colors[1]));
            assert_eq!(p.surface, rgb(spec_colors[3]));
            assert_eq!(p.border, rgb(spec_colors[4]));
            assert_eq!(p.text, rgb(spec_colors[5]));
            assert_eq!(p.muted, rgb(spec_colors[6]));
            assert_eq!(p.accent, rgb(spec_colors[7]));
            if is_light {
                assert_eq!(p.red, Color::Rgb(185, 48, 56));
            } else {
                assert_eq!(p.red, Color::Rgb(235, 118, 128));
            }
            let b = snapshot(&mut app, 100, 30).unwrap();
            for cell in b.content {
                assert!(!cell.symbol().chars().any(char::is_control));
            }
        }
    }

    #[test]
    fn locked_session_hides_cached_messages_and_masks_password_input() {
        let mut app = App::new(true).unwrap();
        app.demo();
        {
            let c = app.chat_mut().unwrap();
            c.messages.push(relay_desktop::model::Message {
                role: "assistant".into(),
                text: "Секретный ответ в истории".into(),
                ..Default::default()
            });
            c.locked = true;
            c.unlocked = false;
        }
        let locked_buf = snapshot(&mut app, 100, 30).unwrap();
        let locked_txt = buffer_text(&locked_buf);
        assert!(locked_txt.contains("Сессия защищена паролем"));
        assert!(!locked_txt.contains("Секретный ответ в истории"));

        app.command("/unlock");
        if let Some(Popup::Field { editor, .. }) = &mut app.popup {
            editor.insert("super-secret-pw");
        }
        let pw_buf = snapshot(&mut app, 100, 30).unwrap();
        let pw_txt = buffer_text(&pw_buf);
        assert!(!pw_txt.contains("super-secret-pw"));
        assert!(pw_txt.contains("••••••"));
    }

    #[test]
    fn missing_backend_renders_install_hint() {
        let mut app = App::new(true).unwrap();
        app.demo();
        app.chat_mut().unwrap().messages.clear();
        app.chat_mut().unwrap().backend = "agy".into();
        let b = snapshot(&mut app, 100, 30).unwrap();
        let txt = buffer_text(&b);
        assert!(txt.contains("Агент не установлен на сервере · введи /install"));
    }

    #[test]
    fn remote_output_cannot_emit_escape_sequences() {
        let mut app = App::new(true).unwrap();
        app.demo();
        app.chat_mut()
            .unwrap()
            .messages
            .push(relay_desktop::model::Message {
                role: "assistant".into(),
                text: "\u{1b}]52;c;owned\u{7}\u{1b}[31mVisible".into(),
                ..Default::default()
            });
        let b = snapshot(&mut app, 100, 40).unwrap();
        for cell in b.content {
            assert!(!cell.symbol().chars().any(char::is_control));
        }
    }
}
