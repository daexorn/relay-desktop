//! Team board tab: a canvas you pan with the mouse, with kanban columns or category lanes,
//! links between tasks and the activity of people and agents. The board itself lives on the
//! server (bridge/board.py); this view only sends operations.
use crate::ui::{self as design, Icon, Kind, Palette, ACCENT, GREEN};
use eframe::egui::{
    self, epaint::Shadow, text::LayoutJob, Align, Color32, FontId, Galley, Layout, Pos2, Rect,
    RichText, Sense, Shape, Stroke, Vec2,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const COLUMN: f32 = 292.;
const FRAME: f32 = 300.;
const FRAME_HEAD: f32 = 64.;
const FRAME_GAP: f32 = 40.;
/// Inside a category frame, work in progress comes first and finished work last.
const FRAME_STATUSES: [&str; 5] = ["doing", "review", "todo", "backlog", "done"];
const CELL: f32 = 256.;
const LABEL: f32 = 228.;
const GAP: f32 = 18.;
const HEADER: f32 = 46.;
const INNER: f32 = 10.;
const CARD_GAP: f32 = 8.;
/// Finished cards shown before "show more": the history is long and rarely needed.
const COLLAPSED: usize = 8;
const LANE_COLLAPSED: usize = 3;
const SWATCHES: [&str; 16] = [
    "#d97757", "#5cb884", "#5b8def", "#c58af9", "#e5b84b", "#4fb6c4", "#e06c9f", "#9aa36b",
    "#b08968", "#8e8e96", "#e8845c", "#6fbf73", "#7aa2f7", "#d4a5ff", "#f2c94c", "#56c2b0",
];

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Categories as frames you can put anywhere on the canvas.
    Canvas,
    Columns,
    Lanes,
}

struct FrameDrag {
    id: String,
    grab: Vec2,
    pos: Pos2,
}

struct Drag {
    id: String,
    grab: Vec2,
    /// Some(kind) when Shift was held: draws a link instead of moving the card.
    link: Option<&'static str>,
}

/// The category editor: buffers for names being typed, so a rename is sent once, when done.
struct Manager {
    names: HashMap<String, String>,
    descriptions: HashMap<String, String>,
    new_name: String,
    new_color: String,
    focus_new: bool,
    focus: Option<String>,
    focus_once: bool,
    palette: Option<String>,
    deleting: Option<(String, String)>,
    drag: Option<String>,
}

impl Manager {
    fn open(board: &Board, focus: Option<String>) -> Self {
        Self {
            names: HashMap::new(),
            descriptions: HashMap::new(),
            new_name: String::new(),
            new_color: SWATCHES[board.categories.len() % SWATCHES.len()].into(),
            focus_new: focus.is_none(),
            focus_once: focus.is_some(),
            focus,
            palette: None,
            deleting: None,
            drag: None,
        }
    }
}

struct Create {
    title: String,
    status: String,
    category: String,
    focus: bool,
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

pub struct BoardView {
    /// Open task in the side panel.
    pub selected: Option<String>,
    pub log: bool,
    mode: Mode,
    filter: Option<String>,
    query: String,
    links: bool,
    expanded: HashSet<String>,
    origin: Pos2,
    scale: f32,
    viewport: Vec2,
    content: Rect,
    focus: Option<String>,
    drag: Option<Drag>,
    create: Option<Create>,
    edit: Option<Edit>,
    comment: String,
    link_to: String,
    link_kind: String,
    queued: Vec<Value>,
    /// View changes made while drawing the canvas; applied after the scene stores its own.
    pending: Option<(Pos2, f32)>,
    fitted: bool,
    manager: Option<Manager>,
    chip_drag: Option<String>,
    lane_drag: Option<String>,
    /// Preview only: open the category editor on the first frame (focus, palette, delete).
    demo_editor: Option<(Option<String>, bool, bool)>,
    /// Screen rects of draggable things from the last frame (chips, lane labels).
    hits: HashMap<String, Rect>,
    frame_drag: Option<FrameDrag>,
    /// Frames just dropped, shown in place until the server confirms the move.
    placed: HashMap<String, Pos2>,
}

impl Default for BoardView {
    fn default() -> Self {
        Self {
            selected: None,
            log: false,
            mode: Mode::Canvas,
            filter: None,
            query: String::new(),
            links: false,
            expanded: HashSet::new(),
            origin: Pos2::new(-24., -18.),
            scale: 1.,
            viewport: Vec2::splat(800.),
            content: Rect::NOTHING,
            focus: None,
            drag: None,
            create: None,
            edit: None,
            comment: String::new(),
            link_to: String::new(),
            link_kind: "blocks".into(),
            queued: vec![],
            pending: None,
            fitted: false,
            manager: None,
            chip_drag: None,
            lane_drag: None,
            demo_editor: None,
            hits: HashMap::new(),
            frame_drag: None,
            placed: HashMap::new(),
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

fn ago(ts: f64) -> String {
    let seconds = (chrono::Local::now().timestamp() as f64 - ts).max(0.) as i64;
    match seconds {
        s if s < 60 => "сейчас".into(),
        s if s < 3600 => format!("{} мин назад", s / 60),
        s if s < 86400 => format!("{} ч назад", s / 3600),
        s => format!("{} дн назад", s / 86400),
    }
}

fn agent(name: &str) -> bool {
    name.starts_with("Claude") || name.starts_with("agent")
}

fn initials(name: &str) -> String {
    if agent(name) {
        return "AI".into();
    }
    name.split(|c: char| c.is_whitespace() || c == '·')
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

/// A stable colour per person, so the same teammate looks the same everywhere.
fn person(name: &str) -> Color32 {
    if agent(name) {
        return ACCENT;
    }
    let hash = name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    color(SWATCHES[1 + hash as usize % (SWATCHES.len() - 1)])
}

fn small(ui: &mut egui::Ui, s: impl Into<String>, size: f32, color: Color32) -> egui::Response {
    ui.label(RichText::new(s.into()).size(size).color(color))
}

fn bar(painter: &egui::Painter, r: Rect, value: f32, fill: Color32, p: Palette) {
    painter.rect_filled(r, r.height() / 2., p.line);
    let mut done = r;
    done.set_width(r.width() * (value / 100.).clamp(0., 1.));
    if done.width() > 0.5 {
        painter.rect_filled(done, r.height() / 2., fill);
    }
}

fn status_name(state: &Value, status: &str) -> String {
    state["columns"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["id"] == status))
        .map(|c| text(&c["name"]))
        .unwrap_or_default()
}

fn status_color(status: &str, p: Palette) -> Color32 {
    match status {
        "doing" => ACCENT,
        "review" => color("#e5b84b"),
        "done" => GREEN,
        "todo" => color("#5b8def"),
        _ => p.muted,
    }
}

impl BoardView {
    /// Preview flags: --demo-lanes, --demo-links.
    pub fn demo(&mut self, args: &[String]) {
        if args.iter().any(|a| a == "--demo-lanes") {
            self.mode = Mode::Lanes;
        }
        if args.iter().any(|a| a == "--demo-columns") {
            self.mode = Mode::Columns;
        }
        self.links = args.iter().any(|a| a == "--demo-links");
        if let Some(i) = args.iter().position(|a| a == "--demo-categories") {
            let focus = args.get(i + 1).filter(|a| !a.starts_with("--")).cloned();
            self.demo_editor = Some((focus, args.iter().any(|a| a == "--demo-palette"), args.iter().any(|a| a == "--demo-delete")));
        }
    }
}

/// New order of categories after moving `dragged` in front of `before` (or to the end);
/// None when nothing actually moves, so the journal stays clean.
fn reorder(board: &Board, dragged: &str, before: Option<&str>) -> Option<Value> {
    let current: Vec<String> = board.categories.iter().map(|c| text(&c["id"])).collect();
    let mut ids: Vec<String> = current.iter().filter(|id| *id != dragged).cloned().collect();
    let at = before.and_then(|b| ids.iter().position(|id| id == b)).unwrap_or(ids.len());
    ids.insert(at, dragged.to_owned());
    (ids != current).then(|| json!({"op":"reorder_categories","ids":ids}))
}

pub fn take_queued(view: &mut BoardView) -> Vec<Value> {
    std::mem::take(&mut view.queued)
}

struct Board<'a> {
    state: &'a Value,
    tasks: Vec<&'a Value>,
    categories: Vec<&'a Value>,
    colors: HashMap<String, Color32>,
    names: HashMap<String, String>,
    columns: Vec<(String, String)>,
}

impl<'a> Board<'a> {
    fn new(state: &'a Value) -> Self {
        let tasks: Vec<&Value> = state["tasks"].as_array().map(|t| t.iter().collect()).unwrap_or_default();
        let mut categories: Vec<&Value> = state["categories"].as_array().map(|c| c.iter().collect()).unwrap_or_default();
        categories.sort_by(|a, b| a["order"].as_f64().unwrap_or(0.).total_cmp(&b["order"].as_f64().unwrap_or(0.)));
        let colors = categories.iter().map(|c| (text(&c["id"]), color(c["color"].as_str().unwrap_or("#8e8e96")))).collect();
        let names = categories.iter().map(|c| (text(&c["id"]), text(&c["name"]))).collect();
        let columns = state["columns"].as_array().into_iter().flatten().map(|c| (text(&c["id"]), text(&c["name"]))).collect();
        Self { state, tasks, categories, colors, names, columns }
    }
    fn task(&self, id: &str) -> Option<&'a Value> {
        self.tasks.iter().copied().find(|t| t["id"] == id)
    }
    fn tint(&self, category: &str, p: Palette) -> Color32 {
        self.colors.get(category).copied().unwrap_or(p.muted)
    }
}

pub fn show(ui: &mut egui::Ui, state: &Value, view: &mut BoardView, p: Palette, me: &str) -> Vec<Value> {
    let mut ops = vec![];
    let board = Board::new(state);
    if let Some((focus, palette, delete)) = view.demo_editor.take() {
        let mut manager = Manager::open(&board, focus.clone());
        manager.palette = focus.clone().filter(|_| palette);
        manager.deleting = focus.filter(|_| delete).map(|id| (id, String::new()));
        manager.focus_once = false;
        view.manager = Some(manager);
    }
    if view.selected.as_ref().is_some_and(|id| board.task(id).is_none()) {
        view.selected = None;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && view.create.is_none() && view.manager.is_none() {
        view.selected = None;
        view.drag = None;
    }
    if view.selected.is_some() || view.log {
        egui::SidePanel::right("board-detail")
            .exact_width(392.)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.side).inner_margin(egui::Margin { left: 18, right: 16, top: 14, bottom: 14 }))
            .show_inside(ui, |ui| {
                if let Some(task) = view.selected.clone().and_then(|id| board.task(&id)) {
                    detail(ui, task, &board, view, p, me, &mut ops);
                } else {
                    journal(ui, &board, view, p);
                }
            });
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(ui, |ui| {
        header(ui, &board, view, p);
        ui.add_space(8.);
        canvas(ui, &board, view, p, &mut ops);
    });
    create_dialog(ui.ctx(), &board, view, p, &mut ops);
    category_editor(ui.ctx(), &board, view, p, &mut ops);
    ops
}

// --- header ---------------------------------------------------------------------------------

fn header(ui: &mut egui::Ui, board: &Board, view: &mut BoardView, p: Palette) {
    let progress = &board.state["progress"]["all"];
    let count = |status: &str| board.tasks.iter().filter(|t| t["status"] == status).count();
    ui.add_space(12.);
    ui.horizontal(|ui| {
        ui.set_min_height(30.);
        ui.add_space(20.);
        let wide = ui.available_width() > 760.;
        small(ui, board.state["title"].as_str().unwrap_or("Доска"), 19., p.text);
        ui.add_space(12.);
        let percent = progress["percent"].as_f64().unwrap_or(0.) as f32;
        let (r, response) = ui.allocate_exact_size(Vec2::new(if wide { 140. } else { 80. }, 18.), Sense::hover());
        bar(ui.painter(), Rect::from_center_size(r.center(), Vec2::new(r.width(), 6.)), percent, GREEN, p);
        response.on_hover_text(format!(
            "Доля сделанного с учётом размера задач в очках и частичной готовности.\nГотово {} из {} задач · в работе {} · на проверке {}",
            progress["done"].as_u64().unwrap_or(0),
            progress["tasks"].as_u64().unwrap_or(0),
            count("doing"),
            count("review")
        ));
        small(ui, format!("{percent:.0}%"), 13., p.text);
        if wide {
            small(
                ui,
                format!(
                    "{} из {} · в работе {} · проверка {}",
                    progress["done"].as_u64().unwrap_or(0),
                    progress["tasks"].as_u64().unwrap_or(0),
                    count("doing"),
                    count("review")
                ),
                12.5,
                p.muted,
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(16.);
            if design::button(ui, "Задача", Some(Icon::Plus), Kind::Primary, p, Vec2::new(0., 30.)).clicked() {
                view.create = Some(Create {
                    title: String::new(),
                    status: "todo".into(),
                    category: view.filter.clone().unwrap_or_default(),
                    focus: true,
                });
            }
            let open = view.log && view.selected.is_none();
            if design::button(ui, "Журнал", None, if open { Kind::Secondary } else { Kind::Ghost }, p, Vec2::new(0., 30.))
                .on_hover_text("Кто, что и с какого хоста менял")
                .clicked()
            {
                view.log = !open;
                view.selected = None;
            }
        });
    });
    ui.add_space(10.);
    chips(ui, board, view, p);
}

enum Chip {
    Mode(Mode, &'static str),
    Arrange,
    Links,
    Search,
    Category(String),
    AddCategory,
}

/// Fixed-size chips laid out by hand, so they wrap instead of being squeezed or cut off.
fn chips(ui: &mut egui::Ui, board: &Board, view: &mut BoardView, p: Palette) {
    let mut items = vec![Chip::Mode(Mode::Canvas, "Полотно"), Chip::Mode(Mode::Columns, "Колонки"), Chip::Mode(Mode::Lanes, "Категории")];
    let arranged = board.categories.iter().any(|c| c["x"].is_number());
    if view.mode == Mode::Canvas && arranged {
        items.push(Chip::Arrange);
    }
    items.extend([Chip::Links, Chip::Search]);
    items.extend(board.categories.iter().map(|c| Chip::Category(text(&c["id"]))));
    items.push(Chip::AddCategory);
    let font = FontId::proportional(12.);
    let width = ui.available_width() - 40.;
    let painter = ui.painter().clone();
    let measure = |s: &str| painter.layout_no_wrap(s.to_owned(), font.clone(), p.text).size().x;
    let label = |chip: &Chip| -> String {
        match chip {
            Chip::Mode(_, name) => (*name).into(),
            Chip::Arrange => "Разложить".into(),
            Chip::Links => "Все связи".into(),
            Chip::Search => String::new(),
            Chip::Category(id) => board.names.get(id).cloned().unwrap_or_default(),
            Chip::AddCategory => "Категория".into(),
        }
    };
    let mut placed = vec![];
    let (mut x, mut y) = (0f32, 0f32);
    for (i, chip) in items.iter().enumerate() {
        let w = match chip {
            Chip::Mode(..) => measure(&label(chip)) + 24.,
            Chip::Arrange => measure(&label(chip)) + 24.,
            Chip::Links => measure(&label(chip)) + 24.,
            Chip::Search => 190.,
            Chip::Category(_) => measure(&label(chip)) + 66.,
            Chip::AddCategory => measure(&label(chip)) + 40.,
        };
        // The mode switch is one segmented control; a wider gap separates the filters.
        let first_category = matches!(chip, Chip::Category(_)) && !matches!(items[i - 1], Chip::Category(_));
        let gap = if i > 0 && matches!(chip, Chip::Mode(..)) && matches!(items[i - 1], Chip::Mode(..)) {
            0.
        } else if first_category {
            14.
        } else {
            6.
        };
        if x > 0. && x + gap + w > width {
            x = 0.;
            y += 30.;
        } else if x > 0. {
            x += gap;
        }
        placed.push(Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, 24.)));
        x += w;
    }
    let mut category_rects: Vec<(String, Rect)> = vec![];
    let mut dropped = false;
    ui.horizontal(|ui| {
        ui.add_space(20.);
        let (area, _) = ui.allocate_exact_size(Vec2::new(width, y + 24.), Sense::hover());
        for (chip, rect) in items.iter().zip(placed) {
            let rect = rect.translate(area.min.to_vec2());
            if let Chip::Category(cid) = chip {
                category_rects.push((cid.clone(), rect));
                view.hits.insert(format!("chip:{cid}"), rect);
            }
            if let Chip::Search = chip {
                let painter = ui.painter();
                painter.rect_filled(rect, 12., design::mix(p.base, p.line, 0.55));
                design::icon(painter, Rect::from_center_size(Pos2::new(rect.left() + 14., rect.center().y), Vec2::splat(13.)), Icon::Search, p.muted);
                ui.put(
                    Rect::from_min_max(Pos2::new(rect.left() + 26., rect.top() + 2.), Pos2::new(rect.right() - 8., rect.bottom() - 2.)),
                    egui::TextEdit::singleline(&mut view.query)
                        .frame(false)
                        .font(FontId::proportional(12.5))
                        .vertical_align(Align::Center)
                        .hint_text(RichText::new("Поиск, T-12, исполнитель").color(p.muted).size(12.)),
                );
                continue;
            }
            let id = ui.id().with(("chip", label(chip)));
            let sense = if matches!(chip, Chip::Category(_)) { Sense::click_and_drag() } else { Sense::click() };
            let response = ui.interact(rect, id, sense);
            let hover = response.hovered();
            let painter = ui.painter();
            let idle = if hover { p.line } else { design::mix(p.base, p.line, 0.5) };
            match chip {
                Chip::Mode(mode, name) => {
                    let active = view.mode == *mode;
                    let corners = match mode {
                        Mode::Canvas => egui::CornerRadius { nw: 12, sw: 12, ne: 0, se: 0 },
                        Mode::Columns => egui::CornerRadius::ZERO,
                        Mode::Lanes => egui::CornerRadius { nw: 0, sw: 0, ne: 12, se: 12 },
                    };
                    painter.rect_filled(rect, corners, if active { p.text } else { idle });
                    painter.text(rect.center(), egui::Align2::CENTER_CENTER, *name, font.clone(), if active { p.base } else { p.text });
                    if response.clicked() {
                        view.mode = *mode;
                        view.fitted = false;
                    }
                    response.on_hover_text(match mode {
                        Mode::Canvas => "Категории — рамки, которые можно двигать по доске за заголовок",
                        Mode::Columns => "Канбан по статусам",
                        Mode::Lanes => "Строки по категориям, столбцы по статусам",
                    });
                }
                Chip::Arrange => {
                    painter.rect_filled(rect, 12., idle);
                    painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Разложить", font.clone(), p.muted);
                    if response.on_hover_text("Вернуть все рамки в автоматическую сетку (для всей команды)").clicked() {
                        for c in &board.categories {
                            if c["x"].is_number() {
                                view.queued.push(json!({"op":"category","id":c["id"],"x":null,"y":null}));
                            }
                        }
                        view.placed.clear();
                        view.fitted = false;
                    }
                }
                Chip::Links => {
                    let active = view.links;
                    painter.rect_filled(rect, 12., if active { ACCENT.gamma_multiply(0.25) } else { idle });
                    painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Все связи", font.clone(), if active { ACCENT } else { p.muted });
                    if response.clicked() {
                        view.links = !view.links;
                    }
                    response.on_hover_text("Показывать все связи на полотне, а не только у карточки под мышью");
                }
                Chip::Search => {}
                Chip::AddCategory => {
                    painter.rect_stroke(rect, 12., Stroke::new(1.0_f32, if hover { p.muted } else { p.line }), egui::StrokeKind::Inside);
                    design::icon(painter, Rect::from_center_size(Pos2::new(rect.left() + 14., rect.center().y), Vec2::splat(12.)), Icon::Plus, p.muted);
                    painter.text(Pos2::new(rect.left() + 25., rect.center().y), egui::Align2::LEFT_CENTER, "Категория", font.clone(), p.muted);
                    if response.on_hover_text("Создать, переименовать, перекрасить или удалить категории").clicked() {
                        view.manager = Some(Manager::open(board, None));
                    }
                }
                Chip::Category(cid) => {
                    if response.drag_started() {
                        view.chip_drag = Some(cid.clone());
                    }
                    if response.drag_stopped() && view.chip_drag.as_deref() == Some(cid.as_str()) {
                        dropped = true;
                    }
                    if response.double_clicked() || response.secondary_clicked() {
                        view.manager = Some(Manager::open(board, Some(cid.clone())));
                    }
                    let dragging = view.chip_drag.as_deref() == Some(cid.as_str());
                    let active = view.filter.as_deref() == Some(cid.as_str());
                    let tint = board.tint(cid, p);
                    let stats = &board.state["progress"]["categories"][cid];
                    let percent = stats["percent"].as_f64().unwrap_or(0.) as f32;
                    painter.rect_filled(rect, 12., if active { tint.gamma_multiply(0.22) } else { idle });
                    if active {
                        painter.rect_stroke(rect, 12., Stroke::new(1.0_f32, tint), egui::StrokeKind::Inside);
                    }
                    painter.circle_filled(Pos2::new(rect.left() + 12., rect.center().y), 4., tint);
                    painter.text(Pos2::new(rect.left() + 21., rect.center().y), egui::Align2::LEFT_CENTER, label(chip), font.clone(), p.text);
                    painter.text(Pos2::new(rect.right() - 10., rect.center().y), egui::Align2::RIGHT_CENTER, format!("{percent:.0}%"), FontId::proportional(11.), p.muted);
                    // Progress as a hairline along the bottom of the chip.
                    let track = Rect::from_min_max(Pos2::new(rect.left() + 10., rect.bottom() - 3.), Pos2::new(rect.right() - 10., rect.bottom() - 1.5));
                    bar(painter, track, percent, tint, p);
                    if dragging {
                        ui.painter().rect_filled(rect, 12., p.base.gamma_multiply(0.6));
                    }
                    if response.clicked() {
                        view.filter = if active { None } else { Some(cid.clone()) };
                    }
                    let description = board
                        .categories
                        .iter()
                        .find(|c| c["id"] == cid.as_str())
                        .map(|c| text(&c["description"]))
                        .filter(|d| !d.is_empty())
                        .map(|d| format!("{d}\n"))
                        .unwrap_or_default();
                    response.on_hover_text(format!(
                        "{} · готово {} из {}\n{description}Щелчок — только она · двойной — изменить · тяни — переставить",
                        label(chip),
                        stats["done"].as_u64().unwrap_or(0),
                        stats["tasks"].as_u64().unwrap_or(0)
                    ));
                }
            }
        }
    });
    // Dragging a category chip: a mark where it will land, the chip under the pointer.
    let Some(dragged) = view.chip_drag.clone() else { return };
    let Some(pointer) = ui.ctx().pointer_latest_pos() else { return };
    let before = category_rects
        .iter()
        .filter(|(id, _)| *id != dragged)
        .find(|(_, r)| r.top() > pointer.y + 6. || (r.y_range().contains(pointer.y) && r.center().x > pointer.x))
        .map(|(id, r)| (id.clone(), *r));
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("chip-drag")));
    let mark = match &before {
        Some((_, r)) => Rect::from_min_size(Pos2::new(r.left() - 4., r.top()), Vec2::new(2.5, r.height())),
        None => category_rects
            .iter()
            .filter(|(id, _)| *id != dragged)
            .next_back()
            .map_or(Rect::NOTHING, |(_, r)| Rect::from_min_size(Pos2::new(r.right() + 2., r.top()), Vec2::new(2.5, r.height()))),
    };
    painter.rect_filled(mark, 1., ACCENT);
    let name = board.names.get(&dragged).cloned().unwrap_or_default();
    let galley = painter.layout_no_wrap(name, FontId::proportional(12.), p.text);
    let ghost = Rect::from_min_size(pointer + Vec2::new(8., -12.), galley.size() + Vec2::new(30., 10.));
    painter.add(Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(60) }.as_shape(ghost, 12.));
    painter.rect_filled(ghost, 12., p.surface);
    painter.circle_filled(Pos2::new(ghost.left() + 12., ghost.center().y), 4., board.tint(&dragged, p));
    painter.galley(Pos2::new(ghost.left() + 21., ghost.center().y - galley.size().y / 2.), galley, p.text);
    ui.ctx().request_repaint();
    if dropped || !ui.input(|i| i.pointer.any_down()) {
        view.queued.extend(reorder(board, &dragged, before.as_ref().map(|(id, _)| id.as_str())));
        view.chip_drag = None;
    }
}

// --- canvas ---------------------------------------------------------------------------------

struct CardBox {
    id: String,
    rect: Rect,
    status: String,
    category: String,
    compact: bool,
    title: Arc<Galley>,
}

/// Where a dragged card can land: a column, or a cell of a category lane.
struct Zone {
    status: String,
    category: Option<String>,
    rect: Rect,
    top: f32,
}

struct Plan {
    cards: Vec<CardBox>,
    zones: Vec<Zone>,
    content: Rect,
    /// Shapes painted just before the card at that index: the frame being dragged floats
    /// above the others together with its cards.
    overlays: Vec<(usize, Vec<Shape>)>,
}

fn canvas(ui: &mut egui::Ui, board: &Board, view: &mut BoardView, p: Palette, ops: &mut Vec<Value>) {
    let size = ui.available_size_before_wrap();
    view.viewport = size;
    let mut scene = Rect::from_min_size(view.origin, size / view.scale);
    let outer = Rect::from_min_size(ui.cursor().min, size);
    let response = egui::Scene::new()
        .zoom_range(0.3..=1.0)
        .max_inner_size(Vec2::splat(40000.))
        .show(ui, &mut scene, |ui| {
            grid(ui, p, view.scale);
            let plan = match view.mode {
                Mode::Canvas => frames(ui, board, view, p),
                Mode::Columns => columns(ui, board, view, p),
                Mode::Lanes => lanes(ui, board, view, p),
            };
            view.content = plan.content;
            if !view.fitted && plan.content.is_finite() {
                // First look: the whole width if it fits at a readable size, from the top.
                view.fitted = true;
                let scale = (size.x / (plan.content.width() + 48.)).clamp(0.75, 1.);
                view.pending = Some((plan.content.min - Vec2::new(24., 18.), scale));
            }
            cards(ui, board, view, p, &plan, ops);
        })
        .response;
    view.origin = scene.min;
    view.scale = (size.x / scene.width()).clamp(0.3, 1.);
    if let Some((origin, scale)) = view.pending.take() {
        view.origin = origin;
        view.scale = scale;
        ui.ctx().request_repaint();
    }
    if response.clicked() && view.drag.is_none() {
        view.selected = None;
    }
    controls(ui, view, p, outer);
}

fn grid(ui: &egui::Ui, p: Palette, scale: f32) {
    let visible = ui.clip_rect();
    let step = if scale < 0.6 { 56. } else { 28. };
    let dot = design::mix(p.base, p.line, 0.9);
    let mut shapes = vec![];
    let mut x = (visible.left() / step).floor() * step;
    while x < visible.right() {
        let mut y = (visible.top() / step).floor() * step;
        while y < visible.bottom() {
            shapes.push(Shape::rect_filled(Rect::from_center_size(Pos2::new(x, y), Vec2::splat(2.)), 0., dot));
            y += step;
        }
        x += step;
    }
    ui.painter().extend(shapes);
}

fn visible(board: &Board, view: &BoardView, task: &Value) -> bool {
    let query = view.query.trim().to_lowercase();
    view.filter.as_deref().is_none_or(|f| task["category"] == f)
        && (query.is_empty()
            || text(&task["title"]).to_lowercase().contains(&query)
            || text(&task["id"]).to_lowercase() == query
            || text(&task["assignee"]).to_lowercase().contains(&query)
            || board.names.get(&text(&task["category"])).is_some_and(|n| n.to_lowercase().contains(&query)))
}

/// Tasks of one column/cell in display order; finished work shows the newest first.
fn ordered<'a>(mut list: Vec<&'a Value>, status: &str) -> Vec<&'a Value> {
    if status == "done" {
        list.sort_by(|a, b| b["done_at"].as_f64().unwrap_or(0.).total_cmp(&a["done_at"].as_f64().unwrap_or(0.)));
    } else {
        list.sort_by(|a, b| a["order"].as_f64().unwrap_or(0.).total_cmp(&b["order"].as_f64().unwrap_or(0.)));
    }
    list
}

fn measure(ui: &egui::Ui, task: &Value, width: f32, compact: bool, p: Palette) -> (Arc<Galley>, f32) {
    let wrap = if compact { width - 62. } else { width - 30. };
    let mut job = LayoutJob::simple(
        text(&task["title"]),
        FontId::proportional(if compact { 12.5 } else { 13.5 }),
        if compact { p.muted } else { p.text },
        wrap,
    );
    job.wrap.max_rows = if compact { 1 } else { 3 };
    job.wrap.break_anywhere = compact;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    let progress = task["progress"].as_f64().unwrap_or(0.);
    let height = if compact {
        34.
    } else {
        10. + galley.size().y + if progress > 0. && progress < 100. { 10. } else { 0. } + 30.
    };
    (galley, height)
}

/// Draws a column/lane frame now and returns where its cards go; cards are painted later.
fn more_button(ui: &egui::Ui, rect: Rect, hidden: usize, open: bool, p: Palette) -> bool {
    let response = ui.interact(rect, ui.id().with(("more", rect.min.x as i32, rect.min.y as i32)), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 8., if response.hovered() { p.line } else { design::mix(p.base, p.line, 0.4) });
    let label = if open { "Свернуть".to_owned() } else { format!("Показать ещё {hidden}") };
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(12.), p.muted);
    response.clicked()
}

fn plus_button(ui: &egui::Ui, rect: Rect, id: egui::Id, p: Palette) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 7., p.line);
    }
    design::icon(ui.painter(), rect.shrink(4.), Icon::Plus, if response.hovered() { p.text } else { p.muted });
    response.clicked()
}

fn columns(ui: &egui::Ui, board: &Board, view: &mut BoardView, p: Palette) -> Plan {
    let painter = ui.painter().clone();
    let mut plan = Plan { cards: vec![], zones: vec![], content: Rect::NOTHING, overlays: vec![] };
    let mut backgrounds = vec![];
    let mut bottom: f32 = HEADER + 160.;
    for (i, (status, name)) in board.columns.iter().enumerate() {
        let x = i as f32 * (COLUMN + GAP);
        let background = painter.add(Shape::Noop);
        let list = ordered(
            board.tasks.iter().copied().filter(|t| t["status"] == status.as_str() && visible(board, view, t)).collect(),
            status,
        );
        let head = Rect::from_min_size(Pos2::new(x, 0.), Vec2::new(COLUMN, HEADER));
        painter.circle_filled(Pos2::new(x + 18., head.center().y), 4., status_color(status, p));
        let title = painter.text(Pos2::new(x + 30., head.center().y), egui::Align2::LEFT_CENTER, name, FontId::proportional(14.), p.text);
        let badge = Rect::from_min_size(Pos2::new(title.right() + 8., head.center().y - 10.), Vec2::new(26., 20.));
        painter.rect_filled(badge, 10., p.line);
        painter.text(badge.center(), egui::Align2::CENTER_CENTER, list.len().to_string(), FontId::proportional(11.5), p.muted);
        if plus_button(ui, Rect::from_center_size(Pos2::new(x + COLUMN - 22., head.center().y), Vec2::splat(26.)), ui.id().with(("add", status)), p) {
            view.create = Some(Create { title: String::new(), status: status.clone(), category: view.filter.clone().unwrap_or_default(), focus: true });
        }
        let compact = status == "done";
        let key = status.clone();
        let open = view.expanded.contains(&key);
        let shown = if compact && !open { list.len().min(COLLAPSED) } else { list.len() };
        let width = COLUMN - 2. * INNER;
        let mut y = HEADER;
        for task in &list[..shown] {
            let (title, h) = measure(ui, task, width, compact, p);
            plan.cards.push(CardBox {
                id: text(&task["id"]),
                rect: Rect::from_min_size(Pos2::new(x + INNER, y), Vec2::new(width, h)),
                status: status.clone(),
                category: text(&task["category"]),
                compact,
                title,
            });
            y += h + CARD_GAP;
        }
        if compact && list.len() > COLLAPSED {
            let rect = Rect::from_min_size(Pos2::new(x + INNER, y), Vec2::new(width, 30.));
            if more_button(ui, rect, list.len() - shown, open, p) {
                if open {
                    view.expanded.remove(&key);
                } else {
                    view.expanded.insert(key);
                }
            }
            y += 30. + CARD_GAP;
        }
        if list.is_empty() {
            painter.text(Pos2::new(x + COLUMN / 2., y + 30.), egui::Align2::CENTER_CENTER, "Перетащи сюда задачу", FontId::proportional(12.), p.muted);
            y += 64.;
        }
        bottom = bottom.max(y + INNER);
        backgrounds.push((background, x));
        plan.zones.push(Zone {
            status: status.clone(),
            category: None,
            rect: Rect::from_min_max(Pos2::new(x - GAP / 2., -f32::INFINITY), Pos2::new(x + COLUMN + GAP / 2., f32::INFINITY)),
            top: HEADER,
        });
    }
    // All columns share the tallest height, so the board reads as one surface.
    for (shape, x) in backgrounds {
        let rect = Rect::from_min_size(Pos2::new(x, 0.), Vec2::new(COLUMN, bottom));
        painter.set(shape, Shape::Vec(vec![
            Shape::rect_filled(rect, 14., design::mix(p.base, p.side, 0.85)),
            Shape::rect_stroke(rect, 14., Stroke::new(1.0_f32, design::mix(p.side, p.line, 0.6)), egui::StrokeKind::Inside),
        ]));
    }
    let width = board.columns.len() as f32 * (COLUMN + GAP) - GAP;
    plan.content = Rect::from_min_size(Pos2::ZERO, Vec2::new(width.max(COLUMN), bottom));
    plan
}

fn lanes(ui: &egui::Ui, board: &Board, view: &mut BoardView, p: Palette) -> Plan {
    let painter = ui.painter().clone();
    let mut plan = Plan { cards: vec![], zones: vec![], content: Rect::NOTHING, overlays: vec![] };
    let x0 = LABEL + GAP;
    let width = x0 + board.columns.len() as f32 * (CELL + GAP) - GAP;
    for (i, (status, name)) in board.columns.iter().enumerate() {
        let x = x0 + i as f32 * (CELL + GAP);
        painter.circle_filled(Pos2::new(x + 8., 16.), 4., status_color(status, p));
        painter.text(Pos2::new(x + 20., 16.), egui::Align2::LEFT_CENTER, name, FontId::proportional(14.), p.text);
    }
    let mut lanes: Vec<(String, String, Color32)> = board
        .categories
        .iter()
        .map(|c| (text(&c["id"]), text(&c["name"]), board.tint(&text(&c["id"]), p)))
        .collect();
    if board.tasks.iter().any(|t| !board.colors.contains_key(&text(&t["category"]))) {
        lanes.push((String::new(), "Без категории".into(), p.muted));
    }
    let mut y = 40.;
    let mut lane_rects: Vec<(String, Rect)> = vec![];
    let mut lane_pointer = None;
    let mut lane_dropped = false;
    for (n, (cid, cname, tint)) in lanes.iter().enumerate() {
        if view.filter.as_ref().is_some_and(|f| f != cid) {
            continue;
        }
        let background = painter.add(Shape::Noop);
        let lane_tasks: Vec<&Value> = board
            .tasks
            .iter()
            .copied()
            .filter(|t| {
                let c = text(&t["category"]);
                (if cid.is_empty() { !board.colors.contains_key(&c) } else { &c == cid }) && visible(board, view, t)
            })
            .collect();
        let mut height: f32 = 96.;
        let mut cells = vec![];
        for (i, (status, _)) in board.columns.iter().enumerate() {
            let x = x0 + i as f32 * (CELL + GAP);
            let list = ordered(lane_tasks.iter().copied().filter(|t| t["status"] == status.as_str()).collect(), status);
            let compact = status == "done";
            let key = format!("{status}:{cid}");
            let open = view.expanded.contains(&key);
            let shown = if compact && !open { list.len().min(LANE_COLLAPSED) } else { list.len() };
            let mut cy = y + INNER;
            for task in &list[..shown] {
                let (title, h) = measure(ui, task, CELL, compact, p);
                plan.cards.push(CardBox {
                    id: text(&task["id"]),
                    rect: Rect::from_min_size(Pos2::new(x, cy), Vec2::new(CELL, h)),
                    status: status.clone(),
                    category: text(&task["category"]),
                    compact,
                    title,
                });
                cy += h + CARD_GAP;
            }
            if compact && list.len() > LANE_COLLAPSED {
                let rect = Rect::from_min_size(Pos2::new(x, cy), Vec2::new(CELL, 28.));
                if more_button(ui, rect, list.len() - shown, open, p) {
                    if open {
                        view.expanded.remove(&key);
                    } else {
                        view.expanded.insert(key);
                    }
                }
                cy += 28. + CARD_GAP;
            }
            height = height.max(cy - y + INNER - CARD_GAP);
            cells.push((status.clone(), x));
        }
        let lane = Rect::from_min_size(Pos2::new(0., y), Vec2::new(width, height));
        painter.set(background, Shape::Vec(vec![
            Shape::rect_filled(lane, 14., design::mix(p.base, p.side, if n % 2 == 0 { 0.85 } else { 0.55 })),
            Shape::rect_filled(Rect::from_min_size(lane.min + Vec2::new(10., 14.), Vec2::new(3., height - 28.)), 2., *tint),
        ]));
        // The label is the lane's handle: drag to reorder, double click to edit the category.
        if !cid.is_empty() {
            let handle = Rect::from_min_size(lane.min, Vec2::new(LABEL, height));
            if let Some(to_global) = ui.ctx().layer_transform_to_global(ui.layer_id()) {
                view.hits.insert(format!("lane:{cid}"), to_global * handle);
            }
            let response = ui.interact(handle, ui.id().with(("lane-label", cid)), Sense::click_and_drag());
            if response.drag_started() {
                view.lane_drag = Some(cid.clone());
            }
            if view.lane_drag.as_deref() == Some(cid.as_str()) {
                lane_pointer = response.interact_pointer_pos();
                lane_dropped |= response.drag_stopped();
                painter.rect_filled(lane, 14., p.base.gamma_multiply(0.5));
            }
            if response.double_clicked() {
                view.manager = Some(Manager::open(board, Some(cid.clone())));
            }
            if response.hovered() && view.lane_drag.is_none() {
                let grip = Pos2::new(LABEL - 46., y + 23.);
                for (dx, dy) in [(0., -4.), (0., 0.), (0., 4.), (4., -4.), (4., 0.), (4., 4.)] {
                    painter.circle_filled(grip + Vec2::new(dx, dy), 1.2, p.muted);
                }
            }
            lane_rects.push((cid.clone(), lane));
        }
        let stats = &board.state["progress"]["categories"][cid];
        let percent = stats["percent"].as_f64().unwrap_or(0.) as f32;
        let mut job = LayoutJob::simple(cname.clone(), FontId::proportional(14.5), p.text, LABEL - 64.);
        job.wrap.max_rows = 2;
        let galley = painter.layout_job(job);
        let title_height = galley.size().y;
        painter.galley(Pos2::new(24., y + 14.), galley, p.text);
        let ty = y + 22. + title_height;
        bar(&painter, Rect::from_min_size(Pos2::new(24., ty), Vec2::new(LABEL - 70., 5.)), percent, *tint, p);
        painter.text(Pos2::new(LABEL - 4., ty + 2.5), egui::Align2::RIGHT_CENTER, format!("{percent:.0}%"), FontId::proportional(12.), p.text);
        painter.text(
            Pos2::new(24., ty + 18.),
            egui::Align2::LEFT_CENTER,
            format!("готово {} из {}", stats["done"].as_u64().unwrap_or(0), stats["tasks"].as_u64().unwrap_or(0)),
            FontId::proportional(11.5),
            p.muted,
        );
        if plus_button(ui, Rect::from_min_size(Pos2::new(LABEL - 30., y + 10.), Vec2::splat(26.)), ui.id().with(("lane-add", cid)), p) {
            view.create = Some(Create { title: String::new(), status: "todo".into(), category: cid.clone(), focus: true });
        }
        for (status, x) in cells {
            plan.zones.push(Zone {
                status,
                category: Some(cid.clone()),
                rect: Rect::from_min_size(Pos2::new(x - GAP / 2., y), Vec2::new(CELL + GAP, height)),
                top: y + INNER,
            });
        }
        y += height + 10.;
    }
    if let (Some(dragged), Some(pos)) = (view.lane_drag.clone(), lane_pointer) {
        let before = lane_rects.iter().filter(|(id, _)| *id != dragged).find(|(_, r)| r.center().y > pos.y);
        let line_y = match before {
            Some((_, r)) => r.top() - 5.,
            None => lane_rects.iter().filter(|(id, _)| *id != dragged).map(|(_, r)| r.bottom() + 5.).fold(40., f32::max),
        };
        painter.line_segment([Pos2::new(0., line_y), Pos2::new(width, line_y)], Stroke::new(3.0_f32, ACCENT));
        let name = board.names.get(&dragged).cloned().unwrap_or_default();
        let galley = painter.layout_no_wrap(name, FontId::proportional(14.), p.text);
        let ghost = Rect::from_min_size(pos + Vec2::new(10., -16.), galley.size() + Vec2::new(32., 14.));
        painter.add(Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(ghost, 10.));
        painter.rect_filled(ghost, 10., p.surface);
        painter.rect_filled(Rect::from_min_size(ghost.min + Vec2::new(8., 7.), Vec2::new(3., ghost.height() - 14.)), 1.5, board.tint(&dragged, p));
        painter.galley(ghost.min + Vec2::new(18., 7.), galley, p.text);
        ui.ctx().request_repaint();
        if lane_dropped {
            view.queued.extend(reorder(board, &dragged, before.map(|(id, _)| id.as_str())));
            view.lane_drag = None;
        }
    } else if view.lane_drag.is_some() && !ui.input(|i| i.pointer.any_down()) {
        view.lane_drag = None;
    }
    plan.content = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, y));
    plan
}

/// Where the button went down, in this layer's coordinates. A drag only starts after the
/// pointer has moved a little; grabbing there instead would make the dragged thing jump.
fn press_origin(ui: &egui::Ui) -> Option<Pos2> {
    let pos = ui.input(|i| i.pointer.press_origin())?;
    Some(match ui.ctx().layer_transform_from_global(ui.layer_id()) {
        Some(from_global) => from_global * pos,
        None => pos,
    })
}

struct Laid {
    cards: Vec<CardBox>,
    sections: Vec<(String, f32, usize)>,
    more: Vec<(Rect, String, usize, bool)>,
    height: f32,
    empty: bool,
}

/// One frame per category, placed where the team put it (or in an automatic grid), with its
/// tasks grouped by status. Frames are dragged by their header.
fn frames(ui: &egui::Ui, board: &Board, view: &mut BoardView, p: Palette) -> Plan {
    let painter = ui.painter().clone();
    let mut plan = Plan { cards: vec![], zones: vec![], content: Rect::NOTHING, overlays: vec![] };
    let mut groups: Vec<(String, String, Color32, Option<Pos2>)> = board
        .categories
        .iter()
        .map(|c| {
            let id = text(&c["id"]);
            let saved = c["x"].as_f64().zip(c["y"].as_f64()).map(|(x, y)| Pos2::new(x as f32, y as f32));
            (id.clone(), text(&c["name"]), board.tint(&id, p), saved)
        })
        .collect();
    if board.tasks.iter().any(|t| !board.colors.contains_key(&text(&t["category"]))) {
        groups.push((String::new(), "Без категории".into(), p.muted, None));
    }
    if let Some(filter) = &view.filter {
        groups.retain(|g| &g.0 == filter);
    }
    // A dropped frame stays where it was put until the server's state says the same.
    view.placed.retain(|id, pos| !groups.iter().any(|g| &g.0 == id && g.3.is_some_and(|s| s.distance(*pos) < 1.)));

    let mut laid = vec![];
    for (cid, _, _, _) in &groups {
        let tasks: Vec<&Value> = board
            .tasks
            .iter()
            .copied()
            .filter(|t| {
                let c = text(&t["category"]);
                (if cid.is_empty() { !board.colors.contains_key(&c) } else { &c == cid }) && visible(board, view, t)
            })
            .collect();
        let mut l = Laid { cards: vec![], sections: vec![], more: vec![], height: 0., empty: tasks.is_empty() };
        let mut y = FRAME_HEAD + 6.;
        for status in FRAME_STATUSES {
            let list = ordered(tasks.iter().copied().filter(|t| t["status"] == status).collect(), status);
            if list.is_empty() {
                continue;
            }
            l.sections.push((status.to_owned(), y, list.len()));
            y += 22.;
            let compact = status == "done";
            let key = format!("frame:{cid}:{status}");
            let open = view.expanded.contains(&key);
            let shown = if compact && !open { list.len().min(LANE_COLLAPSED) } else { list.len() };
            for task in &list[..shown] {
                let (title, h) = measure(ui, task, FRAME - 2. * INNER, compact, p);
                l.cards.push(CardBox {
                    id: text(&task["id"]),
                    rect: Rect::from_min_size(Pos2::new(INNER, y), Vec2::new(FRAME - 2. * INNER, h)),
                    status: status.to_owned(),
                    category: text(&task["category"]),
                    compact,
                    title,
                });
                y += h + CARD_GAP;
            }
            if compact && list.len() > LANE_COLLAPSED {
                l.more.push((Rect::from_min_size(Pos2::new(INNER, y), Vec2::new(FRAME - 2. * INNER, 28.)), key, list.len() - shown, open));
                y += 28. + CARD_GAP;
            }
            y += 6.;
        }
        if l.empty {
            y += 50.;
        }
        l.height = y + INNER - 6.;
        laid.push(l);
    }

    // Saved places first; the rest fill a grid from the left, stepping around placed frames.
    let saved_of = |g: &(String, String, Color32, Option<Pos2>)| view.placed.get(&g.0).copied().or(g.3);
    let mut taken: Vec<Rect> = groups
        .iter()
        .zip(&laid)
        .filter_map(|(g, l)| saved_of(g).map(|pos| Rect::from_min_size(pos, Vec2::new(FRAME, l.height))))
        .collect();
    let mut column_y = [0f32; 4];
    let mut origins: Vec<Pos2> = vec![];
    for (g, l) in groups.iter().zip(&laid) {
        if let Some(pos) = saved_of(g) {
            origins.push(pos);
            continue;
        }
        loop {
            let i = (0..column_y.len()).min_by(|a, b| column_y[*a].total_cmp(&column_y[*b])).unwrap_or(0);
            let slot = Rect::from_min_size(Pos2::new(i as f32 * (FRAME + FRAME_GAP), column_y[i]), Vec2::new(FRAME, l.height));
            if let Some(block) = taken.iter().find(|r| r.expand(FRAME_GAP / 2. - 1.).intersects(slot)) {
                column_y[i] = block.bottom() + FRAME_GAP;
                continue;
            }
            taken.push(slot);
            column_y[i] = slot.bottom() + FRAME_GAP;
            origins.push(slot.min);
            break;
        }
    }

    // The frame being dragged is handled last, so it floats above the others.
    let dragged = view.frame_drag.as_ref().and_then(|d| groups.iter().position(|g| g.0 == d.id));
    let mut order: Vec<usize> = (0..groups.len()).filter(|i| Some(*i) != dragged).collect();
    order.extend(dragged);
    let mut released = None;
    for i in order {
        let (cid, name, tint, _) = &groups[i];
        let l = &laid[i];
        let mut origin = origins[i];
        let floating = Some(i) == dragged;
        if !cid.is_empty() {
            let header = Rect::from_min_size(origin, Vec2::new(FRAME, FRAME_HEAD));
            let response = ui
                .interact(header, ui.id().with(("frame", cid)), Sense::click_and_drag())
                .on_hover_cursor(if floating { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
            if response.drag_started() {
                if let Some(pos) = press_origin(ui).or(response.interact_pointer_pos()) {
                    view.frame_drag = Some(FrameDrag { id: cid.clone(), grab: pos - origin, pos: origin });
                }
            }
            if let Some(drag) = view.frame_drag.as_mut().filter(|d| &d.id == cid) {
                if let Some(pos) = response.interact_pointer_pos() {
                    drag.pos = pos - drag.grab;
                }
                origin = drag.pos;
                if response.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
                    released = Some((cid.clone(), origin));
                }
            }
            if response.double_clicked() {
                view.manager = Some(Manager::open(board, Some(cid.clone())));
            }
            if let Some(to_global) = ui.ctx().layer_transform_to_global(ui.layer_id()) {
                view.hits.insert(format!("frame:{cid}"), to_global * header);
            }
        }
        let rect = Rect::from_min_size(origin, Vec2::new(FRAME, l.height));
        let stats = &board.state["progress"]["categories"][cid];
        let percent = stats["percent"].as_f64().unwrap_or(0.) as f32;
        let mut shapes = vec![];
        if floating {
            shapes.push(Shape::from(Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(80) }.as_shape(rect, 16.)));
        }
        shapes.push(Shape::rect_filled(rect, 16., design::mix(p.base, p.side, 0.9)));
        shapes.push(Shape::rect_stroke(rect, 16., Stroke::new(1.0_f32, if floating { *tint } else { design::mix(p.side, p.line, 0.8) }), egui::StrokeKind::Inside));
        shapes.push(Shape::rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(FRAME, 5.)),
            egui::CornerRadius { nw: 16, ne: 16, sw: 0, se: 0 },
            *tint,
        ));
        if !cid.is_empty() {
            for (dx, dy) in [(-2.5, -4.), (-2.5, 0.), (-2.5, 4.), (2.5, -4.), (2.5, 0.), (2.5, 4.)] {
                shapes.push(Shape::circle_filled(origin + Vec2::new(16. + dx, 24. + dy), 1.3, p.muted));
            }
        }
        let mut job = LayoutJob::simple(name.clone(), FontId::proportional(15.), p.text, FRAME - 100.);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        shapes.push(Shape::galley(origin + Vec2::new(28., 14.), painter.layout_job(job), p.text));
        if !cid.is_empty() {
            shapes.push(Shape::rect_filled(Rect::from_min_size(origin + Vec2::new(28., 44.), Vec2::new(FRAME - 120., 5.)), 2.5, p.line));
            let done = Rect::from_min_size(origin + Vec2::new(28., 44.), Vec2::new((FRAME - 120.) * (percent / 100.).clamp(0., 1.), 5.));
            if done.width() > 0.5 {
                shapes.push(Shape::rect_filled(done, 2.5, *tint));
            }
            let label = format!("{percent:.0}% · {} из {}", stats["done"].as_u64().unwrap_or(0), stats["tasks"].as_u64().unwrap_or(0));
            shapes.push(Shape::galley(
                origin + Vec2::new(FRAME - 84., 39.),
                painter.layout_no_wrap(label, FontId::proportional(11.), p.muted),
                p.muted,
            ));
        }
        for (status, y, count) in &l.sections {
            let label = format!("{} · {count}", status_name(board.state, status).to_uppercase());
            shapes.push(Shape::circle_filled(origin + Vec2::new(INNER + 4., y + 9.), 3., status_color(status, p)));
            shapes.push(Shape::galley(
                origin + Vec2::new(INNER + 12., y + 2.),
                painter.layout_no_wrap(label, FontId::proportional(10.5), p.muted),
                p.muted,
            ));
        }
        if l.empty {
            shapes.push(Shape::galley(
                origin + Vec2::new(FRAME / 2. - 80., FRAME_HEAD + 22.),
                painter.layout_no_wrap("Перетащи сюда задачи".into(), FontId::proportional(12.), p.muted),
                p.muted,
            ));
        }
        if floating {
            plan.overlays.push((plan.cards.len(), shapes));
        } else {
            painter.extend(shapes);
            if !cid.is_empty() && plus_button(ui, Rect::from_center_size(origin + Vec2::new(FRAME - 24., 24.), Vec2::splat(26.)), ui.id().with(("frame-add", cid)), p) {
                view.create = Some(Create { title: String::new(), status: "todo".into(), category: cid.clone(), focus: true });
            }
            for (r, key, hidden, open) in &l.more {
                if more_button(ui, r.translate(origin.to_vec2()), *hidden, *open, p) {
                    if *open {
                        view.expanded.remove(key);
                    } else {
                        view.expanded.insert(key.clone());
                    }
                }
            }
        }
        for card in &l.cards {
            plan.cards.push(CardBox {
                id: card.id.clone(),
                rect: card.rect.translate(origin.to_vec2()),
                status: card.status.clone(),
                category: card.category.clone(),
                compact: card.compact,
                title: card.title.clone(),
            });
        }
        plan.zones.push(Zone { status: String::new(), category: Some(cid.clone()), rect, top: origin.y + FRAME_HEAD + 6. });
        plan.content = plan.content.union(rect);
    }
    if let Some((id, pos)) = released {
        let snapped = Pos2::new((pos.x / 10.).round() * 10., (pos.y / 10.).round() * 10.);
        let saved = groups.iter().find(|g| g.0 == id).and_then(|g| g.3);
        if saved.is_none_or(|s| s.distance(snapped) >= 1.) {
            view.queued.push(json!({"op":"category","id":id,"x":snapped.x,"y":snapped.y}));
            view.placed.insert(id, snapped);
        }
        view.frame_drag = None;
    }
    if view.frame_drag.is_some() {
        ui.ctx().request_repaint();
    }
    if !plan.content.is_finite() {
        plan.content = Rect::from_min_size(Pos2::ZERO, Vec2::splat(FRAME));
    }
    plan
}

/// Cards, links under them, and dragging on top.
fn cards(ui: &egui::Ui, board: &Board, view: &mut BoardView, p: Palette, plan: &Plan, ops: &mut Vec<Value>) {
    let links_layer = ui.painter().add(Shape::Noop);
    let mut hovered = None;
    let mut pointer = None;
    let mut released = false;
    let shift = ui.input(|i| i.modifiers.shift);
    let command = ui.input(|i| i.modifiers.command);
    for card in &plan.cards {
        let response = ui.interact(card.rect, egui::Id::new(("board-card", &card.id)), Sense::click_and_drag());
        if response.hovered() && view.drag.is_none() {
            hovered = Some(card.id.clone());
        }
        if response.drag_started() {
            let grab = press_origin(ui).or(response.interact_pointer_pos()).map_or(Vec2::ZERO, |pos| pos - card.rect.min);
            let link = shift.then_some(if command { "blocks" } else { "relates" });
            view.drag = Some(Drag { id: card.id.clone(), grab, link });
        }
        if view.drag.as_ref().is_some_and(|d| d.id == card.id) {
            pointer = response.interact_pointer_pos().or(pointer);
            released |= response.drag_stopped();
        }
        if response.clicked() {
            view.selected = Some(card.id.clone());
            view.log = false;
        }
    }
    if view.drag.as_ref().is_some_and(|d| !plan.cards.iter().any(|c| c.id == d.id)) {
        view.drag = None;
    }
    if let Some(id) = &view.focus.clone() {
        if let Some(card) = plan.cards.iter().find(|c| &c.id == id) {
            if !ui.clip_rect().contains_rect(card.rect) {
                view.pending = Some((card.rect.center() - view.viewport / view.scale / 2., view.scale));
            }
        }
        view.focus = None;
    }

    // Links: under the cards, routed around them; highlighted for the card in focus.
    let rect_of = |id: &str| plan.cards.iter().find(|c| c.id == id).map(|c| c.rect);
    let focus: Option<String> = hovered.clone().or_else(|| view.selected.clone()).or_else(|| view.drag.as_ref().map(|d| d.id.clone()));
    let mut related: HashMap<String, Color32> = HashMap::new();
    let mut shapes = vec![];
    for task in &board.tasks {
        let from = text(&task["id"]);
        for link in task["links"].as_array().into_iter().flatten() {
            let to = text(&link["to"]);
            let kind = link["kind"].as_str().unwrap_or("relates");
            let lit = focus.as_ref().is_some_and(|f| f == &from || f == &to);
            if !lit && !view.links {
                continue;
            }
            let (Some(a), Some(b)) = (rect_of(&from), rect_of(&to)) else { continue };
            let tint = match kind {
                "blocks" => ACCENT,
                "duplicates" => color("#c58af9"),
                _ => design::mix(p.muted, p.text, 0.25),
            };
            if lit {
                related.insert(if focus.as_deref() == Some(from.as_str()) { to.clone() } else { from.clone() }, tint);
            }
            connector(&mut shapes, a, b, if lit { tint } else { tint.gamma_multiply(0.45) }, if lit { 2. } else { 1.2 }, kind == "blocks");
        }
    }
    ui.painter().set(links_layer, Shape::Vec(shapes));

    let to_global = ui.ctx().layer_transform_to_global(ui.layer_id());
    for (index, card) in plan.cards.iter().enumerate() {
        for (at, shapes) in &plan.overlays {
            if *at == index {
                ui.painter().extend(shapes.clone());
            }
        }
        if let Some(t) = to_global {
            view.hits.insert(format!("card:{}", card.id), t * card.rect);
        }
        let Some(task) = board.task(&card.id) else { continue };
        let dragged = view.drag.as_ref().is_some_and(|d| d.id == card.id && d.link.is_none());
        let look = Look {
            selected: view.selected.as_deref() == Some(card.id.as_str()),
            hovered: hovered.as_deref() == Some(card.id.as_str()),
            related: related.get(&card.id).copied(),
            faded: dragged,
        };
        paint_card(ui.painter(), card, task, board, look, p);
    }
    for (at, shapes) in &plan.overlays {
        if *at >= plan.cards.len() {
            ui.painter().extend(shapes.clone());
        }
    }

    let Some(drag) = &view.drag else { return };
    ui.ctx().request_repaint();
    let Some(pos) = pointer else {
        if !ui.input(|i| i.pointer.any_down()) {
            view.drag = None;
        }
        return;
    };
    let Some(source) = plan.cards.iter().find(|c| c.id == drag.id) else { return };
    let painter = ui.painter();
    if let Some(kind) = drag.link {
        // Shift-drag: draw a link to the card under the pointer.
        let target = plan.cards.iter().find(|c| c.id != drag.id && c.rect.contains(pos));
        let tint = if kind == "blocks" { ACCENT } else { p.text };
        let mut shapes = vec![];
        match target {
            Some(t) => connector(&mut shapes, source.rect, t.rect, tint, 2., kind == "blocks"),
            None => connector(&mut shapes, source.rect, Rect::from_center_size(pos, Vec2::splat(2.)), tint, 2., kind == "blocks"),
        }
        painter.extend(shapes);
        if let Some(t) = target {
            painter.rect_stroke(t.rect, 10., Stroke::new(2.0_f32, tint), egui::StrokeKind::Outside);
        }
        let hint = if kind == "blocks" { "блокирует" } else { "связать" };
        painter.text(pos + Vec2::new(14., -14.), egui::Align2::LEFT_BOTTOM, hint, FontId::proportional(12.), tint);
        if released {
            if let Some(t) = target {
                ops.push(json!({"op":"link","id":drag.id,"to":t.id,"kind":kind}));
            }
            view.drag = None;
        }
        return;
    }
    // Move: ghost under the pointer, insertion mark in the target column or cell.
    // The topmost frame under the pointer wins when frames overlap.
    let zone = plan.zones.iter().rev().find(|z| z.rect.contains(pos));
    let mut target = None;
    if let Some(zone) = zone {
        // A frame keeps the card's status and only changes its category.
        let keep = zone.status.is_empty();
        let status = if keep { source.status.as_str() } else { zone.status.as_str() };
        let peers: Vec<&CardBox> = plan
            .cards
            .iter()
            .filter(|c| c.id != drag.id && c.status == status && zone.category.as_ref().is_none_or(|cat| &c.category == cat || (cat.is_empty() && !board.colors.contains_key(&c.category))))
            .filter(|c| if keep { zone.rect.contains(c.rect.center()) } else { zone.rect.x_range().contains(c.rect.center().x) })
            .collect();
        let before = peers.iter().filter(|c| c.rect.center().y > pos.y).min_by(|a, b| a.rect.top().total_cmp(&b.rect.top()));
        let y = match before {
            Some(c) => c.rect.top() - CARD_GAP / 2.,
            None => peers.iter().map(|c| c.rect.bottom() + CARD_GAP / 2.).fold(zone.top, f32::max),
        };
        let (left, right) = if keep {
            painter.rect_stroke(zone.rect, 16., Stroke::new(2.0_f32, ACCENT.gamma_multiply(0.6)), egui::StrokeKind::Outside);
            (zone.rect.left() + INNER, zone.rect.right() - INNER)
        } else {
            let x = zone.rect.left() + GAP / 2.;
            let w = zone.rect.width() - GAP;
            if zone.category.is_some() { (x, x + w) } else { (x + INNER, x + w - INNER) }
        };
        painter.line_segment([Pos2::new(left, y), Pos2::new(right, y)], Stroke::new(3.0_f32, ACCENT));
        painter.circle_filled(Pos2::new(left, y), 4., ACCENT);
        target = Some((zone, before.map(|c| c.id.clone())));
    }
    let ghost = Rect::from_min_size(pos - drag.grab, source.rect.size());
    painter.add(Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(ghost, 10.));
    let lifted = CardBox { id: source.id.clone(), rect: ghost, status: source.status.clone(), category: source.category.clone(), compact: source.compact, title: source.title.clone() };
    if let Some(task) = board.task(&drag.id) {
        paint_card(painter, &lifted, task, board, Look { selected: true, hovered: false, related: None, faded: false }, p);
    }
    // Near an edge the view follows the card.
    let visible = ui.clip_rect();
    let edge = 60. / view.scale;
    let mut shift = Vec2::ZERO;
    if pos.x < visible.left() + edge { shift.x -= 14.; }
    if pos.x > visible.right() - edge { shift.x += 14.; }
    if pos.y < visible.top() + edge { shift.y -= 14.; }
    if pos.y > visible.bottom() - edge { shift.y += 14.; }
    if shift != Vec2::ZERO {
        view.pending = Some((view.origin + shift / view.scale, view.scale));
    }
    if released {
        if let Some((zone, before)) = target {
            if let Some(category) = &zone.category {
                if *category != source.category {
                    ops.push(json!({"op":"update","id":drag.id,"category":category}));
                }
            }
            if zone.status.is_empty() {
                if let Some(before) = before {
                    ops.push(json!({"op":"move","id":drag.id,"status":source.status,"before":before}));
                }
            } else {
                let mut op = json!({"op":"move","id":drag.id,"status":zone.status});
                if let Some(before) = before {
                    op["before"] = json!(before);
                }
                ops.push(op);
            }
        }
        view.drag = None;
    }
}

/// A connector that leaves from the sides of the cards, so it never crosses their text.
fn connector(shapes: &mut Vec<Shape>, a: Rect, b: Rect, tint: Color32, width: f32, arrow: bool) {
    let (start, end, c1, c2);
    if a.right() + 12. < b.left() || b.right() + 12. < a.left() {
        let forward = a.center().x < b.center().x;
        start = if forward { a.right_center() } else { a.left_center() };
        end = if forward { b.left_center() } else { b.right_center() };
        let bend = ((end.x - start.x).abs() * 0.5).max(40.) * if forward { 1. } else { -1. };
        c1 = start + Vec2::new(bend, 0.);
        c2 = end - Vec2::new(bend, 0.);
    } else {
        // Same column: a bracket on the right edge.
        start = a.right_center();
        end = b.right_center();
        let out = a.right().max(b.right()) + 22. + (end.y - start.y).abs() * 0.08;
        c1 = Pos2::new(out, start.y);
        c2 = Pos2::new(out, end.y);
    }
    shapes.push(Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(
        [start, c1, c2, end],
        false,
        Color32::TRANSPARENT,
        Stroke::new(width, tint),
    )));
    shapes.push(Shape::circle_filled(start, width + 1.5, tint));
    if arrow {
        let dir = (end - c2).normalized();
        let side = Vec2::new(-dir.y, dir.x);
        shapes.push(Shape::convex_polygon(
            vec![end, end - dir * 9. + side * 4.5, end - dir * 9. - side * 4.5],
            tint,
            Stroke::NONE,
        ));
    } else {
        shapes.push(Shape::circle_filled(end, width + 1.5, tint));
    }
}

#[derive(Clone, Copy)]
struct Look {
    selected: bool,
    hovered: bool,
    related: Option<Color32>,
    faded: bool,
}

fn avatar(painter: &egui::Painter, center: Pos2, radius: f32, name: &str, p: Palette) {
    let tint = person(name);
    painter.circle_filled(center, radius, design::mix(p.surface, tint, 0.45));
    painter.text(center, egui::Align2::CENTER_CENTER, initials(name), FontId::proportional(radius * 0.95), p.text);
}

fn paint_card(painter: &egui::Painter, card: &CardBox, task: &Value, board: &Board, look: Look, p: Palette) {
    let rect = card.rect;
    let alpha = if look.faded { 0.3 } else { 1. };
    let tint = board.tint(&card.category, p);
    if (look.hovered || look.selected) && !look.faded {
        painter.add(Shadow { offset: [0, 3], blur: 12, spread: 0, color: Color32::from_black_alpha(45) }.as_shape(rect, 10.));
    }
    painter.rect_filled(rect, 10., p.surface.gamma_multiply(alpha));
    let border = if look.selected {
        Stroke::new(1.5_f32, ACCENT)
    } else if let Some(c) = look.related {
        Stroke::new(1.5_f32, c)
    } else if look.hovered {
        Stroke::new(1.0_f32, design::mix(p.line, p.text, 0.35))
    } else {
        Stroke::new(1.0_f32, p.line.gamma_multiply(alpha))
    };
    painter.rect_stroke(rect, 10., border, egui::StrokeKind::Inside);
    let who = text(&task["assignee"]);
    if card.compact {
        let check = Rect::from_center_size(Pos2::new(rect.left() + 17., rect.center().y), Vec2::splat(14.));
        design::icon(painter, check, Icon::Check, GREEN.gamma_multiply(alpha));
        painter.galley(Pos2::new(rect.left() + 32., rect.center().y - card.title.size().y / 2.), card.title.clone(), p.muted);
        painter.circle_filled(Pos2::new(rect.right() - 36., rect.center().y), 3., tint.gamma_multiply(alpha));
        if !who.is_empty() {
            avatar(painter, Pos2::new(rect.right() - 17., rect.center().y), 9., &who, p);
        }
        return;
    }
    painter.rect_filled(
        Rect::from_min_max(rect.left_top() + Vec2::new(6., 9.), Pos2::new(rect.left() + 9., rect.bottom() - 9.)),
        1.5,
        tint.gamma_multiply(alpha),
    );
    let title_pos = rect.left_top() + Vec2::new(17., 10.);
    painter.galley(title_pos, card.title.clone(), p.text);
    let progress = task["progress"].as_f64().unwrap_or(0.) as f32;
    if progress > 0. && progress < 100. {
        let y = title_pos.y + card.title.size().y + 7.;
        bar(painter, Rect::from_min_size(Pos2::new(rect.left() + 17., y), Vec2::new(rect.width() - 30., 3.)), progress, tint, p);
    }
    // Footer: id, priority, links, comments · size and who.
    let cy = rect.bottom() - 15.;
    let small = FontId::proportional(11.);
    let mut x = rect.left() + 17.;
    let id = painter.text(Pos2::new(x, cy), egui::Align2::LEFT_CENTER, &card.id, small.clone(), p.muted);
    x = id.right() + 8.;
    let priority = match task["priority"].as_str().unwrap_or("") {
        "critical" => Some("срочно"),
        "high" => Some("важно"),
        _ => None,
    };
    if let Some(label) = priority {
        let galley = painter.layout_no_wrap(label.into(), small.clone(), ACCENT);
        let pill = Rect::from_min_size(Pos2::new(x, cy - 8.), Vec2::new(galley.size().x + 12., 16.));
        painter.rect_filled(pill, 8., ACCENT.gamma_multiply(0.16));
        painter.galley(pill.min + Vec2::new(6., 8. - galley.size().y / 2.), galley, ACCENT);
        x = pill.right() + 8.;
    }
    let links = task["links"].as_array().map_or(0, Vec::len);
    if links > 0 {
        // Two rings of a chain.
        for dx in [0., 5.] {
            painter.circle_stroke(Pos2::new(x + 3. + dx, cy), 3., Stroke::new(1.2_f32, p.muted));
        }
        let r = painter.text(Pos2::new(x + 14., cy), egui::Align2::LEFT_CENTER, links.to_string(), small.clone(), p.muted);
        x = r.right() + 8.;
    }
    let comments = task["comments"].as_array().map_or(0, Vec::len);
    if comments > 0 {
        let bubble = Rect::from_min_size(Pos2::new(x, cy - 4.5), Vec2::new(10., 7.5));
        painter.rect_stroke(bubble, 2., Stroke::new(1.2_f32, p.muted), egui::StrokeKind::Inside);
        painter.line_segment([Pos2::new(x + 2., bubble.bottom()), Pos2::new(x + 1., bubble.bottom() + 3.)], Stroke::new(1.2_f32, p.muted));
        painter.text(Pos2::new(x + 14., cy), egui::Align2::LEFT_CENTER, comments.to_string(), small.clone(), p.muted);
    }
    let mut right = rect.right() - 12.;
    if !who.is_empty() {
        avatar(painter, Pos2::new(right - 9., cy), 9.5, &who, p);
        right -= 26.;
    }
    if let Some(estimate) = task["estimate"].as_f64() {
        let label = if estimate.fract() == 0. { format!("{estimate:.0}") } else { format!("{estimate}") };
        painter.text(Pos2::new(right, cy), egui::Align2::RIGHT_CENTER, label, small, p.muted);
    }
}

fn controls(ui: &egui::Ui, view: &mut BoardView, p: Palette, outer: Rect) {
    let ctx = ui.ctx().clone();
    egui::Area::new(egui::Id::new("board-zoom"))
        .order(egui::Order::Middle)
        .fixed_pos(outer.right_bottom() - Vec2::new(250., 52.))
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.line))
                .corner_radius(10.)
                .inner_margin(egui::Margin::same(4))
                .shadow(Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(40) })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.;
                        let zoom = |view: &mut BoardView, to: f32| {
                            let center = view.origin + view.viewport / view.scale / 2.;
                            view.scale = to.clamp(0.3, 1.);
                            view.origin = center - view.viewport / view.scale / 2.;
                        };
                        if design::button(ui, "−", None, Kind::Ghost, p, Vec2::new(30., 28.)).on_hover_text("Отдалить · Ctrl+колесо").clicked() {
                            zoom(view, view.scale - 0.1);
                        }
                        if design::button(ui, &format!("{:.0}%", view.scale * 100.), None, Kind::Ghost, p, Vec2::new(54., 28.))
                            .on_hover_text("Масштаб 100%")
                            .clicked()
                        {
                            zoom(view, 1.);
                        }
                        if design::button(ui, "+", None, Kind::Ghost, p, Vec2::new(30., 28.)).on_hover_text("Приблизить").clicked() {
                            zoom(view, view.scale + 0.1);
                        }
                        if design::button(ui, "Вписать", None, Kind::Ghost, p, Vec2::new(0., 28.))
                            .on_hover_text("Показать всю доску")
                            .clicked()
                            && view.content.is_finite()
                        {
                            let content = view.content.expand(28.);
                            view.scale = (view.viewport.x / content.width()).min(view.viewport.y / content.height()).clamp(0.3, 1.);
                            view.origin = Pos2::new(content.left(), content.top());
                        }
                    });
                });
        });
    if outer.width() > 820. {
        egui::Area::new(egui::Id::new("board-hint"))
            .order(egui::Order::Middle)
            .interactable(false)
            .fixed_pos(outer.left_bottom() + Vec2::new(18., -44.))
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(p.surface.gamma_multiply(0.85))
                    .corner_radius(8.)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        let hint = match view.mode {
                            Mode::Canvas => "Заголовок категории — тяни, чтобы двигать · фон — двигаться · Ctrl+колесо — масштаб",
                            Mode::Lanes => "Название слева — тяни, чтобы переставить · фон — двигаться · Ctrl+колесо — масштаб",
                            Mode::Columns => "Фон — двигаться · Ctrl+колесо — масштаб · Shift+карточка на другую — связать",
                        };
                        small(ui, hint, 11., p.muted);
                    });
            });
    }
}

// --- dialogs and panels ---------------------------------------------------------------------

fn create_dialog(ctx: &egui::Context, board: &Board, view: &mut BoardView, p: Palette, ops: &mut Vec<Value>) {
    let Some(create) = view.create.as_mut() else { return };
    let mut close = false;
    let mut submit = false;
    let modal = egui::Modal::new(egui::Id::new("board-create"))
        .frame(egui::Frame::new().fill(p.surface).corner_radius(14.).inner_margin(20.))
        .show(ctx, |ui| {
            ui.set_width(440.);
            small(ui, "Новая задача", 18., p.text);
            ui.add_space(10.);
            let response = ui.add(
                egui::TextEdit::multiline(&mut create.title)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .font(FontId::proportional(14.))
                    .hint_text(RichText::new("Что нужно сделать · Enter — создать").color(p.muted)),
            );
            if create.focus {
                response.request_focus();
                create.focus = false;
            }
            if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
                submit = true;
            }
            ui.add_space(8.);
            ui.horizontal(|ui| {
                small(ui, "Колонка", 12., p.muted);
                egui::ComboBox::from_id_salt("create-status")
                    .selected_text(status_name(board.state, &create.status))
                    .width(130.)
                    .show_ui(ui, |ui| {
                        for (id, name) in &board.columns {
                            ui.selectable_value(&mut create.status, id.clone(), name);
                        }
                    });
                small(ui, "Категория", 12., p.muted);
                egui::ComboBox::from_id_salt("create-category")
                    .selected_text(board.names.get(&create.category).cloned().unwrap_or_else(|| "—".into()))
                    .width(150.)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut create.category, String::new(), "—");
                        for c in &board.categories {
                            ui.selectable_value(&mut create.category, text(&c["id"]), text(&c["name"]));
                        }
                    });
            });
            ui.add_space(14.);
            ui.horizontal(|ui| {
                if design::button(ui, "Отмена", None, Kind::Secondary, p, Vec2::new(0., 32.)).clicked() {
                    close = true;
                }
                ui.add_enabled_ui(!create.title.trim().is_empty(), |ui| {
                    if design::button(ui, "Создать", Some(Icon::Check), Kind::Primary, p, Vec2::new(0., 32.)).clicked() {
                        submit = true;
                    }
                });
            });
        });
    if submit && !create.title.trim().is_empty() {
        ops.push(json!({"op":"create","title":create.title.trim(),"status":create.status,"category":create.category}));
        close = true;
    }
    if close || modal.should_close() {
        view.create = None;
    }
}

fn plural(n: usize, one: &str, few: &str, many: &str) -> String {
    let word = match (n % 10, n % 100) {
        (1, r) if r != 11 => one,
        (2..=4, r) if !(12..=14).contains(&r) => few,
        _ => many,
    };
    format!("{n} {word}")
}

fn grip(painter: &egui::Painter, center: Pos2, color: Color32) {
    for (dx, dy) in [(-2.5, -5.), (-2.5, 0.), (-2.5, 5.), (2.5, -5.), (2.5, 0.), (2.5, 5.)] {
        painter.circle_filled(center + Vec2::new(dx, dy), 1.4, color);
    }
}

/// Create, rename, recolour, describe, reorder and delete categories. Every change is sent
/// as soon as it is made; names and descriptions when the field is left.
fn category_editor(ctx: &egui::Context, board: &Board, view: &mut BoardView, p: Palette, ops: &mut Vec<Value>) {
    let Some(manager) = view.manager.as_mut() else { return };
    let mut close = false;
    let mut dropped = false;
    let mut deleted = None;
    let mut rows: Vec<(String, Rect)> = vec![];
    let counts: HashMap<String, usize> = board.categories.iter().map(|c| {
        let id = text(&c["id"]);
        let n = board.tasks.iter().filter(|t| t["category"] == id.as_str()).count();
        (id, n)
    }).collect();
    let modal = egui::Modal::new(egui::Id::new("board-categories"))
        .frame(egui::Frame::new().fill(p.surface).corner_radius(14.).inner_margin(20.))
        .show(ctx, |ui| {
            ui.set_width(580.);
            ui.horizontal(|ui| {
                small(ui, "Категории", 18., p.text);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if design::button(ui, "", Some(Icon::Close), Kind::Ghost, p, Vec2::splat(28.)).clicked() {
                        close = true;
                    }
                });
            });
            small(ui, "Тяни за точки слева, чтобы поменять порядок. Изменения сохраняются сразу и видны всей команде.", 11.5, p.muted);
            ui.add_space(10.);
            egui::ScrollArea::vertical().max_height(430.).auto_shrink([false, true]).show(ui, |ui| {
                for (index, category) in board.categories.iter().enumerate() {
                    let id = text(&category["id"]);
                    let current = text(&category["name"]);
                    let tint = board.tint(&id, p);
                    let focused = manager.focus.as_deref() == Some(id.as_str());
                    let dragging = manager.drag.as_deref() == Some(id.as_str());
                    let count = counts.get(&id).copied().unwrap_or(0);
                    let frame = egui::Frame::new()
                        .fill(if focused { tint.gamma_multiply(0.12) } else { design::mix(p.surface, p.line, 0.3) })
                        .corner_radius(10.)
                        .inner_margin(egui::Margin { left: 6, right: 8, top: 8, bottom: 8 })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if dragging {
                                ui.set_opacity(0.35);
                            }
                            ui.horizontal(|ui| {
                                let (handle, response) = ui.allocate_exact_size(Vec2::new(18., 28.), Sense::drag());
                                let response = response.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Перетащи, чтобы поменять порядок");
                                grip(ui.painter(), handle.center(), if response.hovered() { p.text } else { p.muted });
                                if response.drag_started() {
                                    manager.drag = Some(id.clone());
                                }
                                dropped |= response.drag_stopped();
                                let (dot, response) = ui.allocate_exact_size(Vec2::splat(26.), Sense::click());
                                ui.painter().circle_filled(dot.center(), 9., tint);
                                if response.hovered() {
                                    ui.painter().circle_stroke(dot.center(), 11.5, Stroke::new(1.5_f32, p.text));
                                }
                                if response.on_hover_text("Цвет").clicked() {
                                    manager.palette = if manager.palette.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
                                }
                                let name = manager.names.entry(id.clone()).or_insert_with(|| current.clone());
                                let response = ui.add(
                                    egui::TextEdit::singleline(name)
                                        .desired_width(220.)
                                        .font(FontId::proportional(14.))
                                        .hint_text(RichText::new("Название").color(p.muted)),
                                );
                                if focused && manager.focus_once {
                                    response.request_focus();
                                    response.scroll_to_me(Some(Align::Center));
                                    manager.focus_once = false;
                                }
                                if response.lost_focus() {
                                    let new = name.trim().to_owned();
                                    if !new.is_empty() && new != current {
                                        ops.push(json!({"op":"category","id":id,"name":new}));
                                    } else {
                                        *name = current.clone();
                                    }
                                } else if !response.has_focus() {
                                    *name = current.clone();
                                }
                                small(ui, plural(count, "задача", "задачи", "задач"), 11.5, p.muted);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing.x = 2.;
                                    if design::button(ui, "Удалить", None, Kind::Ghost, p, Vec2::new(0., 26.)).clicked() {
                                        manager.deleting = Some((id.clone(), String::new()));
                                    }
                                    let last = index + 1 == board.categories.len();
                                    ui.add_enabled_ui(!last, |ui| {
                                        if design::button(ui, "↓", None, Kind::Ghost, p, Vec2::new(26., 26.)).on_hover_text("Ниже").clicked() {
                                            let after = board.categories.get(index + 2).map(|c| text(&c["id"]));
                                            ops.extend(reorder(board, &id, after.as_deref()));
                                        }
                                    });
                                    ui.add_enabled_ui(index > 0, |ui| {
                                        if design::button(ui, "↑", None, Kind::Ghost, p, Vec2::new(26., 26.)).on_hover_text("Выше").clicked() {
                                            let previous = text(&board.categories[index.saturating_sub(1)]["id"]);
                                            ops.extend(reorder(board, &id, Some(&previous)));
                                        }
                                    });
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(50.);
                                let current = text(&category["description"]);
                                let description = manager.descriptions.entry(id.clone()).or_insert_with(|| current.clone());
                                let response = ui.add(
                                    egui::TextEdit::singleline(description)
                                        .desired_width(f32::INFINITY)
                                        .font(FontId::proportional(12.))
                                        .hint_text(RichText::new("Описание: что сюда входит (необязательно)").color(p.muted)),
                                );
                                if response.lost_focus() {
                                    if description.trim() != current {
                                        ops.push(json!({"op":"category","id":id,"description":description.trim()}));
                                    }
                                } else if !response.has_focus() {
                                    *description = current;
                                }
                            });
                            if manager.palette.as_deref() == Some(id.as_str()) {
                                ui.horizontal_wrapped(|ui| {
                                    ui.add_space(50.);
                                    let selected = category["color"].as_str().unwrap_or("").to_lowercase();
                                    for hex in SWATCHES {
                                        let (r, response) = ui.allocate_exact_size(Vec2::splat(24.), Sense::click());
                                        ui.painter().circle_filled(r.center(), 9., color(hex));
                                        if selected == hex || response.hovered() {
                                            ui.painter().circle_stroke(r.center(), 11.5, Stroke::new(1.5_f32, p.text));
                                        }
                                        if response.clicked() {
                                            ops.push(json!({"op":"category","id":id,"color":hex}));
                                            manager.palette = None;
                                        }
                                    }
                                });
                            }
                            if let Some((target, move_to)) = manager.deleting.as_mut().filter(|(target, _)| *target == id) {
                                ui.add_space(4.);
                                egui::Frame::new()
                                    .fill(ACCENT.gamma_multiply(0.1))
                                    .corner_radius(8.)
                                    .inner_margin(egui::Margin::symmetric(10, 8))
                                    .show(ui, |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            if count == 0 {
                                                small(ui, format!("Удалить «{current}»? Задач в ней нет."), 12.5, p.text);
                                            } else {
                                                small(ui, format!("Удалить «{current}»? Её {} перенести в", plural(count, "задачу", "задачи", "задач")), 12.5, p.text);
                                                egui::ComboBox::from_id_salt("delete-move")
                                                    .selected_text(board.names.get(move_to.as_str()).cloned().unwrap_or_else(|| "без категории".into()))
                                                    .width(170.)
                                                    .show_ui(ui, |ui| {
                                                        ui.selectable_value(move_to, String::new(), "без категории");
                                                        for other in &board.categories {
                                                            let other_id = text(&other["id"]);
                                                            if other_id != *target {
                                                                ui.selectable_value(move_to, other_id, text(&other["name"]));
                                                            }
                                                        }
                                                    });
                                            }
                                        });
                                        ui.add_space(6.);
                                        ui.horizontal(|ui| {
                                            if design::button(ui, "Удалить категорию", None, Kind::Primary, p, Vec2::new(0., 28.)).clicked() {
                                                ops.push(json!({"op":"category","id":target,"delete":true,"move_to":move_to}));
                                                deleted = Some(target.clone());
                                            }
                                            if design::button(ui, "Отмена", None, Kind::Ghost, p, Vec2::new(0., 28.)).clicked() {
                                                deleted = Some(String::new());
                                            }
                                        });
                                    });
                            }
                        });
                    rows.push((id, frame.response.rect));
                    ui.add_space(6.);
                }
            });
            if let Some(dragged) = manager.drag.clone() {
                let pointer = ui.ctx().pointer_latest_pos().unwrap_or_default();
                let before = rows.iter().filter(|(id, _)| *id != dragged).find(|(_, r)| r.center().y > pointer.y);
                let y = match before {
                    Some((_, r)) => r.top() - 3.,
                    None => rows.iter().filter(|(id, _)| *id != dragged).map(|(_, r)| r.bottom() + 3.).fold(f32::MIN, f32::max),
                };
                if let Some((_, first)) = rows.first() {
                    ui.painter().line_segment([Pos2::new(first.left(), y), Pos2::new(first.right(), y)], Stroke::new(3.0_f32, ACCENT));
                }
                ui.ctx().request_repaint();
                if dropped || !ui.input(|i| i.pointer.any_down()) {
                    ops.extend(reorder(board, &dragged, before.map(|(id, _)| id.as_str())));
                    manager.drag = None;
                }
            }
            ui.add_space(6.);
            ui.separator();
            ui.add_space(6.);
            ui.horizontal(|ui| {
                ui.add_space(24.);
                let (dot, response) = ui.allocate_exact_size(Vec2::splat(26.), Sense::click());
                ui.painter().circle_filled(dot.center(), 9., color(&manager.new_color));
                if response.hovered() {
                    ui.painter().circle_stroke(dot.center(), 11.5, Stroke::new(1.5_f32, p.text));
                }
                if response.on_hover_text("Другой цвет").clicked() {
                    let at = SWATCHES.iter().position(|s| *s == manager.new_color).unwrap_or(0);
                    manager.new_color = SWATCHES[(at + 1) % SWATCHES.len()].into();
                }
                let response = ui.add(
                    egui::TextEdit::singleline(&mut manager.new_name)
                        .desired_width(260.)
                        .font(FontId::proportional(14.))
                        .hint_text(RichText::new("Новая категория").color(p.muted)),
                );
                if manager.focus_new {
                    response.request_focus();
                    manager.focus_new = false;
                }
                let name = manager.new_name.trim().to_owned();
                let exists = board.categories.iter().any(|c| text(&c["name"]).to_lowercase() == name.to_lowercase());
                let ready = !name.is_empty() && !exists;
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let mut add = false;
                ui.add_enabled_ui(ready, |ui| {
                    add = design::button(ui, "Добавить", Some(Icon::Plus), Kind::Primary, p, Vec2::new(0., 30.)).clicked();
                });
                if exists {
                    small(ui, "уже есть", 11.5, ACCENT);
                }
                if ready && (add || enter) {
                    ops.push(json!({"op":"category","new":true,"name":name,"color":manager.new_color}));
                    manager.new_name.clear();
                    let at = SWATCHES.iter().position(|s| *s == manager.new_color).unwrap_or(0);
                    manager.new_color = SWATCHES[(at + 1) % SWATCHES.len()].into();
                    manager.focus_new = true;
                }
            });
            ui.add_space(14.);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if design::button(ui, "Готово", None, Kind::Secondary, p, Vec2::new(0., 30.)).clicked() {
                    close = true;
                }
            });
        });
    if let Some(target) = deleted {
        manager.deleting = None;
        if !target.is_empty() && view.filter.as_deref() == Some(target.as_str()) {
            view.filter = None;
        }
    }
    if close || modal.should_close() {
        view.manager = None;
    }
}

fn section(ui: &mut egui::Ui, title: &str, p: Palette) {
    ui.add_space(14.);
    small(ui, title.to_uppercase(), 10.5, p.muted);
    ui.add_space(4.);
}

fn detail(ui: &mut egui::Ui, task: &Value, board: &Board, view: &mut BoardView, p: Palette, me: &str, ops: &mut Vec<Value>) {
    let id = text(&task["id"]);
    if view.edit.as_ref().is_none_or(|e| e.id != id) {
        view.edit = Some(Edit::of(task));
        view.comment.clear();
        view.link_to.clear();
    }
    let original = Edit::of(task);
    let status = text(&task["status"]);
    let mut close = false;
    ui.horizontal(|ui| {
        small(ui, &id, 12.5, p.muted);
        let tint = status_color(&status, p);
        let galley = ui.painter().layout_no_wrap(status_name(board.state, &status), FontId::proportional(11.5), tint);
        let (r, _) = ui.allocate_exact_size(galley.size() + Vec2::new(16., 6.), Sense::hover());
        ui.painter().rect_filled(r, 9., tint.gamma_multiply(0.16));
        ui.painter().galley(r.min + Vec2::new(8., 3.), galley, tint);
        if let Some(tint) = board.colors.get(&text(&task["category"])) {
            ui.add_space(4.);
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.), Sense::hover());
            ui.painter().circle_filled(dot.center(), 4., *tint);
            small(ui, board.names.get(&text(&task["category"])).cloned().unwrap_or_default(), 11.5, p.muted);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if design::button(ui, "", Some(Icon::Close), Kind::Ghost, p, Vec2::splat(26.)).on_hover_text("Закрыть · Esc").clicked() {
                close = true;
            }
        });
    });
    ui.add_space(6.);
    egui::ScrollArea::vertical().id_salt("board-detail-scroll").auto_shrink([false, false]).show(ui, |ui| {
        let edit = view.edit.as_mut().unwrap();
        egui::Frame::new()
            .fill(design::mix(p.side, p.line, 0.35))
            .corner_radius(8.)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut edit.title).frame(false).font(FontId::proportional(16.5)).desired_rows(1).desired_width(f32::INFINITY));
            });
        if *edit != original {
            ui.add_space(8.);
            ui.horizontal(|ui| {
                if design::button(ui, "Сохранить", Some(Icon::Check), Kind::Primary, p, Vec2::new(0., 30.)).clicked() {
                    ops.push(json!({"op":"update","id":id,"title":edit.title.trim(),"description":edit.description,"category":edit.category,"priority":edit.priority,"assignee":edit.assignee.trim(),"estimate":edit.estimate,"progress":edit.progress}));
                }
                if design::button(ui, "Отменить", None, Kind::Ghost, p, Vec2::new(0., 30.)).clicked() {
                    *edit = original.clone();
                }
                small(ui, "есть несохранённые правки", 11.5, p.muted);
            });
        }
        section(ui, "Свойства", p);
        egui::Grid::new("task-fields").num_columns(2).spacing([12., 8.]).min_col_width(90.).show(ui, |ui| {
            small(ui, "Статус", 12.5, p.muted);
            let mut next = status.clone();
            egui::ComboBox::from_id_salt("task-status").selected_text(status_name(board.state, &status)).width(220.).show_ui(ui, |ui| {
                for (sid, name) in &board.columns {
                    ui.selectable_value(&mut next, sid.clone(), name);
                }
            });
            if next != status {
                ops.push(json!({"op":"move","id":id,"status":next}));
            }
            ui.end_row();
            small(ui, "Категория", 12.5, p.muted);
            egui::ComboBox::from_id_salt("task-category")
                .selected_text(board.names.get(&edit.category).cloned().unwrap_or_else(|| "—".into()))
                .width(220.)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut edit.category, String::new(), "—");
                    for c in &board.categories {
                        ui.selectable_value(&mut edit.category, text(&c["id"]), text(&c["name"]));
                    }
                });
            ui.end_row();
            small(ui, "Исполнитель", 12.5, p.muted);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut edit.assignee).desired_width(170.).hint_text(RichText::new("никто").color(p.muted)));
                if !me.is_empty() && edit.assignee != me && design::button(ui, "Я", None, Kind::Secondary, p, Vec2::new(0., 26.)).on_hover_text("Взять себе").clicked() {
                    edit.assignee = me.to_owned();
                }
            });
            ui.end_row();
            small(ui, "Приоритет", 12.5, p.muted);
            egui::ComboBox::from_id_salt("task-priority")
                .selected_text(match edit.priority.as_str() {
                    "low" => "низкий",
                    "high" => "важно",
                    "critical" => "срочно",
                    _ => "обычный",
                })
                .width(220.)
                .show_ui(ui, |ui| {
                    for (v, n) in [("low", "низкий"), ("", "обычный"), ("high", "важно"), ("critical", "срочно")] {
                        ui.selectable_value(&mut edit.priority, v.into(), n);
                    }
                });
            ui.end_row();
            small(ui, "Размер", 12.5, p.muted);
            ui.add(egui::DragValue::new(&mut edit.estimate).range(0.5..=40.).speed(0.25).suffix(" оч."))
                .on_hover_text("Относительный размер: 1 — мелочь, 8 — большая фича. От него считается прогресс.");
            ui.end_row();
            small(ui, "Готовность", 12.5, p.muted);
            ui.add(egui::Slider::new(&mut edit.progress, 0..=100).suffix("%"));
            ui.end_row();
        });
        section(ui, "Описание", p);
        ui.add(
            egui::TextEdit::multiline(&mut edit.description)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text(RichText::new("Что сделать, критерии готовности, ссылки на файлы").color(p.muted)),
        );

        section(ui, "Связи", p);
        let mut links: Vec<(String, String, bool)> = task["links"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| (text(&l["to"]), text(&l["kind"]), true))
            .collect();
        for other in &board.tasks {
            for l in other["links"].as_array().into_iter().flatten() {
                if l["to"] == id.as_str() {
                    links.push((text(&other["id"]), text(&l["kind"]), false));
                }
            }
        }
        if links.is_empty() {
            small(ui, "Нет связей. Перетащи карточку с Shift на другую или укажи номер ниже.", 11.5, p.muted);
        }
        for (other, kind, outgoing) in links {
            let title = board.task(&other).map(|t| text(&t["title"])).unwrap_or_default();
            let label = match (kind.as_str(), outgoing) {
                ("blocks", true) => "блокирует",
                ("blocks", false) => "ждёт",
                ("duplicates", _) => "дубль",
                _ => "связана",
            };
            let tint = if kind == "blocks" { ACCENT } else { p.muted };
            ui.horizontal(|ui| {
                small(ui, label, 11.5, tint);
                if ui.link(RichText::new(format!("{other} · {}", crate::model::shorten(&title, 34))).size(12.5)).clicked() {
                    view.selected = Some(other.clone());
                    view.focus = Some(other.clone());
                }
                if outgoing && ui.small_button("×").on_hover_text("Убрать связь").clicked() {
                    ops.push(json!({"op":"unlink","id":id,"to":other}));
                }
            });
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
            ui.add(egui::TextEdit::singleline(&mut view.link_to).desired_width(70.).hint_text(RichText::new("T-12").color(p.muted)));
            let target = view.link_to.trim().to_uppercase();
            if ui.add_enabled(board.task(&target).is_some() && target != id, egui::Button::new("Связать")).clicked() {
                ops.push(json!({"op":"link","id":id,"to":target,"kind":view.link_kind}));
                view.link_to.clear();
            }
        });

        section(ui, "Обсуждение и история", p);
        ui.add(egui::TextEdit::multiline(&mut view.comment).desired_rows(2).desired_width(f32::INFINITY).hint_text(RichText::new("Комментарий для команды и агентов").color(p.muted)));
        if !view.comment.trim().is_empty() && design::button(ui, "Добавить", None, Kind::Secondary, p, Vec2::new(0., 28.)).clicked() {
            ops.push(json!({"op":"comment","id":id,"text":view.comment.trim()}));
            view.comment.clear();
        }
        let mut entries: Vec<(f64, String, String, String, bool)> = task["comments"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| (c["ts"].as_f64().unwrap_or(0.), text(&c["author"]), String::new(), text(&c["text"]), true))
            .collect();
        for event in board.state["activity"].as_array().into_iter().flatten().filter(|e| e["task"] == id.as_str() && e["op"] != "comment") {
            entries.push((event["ts"].as_f64().unwrap_or(0.), text(&event["actor"]), text(&event["host"]), text(&event["text"]), false));
        }
        entries.sort_by(|a, b| b.0.total_cmp(&a.0));
        ui.add_space(6.);
        for (ts, who, host, body, comment) in entries.iter().take(80) {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(20.), Sense::hover());
                avatar(ui.painter(), r.center(), 9., who, p);
                small(ui, who, 12., p.text);
                let meta = if host.is_empty() { ago(*ts) } else { format!("{} · {}", host, ago(*ts)) };
                small(ui, meta, 11., p.muted);
            });
            ui.horizontal(|ui| {
                ui.add_space(26.);
                ui.add(egui::Label::new(RichText::new(body).size(12.5).color(if *comment { p.text } else { p.muted })).wrap());
            });
            ui.add_space(6.);
        }
        ui.add_space(8.);
        small(ui, format!("Создал {} · {}", text(&task["created_by"]), ago(task["created_at"].as_f64().unwrap_or(0.))), 11., p.muted);
        if let Some(done) = task["done_at"].as_f64() {
            small(ui, format!("Готово: {} · {}", text(&task["done_by"]), ago(done)), 11., p.muted);
        }
        ui.add_space(10.);
        if ui.button(RichText::new("Удалить задачу").color(ACCENT).size(12.)).on_hover_text("Двойной щелчок").double_clicked() {
            ops.push(json!({"op":"delete","id":id}));
            close = true;
        }
    });
    if close {
        view.selected = None;
        view.edit = None;
    }
}

fn journal(ui: &mut egui::Ui, board: &Board, view: &mut BoardView, p: Palette) {
    ui.horizontal(|ui| {
        small(ui, "Журнал", 16., p.text);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if design::button(ui, "", Some(Icon::Close), Kind::Ghost, p, Vec2::splat(26.)).clicked() {
                view.log = false;
            }
        });
    });
    small(ui, "Кто, что и с какого хоста менял — люди и агенты", 11.5, p.muted);
    section(ui, "Категории", p);
    ui.horizontal_wrapped(|ui| {
        small(ui, format!("{} категорий", board.categories.len()), 12., p.muted);
        if design::button(ui, "Изменить категории", Some(Icon::Settings), Kind::Secondary, p, Vec2::new(0., 28.)).clicked() {
            view.manager = Some(Manager::open(board, None));
        }
    });
    section(ui, "Последние изменения", p);
    egui::ScrollArea::vertical().id_salt("board-activity").auto_shrink([false, false]).show(ui, |ui| {
        for event in board.state["activity"].as_array().into_iter().flatten().take(200) {
            let actor = text(&event["actor"]);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(20.), Sense::hover());
                avatar(ui.painter(), r.center(), 9., &actor, p);
                small(ui, &actor, 12., p.text);
                small(ui, format!("{} · {}", text(&event["host"]), ago(event["ts"].as_f64().unwrap_or(0.))), 11., p.muted);
            });
            ui.horizontal_wrapped(|ui| {
                ui.add_space(26.);
                let task = text(&event["task"]);
                if task.starts_with("T-") && ui.link(RichText::new(&task).size(12.)).clicked() {
                    view.selected = Some(task.clone());
                    view.focus = Some(task.clone());
                    view.log = false;
                }
                small(ui, text(&event["text"]), 12., p.muted);
            });
            ui.add_space(6.);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Value {
        json!({"categories":[{"id":"a","name":"A","order":0},{"id":"b","name":"B","order":1},{"id":"c","name":"C","order":2}],"tasks":[],"columns":[]})
    }

    #[test]
    fn reorder_moves_in_front_of_target_or_to_the_end() {
        let state = state();
        let board = Board::new(&state);
        assert_eq!(reorder(&board, "c", Some("a")).unwrap()["ids"], json!(["c", "a", "b"]));
        assert_eq!(reorder(&board, "a", None).unwrap()["ids"], json!(["b", "c", "a"]));
        assert_eq!(reorder(&board, "a", Some("c")).unwrap()["ids"], json!(["b", "a", "c"]));
    }

    #[test]
    fn reorder_to_the_same_place_sends_nothing() {
        let state = state();
        let board = Board::new(&state);
        assert!(reorder(&board, "b", Some("c")).is_none());
        assert!(reorder(&board, "c", None).is_none());
    }

    fn sample() -> Value {
        json!({
            "title": "Test",
            "categories": [{"id":"a","name":"Alpha","order":0},{"id":"b","name":"Beta","order":1},{"id":"c","name":"Gamma","order":2}],
            "columns": [{"id":"backlog","name":"Бэклог"},{"id":"todo","name":"К работе"},{"id":"done","name":"Готово"}],
            "tasks": [
                {"id":"T-1","title":"one","status":"todo","category":"a","order":1,"links":[],"comments":[]},
                {"id":"T-2","title":"two","status":"backlog","category":"b","order":1,"links":[],"comments":[]},
                {"id":"T-3","title":"three","status":"done","category":"c","order":1,"links":[],"comments":[]}
            ],
            "progress": {"all": {}, "categories": {}},
            "activity": []
        })
    }

    /// Drives the real board UI without a window: one egui pass per input step.
    struct Harness {
        ctx: egui::Context,
        view: BoardView,
        state: Value,
        time: f64,
    }

    impl Harness {
        fn new(lanes: bool) -> Self {
            let mut view = BoardView::default();
            if lanes {
                view.mode = Mode::Lanes;
            }
            let mut h = Self { ctx: egui::Context::default(), view, state: sample(), time: 0. };
            for _ in 0..4 {
                h.frame(vec![]);
            }
            h
        }
        fn frame(&mut self, events: Vec<egui::Event>) -> Vec<Value> {
            self.time += 1. / 60.;
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500., 900.))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut ops = vec![];
            let (view, state) = (&mut self.view, &self.state);
            let _ = self.ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ops.extend(show(ui, state, view, design::palette(false), "me"));
                    ops.extend(take_queued(view));
                });
            });
            ops
        }
        fn drag(&mut self, from: Pos2, to: Pos2) -> Vec<Value> {
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let mut ops = self.frame(vec![egui::Event::PointerMoved(from)]);
            ops.extend(self.frame(vec![button(from, true)]));
            for i in 1..=12 {
                ops.extend(self.frame(vec![egui::Event::PointerMoved(from + (to - from) * (i as f32 / 12.))]));
            }
            ops.extend(self.frame(vec![button(to, false)]));
            ops.extend(self.frame(vec![]));
            ops
        }
    }

    fn reorders(ops: &[Value]) -> Vec<Value> {
        ops.iter().filter(|o| o["op"] == "reorder_categories").map(|o| o["ids"].clone()).collect()
    }

    #[test]
    fn dragging_a_chip_reorders_categories() {
        let mut h = Harness::new(false);
        let alpha = h.view.hits["chip:a"];
        let gamma = h.view.hits["chip:c"];
        let ops = h.drag(alpha.center(), gamma.right_center() + Vec2::new(6., 0.));
        assert_eq!(reorders(&ops), vec![json!(["b", "c", "a"])], "ops: {ops:?}");
        assert!(h.view.chip_drag.is_none());
    }

    #[test]
    fn dragging_a_lane_label_reorders_categories() {
        let mut h = Harness::new(true);
        let alpha = h.view.hits["lane:a"];
        let gamma = h.view.hits["lane:c"];
        let ops = h.drag(alpha.center(), Pos2::new(alpha.center().x, gamma.bottom() + 4.));
        assert_eq!(reorders(&ops), vec![json!(["b", "c", "a"])], "ops: {ops:?}");
        assert!(h.view.lane_drag.is_none());
    }

    #[test]
    fn dragging_a_frame_moves_the_category_on_the_canvas() {
        let mut h = Harness::new(false);
        assert!(h.view.mode == Mode::Canvas);
        let header = h.view.hits["frame:b"];
        let ops = h.drag(header.center(), header.center() + Vec2::new(260., 180.));
        let moved: Vec<&Value> = ops.iter().filter(|o| o["op"] == "category" && o["id"] == "b").collect();
        assert_eq!(moved.len(), 1, "ops: {ops:?}");
        let (x, y) = (moved[0]["x"].as_f64().unwrap(), moved[0]["y"].as_f64().unwrap());
        assert_eq!((x % 10., y % 10.), (0., 0.), "snapped to the grid");
        assert!(x > 300. && y > 100., "moved right and down: {x}, {y}");
        // Until the server confirms, the frame stays where it was dropped.
        h.frame(vec![]);
        assert!(h.view.placed.contains_key("b"));
        let now = h.view.hits["frame:b"];
        assert!(now.center().distance(header.center() + Vec2::new(260., 180.)) < 12., "before {header:?} after {now:?} placed {:?} origin {:?} scale {}", h.view.placed, h.view.origin, h.view.scale);
    }

    #[test]
    fn dropping_a_card_into_another_frame_changes_its_category() {
        let mut h = Harness::new(false);
        let card = h.view.hits["card:T-1"];
        let frame = h.view.hits["frame:b"];
        let ops = h.drag(card.center(), frame.center_bottom() + Vec2::new(0., 40.));
        assert!(ops.iter().any(|o| o["op"] == "update" && o["id"] == "T-1" && o["category"] == "b"), "ops: {ops:?}");
        assert!(!ops.iter().any(|o| o["op"] == "move" && o["status"] != "todo"), "status kept: {ops:?}");
    }

    #[test]
    fn dragging_empty_space_pans_the_view() {
        let mut h = Harness::new(false);
        let before = h.view.origin;
        // Below every frame: empty canvas.
        let empty = Pos2::new(700., 860.);
        let ops = h.drag(empty, empty + Vec2::new(-200., -150.));
        assert!(ops.is_empty(), "ops: {ops:?}");
        assert!(h.view.origin.x > before.x + 150. && h.view.origin.y > before.y + 100., "origin {:?} -> {:?}", before, h.view.origin);
    }

    #[test]
    fn clicking_a_chip_filters_instead_of_moving() {
        let mut h = Harness::new(false);
        let beta = h.view.hits["chip:b"].center();
        let button = |pressed| egui::Event::PointerButton { pos: beta, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        h.frame(vec![egui::Event::PointerMoved(beta)]);
        h.frame(vec![button(true)]);
        let ops = h.frame(vec![button(false)]);
        h.frame(vec![]);
        assert!(reorders(&ops).is_empty());
        assert_eq!(h.view.filter.as_deref(), Some("b"));
    }

    #[test]
    fn russian_plurals() {
        assert_eq!(plural(1, "задача", "задачи", "задач"), "1 задача");
        assert_eq!(plural(3, "задача", "задачи", "задач"), "3 задачи");
        assert_eq!(plural(11, "задача", "задачи", "задач"), "11 задач");
        assert_eq!(plural(22, "задача", "задачи", "задач"), "22 задачи");
        assert_eq!(plural(15, "задача", "задачи", "задач"), "15 задач");
    }
}
