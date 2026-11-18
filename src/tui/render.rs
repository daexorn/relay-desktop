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

#[derive(Clone, Copy)]
struct Palette {
    base: Color,
    panel: Color,
    border: Color,
    text: Color,
    muted: Color,
    accent: Color,
    red: Color,
}
impl Palette {
    fn new(theme: &str) -> Self {
        match theme {
            "paper" => Self {
                base: Color::Rgb(242, 239, 232),
                panel: Color::Rgb(233, 229, 220),
                border: Color::Rgb(192, 190, 179),
                text: Color::Rgb(41, 48, 50),
                muted: Color::Rgb(111, 116, 111),
                accent: Color::Rgb(40, 112, 102),
                red: Color::Rgb(171, 63, 64),
            },
            "midnight" => Self {
                base: Color::Rgb(10, 17, 30),
                panel: Color::Rgb(18, 29, 47),
                border: Color::Rgb(39, 57, 80),
                text: Color::Rgb(221, 233, 246),
                muted: Color::Rgb(119, 144, 170),
                accent: Color::Rgb(130, 184, 241),
                red: Color::Rgb(225, 139, 147),
            },
            _ => Self {
                base: Color::Rgb(16, 20, 26),
                panel: Color::Rgb(21, 27, 35),
                border: Color::Rgb(42, 54, 67),
                text: Color::Rgb(230, 237, 243),
                muted: Color::Rgb(141, 156, 171),
                accent: Color::Rgb(131, 196, 179),
                red: Color::Rgb(223, 140, 144),
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
            if app.connected() { p.accent } else { p.muted },
        ));
        head.push(label(
            fit(&server.name, (area.width as usize).saturating_sub(52)),
            p.text,
        ));
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
    let controls = " F2 Хосты   F3 Сессии ";
    if area.width >= 76 {
        let r = Rect {
            x: header.right() - controls.width() as u16 - 2,
            y: header.y + 1,
            width: controls.width() as u16,
            height: 1,
        };
        put(
            frame,
            r,
            Line::styled(controls, Style::default().fg(p.accent)),
            p,
        );
        app.hits.push((Rect { width: 11, ..r }, Hit::Hosts));
        app.hits.push((
            Rect {
                x: r.x + 11,
                width: r.width - 11,
                ..r
            },
            Hit::Sessions,
        ));
    }
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(p.border)),
        header,
    );
    let body = layout[1];
    let show_sidebar = app.sidebar && body.width >= 108;
    let content = if show_sidebar {
        let columns = Layout::horizontal([
            Constraint::Length(28),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(body);
        sidebar(frame, app, columns[0], p);
        columns[2]
    } else {
        body
    };
    let content = Rect {
        x: content.x + 1,
        y: content.y,
        width: content.width.saturating_sub(2),
        height: content.height,
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
    let title = app
        .chat()
        .map(|c| c.title.as_str())
        .unwrap_or("Рабочее пространство");
    put(
        frame,
        Rect {
            height: 1,
            ..sections[0]
        },
        Line::styled(
            clean(title),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
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
                label(fit(&clean(&c.cwd), content.width as usize - 2), p.muted),
            ]),
            p,
        );
    }
    app.chat_rect = sections[1];
    if app.chat().is_some_and(|c| !c.messages.is_empty()) {
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
    let hint = if app.popup.is_some() {
        " Esc закрыть   ↑↓ выбор   Enter открыть "
    } else if area.width < 80 {
        " Enter отправить · Alt+Enter строка · F1 справка "
    } else {
        " Enter отправить   Alt+Enter строка   Ctrl+N новая   Ctrl+O вывод   PgUp/PgDn история   F1 справка "
    };
    put(
        frame,
        layout[2],
        Line::styled(hint, Style::default().fg(p.muted)),
        p,
    );
    if let Some(popup) = &app.popup {
        popup_view(frame, app, popup, p);
    }
}
fn sidebar(frame: &mut Frame, app: &mut App, area: Rect, p: Palette) {
    frame.render_widget(Block::default().style(Style::default().bg(p.panel)), area);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width - 2,
        height: area.height.saturating_sub(2),
    };
    let mut y = inner.y;
    put(
        frame,
        Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 1,
        },
        Line::from(vec![
            label("СЕРВЕРЫ", p.muted),
            label("            F2", p.accent),
        ]),
        p,
    );
    y += 2;
    for server in app.servers().into_iter().take(3) {
        if y + 2 >= inner.bottom() {
            break;
        }
        let selected = app.active.as_deref() == Some(&server.id);
        let r = app.remote.get(&server.id);
        let text = format!(
            "{} {} {}",
            if selected { "▎" } else { " " },
            if r.is_some_and(|r| r.connected) {
                "●"
            } else {
                "○"
            },
            fit(&clean(&server.name), inner.width as usize - 6)
        );
        put(
            frame,
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            },
            Line::styled(
                text,
                Style::default().fg(if selected { p.accent } else { p.text }),
            ),
            p,
        );
        app.hits.push((
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            },
            Hit::Host(server.id),
        ));
        y += 1;
    }
    y += 1;
    let new = Rect {
        x: inner.x,
        y,
        width: inner.width,
        height: 1,
    };
    put(
        frame,
        new,
        Line::styled("＋ Новая сессия     Ctrl+N", Style::default().fg(p.accent)),
        p,
    );
    app.hits.push((new, Hit::New));
    y += 2;
    put(
        frame,
        Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 1,
        },
        Line::styled("СЕССИИ                  F3", Style::default().fg(p.muted)),
        p,
    );
    y += 2;
    let max = (inner.bottom().saturating_sub(y + 7) / 3).max(1) as usize;
    let sessions = app
        .active
        .as_ref()
        .and_then(|h| app.remote.get(h))
        .map(|r| r.sessions.clone())
        .unwrap_or_default();
    for s in sessions.into_iter().take(max) {
        if y + 2 >= inner.bottom() {
            break;
        }
        let selected = app.chat().is_some_and(|c| c.sid == s.id);
        let color = if selected { p.accent } else { p.text };
        put(
            frame,
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            },
            Line::styled(
                format!(
                    "{}{}",
                    if selected { "▎ " } else { "  " },
                    fit(&clean(&s.title), inner.width as usize - 2)
                ),
                Style::default().fg(color),
            ),
            p,
        );
        let sub = match s.backend.as_str() {
            "codex" => format!("Codex · {}", clean(&s.cwd)),
            "agy" => format!("AGY · {}", clean(&s.cwd)),
            "api" => format!("API · {}", clean(&s.cwd)),
            _ => clean(&s.cwd),
        };
        put(
            frame,
            Rect {
                x: inner.x + 2,
                y: y + 1,
                width: inner.width - 2,
                height: 1,
            },
            Line::styled(
                fit(&sub, inner.width as usize - 2),
                Style::default().fg(p.muted),
            ),
            p,
        );
        app.hits.push((
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 2,
            },
            Hit::Session(s),
        ));
        y += 3;
    }
    if inner.height >= 18 {
        usage(
            frame,
            app,
            Rect {
                x: inner.x,
                y: inner.bottom() - 6,
                width: inner.width,
                height: 6,
            },
            p,
        );
    }
}
fn usage(frame: &mut Frame, app: &App, area: Rect, p: Palette) {
    let active_backend = app
        .chat()
        .map(|c| c.backend.as_str())
        .or_else(|| app.server().map(|_| "claude"))
        .unwrap_or("claude");
    if active_backend != "claude" {
        let remote = app.active.as_ref().and_then(|h| app.remote.get(h));
        let ver = remote
            .and_then(|r| r.backends[active_backend]["version"].as_str())
            .unwrap_or("");
        let name = relay_desktop::model::backend_label(active_backend);
        let mut rows = vec![
            Line::styled("АГЕНТ", Style::default().fg(p.muted)),
            Line::styled(fit(name, area.width as usize), Style::default().fg(p.accent)),
        ];
        if !ver.is_empty() {
            rows.push(Line::styled(
                fit(&clean(ver), area.width as usize),
                Style::default().fg(p.muted),
            ));
        } else {
            rows.push(Line::styled(
                "/backend — сменить",
                Style::default().fg(p.muted),
            ));
        }
        put(frame, area, Text::from(rows), p);
        return;
    }
    let usage = app
        .active
        .as_ref()
        .and_then(|h| app.remote.get(h))
        .map(|r| &r.usage);
    let Some(windows) = usage.and_then(|v| v["windows"].as_array()) else {
        put(
            frame,
            area,
            Line::styled("Лимиты пока недоступны", Style::default().fg(p.muted)),
            p,
        );
        return;
    };
    let mut rows = vec![Line::styled("ЛИМИТЫ", Style::default().fg(p.muted))];
    for window in windows.iter().take(2) {
        let name = window["label"]
            .as_str()
            .unwrap_or(if window["id"] == "five_hour" {
                "5 часов"
            } else {
                "Неделя"
            });
        if let Some(value) = window["utilization"].as_f64().filter(|v| v.is_finite()) {
            let value = value.clamp(0., 100.);
            let length = area.width.saturating_sub(7) as usize;
            let filled = ((value / 100.) * length as f64).round() as usize;
            rows.push(Line::from(vec![label(clean(name), p.text)]));
            rows.push(Line::from(vec![
                label(
                    "━".repeat(filled),
                    if value >= 95. { p.red } else { p.accent },
                ),
                label("─".repeat(length.saturating_sub(filled)), p.border),
                label(format!(" {value:>3.0}%"), p.muted),
            ]));
        } else {
            rows.push(Line::styled(
                format!("{} · нет процентов", clean(name)),
                Style::default().fg(p.muted),
            ));
        }
    }
    put(frame, area, Text::from(rows), p);
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
        "F2 — выбрать хост · /connect — подключиться. История остаётся на сервере."
    } else {
        "Продолжи сессию через F3 или напиши новую задачу.\nClaude Code, Codex, AGY и собственный API работают с общей историей."
    };
    let rows = vec![
        Line::default(),
        Line::styled(
            title,
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Line::default(),
    ];
    let mut rows = rows;
    rows.extend(markdown::wrap(
        Line::styled(detail.replace('\n', " "), Style::default().fg(p.muted)),
        area.width.saturating_sub(4) as usize,
    ));
    if connected {
        rows.push(Line::default());
        rows.push(Line::styled(
            "/backend  агент    /attach  файлы    /model  модель    /help  команды",
            Style::default().fg(p.accent),
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
            format!(
                " {} · {} · {} · {} ",
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
            Style::default().fg(p.muted),
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
        editor(
            frame,
            &c.editor,
            edit_area,
            p,
            false,
            app.popup.is_none(),
            "Напиши задачу…  /help",
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
                "Выбери сервер: F2  ·  Добавить: F2 → Ctrl+A",
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
            Line::styled(placeholder.to_owned(), Style::default().fg(p.muted)),
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
fn popup_view(frame: &mut Frame, app: &App, popup: &Popup, p: Palette) {
    let outer = frame.area();
    let width = outer.width.saturating_sub(4).min(86);
    let height = outer.height.saturating_sub(4).min(match popup {
        Popup::Picker { .. } | Popup::Help { .. } => 28,
        Popup::Permission { .. } => 24,
        Popup::Queue { .. } => 20,
        _ => 14,
    });
    let area = Rect {
        x: outer.x + (outer.width - width) / 2,
        y: outer.y + (outer.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, area);
    let title = match popup {
        Popup::Picker { kind, .. } => match kind {
            Picker::Hosts => "Серверы",
            Picker::Sessions => "Сессии",
            Picker::Models => "Модель",
            Picker::Backends => "Агент сессии",
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
                    if *kind == Picker::Hosts {
                        "↑↓ выбрать · Enter подключиться · Ctrl+A добавить · Esc назад"
                    } else {
                        "↑↓ выбрать · Enter открыть · Esc назад"
                    },
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
            let text=format!("Первое подключение к {name}.\n\nОтпечаток SSH:\n{fingerprint}\n\nСверь его с данными сервера. Изменённый сохранённый ключ автоматически не принимается.");
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
            let secret = matches!(kind, Field::Api { stage: 2, .. });
            let gap = if n > 0 { 1 } else { 0 };
            let field = Rect {
                y: inner.y + n + gap,
                height: inner.height.saturating_sub(n + gap + 2).max(1),
                ..inner
            };
            editor(frame, input, field, p, secret, true, "Введи значение…");
            if !error.is_empty() && inner.height >= 3 {
                put(
                    frame,
                    Rect {
                        y: inner.bottom().saturating_sub(2),
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
            let text="Enter — отправить или добавить в очередь\nAlt+Enter / Shift+Enter / Ctrl+J — новая строка\nCtrl+C — остановить ответ; Ctrl+Q — выйти\nF2 — хосты; Ctrl+A в списке — добавить сервер\nF3 — сессии; F4 — тема; Ctrl+N — новая сессия\nCtrl+O — полный вывод команд; Ctrl+B — боковая панель\nPgUp/PgDn — история; Ctrl+End — к последнему сообщению\nCtrl+P / Alt+N — предыдущие отправленные сообщения\nСтрелки / Home / End — редактирование; Ctrl+W — убрать слово\nTab — дополнить slash-команду; Shift+мышь — выделение в терминале\n\nСессии и авторизация общие с графическим Relay.\nПри запуске восстановленная очередь всегда на паузе.\nБраузер и управление кластером остаются в графическом Relay.\n";
            let mut lines = markdown::render(text, inner.width as usize, p.text, p.accent, p.muted);
            lines.push(Line::default());
            for (command, description) in COMMANDS {
                lines.extend(markdown::wrap(
                    Line::from(vec![
                        label(format!("{command:<14} "), p.accent),
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
            for kind in [Picker::Hosts, Picker::Sessions, Picker::Themes] {
                app.picker(kind);
                snapshot(&mut app, w, h).unwrap();
            }
            app.popup = Some(Popup::Help { scroll: 0 });
            snapshot(&mut app, w, h).unwrap();
        }
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
