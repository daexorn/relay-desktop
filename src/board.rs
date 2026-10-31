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
const CELL: f32 = 256.;
const LABEL: f32 = 228.;
const GAP: f32 = 18.;
const HEADER: f32 = 46.;
const INNER: f32 = 10.;
const CARD_GAP: f32 = 8.;
/// Finished cards shown before "show more": the history is long and rarely needed.
const COLLAPSED: usize = 8;
const LANE_COLLAPSED: usize = 3;
const SWATCHES: [&str; 8] = ["#d97757", "#5cb884", "#5b8def", "#c58af9", "#e5b84b", "#4fb6c4", "#e06c9f", "#9aa36b"];

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Columns,
    Lanes,
}

struct Drag {
    id: String,
    grab: Vec2,
    /// Some(kind) when Shift was held: draws a link instead of moving the card.
    link: Option<&'static str>,
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
    new_category: String,
    queued: Vec<Value>,
    /// View changes made while drawing the canvas; applied after the scene stores its own.
    pending: Option<(Pos2, f32)>,
    fitted: bool,
}

impl Default for BoardView {
    fn default() -> Self {
        Self {
            selected: None,
            log: false,
            mode: Mode::Columns,
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
            new_category: String::new(),
            queued: vec![],
            pending: None,
            fitted: false,
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
        self.links = args.iter().any(|a| a == "--demo-links");
    }
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
    if view.selected.as_ref().is_some_and(|id| board.task(id).is_none()) {
        view.selected = None;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && view.create.is_none() {
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
    Links,
    Search,
    Category(String),
}

/// Fixed-size chips laid out by hand, so they wrap instead of being squeezed or cut off.
fn chips(ui: &mut egui::Ui, board: &Board, view: &mut BoardView, p: Palette) {
    let mut items = vec![Chip::Mode(Mode::Columns, "Колонки"), Chip::Mode(Mode::Lanes, "Категории"), Chip::Links, Chip::Search];
    items.extend(board.categories.iter().map(|c| Chip::Category(text(&c["id"]))));
    let font = FontId::proportional(12.);
    let width = ui.available_width() - 40.;
    let painter = ui.painter().clone();
    let measure = |s: &str| painter.layout_no_wrap(s.to_owned(), font.clone(), p.text).size().x;
    let label = |chip: &Chip| -> String {
        match chip {
            Chip::Mode(_, name) => (*name).into(),
            Chip::Links => "Все связи".into(),
            Chip::Search => String::new(),
            Chip::Category(id) => board.names.get(id).cloned().unwrap_or_default(),
        }
    };
    let mut placed = vec![];
    let (mut x, mut y) = (0f32, 0f32);
    for (i, chip) in items.iter().enumerate() {
        let w = match chip {
            Chip::Mode(..) => measure(&label(chip)) + 24.,
            Chip::Links => measure(&label(chip)) + 24.,
            Chip::Search => 190.,
            Chip::Category(_) => measure(&label(chip)) + 66.,
        };
        // The mode switch is one segmented control; a wider gap separates the filters.
        let gap = match i {
            1 => 0.,
            4 => 14.,
            _ => 6.,
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
    ui.horizontal(|ui| {
        ui.add_space(20.);
        let (area, _) = ui.allocate_exact_size(Vec2::new(width, y + 24.), Sense::hover());
        for (chip, rect) in items.iter().zip(placed) {
            let rect = rect.translate(area.min.to_vec2());
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
            let response = ui.interact(rect, id, Sense::click());
            let hover = response.hovered();
            let painter = ui.painter();
            let idle = if hover { p.line } else { design::mix(p.base, p.line, 0.5) };
            match chip {
                Chip::Mode(mode, name) => {
                    let active = view.mode == *mode;
                    let corners = if *mode == Mode::Columns {
                        egui::CornerRadius { nw: 12, sw: 12, ne: 0, se: 0 }
                    } else {
                        egui::CornerRadius { nw: 0, sw: 0, ne: 12, se: 12 }
                    };
                    painter.rect_filled(rect, corners, if active { p.text } else { idle });
                    painter.text(rect.center(), egui::Align2::CENTER_CENTER, *name, font.clone(), if active { p.base } else { p.text });
                    if response.clicked() {
                        view.mode = *mode;
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
                Chip::Category(cid) => {
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
                    if response.clicked() {
                        view.filter = if active { None } else { Some(cid.clone()) };
                    }
                    response.on_hover_text(format!(
                        "{} · готово {} из {}\nНажми, чтобы оставить только её",
                        label(chip),
                        stats["done"].as_u64().unwrap_or(0),
                        stats["tasks"].as_u64().unwrap_or(0)
                    ));
                }
            }
        }
    });
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
                Mode::Columns => columns(ui, board, view, p),
                Mode::Lanes => lanes(ui, board, view, p),
            };
            view.content = plan.content;
            if !view.fitted && plan.content.is_finite() {
                // First look: the whole width if it fits at a readable size, from the top.
                view.fitted = true;
                let scale = (size.x / (plan.content.width() + 48.)).clamp(0.75, 1.);
                view.pending = Some((Pos2::new(-24., -18.), scale));
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
    let mut plan = Plan { cards: vec![], zones: vec![], content: Rect::NOTHING };
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
    let mut plan = Plan { cards: vec![], zones: vec![], content: Rect::NOTHING };
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
        let stats = &board.state["progress"]["categories"][cid];
        let percent = stats["percent"].as_f64().unwrap_or(0.) as f32;
        let mut job = LayoutJob::simple(cname.clone(), FontId::proportional(14.5), p.text, LABEL - 40.);
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
    plan.content = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, y));
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
            let grab = response.interact_pointer_pos().map_or(Vec2::ZERO, |pos| pos - card.rect.min);
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

    for card in &plan.cards {
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
    let zone = plan.zones.iter().find(|z| z.rect.contains(pos));
    let mut target = None;
    if let Some(zone) = zone {
        let peers: Vec<&CardBox> = plan
            .cards
            .iter()
            .filter(|c| c.id != drag.id && c.status == zone.status && zone.category.as_ref().is_none_or(|cat| &c.category == cat || (cat.is_empty() && !board.colors.contains_key(&c.category))))
            .filter(|c| zone.rect.x_range().contains(c.rect.center().x))
            .collect();
        let before = peers.iter().filter(|c| c.rect.center().y > pos.y).min_by(|a, b| a.rect.top().total_cmp(&b.rect.top()));
        let y = match before {
            Some(c) => c.rect.top() - CARD_GAP / 2.,
            None => peers.iter().map(|c| c.rect.bottom() + CARD_GAP / 2.).fold(zone.top, f32::max),
        };
        let x = zone.rect.left() + GAP / 2.;
        let w = zone.rect.width() - GAP;
        let (left, right) = if zone.category.is_some() { (x, x + w) } else { (x + INNER, x + w - INNER) };
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
            let mut op = json!({"op":"move","id":drag.id,"status":zone.status});
            if let Some(before) = before {
                op["before"] = json!(before);
            }
            ops.push(op);
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
                        small(ui, "Тяни фон — двигаться · Ctrl+колесо — масштаб · Shift+тяни карточку на другую — связать", 11., p.muted);
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
                    .hint_text("Что нужно сделать · Enter — создать"),
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
                ui.add(egui::TextEdit::singleline(&mut edit.assignee).desired_width(170.).hint_text("никто"));
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
                .hint_text("Что сделать, критерии готовности, ссылки на файлы"),
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
            ui.add(egui::TextEdit::singleline(&mut view.link_to).desired_width(70.).hint_text("T-12"));
            let target = view.link_to.trim().to_uppercase();
            if ui.add_enabled(board.task(&target).is_some() && target != id, egui::Button::new("Связать")).clicked() {
                ops.push(json!({"op":"link","id":id,"to":target,"kind":view.link_kind}));
                view.link_to.clear();
            }
        });

        section(ui, "Обсуждение и история", p);
        ui.add(egui::TextEdit::multiline(&mut view.comment).desired_rows(2).desired_width(f32::INFINITY).hint_text("Комментарий для команды и агентов"));
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
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut view.new_category).desired_width(200.).hint_text("Новая категория"));
        if !view.new_category.trim().is_empty() && design::button(ui, "Добавить", None, Kind::Secondary, p, Vec2::new(0., 26.)).clicked() {
            let n = board.categories.len();
            let name = view.new_category.trim().to_owned();
            view.queued.push(json!({"op":"category","id":name.to_lowercase().replace(' ', "-"),"name":name,"color":SWATCHES[n % SWATCHES.len()],"order":n}));
            view.new_category.clear();
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
