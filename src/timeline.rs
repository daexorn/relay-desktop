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
        "Read" | "read_file" => "Чтение файла",
        "Write" | "write_file" => "Создание файла",
        "Edit" | "MultiEdit" | "edit_file" | "fileChange" | "apply_patch" => "Изменение кода",
        "Bash" | "run_command" | "commandExecution" => "Команда на сервере",
        "SlashCommand" => "Команда агента",
        "Glob" | "Grep" | "grep_search" | "find_by_name" | "LS" => "Поиск по проекту",
        "WebSearch" | "WebFetch" | "search_web" | "webSearch" => "Поиск в интернете",
        "Task" | "Agent" | "invoke_subagent" | "spawn_agent" => "Запуск субагента",
        "swarm" | "run_workflow" => "Оркестрация роя",
        "mcp__relay_desktop__download_file" => "Скачивание на этот ПК",
        "mcp__relay_desktop__run_local_command" => "Команда на этом ПК",
        "mcp__relay_cluster__dispatch_task" => "Назначение исполнителю",
        "mcp__relay_cluster__wait_for_task" => "Ожидание результата",
        "mcp__relay_cluster__task_status" => "Результат исполнителя",
        "mcp__relay_cluster__list_workers" => "Состав команды",
        "mcp__relay_cluster__cluster_status" => "Состояние команды",
        "mcp__relay_cluster__cancel_task" => "Отмена задачи",
        _ => "Действие агента",
    };
    let file = message.input["file_path"]
        .as_str()
        .or_else(|| message.input["Path"].as_str())
        .or_else(|| message.input["server_path"].as_str());
    match file.and_then(|path| path.rsplit(['/', '\\']).find(|part| !part.is_empty())) {
        Some(file) => format!("{kind} · {}", model::shorten(file, 44)),
        None => {
            if matches!(
                message.tool.as_str(),
                "Task" | "Agent" | "invoke_subagent" | "spawn_agent" | "mcp__relay_cluster__dispatch_task"
            ) {
                let role = message.input["Role"]
                    .as_str()
                    .or_else(|| message.input["worker"].as_str())
                    .or_else(|| message.input["subagent_type"].as_str())
                    .or_else(|| message.input["agent_type"].as_str())
                    .unwrap_or("");
                if !role.trim().is_empty() {
                    return format!("{kind} · {}", model::shorten(role.trim(), 34));
                }
            }
            kind.into()
        }
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
    let light = !ui.visuals().dark_mode;
    let rows = actions(messages, range.clone());
    let tel = model::analyze_turn(&messages[..range.end], range.start, busy, "");
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
        (auto_errors && errors > 0) || busy,
    );
    let error_color = egui::Color32::from_rgb(228, 126, 136);
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

    let subtitle = if busy && !tel.active_target.is_empty() {
        Some(tel.active_target.clone())
    } else if !tel.subagents.is_empty() {
        let names: Vec<_> = tel.subagents.iter().take(3).map(|s| s.role.as_str()).collect();
        Some(format!("Субагенты: {}", names.join(" · ")))
    } else {
        None
    };
    let hdr_h = if subtitle.is_some() { 58. } else { 46. };
    let (r, response) = ui.allocate_exact_size(egui::vec2(width, hdr_h), egui::Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        design::duration(ui.ctx(), 0.2),
    );
    let active_tint = tel
        .active_phase
        .map(|ph| design::phase_color(ph, light))
        .unwrap_or_else(accent);
    let border_col = if errors > 0 {
        design::mix(p.line, error_color, 0.48)
    } else if busy {
        design::mix(p.line, active_tint, 0.45 + 0.2 * hover)
    } else {
        design::mix(p.line, accent(), hover * 0.28)
    };
    ui.painter().rect_filled(
        r,
        12.,
        design::mix(
            p.side,
            design::mix(p.surface, active_tint, if busy { 0.08 } else { 0.02 }),
            0.62 + 0.35 * hover,
        ),
    );
    ui.painter().rect_stroke(
        r,
        12.,
        egui::Stroke::new(1.0_f32, border_col),
        egui::StrokeKind::Inside,
    );
    design::specular_rim(ui.painter(), r, 12., light);

    // Animated scanline shimmer along the bottom edge while agent pipeline is executing
    if busy {
        let animated = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(egui::Id::new("relay-animations")))
            .unwrap_or(true);
        let phase_t = if animated {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(24));
            (ui.input(|i| i.time) as f32 * 0.65) % 1.0
        } else {
            0.4
        };
        let span = (r.width() * 0.28).clamp(40., 160.);
        let x0 = r.left() + 12. + (r.width() - 24. - span).max(0.) * phase_t;
        ui.painter().line_segment(
            [
                egui::pos2(x0, r.bottom() - 1.0),
                egui::pos2(x0 + span, r.bottom() - 1.0),
            ],
            egui::Stroke::new(2.0_f32, active_tint),
        );
    }

    let badge_rect = egui::Rect::from_center_size(
        egui::pos2(r.left() + 23., r.center().y),
        egui::vec2(24., 24.),
    );
    if busy {
        design::orbital_core(ui, badge_rect, active_tint, true);
    } else if errors > 0 {
        ui.painter()
            .circle_filled(badge_rect.center(), 11., error_color.gamma_multiply(0.16));
        design::icon(ui.painter(), badge_rect.shrink(4.), design::Icon::Pulse, error_color);
    } else {
        ui.painter()
            .circle_filled(badge_rect.center(), 11., design::GREEN.gamma_multiply(0.15));
        design::icon(ui.painter(), badge_rect.shrink(4.5), design::Icon::Layers, design::GREEN);
    }

    let right_metrics_w = if width >= 500. {
        196.
    } else if width >= 370. {
        108.
    } else {
        34.
    };
    let headline = if busy && !tel.active_headline.is_empty() {
        format!(
            "{} · {}/{}",
            tel.active_headline,
            tel.done_actions,
            rows.len().max(1)
        )
    } else {
        format!(
            "Конвейер агента · {}{}",
            action_count(rows.len()),
            if errors > 0 {
                format!(" · сбоев: {errors}")
            } else {
                String::new()
            }
        )
    };
    let text_clip = ui.painter().with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(r.left() + 44., r.top() + 4.),
        egui::pos2((r.right() - right_metrics_w).max(r.left() + 80.), r.bottom() - 4.),
    ));
    let title_y = if subtitle.is_some() {
        r.center().y - 8.5
    } else {
        r.center().y
    };
    text_clip.text(
        egui::pos2(r.left() + 44., title_y),
        egui::Align2::LEFT_CENTER,
        headline,
        egui::FontId::proportional(13.0),
        if errors > 0 {
            error_color
        } else if busy {
            p.text
        } else {
            design::mix(p.text, p.muted, 0.15)
        },
    );
    if let Some(sub) = &subtitle {
        text_clip.text(
            egui::pos2(r.left() + 44., r.center().y + 10.5),
            egui::Align2::LEFT_CENTER,
            sub,
            egui::FontId::monospace(11.0),
            if busy { active_tint } else { p.muted },
        );
    }

    // Segmented phase bar & counter pills on the right side of the pipeline header
    if width >= 370. {
        let bar_right = r.right() - 36.;
        let bar_w = if width >= 500. { 78. } else { 58. };
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(bar_right - bar_w, r.center().y - 3.),
            egui::vec2(bar_w, 6.),
        );
        design::phase_bar(ui.painter(), bar_rect, &tel, light, p);

        if width >= 500. {
            let mut px = bar_rect.left() - 8.;
            for (cnt, ph) in [
                (tel.delegations, model::ToolPhase::Subagent),
                (tel.commands, model::ToolPhase::Terminal),
                (tel.edits, model::ToolPhase::Edit),
                (tel.reads, model::ToolPhase::Research),
            ] {
                if cnt == 0 {
                    continue;
                }
                let col = design::phase_color(ph, light);
                let txt = format!("{cnt}");
                let g = ui
                    .painter()
                    .layout_no_wrap(txt, egui::FontId::proportional(10.5), col);
                let pw = g.size().x + 21.;
                if px - pw < r.left() + 180. {
                    break;
                }
                let pill = egui::Rect::from_min_size(
                    egui::pos2(px - pw, r.center().y - 9.5),
                    egui::vec2(pw, 19.),
                );
                ui.painter()
                    .rect_filled(pill, 9.5, design::mix(p.surface, col, 0.15));
                design::icon(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(pill.left() + 8.5, pill.center().y),
                        egui::vec2(10.5, 10.5),
                    ),
                    design::phase_icon(ph),
                    col,
                );
                ui.painter().galley(
                    egui::pos2(pill.left() + 15.5, pill.center().y - g.size().y * 0.5),
                    g,
                    col,
                );
                px = pill.left() - 5.;
            }
        }
    }

    design::chevron(
        ui.painter(),
        egui::Rect::from_center_size(r.right_center() - egui::vec2(17., 0.), egui::vec2(16., 16.)),
        state.openness(ui.ctx()),
        p.muted,
    );
    if response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Открыть или свернуть конвейер шагов агента")
        .clicked()
    {
        state.toggle(ui);
    }

    state.show_body_unindented(ui, |ui| {
        ui.add_space(4.);
        egui::Frame::new()
            .fill(design::mix(p.side, p.base, 0.35))
            .stroke(egui::Stroke::new(1.0_f32, p.line))
            .corner_radius(12.)
            .inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 12,
                bottom: 10,
            })
            .show(ui, |ui| {
                let inner_w = (width - 24.).max(80.);
                ui.set_width(inner_w);
                ui.set_max_width(inner_w);
                egui::ScrollArea::vertical()
                    .id_salt(id.with("journal"))
                    .max_width(inner_w)
                    .max_height(560.)
                    .stick_to_bottom(busy)
                    .min_scrolled_height(
                        (rows.len() as f32 * 108. + detail_openness * 190.).clamp(68., 560.),
                    )
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let row_w = inner_w.min(ui.available_width()).max(80.);
                        ui.set_width(row_w);
                        ui.set_max_width(row_w);
                        let mut prev_result_end = 0usize;
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
                            let call = row.call.map(|i| &messages[i]);
                            let local =
                                call.is_some_and(|m| m.tool.starts_with("mcp__relay_desktop__"));
                            let running = busy && row.results.is_empty();
                            let tool = call.map(|m| m.tool.as_str()).unwrap_or("");
                            let phase = model::classify_tool(tool);
                            let phase_col = design::phase_color(phase, light);
                            let is_subagent = phase == model::ToolPhase::Subagent;
                            let parallel = index > 0
                                && row.call.is_some_and(|c_idx| c_idx < prev_result_end);
                            if let Some(&first_res) = row.results.first() {
                                prev_result_end = prev_result_end.max(first_res);
                            } else if running {
                                prev_result_end = usize::MAX;
                            }

                            let kind_icon = if local {
                                design::Icon::Computer
                            } else if tool.starts_with("mcp__relay_cluster__") {
                                design::Icon::Cluster
                            } else {
                                design::phase_icon(phase)
                            };
                            let heading = call
                                .and_then(|m| m.input["description"].as_str())
                                .filter(|d| {
                                    !d.trim().is_empty() && !tool.starts_with("mcp__relay_cluster__")
                                })
                                .map(|d| model::shorten(d.trim(), 90))
                                .unwrap_or(title);
                            let line = call.map(command_line).unwrap_or_default();
                            let shown = ui.ctx().animate_bool_with_time(
                                row_id.with("shown"),
                                true,
                                design::duration(ui.ctx(), 0.28),
                            );
                            let tint = if failed {
                                error_color
                            } else if running || is_subagent {
                                phase_col
                            } else {
                                p.line
                            };
                            let is_last = index + 1 == rows.len();

                            ui.scope(|ui| {
                                ui.multiply_opacity(shown);
                                ui.add_space((1. - shown) * 8.);
                                let row_top = ui.cursor().top();
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.;
                                    // Reserve 26px left gutter for continuous timeline rail & status node
                                    let (gutter, _) = ui.allocate_exact_size(
                                        egui::vec2(26., 28.),
                                        egui::Sense::hover(),
                                    );
                                    let card_w = (row_w - 36.).max(60.);
                                    let card_fill = if is_subagent {
                                        design::mix(p.base, phase_col, 0.08)
                                    } else if running {
                                        design::mix(p.base, phase_col, 0.05)
                                    } else {
                                        p.base
                                    };
                                    let card_resp = egui::Frame::new()
                                        .fill(card_fill)
                                        .stroke(egui::Stroke::new(
                                            if running || is_subagent { 1.15_f32 } else { 1.0_f32 },
                                            design::mix(
                                                p.line,
                                                tint,
                                                if failed || running {
                                                    0.62
                                                } else if is_subagent {
                                                    0.42
                                                } else {
                                                    0.12
                                                },
                                            ),
                                        ))
                                        .corner_radius(10.)
                                        .inner_margin(egui::Margin {
                                            left: 11,
                                            right: 10,
                                            top: 8,
                                            bottom: 8,
                                        })
                                        .show(ui, |ui| {
                                            let inner_card_w = (card_w - 21.).max(48.);
                                            ui.vertical(|ui| {
                                                ui.set_width(inner_card_w);
                                                ui.set_max_width(inner_card_w);
                                                let (r, res) = ui.allocate_exact_size(
                                                    egui::vec2(inner_card_w, 22.),
                                                    egui::Sense::click(),
                                                );
                                            design::icon(
                                                ui.painter(),
                                                egui::Rect::from_center_size(
                                                    r.left_center() + egui::vec2(8., 0.),
                                                    egui::vec2(15., 15.),
                                                ),
                                                kind_icon,
                                                if failed { error_color } else { phase_col },
                                            );
                                            // Phase micro-badge inside step header
                                            let tag_text = phase.label();
                                            let tag_galley = ui.painter().layout_no_wrap(
                                                tag_text.into(),
                                                egui::FontId::proportional(9.5),
                                                phase_col,
                                            );
                                            let tag_w = tag_galley.size().x + 11.;
                                            let tag_rect = egui::Rect::from_min_size(
                                                egui::pos2(r.left() + 22., r.center().y - 8.5),
                                                egui::vec2(tag_w, 17.),
                                            );
                                            ui.painter().rect_filled(
                                                tag_rect,
                                                5.,
                                                design::mix(p.surface, phase_col, 0.16),
                                            );
                                            ui.painter().galley(
                                                egui::pos2(
                                                    tag_rect.left() + 5.5,
                                                    tag_rect.center().y - tag_galley.size().y * 0.5,
                                                ),
                                                tag_galley,
                                                phase_col,
                                            );

                                            let mut text_x = tag_rect.right() + 8.;
                                            if parallel && r.width() > 260. {
                                                let par_col = accent();
                                                let pg = ui.painter().layout_no_wrap(
                                                    "⚡".into(),
                                                    egui::FontId::proportional(10.),
                                                    par_col,
                                                );
                                                let pr = egui::Rect::from_min_size(
                                                    egui::pos2(text_x, r.center().y - 8.),
                                                    egui::vec2(pg.size().x + 8., 16.),
                                                );
                                                ui.painter().rect_filled(
                                                    pr,
                                                    5.,
                                                    par_col.gamma_multiply(0.14),
                                                );
                                                ui.painter().galley(
                                                    egui::pos2(
                                                        pr.left() + 4.,
                                                        pr.center().y - pg.size().y * 0.5,
                                                    ),
                                                    pg,
                                                    par_col,
                                                );
                                                text_x = pr.right() + 6.;
                                            }

                                            ui.painter()
                                                .with_clip_rect(egui::Rect::from_min_max(
                                                    egui::pos2(text_x, r.top()),
                                                    r.max - egui::vec2(20., 0.),
                                                ))
                                                .text(
                                                    egui::pos2(text_x, r.center().y),
                                                    egui::Align2::LEFT_CENTER,
                                                    heading,
                                                    egui::FontId::proportional(12.5),
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
                                                .on_hover_text("Показать аргументы и полный вывод шага")
                                                .clicked()
                                            {
                                                detail.toggle(ui);
                                            }
                                            if !line.is_empty() {
                                                ui.add_space(2.);
                                                clipped(
                                                    ui,
                                                    &line,
                                                    22.,
                                                    2,
                                                    egui::FontId::monospace(11.5),
                                                    if is_subagent {
                                                        design::mix(p.text, phase_col, 0.35)
                                                    } else {
                                                        design::mix(p.text, p.muted, 0.38)
                                                    },
                                                );
                                            }
                                            if detail.openness(ui.ctx()) < 0.5 {
                                                egui::Frame::new()
                                                    .inner_margin(egui::Margin {
                                                        left: 22,
                                                        ..Default::default()
                                                    })
                                                    .show(ui, |ui| {
                                                        preview(
                                                            ui,
                                                            call,
                                                            row.results
                                                                .iter()
                                                                .map(|i| &messages[*i])
                                                                .collect(),
                                                            running,
                                                            p,
                                                            error_color,
                                                        );
                                                    });
                                            }
                                            detail.show_body_unindented(ui, |ui| {
                                                ui.add_space(6.);
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
                                                                "Команда агентов"
                                                            } else if is_subagent {
                                                                "Делегирование субагенту"
                                                            } else if local {
                                                                "Этот ПК"
                                                            } else {
                                                                "Сервер"
                                                            })
                                                            .size(10.5)
                                                            .color(phase_col),
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
                                                        .max_width(inner_card_w)
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
                                        })
                                        .response;

                                    // Paint the continuous timeline spine and glowing node in the left gutter!
                                    let spine_x = gutter.left() + 10.;
                                    let node_y = row_top + 19.;
                                    let row_bottom = card_resp.rect.bottom() + if is_last { 0. } else { 8. };
                                    if index > 0 {
                                        ui.painter().line_segment(
                                            [
                                                egui::pos2(spine_x, row_top - 8.),
                                                egui::pos2(spine_x, node_y - 8.),
                                            ],
                                            egui::Stroke::new(
                                                1.5_f32,
                                                if running {
                                                    phase_col.gamma_multiply(0.55)
                                                } else {
                                                    p.line
                                                },
                                            ),
                                        );
                                    }
                                    if !is_last {
                                        ui.painter().line_segment(
                                            [
                                                egui::pos2(spine_x, node_y + 8.),
                                                egui::pos2(spine_x, row_bottom),
                                            ],
                                            egui::Stroke::new(
                                                1.5_f32,
                                                if running {
                                                    phase_col.gamma_multiply(0.55)
                                                } else {
                                                    p.line
                                                },
                                            ),
                                        );
                                    }
                                    // Horizontal connector branch from timeline node to card
                                    ui.painter().line_segment(
                                        [
                                            egui::pos2(spine_x + 7.5, node_y),
                                            egui::pos2(card_resp.rect.left(), node_y),
                                        ],
                                        egui::Stroke::new(1.1_f32, design::mix(p.line, phase_col, 0.35)),
                                    );
                                    let mark_rect = egui::Rect::from_center_size(
                                        egui::pos2(spine_x, node_y),
                                        egui::vec2(16., 16.),
                                    );
                                    status_mark(
                                        ui,
                                        mark_rect,
                                        if running {
                                            Mark::Running
                                        } else if failed {
                                            Mark::Failed
                                        } else {
                                            Mark::Done
                                        },
                                        error_color,
                                        phase_col,
                                    );
                                });
                            });
                            if !is_last {
                                ui.add_space(7.);
                            }
                        }
                    });
            });
    });
}

enum Mark {
    Running,
    Done,
    Failed,
}

/// A dual orbital arc while the action runs, a glowing green tick or red cross when done.
fn status_mark(
    ui: &egui::Ui,
    r: egui::Rect,
    mark: Mark,
    error: egui::Color32,
    phase_col: egui::Color32,
) {
    let painter = ui.painter();
    let c = r.center();
    let radius = r.width() / 2. - 1.;
    match mark {
        Mark::Running => {
            design::orbital_core(ui, r, phase_col, true);
        }
        Mark::Done => {
            painter.circle_filled(c, radius + 1.5, ui.visuals().panel_fill);
            painter.circle_filled(c, radius, design::GREEN.gamma_multiply(0.18));
            painter.circle_stroke(
                c,
                radius,
                egui::Stroke::new(1.1_f32, design::GREEN.gamma_multiply(0.45)),
            );
            painter.add(egui::Shape::line(
                vec![
                    c + egui::vec2(-3.3, 0.2),
                    c + egui::vec2(-0.9, 2.7),
                    c + egui::vec2(3.6, -2.5),
                ],
                egui::Stroke::new(1.65_f32, design::GREEN),
            ));
        }
        Mark::Failed => {
            painter.circle_filled(c, radius + 1.5, ui.visuals().panel_fill);
            painter.circle_filled(c, radius, error.gamma_multiply(0.18));
            painter.circle_stroke(c, radius, egui::Stroke::new(1.1_f32, error.gamma_multiply(0.5)));
            let s = egui::Stroke::new(1.6_f32, error);
            painter.line_segment([c + egui::vec2(-2.8, -2.8), c + egui::vec2(2.8, 2.8)], s);
            painter.line_segment([c + egui::vec2(2.8, -2.8), c + egui::vec2(-2.8, 2.8)], s);
        }
    }
}

/// Text that never pushes its container wider: at most `rows` rows, the rest becomes "…".
/// (A Label in a horizontal layout ignores the job's width and would grow the whole card.)
fn clipped(ui: &mut egui::Ui, text: &str, indent: f32, rows: usize, font: egui::FontId, color: egui::Color32) {
    let width = (ui.available_width() - indent).max(40.);
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, width);
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width + indent, galley.size().y), egui::Sense::hover());
    ui.painter().galley(rect.min + egui::vec2(indent, 0.), galley, color);
}

/// The one line that says what exactly ran: `$ ./gradlew build`, a path, a subagent mission.
fn command_line(m: &Message) -> String {
    let i = &m.input;
    let s = |k: &str| i[k].as_str().unwrap_or("").trim().to_owned();
    match m.tool.as_str() {
        "Bash" | "run_command" | "commandExecution" | "mcp__relay_desktop__run_local_command" => {
            let cmd = s("command");
            let cmd = if cmd.is_empty() { s("CommandLine") } else { cmd };
            if cmd.is_empty() {
                String::new()
            } else {
                format!("$ {}", cmd.lines().collect::<Vec<_>>().join(" ⏎ "))
            }
        }
        "Grep" | "grep_search" => {
            let pat = s("pattern");
            let pat = if pat.is_empty() { s("Query") } else { pat };
            let place = s("path");
            format!(
                "«{}»{}",
                pat,
                if place.is_empty() {
                    String::new()
                } else {
                    format!(" в {place}")
                }
            )
        }
        "Glob" | "find_by_name" => {
            let pat = s("pattern");
            if pat.is_empty() { s("Pattern") } else { pat }
        }
        "SlashCommand" => s("command"),
        "WebSearch" | "search_web" => s("query"),
        "WebFetch" => s("url"),
        "Read" | "Write" | "Edit" | "MultiEdit" | "read_file" | "write_file" | "edit_file" => {
            let p = s("file_path");
            if p.is_empty() { s("Path") } else { p }
        }
        "Task" | "Agent" | "invoke_subagent" | "spawn_agent" => {
            let prompt = s("Prompt");
            let prompt = if prompt.is_empty() { s("prompt") } else { prompt };
            let prompt = if prompt.is_empty() { s("message") } else { prompt };
            model::shorten(prompt.lines().next().unwrap_or("").trim(), 120)
        }
        "mcp__relay_cluster__dispatch_task" => {
            let worker = s("worker");
            let task = s("task");
            if !worker.is_empty() && !task.is_empty() {
                format!("◈ {worker} -> {}", model::shorten(task.lines().next().unwrap_or(""), 100))
            } else {
                String::new()
            }
        }
        t if t.starts_with("mcp__relay_cluster__") => String::new(),
        _ => {
            let text = model::summary(i);
            if text.starts_with('{') { String::new() } else { text }
        }
    }
}

/// Without opening the action: the change of an edit, or the last lines of the output.
fn preview(ui: &mut egui::Ui, call: Option<&Message>, results: Vec<&Message>, running: bool, p: Palette, error: egui::Color32) {
    let edit = call.filter(|m| matches!(m.tool.as_str(), "Edit" | "MultiEdit" | "edit_file"));
    if let Some(m) = edit {
        let pairs: Vec<(String, String)> = if m.tool == "MultiEdit" {
            m.input["edits"].as_array().map(|a| a.iter().map(|e| (e["old_string"].as_str().unwrap_or("").to_owned(), e["new_string"].as_str().unwrap_or("").to_owned())).collect()).unwrap_or_default()
        } else {
            let old = m.input["old_string"]
                .as_str()
                .or_else(|| m.input["Target"].as_str())
                .unwrap_or("")
                .to_owned();
            let new = m.input["new_string"]
                .as_str()
                .or_else(|| m.input["Replacement"].as_str())
                .unwrap_or("")
                .to_owned();
            vec![(old, new)]
        };
        let mut shown = 0;
        ui.add_space(5.);
        for (old, new) in pairs {
            for (sign, text, color) in [("−", old, error), ("+", new, design::GREEN)] {
                for line in text.lines().filter(|l| !l.trim().is_empty()).take(3) {
                    if shown >= 8 {
                        break;
                    }
                    shown += 1;
                    let f_resp = egui::Frame::new()
                        .fill(color.gamma_multiply(0.10))
                        .corner_radius(5.)
                        .inner_margin(egui::Margin {
                            left: 9,
                            right: 6,
                            top: 2,
                            bottom: 2,
                        })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            clipped(
                                ui,
                                &format!("{sign} {}", line.trim_end()),
                                0.,
                                1,
                                egui::FontId::monospace(11.5),
                                color,
                            );
                        })
                        .response;
                    ui.painter().line_segment(
                        [
                            egui::pos2(f_resp.rect.left() + 1.5, f_resp.rect.top() + 3.),
                            egui::pos2(f_resp.rect.left() + 1.5, f_resp.rect.bottom() - 3.),
                        ],
                        egui::Stroke::new(2.2_f32, color),
                    );
                }
            }
        }
        return;
    }
    if running {
        ui.add_space(4.);
        ui.horizontal(|ui| {
            design::waveform(ui, accent(), 13.);
            ui.add_space(5.);
            ui.label(RichText::new("поток выполняется в реальном времени…").size(11.0).color(accent()));
        });
        return;
    }
    let Some(result) = results.last() else {
        return;
    };
    if call.is_some_and(|m| matches!(m.tool.as_str(), "Read" | "read_file")) {
        return;
    }
    let text = clean_terminal(&result.text);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() || decoded(&result.text).is_some() {
        return;
    }
    let tail = &lines[lines.len().saturating_sub(3)..];
    ui.add_space(5.);
    egui::Frame::new()
        .fill(if result.failed {
            error.gamma_multiply(0.08)
        } else {
            design::mix(p.side, p.base, 0.5)
        })
        .stroke(egui::Stroke::new(1.0_f32, p.line.gamma_multiply(0.7)))
        .corner_radius(7.)
        .inner_margin(egui::Margin::symmetric(9, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dots_r, _) = ui.allocate_exact_size(egui::vec2(26., 10.), egui::Sense::hover());
                for (di, dcol) in [
                    error.gamma_multiply(0.75),
                    egui::Color32::from_rgb(235, 180, 90).gamma_multiply(0.75),
                    design::GREEN.gamma_multiply(0.75),
                ]
                .into_iter()
                .enumerate()
                {
                    ui.painter().circle_filled(
                        egui::pos2(dots_r.left() + 3. + di as f32 * 8., dots_r.center().y),
                        2.3,
                        dcol,
                    );
                }
                if lines.len() > tail.len() {
                    let n = lines.len() - tail.len();
                    let word = if n % 10 == 1 && n % 100 != 11 {
                        "строка"
                    } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) {
                        "строки"
                    } else {
                        "строк"
                    };
                    ui.label(
                        RichText::new(format!("… ещё {n} {word} выше"))
                            .size(10.0)
                            .color(p.muted),
                    );
                } else {
                    ui.label(RichText::new("КОНСОЛЬ").size(9.5).color(p.muted));
                }
            });
            for line in tail {
                clipped(
                    ui,
                    line.trim_end(),
                    0.,
                    1,
                    egui::FontId::monospace(11.5),
                    if result.failed {
                        error
                    } else {
                        design::mix(p.text, p.muted, 0.28)
                    },
                );
            }
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
