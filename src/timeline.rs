use crate::{
    model::{self, Message},
    ui::{self as design, Palette, accent},
};
use eframe::egui::{self, RichText};
use egui_commonmark::CommonMarkCache;
use serde_json::Value;
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
        "mcp__relay_cluster__dispatch_task" => "Назначение исполнителю",
        "mcp__relay_cluster__wait_for_task" => "Ожидание результата",
        "mcp__relay_cluster__task_status" => "Результат исполнителя",
        "mcp__relay_cluster__list_workers" => "Состав команды",
        "mcp__relay_cluster__cluster_status" => "Состояние команды",
        "mcp__relay_cluster__cancel_task" => "Отмена задачи",
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
    p: Palette,
    cache: &mut CommonMarkCache,
) {
    let rows = actions(messages, range.clone());
    let errors = rows
        .iter()
        .filter(|r| r.results.iter().any(|i| messages[*i].failed))
        .count();
    let id = egui::Id::new(("activity-group", chat, range.start));
    let auto_errors = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("relay-open-errors")))
        .unwrap_or(false);
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        auto_errors && errors > 0,
    );
    let error_color = egui::Color32::from_rgb(222, 143, 151);
    let width = ui.available_width().max(100.);
    let detail_openness: f32 = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let failed = row.results.iter().any(|i| messages[*i].failed);
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id.with(index),
                auto_errors && failed,
            )
            .openness(ui.ctx())
        })
        .sum();
    let (r, response) = ui.allocate_exact_size(egui::vec2(width, 38.), egui::Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        design::duration(ui.ctx(), 0.2),
    );
    ui.painter()
        .rect_filled(r, 10., design::mix(p.base, p.surface, 0.55 + 0.4 * hover));
    ui.painter().rect_stroke(
        r,
        10.,
        egui::Stroke::new(1_f32, design::mix(p.line, p.muted, hover * 0.2)),
        egui::StrokeKind::Inside,
    );
    design::icon(
        ui.painter(),
        egui::Rect::from_min_size(r.min + egui::vec2(11., 10.), egui::vec2(18., 18.)),
        design::Icon::Server,
        if busy { accent() } else { p.muted },
    );
    let text = format!(
        "{} · {}{}",
        if busy {
            "В работе"
        } else {
            "Действия"
        },
        action_count(rows.len()),
        if errors > 0 {
            format!(" · ошибок: {errors}")
        } else {
            String::new()
        }
    );
    let clip = ui.painter().with_clip_rect(egui::Rect::from_min_max(
        r.min + egui::vec2(36., 0.),
        r.max - egui::vec2(31., 0.),
    ));
    clip.text(
        egui::pos2(r.left() + 38., r.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(12.5),
        if errors > 0 { error_color } else { p.muted },
    );
    design::chevron(
        ui.painter(),
        egui::Rect::from_center_size(r.right_center() - egui::vec2(17., 0.), egui::vec2(16., 16.)),
        state.openness(ui.ctx()),
        p.muted,
    );
    if response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Открыть или свернуть команды и результаты")
        .clicked()
    {
        state.toggle(ui);
    }
    state.show_body_unindented(ui, |ui| {
        egui::Frame::new()
            .fill(p.side)
            .corner_radius(12.)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_width((width - 20.).max(80.));
                egui::ScrollArea::vertical()
                    .id_salt(id.with("journal"))
                    .max_height(420.)
                    .min_scrolled_height(
                        (rows.len() as f32 * 54. + detail_openness * 190.).clamp(68., 420.),
                    )
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (index, row) in rows.iter().enumerate() {
                            let failed = row.results.iter().any(|i| messages[*i].failed);
                            let row_id = id.with(index);
                            let mut detail =
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    row_id,
                                    auto_errors && failed,
                                );
                            let title = row
                                .call
                                .map(|i| action_title(&messages[i]))
                                .unwrap_or_else(|| "Результат действия".into());
                            let local = row.call.is_some_and(|i| {
                                messages[i].tool.starts_with("mcp__relay_desktop__")
                            });
                            egui::Frame::new()
                                .fill(p.base)
                                .stroke(egui::Stroke::new(1_f32, p.line))
                                .corner_radius(9.)
                                .inner_margin(10)
                                .show(ui, |ui| {
                                    let (r, res) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), 26.),
                                        egui::Sense::click(),
                                    );
                                    design::icon(
                                        ui.painter(),
                                        egui::Rect::from_min_size(
                                            r.min + egui::vec2(0., 5.),
                                            egui::vec2(16., 16.),
                                        ),
                                        if local {
                                            design::Icon::Computer
                                        } else {
                                            design::Icon::File
                                        },
                                        if failed { error_color } else { p.muted },
                                    );
                                    let text = if failed {
                                        format!("{title} · ошибка")
                                    } else {
                                        title
                                    };
                                    ui.painter()
                                        .with_clip_rect(r.shrink2(egui::vec2(22., 0.)))
                                        .text(
                                            r.left_center() + egui::vec2(24., 0.),
                                            egui::Align2::LEFT_CENTER,
                                            text,
                                            egui::FontId::proportional(12.),
                                            if failed { error_color } else { p.text },
                                        );
                                    design::chevron(
                                        ui.painter(),
                                        egui::Rect::from_center_size(
                                            r.right_center() - egui::vec2(5., 0.),
                                            egui::vec2(12., 12.),
                                        ),
                                        detail.openness(ui.ctx()),
                                        p.muted,
                                    );
                                    if res
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        detail.toggle(ui);
                                    }
                                    detail.show_body_unindented(ui, |ui| {
                                        if let Some(call) = row.call {
                                            let cluster = messages[call]
                                                .tool
                                                .starts_with("mcp__relay_cluster__");
                                            let text = if cluster {
                                                messages[call].input["task"]
                                                    .as_str()
                                                    .map(str::to_owned)
                                                    .unwrap_or_else(|| {
                                                        match messages[call].tool.as_str() {
                                                            "mcp__relay_cluster__list_workers" => {
                                                                "Состав и доступность исполнителей"
                                                            }
                                                            "mcp__relay_cluster__dispatch_task" => {
                                                                "Назначение работы исполнителю"
                                                            }
                                                            "mcp__relay_cluster__cancel_task" => {
                                                                "Отмена назначенной задачи"
                                                            }
                                                            _ => "Запрос результата исполнителя",
                                                        }
                                                        .into()
                                                    })
                                            } else {
                                                model::summary(&messages[call].input)
                                            };
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(if cluster {
                                                        "Команда"
                                                    } else if local {
                                                        "Этот ПК"
                                                    } else {
                                                        "Сервер"
                                                    })
                                                    .size(10.5)
                                                    .color(p.muted),
                                                );
                                                if design::button(
                                                    ui,
                                                    "Копировать",
                                                    Some(design::Icon::Copy),
                                                    design::Kind::Ghost,
                                                    p,
                                                    egui::vec2(0., 24.),
                                                )
                                                .clicked()
                                                {
                                                    ui.ctx().copy_text(text.clone());
                                                }
                                            });
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(text)
                                                        .monospace()
                                                        .size(12.)
                                                        .color(p.text),
                                                )
                                                .wrap()
                                                .selectable(true),
                                            );
                                        }
                                        for result in &row.results {
                                            ui.add_space(7.);
                                            ui.separator();
                                            ui.add_space(5.);
                                            let text = &messages[*result].text;
                                            let wrap = ui
                                                .ctx()
                                                .data(|d| {
                                                    d.get_temp::<bool>(egui::Id::new(
                                                        "relay-wrap-output",
                                                    ))
                                                })
                                                .unwrap_or(true);
                                            egui::ScrollArea::both()
                                                .id_salt((row_id, *result))
                                                .max_height(230.)
                                                .auto_shrink([false, true])
                                                .show(ui, |ui| {
                                                    result_view(
                                                        ui,
                                                        text,
                                                        messages[*result].failed,
                                                        p,
                                                        cache,
                                                        wrap,
                                                    );
                                                });
                                        }
                                    });
                                });
                            ui.add_space(6.);
                        }
                    });
            });
    });
}

fn decoded(text: &str) -> Option<Value> {
    fn unwrap(value: Value, depth: usize) -> Value {
        if depth > 3 {
            return value;
        }
        if let Some(items) = value
            .as_array()
            .or_else(|| value.get("content").and_then(Value::as_array))
        {
            let text = items
                .iter()
                .filter_map(|v| v["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if let Ok(v) = serde_json::from_str(&text) {
                return unwrap(v, depth + 1);
            }
        }
        if value.get("result").is_some_and(Value::is_object)
            && value.get("stdout").is_none()
            && value.get("job").is_none()
        {
            return unwrap(value["result"].clone(), depth + 1);
        }
        value
    }
    serde_json::from_str(text).ok().map(|v| unwrap(v, 0))
}
fn status_label(value: &Value) -> &str {
    match value.as_str().unwrap_or("") {
        "queued" => "В очереди",
        "running" => "В работе",
        "waiting" => "Нужно подтверждение",
        "cancelling" => "Останавливается",
        "done" => "Ответ получен",
        "failed" => "Ошибка",
        "cancelled" => "Отменено",
        "interrupted" => "Прервано",
        _ => "Результат",
    }
}
fn terminal(ui: &mut egui::Ui, text: &str, ink: egui::Color32, wrap: bool) {
    let label = egui::Label::new(
        RichText::new(clean_terminal(text))
            .monospace()
            .size(12.)
            .color(ink),
    )
    .selectable(true);
    ui.add(if wrap { label.wrap() } else { label.extend() });
}
fn clean_terminal(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}
pub fn result_view(
    ui: &mut egui::Ui,
    text: &str,
    failed: bool,
    p: Palette,
    cache: &mut CommonMarkCache,
    wrap: bool,
) {
    let error = egui::Color32::from_rgb(222, 143, 151);
    if let Some(v) = decoded(text) {
        if v["job"].is_string() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(v["worker_name"].as_str().unwrap_or("Исполнитель"))
                        .size(12.)
                        .color(p.text),
                );
                ui.label(RichText::new(status_label(&v["state"])).size(11.).color(
                    if v["state"] == "done" {
                        crate::ui::GREEN
                    } else if v["state"] == "failed" {
                        error
                    } else {
                        accent()
                    },
                ));
            });
            if let Some(title) = v["title"].as_str() {
                ui.add(egui::Label::new(RichText::new(title).size(13.)).wrap());
            }
            if let Some(message) = v["error"].as_str().filter(|s| !s.is_empty()) {
                ui.label(RichText::new(message).size(12.).color(error));
            }
            if let Some(result) = v["result"].as_str().filter(|s| !s.is_empty()) {
                ui.add_space(6.);
                crate::math::show(ui, cache, result);
            }
            return;
        }
        if let Some(workers) = v["workers"].as_array() {
            for w in workers {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(w["name"].as_str().unwrap_or("Исполнитель"))
                            .size(12.)
                            .color(p.text),
                    );
                    ui.label(
                        RichText::new(if w["connected"] == true {
                            if w["busy"] == true {
                                "Работает"
                            } else {
                                "Готов"
                            }
                        } else {
                            "Не подключён"
                        })
                        .size(11.)
                        .color(p.muted),
                    );
                    if let Some(host) = w["host"].as_str() {
                        ui.label(RichText::new(host).size(11.).color(p.muted));
                    }
                });
            }
            return;
        }
        if let Some(tasks) = v["tasks"].as_array() {
            for t in tasks {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(status_label(&t["state"]))
                            .size(11.)
                            .color(p.muted),
                    );
                    ui.label(RichText::new(t["title"].as_str().unwrap_or("Задача")).size(12.));
                });
            }
            return;
        }
        if v.get("stdout").is_some() || v.get("stderr").is_some() {
            let ok = v["ok"].as_bool().unwrap_or(!failed);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if ok {
                        "Выполнено"
                    } else {
                        "Ошибка выполнения"
                    })
                    .size(11.5)
                    .color(if ok { crate::ui::GREEN } else { error }),
                );
                if let Some(code) = v["exit_code"].as_i64() {
                    ui.label(
                        RichText::new(format!("код {code}"))
                            .size(10.5)
                            .color(p.muted),
                    );
                }
                if design::button(
                    ui,
                    "Копировать вывод",
                    Some(design::Icon::Copy),
                    design::Kind::Ghost,
                    p,
                    egui::vec2(0., 24.),
                )
                .clicked()
                {
                    ui.ctx().copy_text(format!(
                        "{}{}",
                        v["stdout"].as_str().unwrap_or(""),
                        v["stderr"].as_str().unwrap_or("")
                    ));
                }
            });
            if let Some(stdout) = v["stdout"].as_str().filter(|s| !s.is_empty()) {
                terminal(ui, stdout, p.text, wrap);
            }
            if let Some(stderr) = v["stderr"].as_str().filter(|s| !s.is_empty()) {
                ui.add_space(6.);
                egui::Frame::new()
                    .fill(error.gamma_multiply(0.07))
                    .corner_radius(7.)
                    .inner_margin(8)
                    .show(ui, |ui| {
                        terminal(ui, stderr, error, wrap);
                    });
            }
            if v["output_truncated"] == true {
                ui.label(
                    RichText::new("Показана часть вывода")
                        .size(10.5)
                        .color(p.muted),
                );
            }
            return;
        }
        if v["ok"] == false {
            if let Some(message) = v["message"].as_str() {
                ui.label(RichText::new(message).size(12.).color(error));
                return;
            }
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(if failed { "Ошибка" } else { "Вывод" })
                .size(11.)
                .color(if failed { error } else { p.muted }),
        );
        if design::button(
            ui,
            "Копировать вывод",
            Some(design::Icon::Copy),
            design::Kind::Ghost,
            p,
            egui::vec2(0., 24.),
        )
        .clicked()
        {
            ui.ctx().copy_text(clean_terminal(text));
        }
    });
    terminal(ui, text, if failed { error } else { p.text }, wrap);
}
pub fn cluster_update_view(ui: &mut egui::Ui, text: &str, p: Palette, cache: &mut CommonMarkCache) {
    let body = text
        .strip_prefix("<relay-cluster-results>")
        .and_then(|s| s.split_once("</relay-cluster-results>").map(|(v, _)| v));
    egui::Frame::new()
        .fill(p.surface)
        .stroke(egui::Stroke::new(1.0_f32, accent().gamma_multiply(0.35)))
        .corner_radius(12.)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                design::chip(
                    ui,
                    "Результаты исполнителей",
                    design::Icon::Cluster,
                    p,
                    true,
                );
            });
            if let Some(items) = body.and_then(|s| serde_json::from_str::<Vec<Value>>(s).ok()) {
                for item in items {
                    ui.add_space(8.);
                    result_view(ui, &item.to_string(), false, p, cache, true);
                }
            } else {
                ui.label(RichText::new("Результаты переданы руководителю").color(p.muted));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_mcp_results_decode_without_losing_real_exit_status() {
        let raw=serde_json::json!([{"type":"text","text":serde_json::json!({"ok":false,"exit_code":9,"stdout":"out","stderr":"failure"}).to_string()}]).to_string();
        let result = decoded(&raw).unwrap();
        assert_eq!(result["exit_code"], 9);
        assert_eq!(result["stderr"], "failure");
        assert_eq!(clean_terminal("\u{1b}[32mГотово\u{1b}[0m\r\n"), "Готово\n");
    }
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
