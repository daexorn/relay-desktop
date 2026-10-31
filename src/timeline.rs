use crate::{
    model::{self, Message},
    ui::{self as design, Palette, ACCENT},
};
use eframe::egui::{self, RichText};
use std::{collections::HashMap, ops::Range};

#[derive(Debug, PartialEq)]
pub enum Item {
    Message(usize),
    Actions(Range<usize>),
}

fn empty_assistant(message: &Message) -> bool {
    message.role == "assistant" && message.text.trim().is_empty()
}

pub fn items(messages: &[Message]) -> Vec<Item> {
    let mut items = Vec::new();
    let mut at = 0;
    while at < messages.len() {
        match messages[at].role.as_str() {
            "tool" | "tool_result" => {
                let start = at;
                at += 1;
                while at < messages.len()
                    && (matches!(messages[at].role.as_str(), "tool" | "tool_result")
                        || empty_assistant(&messages[at]))
                {
                    at += 1;
                }
                items.push(Item::Actions(start..at));
            }
            _ => {
                if !empty_assistant(&messages[at]) {
                    items.push(Item::Message(at));
                }
                at += 1;
            }
        }
    }
    items
}

#[derive(Debug, PartialEq)]
struct Action {
    call: Option<usize>,
    results: Vec<usize>,
}

fn actions(messages: &[Message], range: Range<usize>) -> Vec<Action> {
    let mut rows: Vec<Action> = Vec::new();
    let mut calls = HashMap::new();
    for index in range {
        let message = &messages[index];
        if message.role == "tool" {
            if !message.tool_id.is_empty() {
                calls.insert(message.tool_id.as_str(), rows.len());
            }
            rows.push(Action {
                call: Some(index),
                results: vec![],
            });
        } else if message.role == "tool_result" {
            let owner = if message.tool_id.is_empty() {
                rows.iter()
                    .rposition(|row| row.call.is_some() && row.results.is_empty())
            } else {
                calls.get(message.tool_id.as_str()).copied()
            };
            if let Some(owner) = owner {
                rows[owner].results.push(index);
            } else {
                rows.push(Action {
                    call: None,
                    results: vec![index],
                });
            }
        }
    }
    rows
}

fn action_count(count: usize) -> String {
    let word = if (11..=14).contains(&(count % 100)) {
        "действий"
    } else {
        match count % 10 {
            1 => "действие",
            2..=4 => "действия",
            _ => "действий",
        }
    };
    format!("{count} {word}")
}

fn action_title(message: &Message) -> String {
    let kind = match message.tool.as_str() {
        "Read" => "Чтение файла",
        "Write" => "Создание файла",
        "Edit" | "MultiEdit" => "Изменение файла",
        "Bash" => "Команда на сервере",
        "Glob" | "Grep" => "Поиск по проекту",
        "WebSearch" | "WebFetch" => "Поиск в интернете",
        "mcp__relay_desktop__download_file" => "Скачивание на этот ПК",
        "mcp__relay_desktop__run_local_command" => "Команда на этом ПК",
        _ => "Действие",
    };
    let file = message.input["file_path"]
        .as_str()
        .or_else(|| message.input["server_path"].as_str());
    match file.and_then(|path| path.rsplit(['/', '\\']).find(|part| !part.is_empty())) {
        Some(file) => format!("{kind} · {}", model::shorten(file, 44)),
        None => kind.into(),
    }
}

pub fn show(
    ui: &mut egui::Ui,
    chat: &str,
    messages: &[Message],
    range: Range<usize>,
    busy: bool,
    palette: Palette,
) {
    let rows = actions(messages, range.clone());
    let errors = rows
        .iter()
        .filter(|row| row.results.iter().any(|index| messages[*index].failed))
        .count();
    let title = format!(
        "{}{}{}",
        if busy { "Работает · " } else { "" },
        action_count(rows.len()),
        if errors > 0 {
            " · есть ошибки"
        } else {
            ""
        }
    );
    let id = egui::Id::new(("activity-group", chat, range.start));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let color = if errors > 0 {
        egui::Color32::from_rgb(204, 133, 138)
    } else {
        palette.muted
    };
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 24.;
        ui.horizontal(|ui| {
            if busy {
                design::dots(ui, ACCENT);
            }
            let open = state.openness(ui.ctx());
            let title = ui.add(
                egui::Button::new(RichText::new(format!("     {title}")).size(12.).color(color))
                    .fill(design::mix(palette.base, palette.line, 0.45))
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(12.)
                    .min_size(egui::vec2(0., 24.)),
            );
            // Chevron painted by hand: the bundled fonts have no triangle glyphs.
            let c = egui::pos2(title.rect.left() + 13., title.rect.center().y);
            let turn = egui::emath::Rot2::from_angle(open * std::f32::consts::FRAC_PI_2);
            let points = [egui::vec2(-1.5, -3.5), egui::vec2(2., 0.), egui::vec2(-1.5, 3.5)]
                .map(|v| c + turn * v)
                .to_vec();
            ui.painter()
                .add(egui::Shape::line(points, egui::Stroke::new(1.4_f32, color)));
            let clicked = title
                .on_hover_text(if state.is_open() { "Скрыть журнал действий" } else { "Открыть журнал действий" })
                .clicked();
            if clicked {
                state.toggle(ui);
            }
        });
    });
    state.show_body_unindented(ui, |ui| {
        egui::Frame::new()
            .fill(palette.side)
            .corner_radius(10.)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(id.with("journal"))
                    .max_height(320.)
                    .min_scrolled_height((rows.len() as f32 * 27.).clamp(32., 320.))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().interact_size.y = 24.;
                        ui.spacing_mut().item_spacing.y = 3.;
                        for (index, row) in rows.iter().enumerate() {
                            let failed = row.results.iter().any(|index| messages[*index].failed);
                            let title = row
                                .call
                                .map(|index| action_title(&messages[index]))
                                .unwrap_or_else(|| "Результат действия".into());
                            let title = if failed {
                                format!("{title} · ошибка")
                            } else {
                                title
                            };
                            let row_id = id.with(index);
                            let mut detail =
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    row_id,
                                    false,
                                );
                            if ui
                                .selectable_label(
                                    detail.is_open(),
                                    RichText::new(title).size(12.).color(if failed {
                                        color
                                    } else {
                                        palette.muted
                                    }),
                                )
                                .on_hover_text("Параметры и результат")
                                .clicked()
                            {
                                detail.toggle(ui);
                            }
                            detail.show_body_unindented(ui, |ui| {
                                if let Some(call) = row.call {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(model::summary(&messages[call].input))
                                                .monospace()
                                                .size(12.),
                                        )
                                        .wrap()
                                        .selectable(true),
                                    );
                                }
                                for result in &row.results {
                                    egui::ScrollArea::vertical()
                                        .id_salt((row_id, *result))
                                        .max_height(160.)
                                        .show(ui, |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&messages[*result].text)
                                                        .monospace()
                                                        .size(12.),
                                                )
                                                .wrap()
                                                .selectable(true),
                                            );
                                        });
                                }
                                if row.results.is_empty() {
                                    ui.label(
                                        RichText::new("Результат ещё не получен")
                                            .size(11.)
                                            .color(palette.muted),
                                    );
                                }
                                ui.add_space(6.);
                            });
                        }
                    });
            });
    });
    ui.add_space(10.);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(role: &str, text: &str, id: &str) -> Message {
        Message {
            role: role.into(),
            text: text.into(),
            tool_id: id.into(),
            ..Default::default()
        }
    }
    #[test]
    fn grouping_preserves_visible_conversation_and_ignores_empty_stream_blocks() {
        let messages = vec![
            message("user", "Check slides", ""),
            message("assistant", "Checking", ""),
            message("tool", "", "a"),
            message("tool_result", "ok", "a"),
            message("assistant", "  ", ""),
            message("tool", "", "b"),
            message("tool_result", "ok", "b"),
            message("assistant", "Done", ""),
            message("user", "", ""),
            message("tool", "", "c"),
        ];
        assert_eq!(
            items(&messages),
            vec![
                Item::Message(0),
                Item::Message(1),
                Item::Actions(2..7),
                Item::Message(7),
                Item::Message(8),
                Item::Actions(9..10)
            ]
        );
        assert_eq!(
            actions(&messages, 2..7),
            vec![
                Action {
                    call: Some(2),
                    results: vec![3]
                },
                Action {
                    call: Some(5),
                    results: vec![6]
                }
            ]
        );
    }
    #[test]
    fn parallel_results_and_orphan_errors_are_preserved_without_double_counting() {
        let messages = vec![
            message("tool", "", "a"),
            message("tool", "", "b"),
            message("tool_result", "b result", "b"),
            message("tool_result", "a result", "a"),
            Message {
                failed: true,
                ..message("tool_result", "orphan error", "missing")
            },
        ];
        assert_eq!(
            actions(&messages, 0..5),
            vec![
                Action {
                    call: Some(0),
                    results: vec![3]
                },
                Action {
                    call: Some(1),
                    results: vec![2]
                },
                Action {
                    call: None,
                    results: vec![4]
                }
            ]
        );
    }
}
