//! Team board tab: kanban columns, categories, links and the activity of people and agents.
//! The board itself lives on the server (bridge/board.py); this view only sends operations.
use crate::ui::{self as design, Icon, Kind, Palette, ACCENT, GREEN};
use eframe::egui::{self, Align, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::{json, Value};
use std::collections::HashMap;

const COLUMN_WIDTH: f32 = 272.;

#[derive(Default)]
pub struct BoardView {
    /// Open task in the side panel.
    pub selected: Option<String>,
    filter: Option<String>,
    query: String,
    grouped: bool,
    pub log: bool,
    quick: HashMap<String, String>,
    adding: Option<String>,
    edit: Option<Edit>,
    comment: String,
    link_to: String,
    link_kind: String,
    new_category: String,
    dragging: Option<String>,
}

#[derive(Clone, PartialEq)]
struct Edit {
    id: String,
    title: String,
    description: String,
    category: String,
    priority: String,
    assignee: String,
    estimate: f32,
    progress: i32,
}

impl Edit {
    fn of(task: &Value) -> Self {
        Self {
            id: text(&task["id"]),
            title: text(&task["title"]),
            description: text(&task["description"]),
            category: text(&task["category"]),
            priority: text(&task["priority"]),
            assignee: text(&task["assignee"]),
            estimate: task["estimate"].as_f64().unwrap_or(1.) as f32,
            progress: task["progress"].as_i64().unwrap_or(0) as i32,
        }
    }
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").to_owned()
}

fn color(hex: &str) -> Color32 {
    let hex = hex.trim_start_matches('#');
    let part = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("8e"), 16).unwrap_or(142);
    Color32::from_rgb(part(0), part(2), part(4))
}

/// Colours for new categories, readable on both themes.
const SWATCHES: [&str; 8] = ["#d97757", "#5cb884", "#5b8def", "#c58af9", "#e5b84b", "#4fb6c4", "#e06c9f", "#9aa36b"];

fn ago(ts: f64) -> String {
    let seconds = (chrono::Local::now().timestamp() as f64 - ts).max(0.) as i64;
    match seconds {
        s if s < 60 => "сейчас".into(),
        s if s < 3600 => format!("{} мин", s / 60),
        s if s < 86400 => format!("{} ч", s / 3600),
        s => format!("{} дн", s / 86400),
    }
}

fn agent(name: &str) -> bool {
    name.starts_with("Claude") || name.starts_with("agent")
}

fn initials(name: &str) -> String {
    if agent(name) {
        return "AI".into();
    }
    name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase()
}

fn small(ui: &mut egui::Ui, s: impl Into<String>, size: f32, color: Color32) -> egui::Response {
    ui.label(RichText::new(s.into()).size(size).color(color))
}

fn bar(ui: &mut egui::Ui, width: f32, height: f32, value: f32, fill: Color32, p: Palette) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    ui.painter().rect_filled(r, height / 2., p.line);
    let mut done = r;
    done.set_width(r.width() * (value / 100.).clamp(0., 1.));
    if done.width() > 0. {
        ui.painter().rect_filled(done, height / 2., fill);
    }
}

pub fn show(ui: &mut egui::Ui, state: &Value, view: &mut BoardView, p: Palette, me: &str) -> Vec<Value> {
    let mut ops = vec![];
    let tasks: Vec<&Value> = state["tasks"].as_array().map(|t| t.iter().collect()).unwrap_or_default();
    let categories: Vec<&Value> = {
        let mut c: Vec<&Value> = state["categories"].as_array().map(|c| c.iter().collect()).unwrap_or_default();
        c.sort_by(|a, b| a["order"].as_f64().unwrap_or(0.).total_cmp(&b["order"].as_f64().unwrap_or(0.)));
        c
    };
    let category_color: HashMap<String, Color32> =
        categories.iter().map(|c| (text(&c["id"]), color(c["color"].as_str().unwrap_or("#8e8e96")))).collect();
    let category_name: HashMap<String, String> = categories.iter().map(|c| (text(&c["id"]), text(&c["name"]))).collect();

    if view.selected.as_ref().is_some_and(|id| !tasks.iter().any(|t| t["id"] == id.as_str())) {
        view.selected = None;
    }
    if view.selected.is_some() || view.log {
        egui::SidePanel::right("board-detail")
            .exact_width(380.)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.side).inner_margin(16.))
            .show_inside(ui, |ui| {
                if let Some(id) = view.selected.clone() {
                    if let Some(task) = tasks.iter().find(|t| t["id"] == id.as_str()) {
                        detail(ui, task, state, &tasks, &categories, view, p, me, &mut ops);
                    }
                } else {
                    activity(ui, state, view, p);
                }
            });
    }

    egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(ui, |ui| {
    // Header: overall progress, categories with their own progress, filters.
    let progress = &state["progress"];
    ui.add_space(14.);
    ui.horizontal(|ui| {
        ui.add_space(18.);
        small(ui, state["title"].as_str().unwrap_or("Доска"), 18., p.text);
        ui.add_space(10.);
        let all = &progress["all"];
        let percent = all["percent"].as_f64().unwrap_or(0.) as f32;
        bar(ui, 180., 6., percent, GREEN, p);
        small(
            ui,
            format!("{percent:.0}% · готово {} из {}", all["done"].as_u64().unwrap_or(0), all["tasks"].as_u64().unwrap_or(0)),
            12.5,
            p.muted,
        )
        .on_hover_text("Доля сделанного с учётом размера задач (оценка в очках) и частичного прогресса");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(14.);
            if design::button(ui, "", Some(Icon::Refresh), Kind::Ghost, p, Vec2::splat(28.))
                .on_hover_text("Журнал: кто, что и откуда менял")
                .clicked()
            {
                view.log = !view.log;
                view.selected = None;
            }
            let label = if view.grouped { "По категориям ✓" } else { "По категориям" };
            if design::button(ui, label, None, Kind::Ghost, p, Vec2::new(0., 28.)).clicked() {
                view.grouped = !view.grouped;
            }
            egui::Frame::new()
                .fill(design::mix(p.base, p.line, 0.5))
                .corner_radius(8.)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut view.query)
                            .frame(false)
                            .desired_width(150.)
                            .hint_text(RichText::new("Поиск задач").color(p.muted)),
                    );
                });
        });
    });
    ui.add_space(8.);
    let width = ui.available_width() - 36.;
    ui.horizontal(|ui| {
        ui.add_space(18.);
        ui.allocate_ui_with_layout(Vec2::new(width, 0.), Layout::left_to_right(Align::Center).with_main_wrap(true), |ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6., 6.);
        for c in &categories {
            let id = text(&c["id"]);
            let stats = &progress["categories"][&id];
            let active = view.filter.as_deref() == Some(id.as_str());
            let fill = category_color.get(&id).copied().unwrap_or(p.muted);
            let response = egui::Frame::new()
                .fill(if active { fill.gamma_multiply(0.22) } else { design::mix(p.base, p.line, 0.35) })
                .stroke(Stroke::new(1.0_f32, if active { fill } else { Color32::TRANSPARENT }))
                .corner_radius(12.)
                .inner_margin(egui::Margin::symmetric(9, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(8.), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4., fill);
                        small(ui, text(&c["name"]), 12., p.text);
                        bar(ui, 34., 4., stats["percent"].as_f64().unwrap_or(0.) as f32, fill, p);
                        small(ui, format!("{:.0}%", stats["percent"].as_f64().unwrap_or(0.)), 11., p.muted);
                    });
                })
                .response
                .interact(Sense::click());
            if response
                .on_hover_text(format!(
                    "{} · {} из {} задач готово\nНажми, чтобы показать только её",
                    text(&c["name"]),
                    stats["done"].as_u64().unwrap_or(0),
                    stats["tasks"].as_u64().unwrap_or(0)
                ))
                .clicked()
            {
                view.filter = if active { None } else { Some(id) };
            }
        }
        });
    });
    ui.add_space(10.);

    let query = view.query.to_lowercase();
    let filter = view.filter.clone();
    let visible = |t: &&Value| {
        filter.as_deref().is_none_or(|f| t["category"] == f)
            && (query.is_empty()
                || text(&t["title"]).to_lowercase().contains(&query)
                || text(&t["id"]).to_lowercase() == query
                || text(&t["assignee"]).to_lowercase().contains(&query))
    };
    let mut card_rects: Vec<(String, String, Rect)> = vec![];
    let mut column_rects: Vec<(String, Rect)> = vec![];
    let mut hovered: Option<String> = None;
    let columns: Vec<(String, String)> = state["columns"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| (text(&c["id"]), text(&c["name"])))
        .collect();

    egui::ScrollArea::horizontal().id_salt("board-columns").auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.add_space(18.);
            for (status, name) in &columns {
                let mut column: Vec<&Value> = tasks.iter().copied().filter(|t| t["status"] == status.as_str()).filter(visible).collect();
                column.sort_by(|a, b| a["order"].as_f64().unwrap_or(0.).total_cmp(&b["order"].as_f64().unwrap_or(0.)));
                if view.grouped {
                    let rank: HashMap<String, usize> = categories.iter().enumerate().map(|(i, c)| (text(&c["id"]), i)).collect();
                    column.sort_by_key(|t| rank.get(&text(&t["category"])).copied().unwrap_or(usize::MAX));
                }
                let frame = egui::Frame::new()
                    .fill(design::mix(p.base, p.side, 0.7))
                    .corner_radius(12.)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| ui.vertical(|ui| {
                        ui.set_width(COLUMN_WIDTH);
                        ui.set_min_height(ui.available_height() - 12.);
                        ui.horizontal(|ui| {
                            ui.add_space(4.);
                            small(ui, name, 13., p.text);
                            small(ui, column.len().to_string(), 12., p.muted);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if design::button(ui, "", Some(Icon::Plus), Kind::Ghost, p, Vec2::splat(24.))
                                    .on_hover_text("Новая задача в этой колонке")
                                    .clicked()
                                {
                                    view.adding = Some(status.clone());
                                }
                            });
                        });
                        ui.add_space(4.);
                        if view.adding.as_deref() == Some(status.as_str()) {
                            let draft = view.quick.entry(status.clone()).or_default();
                            let response = ui.add(
                                egui::TextEdit::multiline(draft)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("Название задачи · Enter — добавить, Esc — отмена"),
                            );
                            response.request_focus();
                            if ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
                                let title = draft.trim().to_owned();
                                if !title.is_empty() {
                                    ops.push(json!({"op":"create","title":title,"status":status,"category":view.filter.clone().unwrap_or_default()}));
                                }
                                draft.clear();
                                view.adding = None;
                            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                draft.clear();
                                view.adding = None;
                            }
                            ui.add_space(6.);
                        }
                        egui::ScrollArea::vertical().id_salt(("board-column", status)).auto_shrink([false, false]).show(ui, |ui| {
                            let mut group = None;
                            for task in &column {
                                if view.grouped {
                                    let c = text(&task["category"]);
                                    if group.as_ref() != Some(&c) {
                                        ui.add_space(4.);
                                        small(
                                            ui,
                                            category_name.get(&c).cloned().unwrap_or_else(|| "Без категории".into()).to_uppercase(),
                                            10.5,
                                            category_color.get(&c).copied().unwrap_or(p.muted),
                                        );
                                        group = Some(c);
                                    }
                                }
                                let id = text(&task["id"]);
                                let rect = card(ui, task, view, &category_color, p);
                                let response = ui.interact(rect, ui.id().with(("card", &id)), Sense::click_and_drag());
                                if response.hovered() && view.dragging.is_none() {
                                    hovered = Some(id.clone());
                                }
                                if response.drag_started() {
                                    view.dragging = Some(id.clone());
                                }
                                if response.clicked() {
                                    view.selected = Some(id.clone());
                                    view.log = false;
                                    view.edit = None;
                                }
                                card_rects.push((id, status.clone(), rect));
                                ui.add_space(6.);
                            }
                            if column.is_empty() {
                                ui.add_space(10.);
                                ui.vertical_centered(|ui| small(ui, "Пусто", 12., p.muted));
                            }
                        });
                    }));
                column_rects.push((status.clone(), frame.response.rect));
                ui.add_space(10.);
            }
        });
    });

    // Links of the hovered card, drawn over the columns.
    if let Some(id) = &hovered {
        let rect_of = |id: &str| card_rects.iter().find(|(c, _, _)| c == id).map(|(_, _, r)| *r);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("board-links")));
        let mut pairs = vec![];
        for task in &tasks {
            for link in task["links"].as_array().into_iter().flatten() {
                let from = text(&task["id"]);
                let to = text(&link["to"]);
                if &from == id || &to == id {
                    pairs.push((from, to, text(&link["kind"])));
                }
            }
        }
        for (from, to, kind) in pairs {
            if let (Some(a), Some(b)) = (rect_of(&from), rect_of(&to)) {
                let color = if kind == "blocks" { ACCENT } else { p.muted };
                let (start, end) = if a.center().x <= b.center().x {
                    (a.right_center(), b.left_center())
                } else {
                    (a.left_center(), b.right_center())
                };
                let bend = ((end.x - start.x).abs() * 0.4).max(30.);
                let dir = if end.x >= start.x { 1. } else { -1. };
                let curve = egui::epaint::CubicBezierShape::from_points_stroke(
                    [start, start + Vec2::new(bend * dir, 0.), end - Vec2::new(bend * dir, 0.), end],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(2.0_f32, color),
                );
                painter.add(curve);
                painter.circle_filled(end, 3.5, color);
                for r in [a, b] {
                    painter.rect_stroke(r, 10., Stroke::new(1.5_f32, color), egui::StrokeKind::Outside);
                }
            }
        }
    }

    // Dragging: ghost under the pointer, insertion mark, drop on release.
    if let Some(dragged) = view.dragging.clone() {
        let pointer = ui.ctx().pointer_interact_pos();
        let target = pointer.and_then(|pos| {
            let (status, _) = column_rects.iter().find(|(_, r)| r.x_range().contains(pos.x))?;
            let before = card_rects
                .iter()
                .filter(|(id, s, r)| s == status && id != &dragged && r.center().y > pos.y)
                .min_by(|a, b| a.2.top().total_cmp(&b.2.top()))
                .map(|(id, _, r)| (id.clone(), *r));
            Some((status.clone(), before))
        });
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("board-drag")));
        if let (Some(pos), Some(task)) = (pointer, tasks.iter().find(|t| t["id"] == dragged.as_str())) {
            let ghost = Rect::from_min_size(pos + Vec2::new(-40., -14.), Vec2::new(COLUMN_WIDTH - 20., 34.));
            painter.rect_filled(ghost, 10., p.surface.gamma_multiply(0.95));
            painter.rect_stroke(ghost, 10., Stroke::new(1.0_f32, ACCENT), egui::StrokeKind::Inside);
            painter.text(
                ghost.left_center() + Vec2::new(12., 0.),
                egui::Align2::LEFT_CENTER,
                crate::model::shorten(&text(&task["title"]), 34),
                FontId::proportional(13.),
                p.text,
            );
        }
        if let Some((status, before)) = &target {
            let column = column_rects.iter().find(|(s, _)| s == status).map(|(_, r)| *r);
            let y = match before {
                Some((_, r)) => r.top() - 3.,
                None => card_rects
                    .iter()
                    .filter(|(id, s, _)| s == status && id != &dragged)
                    .map(|(_, _, r)| r.bottom() + 3.)
                    .fold(column.map_or(0., |c| c.top() + 44.), f32::max),
            };
            if let Some(c) = column {
                painter.line_segment([egui::pos2(c.left() + 10., y), egui::pos2(c.right() - 10., y)], Stroke::new(2.5_f32, ACCENT));
            }
        }
        if ui.input(|i| i.pointer.any_released()) {
            if let Some((status, before)) = target {
                let mut op = json!({"op":"move","id":dragged,"status":status});
                if let Some((id, _)) = before {
                    op["before"] = json!(id);
                }
                ops.push(op);
            }
            view.dragging = None;
        }
        ui.ctx().request_repaint();
    }
    });
    let _ = me;
    ops
}

fn card(ui: &mut egui::Ui, task: &Value, view: &BoardView, colors: &HashMap<String, Color32>, p: Palette) -> Rect {
    let id = text(&task["id"]);
    let selected = view.selected.as_deref() == Some(id.as_str());
    let dragged = view.dragging.as_deref() == Some(id.as_str());
    let category = colors.get(&text(&task["category"])).copied().unwrap_or(p.line);
    let done = task["status"] == "done";
    let inner = egui::Frame::new()
        .fill(if selected { design::mix(p.surface, ACCENT, 0.1) } else { p.surface })
        .stroke(Stroke::new(1.0_f32, if selected { ACCENT } else { p.line }))
        .corner_radius(10.)
        .inner_margin(egui::Margin { left: 14, right: 10, top: 9, bottom: 8 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if dragged {
                ui.set_opacity(0.35);
            }
            ui.spacing_mut().item_spacing.y = 5.;
            let title = RichText::new(text(&task["title"])).size(13.5).color(if done { p.muted } else { p.text });
            ui.add(egui::Label::new(if done { title.strikethrough() } else { title }).wrap().selectable(false));
            let progress = task["progress"].as_f64().unwrap_or(0.) as f32;
            if !done && progress > 0. {
                bar(ui, ui.available_width(), 3., progress, category, p);
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.;
                small(ui, &id, 11., p.muted);
                match task["priority"].as_str().unwrap_or("") {
                    "critical" => small(ui, "срочно", 11., ACCENT),
                    "high" => small(ui, "важно", 11., ACCENT.gamma_multiply(0.8)),
                    _ => ui.label(""),
                };
                let links = task["links"].as_array().map_or(0, Vec::len);
                if links > 0 {
                    small(ui, format!("⛓{links}"), 11., p.muted).on_hover_text("Связи: наведи на карточку, чтобы увидеть их на доске");
                }
                let comments = task["comments"].as_array().map_or(0, Vec::len);
                if comments > 0 {
                    small(ui, format!("💬{comments}"), 11., p.muted).on_hover_text("Комментарии");
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let who = text(&task["assignee"]);
                    if !who.is_empty() {
                        let (r, response) = ui.allocate_exact_size(Vec2::splat(20.), Sense::hover());
                        let fill = if agent(&who) { design::mix(p.surface, ACCENT, 0.35) } else { design::mix(p.surface, GREEN, 0.35) };
                        ui.painter().circle_filled(r.center(), 10., fill);
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, initials(&who), FontId::proportional(9.5), p.text);
                        response.on_hover_text(&who);
                    }
                    if let Some(est) = task["estimate"].as_f64().filter(|e| *e != 1.) {
                        small(ui, format!("{est}"), 11., p.muted).on_hover_text("Размер в очках");
                    }
                });
            });
        });
    let rect = inner.response.rect;
    ui.painter().rect_filled(
        Rect::from_min_max(rect.left_top() + Vec2::new(0., 8.), egui::pos2(rect.left() + 3., rect.bottom() - 8.)),
        2.,
        category,
    );
    rect
}

#[allow(clippy::too_many_arguments)]
fn detail(
    ui: &mut egui::Ui,
    task: &Value,
    state: &Value,
    tasks: &[&Value],
    categories: &[&Value],
    view: &mut BoardView,
    p: Palette,
    me: &str,
    ops: &mut Vec<Value>,
) {
    let id = text(&task["id"]);
    if view.edit.as_ref().is_none_or(|e| e.id != id) {
        view.edit = Some(Edit::of(task));
        view.comment.clear();
        view.link_to.clear();
        view.link_kind = "blocks".into();
    }
    let original = Edit::of(task);
    let mut close = false;
    egui::ScrollArea::vertical().id_salt("board-detail-scroll").show(ui, |ui| {
        let edit = view.edit.as_mut().unwrap();
        ui.horizontal(|ui| {
            small(ui, &id, 12., p.muted);
            let status = text(&task["status"]);
            let name = state["columns"]
                .as_array()
                .and_then(|c| c.iter().find(|c| c["id"] == status.as_str()))
                .map(|c| text(&c["name"]))
                .unwrap_or_default();
            small(ui, format!("· {name}"), 12., p.muted);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if design::button(ui, "", Some(Icon::Close), Kind::Ghost, p, Vec2::splat(26.)).clicked() {
                    close = true;
                }
            });
        });
        ui.add(egui::TextEdit::multiline(&mut edit.title).font(FontId::proportional(16.)).desired_rows(1).desired_width(f32::INFINITY));
        ui.add_space(6.);
        egui::Grid::new("task-fields").num_columns(2).spacing([10., 6.]).show(ui, |ui| {
            small(ui, "Категория", 12., p.muted);
            egui::ComboBox::from_id_salt("task-category")
                .selected_text(
                    categories
                        .iter()
                        .find(|c| c["id"] == edit.category.as_str())
                        .map(|c| text(&c["name"]))
                        .unwrap_or_else(|| "—".into()),
                )
                .width(200.)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut edit.category, String::new(), "—");
                    for c in categories {
                        ui.selectable_value(&mut edit.category, text(&c["id"]), text(&c["name"]));
                    }
                });
            ui.end_row();
            small(ui, "Статус", 12., p.muted);
            let mut status = text(&task["status"]);
            egui::ComboBox::from_id_salt("task-status")
                .selected_text(
                    state["columns"]
                        .as_array()
                        .and_then(|c| c.iter().find(|c| c["id"] == status.as_str()))
                        .map(|c| text(&c["name"]))
                        .unwrap_or_default(),
                )
                .width(200.)
                .show_ui(ui, |ui| {
                    for c in state["columns"].as_array().into_iter().flatten() {
                        ui.selectable_value(&mut status, text(&c["id"]), text(&c["name"]));
                    }
                });
            if status != task["status"].as_str().unwrap_or("") {
                ops.push(json!({"op":"move","id":id,"status":status}));
            }
            ui.end_row();
            small(ui, "Исполнитель", 12., p.muted);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut edit.assignee).desired_width(140.).hint_text("никто"));
                if !me.is_empty() && edit.assignee != me && ui.small_button("я").on_hover_text("Взять себе").clicked() {
                    edit.assignee = me.to_owned();
                }
            });
            ui.end_row();
            small(ui, "Приоритет", 12., p.muted);
            egui::ComboBox::from_id_salt("task-priority")
                .selected_text(match edit.priority.as_str() {
                    "low" => "низкий",
                    "high" => "важно",
                    "critical" => "срочно",
                    _ => "обычный",
                })
                .width(200.)
                .show_ui(ui, |ui| {
                    for (v, n) in [("low", "низкий"), ("", "обычный"), ("high", "важно"), ("critical", "срочно")] {
                        ui.selectable_value(&mut edit.priority, v.into(), n);
                    }
                });
            ui.end_row();
            small(ui, "Размер", 12., p.muted);
            ui.add(egui::DragValue::new(&mut edit.estimate).range(0.5..=40.).speed(0.25).suffix(" оч."))
                .on_hover_text("Относительный размер: 1 — мелочь, 8 — большая фича. От него считается прогресс.");
            ui.end_row();
            small(ui, "Готовность", 12., p.muted);
            ui.add(egui::Slider::new(&mut edit.progress, 0..=100).suffix("%"));
            ui.end_row();
        });
        ui.add_space(6.);
        small(ui, "Описание", 12., p.muted);
        ui.add(
            egui::TextEdit::multiline(&mut edit.description)
                .desired_rows(5)
                .desired_width(f32::INFINITY)
                .hint_text("Что сделать, критерии готовности, ссылки на файлы"),
        );
        if *edit != original {
            ui.add_space(6.);
            ui.horizontal(|ui| {
                if design::button(ui, "Сохранить", Some(Icon::Check), Kind::Primary, p, Vec2::new(0., 30.)).clicked() {
                    ops.push(json!({"op":"update","id":id,"title":edit.title.trim(),"description":edit.description,"category":edit.category,"priority":edit.priority,"assignee":edit.assignee.trim(),"estimate":edit.estimate,"progress":edit.progress}));
                }
                if design::button(ui, "Отменить", None, Kind::Ghost, p, Vec2::new(0., 30.)).clicked() {
                    *edit = original.clone();
                }
            });
        }

        ui.add_space(12.);
        small(ui, "Связи", 13., p.text);
        for link in task["links"].as_array().into_iter().flatten() {
            let to = text(&link["to"]);
            let title = tasks.iter().find(|t| t["id"] == to.as_str()).map(|t| text(&t["title"])).unwrap_or_default();
            ui.horizontal(|ui| {
                let kind = match link["kind"].as_str() {
                    Some("blocks") => "блокирует",
                    Some("duplicates") => "дублирует",
                    _ => "связана с",
                };
                small(ui, format!("{kind} {to}"), 12., if link["kind"] == "blocks" { ACCENT } else { p.muted });
                if ui.link(RichText::new(crate::model::shorten(&title, 30)).size(12.)).clicked() {
                    view.selected = Some(to.clone());
                }
                if ui.small_button("×").on_hover_text("Убрать связь").clicked() {
                    ops.push(json!({"op":"unlink","id":id,"to":to}));
                }
            });
        }
        let incoming: Vec<_> = tasks
            .iter()
            .filter(|t| t["links"].as_array().is_some_and(|l| l.iter().any(|l| l["to"] == id.as_str() && l["kind"] == "blocks")))
            .map(|t| text(&t["id"]))
            .collect();
        if !incoming.is_empty() {
            small(ui, format!("Ждёт: {}", incoming.join(", ")), 12., ACCENT);
        }
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("link-kind")
                .selected_text(match view.link_kind.as_str() {
                    "blocks" => "блокирует",
                    "duplicates" => "дублирует",
                    _ => "связана с",
                })
                .width(100.)
                .show_ui(ui, |ui| {
                    for (v, n) in [("blocks", "блокирует"), ("relates", "связана с"), ("duplicates", "дублирует")] {
                        ui.selectable_value(&mut view.link_kind, v.into(), n);
                    }
                });
            ui.add(egui::TextEdit::singleline(&mut view.link_to).desired_width(70.).hint_text("T-12"));
            let target = view.link_to.trim().to_uppercase();
            if ui.add_enabled(tasks.iter().any(|t| t["id"] == target.as_str()) && target != id, egui::Button::new("Связать")).clicked() {
                ops.push(json!({"op":"link","id":id,"to":target,"kind":view.link_kind}));
                view.link_to.clear();
            }
        });

        ui.add_space(12.);
        small(ui, "Обсуждение и история", 13., p.text);
        let mut entries: Vec<(f64, String, String, bool)> = task["comments"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| (c["ts"].as_f64().unwrap_or(0.), text(&c["author"]), text(&c["text"]), true))
            .collect();
        for event in state["activity"].as_array().into_iter().flatten().filter(|e| e["task"] == id.as_str() && e["op"] != "comment") {
            entries.push((
                event["ts"].as_f64().unwrap_or(0.),
                format!("{} · {}", text(&event["actor"]), text(&event["host"])),
                text(&event["text"]),
                false,
            ));
        }
        entries.sort_by(|a, b| b.0.total_cmp(&a.0));
        ui.add(egui::TextEdit::multiline(&mut view.comment).desired_rows(2).desired_width(f32::INFINITY).hint_text("Комментарий"));
        if !view.comment.trim().is_empty() && design::button(ui, "Добавить", None, Kind::Secondary, p, Vec2::new(0., 28.)).clicked() {
            ops.push(json!({"op":"comment","id":id,"text":view.comment.trim()}));
            view.comment.clear();
        }
        for (ts, who, body, comment) in entries.iter().take(80) {
            ui.add_space(4.);
            ui.horizontal(|ui| {
                small(ui, who, 11.5, if agent(who) { ACCENT } else { p.text });
                small(ui, ago(*ts), 11., p.muted);
            });
            ui.add(egui::Label::new(RichText::new(body).size(12.5).color(if *comment { p.text } else { p.muted })).wrap());
        }
        ui.add_space(12.);
        let created = format!(
            "Создал {} · {} назад",
            text(&task["created_by"]),
            ago(task["created_at"].as_f64().unwrap_or(0.))
        );
        small(ui, created, 11., p.muted);
        if let Some(done) = task["done_at"].as_f64() {
            small(ui, format!("Готово: {} · {} назад", text(&task["done_by"]), ago(done)), 11., p.muted);
        }
        ui.add_space(8.);
        if ui.button(RichText::new("Удалить задачу").color(ACCENT).size(12.)).double_clicked() {
            ops.push(json!({"op":"delete","id":id}));
            close = true;
        }
        small(ui, "двойной щелчок, чтобы удалить", 10.5, p.muted);
    });
    if close {
        view.selected = None;
        view.edit = None;
    }
}

fn activity(ui: &mut egui::Ui, state: &Value, view: &mut BoardView, p: Palette) {
    ui.horizontal(|ui| {
        small(ui, "Журнал", 15., p.text);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if design::button(ui, "", Some(Icon::Close), Kind::Ghost, p, Vec2::splat(26.)).clicked() {
                view.log = false;
            }
        });
    });
    small(ui, "Кто, что и с какого хоста менял — люди и агенты", 11.5, p.muted);
    ui.add_space(6.);
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut view.new_category).desired_width(170.).hint_text("Новая категория"));
        if !view.new_category.trim().is_empty() && ui.button("Добавить").clicked() {
            view.selected = None;
            let n = state["categories"].as_array().map_or(0, Vec::len);
            view.quick.insert(
                "__category".into(),
                json!({"op":"category","id":view.new_category.trim().to_lowercase().replace(' ', "-"),"name":view.new_category.trim(),"color":SWATCHES[n % SWATCHES.len()],"order":n}).to_string(),
            );
            view.new_category.clear();
        }
    });
    ui.add_space(6.);
    egui::ScrollArea::vertical().id_salt("board-activity").show(ui, |ui| {
        for event in state["activity"].as_array().into_iter().flatten().take(200) {
            let actor = text(&event["actor"]);
            ui.horizontal(|ui| {
                small(ui, &actor, 12., if agent(&actor) { ACCENT } else { p.text });
                small(ui, format!("· {} · {}", text(&event["host"]), ago(event["ts"].as_f64().unwrap_or(0.))), 11., p.muted);
            });
            ui.horizontal_wrapped(|ui| {
                let task = text(&event["task"]);
                if task.starts_with("T-") && ui.link(RichText::new(&task).size(12.)).clicked() {
                    view.selected = Some(task.clone());
                    view.log = false;
                }
                small(ui, text(&event["text"]), 12., p.muted);
            });
            ui.add_space(6.);
        }
    });
}

/// Operations queued from places that cannot return them directly (the journal panel).
pub fn take_queued(view: &mut BoardView) -> Option<Value> {
    view.quick.remove("__category").and_then(|s| serde_json::from_str(&s).ok())
}
