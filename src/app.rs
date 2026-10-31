use crate::{
    connection::{Controller, Event},
    images::{self, Attachment},
    limits::{self, Usage},
    model::{self, Folder, Message, Permission, Server, Session, Settings},
    storage,
};
use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, Stroke, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::mpsc,
    time::{Duration, Instant},
};
use zeroize::Zeroize;

use crate::ui::{self as design, Icon, Kind, Palette, ACCENT, GREEN};
#[derive(Default)]
struct Remote {
    connected: bool,
    connecting: bool,
    version: String,
    sessions: Vec<Session>,
    home: String,
    usage: Usage,
    usage_requested: Option<Instant>,
    sessions_requested: Option<Instant>,
    sessions_loaded: bool,
    sessions_retries: u8,
    sessions_error: String,
}
struct Chat {
    sid: String,
    cwd: String,
    title: String,
    messages: Vec<Message>,
    draft: String,
    images: Vec<Attachment>,
    busy: bool,
    loading: bool,
    loaded: bool,
    status: String,
    error: String,
    model: String,
    mode: String,
    effort: String,
    permissions: Vec<Permission>,
    blocks: HashMap<usize, usize>,
    user_at: usize,
    sent_images: HashMap<usize, Vec<Attachment>>,
}
impl Chat {
    fn new(sid: String, cwd: String, title: String) -> Self {
        Self {
            sid,
            cwd,
            title,
            messages: vec![],
            draft: String::new(),
            images: vec![],
            busy: false,
            loading: false,
            loaded: false,
            status: String::new(),
            error: String::new(),
            model: "default".into(),
            mode: "auto".into(),
            effort: "default".into(),
            permissions: vec![],
            blocks: HashMap::new(),
            user_at: 0,
            sent_images: HashMap::new(),
        }
    }
}
struct Editor {
    server: Server,
    password: String,
    is_new: bool,
    error: String,
}
struct Trust {
    server: String,
    fingerprint: String,
    answer: tokio::sync::oneshot::Sender<bool>,
}
pub struct RelayApp {
    settings: Settings,
    controller: Controller,
    events: mpsc::Receiver<Event>,
    remote: HashMap<String, Remote>,
    chats: HashMap<String, Chat>,
    selected_chat: HashMap<String, String>,
    search: String,
    all_folders: bool,
    editor: Option<Editor>,
    trust: Option<Trust>,
    folder_open: bool,
    folder_path: String,
    folder_parent: String,
    folders: Vec<Folder>,
    folder_hidden: bool,
    folder_request: u64,
    folder_confirmed: String,
    answers: HashMap<String, String>,
    notice: String,
    markdown: CommonMarkCache,
    demo: bool,
    screenshot: Option<String>,
    frames: u32,
    capture_at: f64,
    capture_requested: bool,
    close_confirm: bool,
    desktop_requests: Vec<(String, Value)>,
    downloads: crate::artifacts::Downloads,
}
fn key(server: &str, sid: &str) -> String {
    format!("{server}/{sid}")
}
fn label(ui: &mut egui::Ui, s: impl Into<String>, size: f32, color: Color32) -> egui::Response {
    ui.label(RichText::new(s).size(size).color(color))
}
impl RelayApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (settings, error) = match storage::load() {
            Ok(s) => (s, String::new()),
            Err(e) => (Settings::default(), e.to_string()),
        };
        let (controller, events) = Controller::new(cc.egui_ctx.clone());
        let args: Vec<_> = std::env::args().collect();
        let demo = args.iter().any(|a| a == "--demo");
        let screenshot = args
            .iter()
            .position(|a| a == "--screenshot")
            .and_then(|i| args.get(i + 1))
            .cloned();
        let mut app = Self {
            settings,
            controller,
            events,
            remote: HashMap::new(),
            chats: HashMap::new(),
            selected_chat: HashMap::new(),
            search: String::new(),
            all_folders: false,
            editor: None,
            trust: None,
            folder_open: false,
            folder_path: String::new(),
            folder_parent: String::new(),
            folders: vec![],
            folder_hidden: false,
            folder_request: 0,
            folder_confirmed: String::new(),
            answers: HashMap::new(),
            notice: error,
            markdown: CommonMarkCache::default(),
            demo,
            screenshot,
            frames: 0,
            capture_at: 0.0,
            capture_requested: false,
            close_confirm: false,
            desktop_requests: vec![],
            downloads: Default::default(),
        };
        if demo {
            app.seed_demo();
            if args.iter().any(|a| a == "--demo-connect") {
                app.editor = Some(Editor {
                    server: Server {
                        name: "Рабочий сервер".into(),
                        host: "192.168.1.100".into(),
                        cwd: "/opt/projects".into(),
                        ..Default::default()
                    },
                    password: String::new(),
                    is_new: true,
                    error: String::new(),
                });
            }
            if args.iter().any(|a| a == "--demo-empty") {
                app.new_chat();
            }
            if args
                .iter()
                .any(|a| a == "--demo-activity" || a == "--demo-activity-open")
            {
                app.settings.servers[0].local_control = true;
                app.settings.servers[0].local_workspace = Some("/home/player".into());
                if let Some(k) = app.current_key() {
                    let chat = app.chats.get_mut(&k).unwrap();
                    chat.messages = vec![Message {
                        role: "assistant".into(),
                        text: "Проверяю каждый слайд визуально.".into(),
                        ..Default::default()
                    }];
                    for slide in 1..=12 {
                        chat.messages.push(Message{role:"tool".into(),tool:"Read".into(),tool_id:format!("slide-{slide}"),input:json!({"file_path":format!("/tmp/presentation/slides/s-{slide:02}.jpg")}),..Default::default()});
                        chat.messages.push(Message {
                            role: "tool_result".into(),
                            tool_id: format!("slide-{slide}"),
                            text: "Изображение открыто.".into(),
                            ..Default::default()
                        });
                    }
                    chat.messages.push(Message{role:"assistant".into(),text:"Проверил 12 слайдов. Текст читается, таблицы помещаются, номера на месте.".into(),..Default::default()});
                    if args.iter().any(|a| a == "--demo-activity-open") {
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            &cc.egui_ctx,
                            egui::Id::new(("activity-group", k.as_str(), 1usize)),
                            true,
                        )
                        .store(&cc.egui_ctx);
                    }
                }
            }
            if args.iter().any(|a| a == "--demo-artifact") {
                if let Some(k) = app.current_key() {
                    app.chats.get_mut(&k).unwrap().messages=vec![Message{role:"assistant".into(),text:"Собрал мод. Готовый файл:\n\n[sbrp-client-0.7.300.jar](/opt/project/build/libs/sbrp-client-0.7.300.jar)\n\nСкачай его кнопкой ниже.".into(),..Default::default()}];
                }
            }
            if args.iter().any(|a| a == "--demo-desktop") {
                app.settings.servers[0].local_workspace = Some("/home/player/Minecraft".into());
                app.settings.servers[0].local_control = true;
                if let Some(k) = app.current_key() {
                    app.chats.get_mut(&k).unwrap().mode = "manual".into();
                }
                app.desktop_requests.push(("demo".into(),json!({"id":"demo-action","tool":"run_local_command","input":{"command":"ls -lh mods/ && tail -n 20 logs/latest.log","timeout_seconds":30}})));
            }
            if args.iter().any(|a| a == "--demo-tables") {
                if let Some(k) = app.current_key() {
                    app.chats.get_mut(&k).unwrap().messages=vec![Message{role:"assistant".into(),text:"Шесть рисков, все взяты из проекта:\n\n| № | Риск | Ранг | Стратегия |\n|---|---|---|---|\n| 1 | Распознавание не справляется с нарезками из коротких видео | 3×3 = 9 | Снижение влияния: добавить обработку аудио |\n| 2 | Потеря файлов при передаче готовой сборки | 2×3 = 6 | Проверять размер и контрольную сумму |\n| 3 | Автоматические проверки занимают много времени | 3×2 = 6 | Разделить запуск и проверку результата |\n\nТекст после таблицы остаётся читаемым.".into(),..Default::default()}];
                }
            }
            if args.iter().any(|a| a == "--demo-lists") {
                if let Some(k) = app.current_key() {
                    let c = app.chats.get_mut(&k).unwrap();
                    c.messages=vec![Message{role:"user".into(),text:"Как прикрепить файл к сессии?".into(),..Default::default()},Message{role:"assistant".into(),text:"Прикрепить файл можно прямо в приложении.\n\n1. Нажми **Прикрепить файлы** под полем задачи.\n2. Выбери документ или изображение на компьютере. Несколько файлов можно выбрать вместе.\n3. Отправь сообщение — вложения останутся в этой сессии.\n\nТакже доступны:\n\n- Перетаскивание файлов в окно.\n- Вставка изображения через **Ctrl+V**.\n\nСохранённую сессию можно продолжить в другой день. Контекст и прикреплённые файлы останутся доступны.\n\n- Длинный путь: `C:\\Users\\Player\\AppData\\Roaming\\Launcher\\profiles\\248a2ae9\\mods\\project-client-0.7.252.jar`. После пути текст должен переноситься внутри сообщения.\n- Ссылка: https://example.com/releases/very-long-project-name-with-a-long-path/downloads/project-client-0.7.252.jar — и пояснение после неё.\n\nОбычный абзац с `settings.json` и длинным именем `professions.json.bak-1018-smithpath-release-original-configuration` тоже должен помещаться в ширину окна.".into(),..Default::default()}];
                }
            }
            if args.iter().any(|a| a == "--light") {
                app.settings.light = true;
            }
        }
        if !demo {
            if let Some(server) = app.server() {
                if let Some(sid) = server.last_session.clone() {
                    let mut c =
                        Chat::new(sid.clone(), server.cwd.clone(), "Последний разговор".into());
                    c.mode = server.mode.clone();
                    c.model = server.model.clone();
                    c.effort = server.effort.clone();
                    app.chats.insert(key(&server.id, &sid), c);
                    app.selected_chat.insert(server.id.clone(), sid);
                }
                if server.remember {
                    app.connect(server);
                }
            }
        }
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "inter".into(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../assets/InterVariable.ttf"
            ))),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "inter".into());
        cc.egui_ctx.set_fonts(fonts);
        app.theme(&cc.egui_ctx);
        app
    }
    fn theme(&self, ctx: &egui::Context) {
        let p = design::animated_palette(ctx, self.settings.light);
        let mut v = if self.settings.light {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        v.panel_fill = p.base;
        v.window_corner_radius = egui::CornerRadius::same(22);
        v.menu_corner_radius = egui::CornerRadius::same(14);
        v.window_shadow = egui::epaint::Shadow {
            offset: [0, 12],
            blur: 48,
            spread: 0,
            color: Color32::from_black_alpha(75),
        };
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.noninteractive,
        ] {
            w.corner_radius = egui::CornerRadius::same(11);
        }
        v.window_stroke = Stroke::new(1.0_f32, p.line);
        v.window_fill = p.surface;
        v.faint_bg_color = p.side;
        v.extreme_bg_color = p.side;
        v.override_text_color = Some(p.text);
        v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0_f32, p.text);
        v.widgets.inactive.bg_fill = p.surface;
        v.widgets.inactive.weak_bg_fill = p.surface;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.line);
        v.widgets.hovered.bg_fill = p.line;
        v.widgets.hovered.weak_bg_fill = p.line;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        v.widgets.active.bg_fill = ACCENT.gamma_multiply(0.35);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.line);
        ctx.set_visuals(v);
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(6.0, 5.0);
        style.spacing.button_padding = Vec2::new(12.0, 9.0);
        style.spacing.interact_size.y = 30.;
        style.spacing.window_margin = egui::Margin::same(24);
        style.animation_time = 0.28;
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
        ctx.set_style(style);
    }
    fn save(&mut self) {
        if !self.demo {
            if let Err(e) = storage::save(&self.settings) {
                self.notice = e.to_string();
            }
        }
    }
    fn server(&self) -> Option<Server> {
        self.settings
            .selected
            .as_ref()
            .and_then(|id| self.settings.servers.iter().find(|s| &s.id == id))
            .cloned()
    }
    fn current_key(&self) -> Option<String> {
        let s = self.settings.selected.as_ref()?;
        Some(key(s, self.selected_chat.get(s)?))
    }
    fn connected(&self, id: &str) -> bool {
        self.demo || self.remote.get(id).is_some_and(|r| r.connected)
    }
    fn new_chat(&mut self) {
        if let Some(s) = self.server() {
            let sid = uuid::Uuid::new_v4().to_string();
            let mut chat = Chat::new(sid.clone(), s.cwd, "Новая сессия".into());
            chat.mode = s.mode;
            chat.model = s.model;
            chat.effort = s.effort;
            chat.loaded = true;
            self.controller.bus.ctx.animate_bool_with_time(
                egui::Id::new(("chat-enter", key(&s.id, &sid))),
                false,
                0.,
            );
            self.chats.insert(key(&s.id, &sid), chat);
            self.selected_chat.insert(s.id, sid);
        }
    }
    fn select_chat(&mut self, session: Session) {
        if let Some(server) = self.server() {
            self.controller.bus.ctx.animate_bool_with_time(
                egui::Id::new(("chat-enter", key(&server.id, &session.id))),
                false,
                0.,
            );
        }
        if let Some(s) = self.server() {
            let k = key(&s.id, &session.id);
            let chat = self.chats.entry(k).or_insert_with(|| {
                let mut chat = Chat::new(session.id.clone(), session.cwd.clone(), session.title);
                chat.mode = s.mode.clone();
                chat.model = s.model.clone();
                chat.effort = s.effort.clone();
                chat
            });
            if !chat.loaded && !chat.loading {
                chat.loading = true;
                self.controller
                    .send(&s.id, json!({"op":"history","sid":session.id}));
            }
            self.selected_chat.insert(s.id.clone(), session.id.clone());
            if let Some(p) = self.settings.servers.iter_mut().find(|p| p.id == s.id) {
                p.cwd = session.cwd;
                p.last_session = Some(session.id);
            }
            self.save();
        }
    }
    fn connect(&mut self, server: Server) {
        if self.demo {
            return;
        }
        self.notice.clear();
        self.remote.entry(server.id.clone()).or_default().connecting = true;
        let bus = self.controller.bus.clone();
        self.controller.runtime.spawn_blocking(move || {
            let password = storage::password(&server.id);
            bus.send(Event::Password { server, password });
        });
    }
    fn establish(&mut self, server: Server, password: String) {
        self.remote.entry(server.id.clone()).or_default().connecting = true;
        self.controller.connect(server, password);
    }
    fn request_folders(&mut self) {
        if let Some(s) = self.server() {
            self.folder_request += 1;
            self.controller.send(&s.id,json!({"op":"folders","path":self.folder_path,"hidden":self.folder_hidden,"request":self.folder_request}));
        }
    }
    fn load_images(&self, server: String, sid: String, paths: Vec<std::path::PathBuf>) {
        let bus = self.controller.bus.clone();
        self.controller.runtime.spawn_blocking(move || {
            let mut images = vec![];
            let mut errors = vec![];
            for path in paths.into_iter().take(8) {
                match images::from_file(&bus.ctx, &path) {
                    Ok(a) => images.push(a),
                    Err(e) => errors.push(e.to_string()),
                }
            }
            bus.send(Event::Images {
                server,
                sid,
                images,
                errors,
            });
        });
    }
    fn pick_images(&mut self) {
        if self.current_key().is_none() {
            self.new_chat();
        }
        let Some(server) = self.server() else {
            return;
        };
        let Some(sid) = self.selected_chat.get(&server.id).cloned() else {
            return;
        };
        let bus = self.controller.bus.clone();
        self.controller.runtime.spawn(async move {
            let paths = rfd::AsyncFileDialog::new()
                .set_title("Прикрепить файлы")
                .pick_files()
                .await;
            if let Some(files) = paths {
                bus.send(Event::Files {
                    server: server.id,
                    sid,
                    paths: files.into_iter().map(|f| f.path().to_owned()).collect(),
                });
            }
        });
    }
    fn paste_image(&mut self) {
        if self.current_key().is_none() {
            self.new_chat();
        }
        let Some(server) = self.server() else {
            return;
        };
        let Some(sid) = self.selected_chat.get(&server.id).cloned() else {
            return;
        };
        let bus = self.controller.bus.clone();
        self.controller
            .runtime
            .spawn_blocking(move || match images::from_clipboard(&bus.ctx) {
                Ok(Some(a)) => {
                    bus.send(Event::Images {
                        server: server.id,
                        sid,
                        images: vec![a],
                        errors: vec![],
                    });
                }
                Ok(None) => {}
                Err(error) => bus.send(Event::Images {
                    server: server.id,
                    sid,
                    images: vec![],
                    errors: vec![format!("Не удалось вставить изображение: {error}")],
                }),
            });
    }
    fn poll(&mut self) {
        for _ in 0..512 {
            let Ok(event) = self.events.try_recv() else {
                break;
            };
            let event = match event {
                Event::Epoch {
                    server,
                    generation,
                    event,
                } => {
                    if !self.controller.is_current(&server, generation) {
                        continue;
                    }
                    *event
                }
                event => event,
            };
            match event {
                Event::Epoch { .. } => {}
                Event::Trust {
                    server,
                    fingerprint,
                    answer,
                } => {
                    self.trust = Some(Trust {
                        server,
                        fingerprint,
                        answer,
                    })
                }
                Event::Password { server, password } => {
                    if let Some(password) = password {
                        self.establish(server, password);
                    } else {
                        self.remote.entry(server.id.clone()).or_default().connecting = false;
                        self.editor = Some(Editor {
                            server,
                            password: String::new(),
                            is_new: false,
                            error: String::new(),
                        });
                    }
                }
                Event::Error { server, message } => {
                    self.notice = message;
                    self.remote.entry(server).or_default().connecting = false;
                }
                Event::Disconnected { server } => {
                    self.downloads.disconnected(&server);
                    let r = self.remote.entry(server.clone()).or_default();
                    self.desktop_requests.retain(|(s, _)| s != &server);
                    r.connected = false;
                    r.connecting = false;
                    for (k, c) in &mut self.chats {
                        if k.starts_with(&format!("{server}/")) {
                            if c.busy {
                                c.error="Подключение прервалось. Подключись снова, чтобы продолжить эту сессию.".into();
                            }
                            c.busy = false;
                            c.loading = false;
                            c.permissions.clear();
                        }
                    }
                }
                Event::Files { server, sid, paths } => self.load_images(server, sid, paths),
                Event::Images {
                    server,
                    sid,
                    images,
                    errors,
                } => {
                    if let Some(c) = self.chats.get_mut(&key(&server, &sid)) {
                        for a in images {
                            if c.images.len() < 8
                                && c.images.iter().map(|f| f.bytes).sum::<usize>() + a.bytes
                                    <= 12 * 1024 * 1024
                            {
                                c.images.push(a)
                            } else {
                                c.error = "До восьми файлов общим размером 12 МБ.".into();
                            }
                        }
                        if !errors.is_empty() {
                            c.error = errors.join("\n");
                        }
                    }
                }
                Event::Data { server, value } => self.data(server, value),
            }
        }
    }
    fn data(&mut self, server: String, value: Value) {
        let kind = value["type"].as_str().unwrap_or("");
        if kind == "error" && value["op"] == "sessions" {
            let remote = self.remote.entry(server).or_default();
            remote.sessions_error = "Не удалось загрузить сессии. Повторяем…".into();
            remote.sessions_requested = Some(Instant::now());
            return;
        }
        match kind {
            "artifact_status" => self.downloads.event(&server, &value),
            "desktop_request" => {
                if self.desktop_requests.len() < 8
                    && !self
                        .desktop_requests
                        .iter()
                        .any(|(s, v)| s == &server && v["id"] == value["id"])
                {
                    self.desktop_requests.push((server, value));
                }
            }
            "desktop_cancel" => {
                self.desktop_requests
                    .retain(|(s, v)| s != &server || v["id"] != value["id"]);
            }
            "desktop_status" => {
                self.notice = value["message"].as_str().unwrap_or("").into();
            }
            "hello" => {
                let r = self.remote.entry(server.clone()).or_default();
                r.connected = true;
                r.connecting = false;
                r.version = value["version"].as_str().unwrap_or("Claude Code").into();
                r.home = value["home"].as_str().unwrap_or("~").into();
                r.usage_requested = Some(Instant::now());
                r.sessions_requested = Some(Instant::now());
                r.sessions_loaded = false;
                r.sessions_retries = 0;
                if let Some(s) = self.settings.servers.iter_mut().find(|s| s.id == server) {
                    if s.cwd == "~" {
                        s.cwd = r.home.clone();
                    }
                }
                self.controller.send(&server, json!({"op":"sessions"}));
                self.controller.send(&server, json!({"op":"usage"}));
                let workspace = self
                    .settings
                    .servers
                    .iter()
                    .find(|s| s.id == server)
                    .filter(|s| s.local_control)
                    .and_then(|s| s.local_workspace.clone());
                self.controller
                    .send(&server, json!({"op":"_desktop_workspace","path":workspace}));
                if let Some(sid) = self.selected_chat.get(&server) {
                    if let Some(c) = self.chats.get_mut(&key(&server, sid)) {
                        if !c.loaded && !c.loading {
                            c.loading = true;
                            self.controller
                                .send(&server, json!({"op":"history","sid":sid}));
                        }
                    }
                }
                self.save();
            }
            "usage" => {
                if let Ok(usage) = serde_json::from_value(value.clone()) {
                    self.remote.entry(server.clone()).or_default().usage = usage;
                }
            }
            "sessions" => {
                let Some(rows) = value["sessions"].as_array() else {
                    return;
                };
                let sessions: Vec<Session> = rows
                    .iter()
                    .filter_map(|row| serde_json::from_value::<Session>(row.clone()).ok())
                    .filter(|s| !s.id.is_empty())
                    .collect();
                let remote = self.remote.entry(server.clone()).or_default();
                remote.sessions = sessions.clone();
                remote.sessions_loaded = true;
                remote.sessions_requested = None;
                remote.sessions_error.clear();
                for s in &sessions {
                    if let Some(c) = self.chats.get_mut(&key(&server, &s.id)) {
                        c.title = s.title.clone();
                        c.cwd = s.cwd.clone();
                    }
                }
                if !self.selected_chat.contains_key(&server)
                    && self.settings.selected.as_deref() == Some(&server)
                {
                    let last = self.server().and_then(|s| s.last_session);
                    if let Some(session) = sessions
                        .iter()
                        .find(|s| Some(&s.id) == last.as_ref())
                        .cloned()
                    {
                        self.select_chat(session);
                    } else {
                        self.new_chat();
                    }
                }
            }
            "folders" => {
                if self.settings.selected.as_deref() != Some(&server)
                    || value["request"].as_u64() != Some(self.folder_request)
                {
                    return;
                }
                self.folder_confirmed = value["path"].as_str().unwrap_or("~").into();
                self.folder_path = value["path"].as_str().unwrap_or("~").into();
                self.folder_parent = value["parent"].as_str().unwrap_or("/").into();
                self.folders = serde_json::from_value(value["folders"].clone()).unwrap_or_default();
            }
            "history" => {
                let sid = value["sid"].as_str().unwrap_or("");
                if let Some(c) = self.chats.get_mut(&key(&server, sid)) {
                    if !c.busy {
                        c.messages = value["messages"]
                            .as_array()
                            .map(|rows| {
                                rows.iter()
                                    .filter_map(|r| serde_json::from_value(r.clone()).ok())
                                    .collect()
                            })
                            .unwrap_or_default();
                        c.loading = false;
                        c.loaded = true;
                    }
                }
            }
            "claude" | "permission" | "permission_cancelled" | "run_closed" | "error" => {
                let sid = value["sid"].as_str().unwrap_or("");
                if let Some(c) = self.chats.get_mut(&key(&server, sid)) {
                    match kind {
                        "claude" => {
                            reduce(c, &value["data"]);
                            if value["data"]["type"] == "result" {
                                self.controller.send(&server, json!({"op":"sessions"}));
                            }
                        }
                        "permission" => c.permissions.push(Permission {
                            id: value["id"].as_str().unwrap_or("").into(),
                            tool: value["tool"].as_str().unwrap_or("Действие").into(),
                            input: value["input"].clone(),
                        }),
                        "permission_cancelled" => c
                            .permissions
                            .retain(|p| Some(p.id.as_str()) != value["id"].as_str()),
                        "run_closed" => {
                            if c.busy {
                                c.error = if value["detail"].as_str().unwrap_or("").is_empty() {
                                    "Claude остановлен. История сохранена, можно продолжить разговор.".into()
                                } else {
                                    value["detail"].as_str().unwrap_or("").into()
                                };
                            }
                            c.busy = false;
                            c.permissions.clear();
                        }
                        "error" => {
                            c.error = value["message"]
                                .as_str()
                                .unwrap_or("Не удалось выполнить действие.")
                                .into();
                            if value["op"] == "prompt" {
                                c.busy = false;
                            }
                            if value["op"] == "history" {
                                c.loading = false;
                            }
                        }
                        _ => {}
                    }
                } else if kind == "error" {
                    self.notice = value["message"].as_str().unwrap_or("").into();
                }
            }
            "fatal" => {
                self.notice = value["message"]
                    .as_str()
                    .unwrap_or("Не удалось запустить помощник.")
                    .into();
                self.remote.entry(server).or_default().connecting = false;
            }
            _ => {}
        }
    }
    fn send_prompt(&mut self) {
        let Some(server) = self.server() else {
            return;
        };
        let Some(k) = self.current_key() else {
            return;
        };
        if !self.connected(&server.id) {
            self.connect(server);
            return;
        }
        let c = self.chats.get_mut(&k).unwrap();
        if c.busy || c.loading || (c.draft.trim().is_empty() && c.images.is_empty()) {
            return;
        }
        let text = c.draft.trim().to_owned();
        let images: Vec<_> = c
            .images
            .iter()
            .filter(|a| a.source["type"] != "file")
            .map(|a| a.source.clone())
            .collect();
        let files: Vec<_> = c
            .images
            .iter()
            .filter(|a| a.source["type"] == "file")
            .map(|a| a.source.clone())
            .collect();
        self.controller.send(
            &server.id,
            json!({"op":"_desktop_workspace","path":if server.local_control {server.local_workspace.clone()}else{None}}),
        );
        self.controller.send(&server.id,json!({"op":"_desktop_policy","sid":c.sid,"auto":server.local_control && c.mode=="auto"}));
        let request = json!({"op":"prompt","sid":c.sid,"cwd":c.cwd,"text":text,"images":images,"files":files,"model":c.model,"mode":c.mode,"effort":c.effort,"desktop_enabled":server.local_control && server.local_workspace.is_some(),"desktop_context":{"os":std::env::consts::OS,"workspace":server.local_workspace,"downloads":dirs::download_dir(),"auto":c.mode=="auto"}});
        if self.demo {
            c.error = "Это предпросмотр интерфейса. Добавь свой сервер для работы.".into();
            return;
        }
        if !self.controller.send(&server.id, request) {
            c.error = "Подключись к серверу перед отправкой.".into();
            return;
        }
        c.user_at = c.messages.len();
        let text = if c.images.is_empty() {
            text
        } else {
            format!(
                "{}\n\n{}",
                text,
                c.images
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<_>>()
                    .join(" · ")
            )
        };
        c.messages.push(Message {
            role: "user".into(),
            text,
            ..Default::default()
        });
        c.sent_images
            .insert(c.user_at, std::mem::take(&mut c.images));
        c.draft.clear();
        c.blocks.clear();
        c.busy = true;
        c.error.clear();
        c.status = "Думает над ответом".into();
        if c.title == "Новая сессия" {
            c.title = model::shorten(&c.messages[c.user_at].text, 60);
        }
        if let Some(s) = self.settings.servers.iter_mut().find(|s| s.id == server.id) {
            s.last_session = Some(c.sid.clone());
        }
        self.save();
    }
    fn rail(&mut self, ctx: &egui::Context) {
        let p = design::animated_palette(ctx, self.settings.light);
        egui::SidePanel::left("rail")
            .exact_width(76.)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.rail).inner_margin(14.))
            .show(ctx, |ui| {
                ui.add_space(12.);
                ui.add_space(16.);
                let mut marker = None;
                for server in self.settings.servers.clone() {
                    let selected = self.settings.selected.as_deref() == Some(&server.id);
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::splat(48.), egui::Sense::click());
                    let t = ctx.animate_bool_with_time(response.id.with("active"), selected, 0.2);
                    let hover = ctx.animate_bool_with_time(
                        response.id.with("hover"),
                        response.hovered(),
                        0.23,
                    );
                    ui.painter().rect_filled(
                        rect,
                        16.,
                        design::mix(
                            p.rail,
                            design::mix(p.surface, ACCENT, 0.16),
                            t.max(hover * 0.6),
                        ),
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        server
                            .name
                            .chars()
                            .take(2)
                            .collect::<String>()
                            .to_uppercase(),
                        FontId::proportional(13.),
                        design::mix(p.muted, ACCENT, t),
                    );
                    if selected {
                        marker = Some((rect.left() - 11., rect.center().y));
                    }
                    if self.connected(&server.id) {
                        ui.painter().circle_filled(
                            rect.right_bottom() - Vec2::splat(5.),
                            5.,
                            p.rail,
                        );
                        ui.painter().circle_filled(
                            rect.right_bottom() - Vec2::splat(5.),
                            3.,
                            GREEN,
                        );
                    }
                    if response
                        .on_hover_text(format!("{} · {}", server.name, server.host))
                        .clicked()
                    {
                        self.settings.selected = Some(server.id.clone());
                        self.search.clear();
                        self.save();
                        if !self.connected(&server.id)
                            && !self.remote.get(&server.id).is_some_and(|r| r.connecting)
                        {
                            self.connect(server);
                        }
                    }
                    ui.add_space(9.);
                }
                if let Some((x, y)) = marker {
                    let y = design::glide(ctx, egui::Id::new("server-selection-y"), y);
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(egui::pos2(x, y), Vec2::new(3., 20.)),
                        2.,
                        ACCENT,
                    );
                }
                ui.add_space(5.);
                if design::button(ui, "", Some(Icon::Plus), Kind::Ghost, p, Vec2::splat(48.))
                    .on_hover_text("Добавить сервер")
                    .clicked()
                {
                    self.editor = Some(Editor {
                        server: Server::default(),
                        password: String::new(),
                        is_new: true,
                        error: String::new(),
                    });
                }
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    ui.add_space(4.);
                    if design::button(
                        ui,
                        "",
                        Some(if self.settings.light {
                            Icon::Moon
                        } else {
                            Icon::Sun
                        }),
                        Kind::Ghost,
                        p,
                        Vec2::splat(44.),
                    )
                    .on_hover_text("Сменить тему")
                    .clicked()
                    {
                        self.settings.light = !self.settings.light;
                        self.theme(ctx);
                        self.save();
                    }
                });
            });
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        let p = design::animated_palette(ctx, self.settings.light);
        egui::SidePanel::left("sessions")
            .default_width(258.)
            .width_range(224.0..=320.0)
            .frame(egui::Frame::new().fill(p.side).inner_margin(16.))
            .show(ctx, |ui| {
                ui.add_space(8.);
                ui.horizontal(|ui| {
                    label(ui, "Relay", 25., p.text);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if design::button(
                            ui,
                            "",
                            Some(Icon::Settings),
                            Kind::Ghost,
                            p,
                            Vec2::splat(34.),
                        )
                        .on_hover_text("Настройки подключения")
                        .clicked()
                        {
                            if let Some(server) = self.server() {
                                self.editor = Some(Editor {
                                    server,
                                    password: String::new(),
                                    is_new: false,
                                    error: String::new(),
                                });
                            }
                        }
                    });
                });
                ui.add_space(14.);
                if design::button(
                    ui,
                    "Новая сессия",
                    Some(Icon::Plus),
                    Kind::Primary,
                    p,
                    Vec2::new(ui.available_width(), 42.),
                )
                .clicked()
                {
                    self.new_chat();
                }
                ui.add_space(14.);
                egui::Frame::new()
                    .fill(p.base)
                    .corner_radius(12.)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (r, _) =
                                ui.allocate_exact_size(Vec2::splat(17.), egui::Sense::hover());
                            design::icon(ui.painter(), r, Icon::Search, p.muted);
                            ui.add(
                                egui::TextEdit::singleline(&mut self.search)
                                    .frame(false)
                                    .hint_text("Поиск сессий")
                                    .desired_width(ui.available_width()),
                            );
                        });
                    });
                ui.add_space(14.);
                let Some(server) = self.server() else {
                    label(ui, "Подключи свой первый сервер", 13., p.muted);
                    return;
                };
                label(ui, "Рабочая папка", 12., p.muted);
                ui.add_space(3.);
                if design::button(
                    ui,
                    &model::shorten(&server.cwd, 27),
                    Some(Icon::Folder),
                    Kind::Secondary,
                    p,
                    Vec2::new(ui.available_width(), 40.),
                )
                .on_hover_text(&server.cwd)
                .clicked()
                {
                    if self.connected(&server.id) {
                        self.folder_path = server.cwd.clone();
                        self.folder_open = true;
                        self.request_folders();
                    } else {
                        self.connect(server.clone());
                    }
                }
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    design::toggle(ui, &mut self.all_folders, "Все папки", p);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if design::button(
                            ui,
                            "",
                            Some(Icon::Refresh),
                            Kind::Ghost,
                            p,
                            Vec2::splat(30.),
                        )
                        .on_hover_text("Обновить список")
                        .clicked()
                        {
                            if let Some(remote) = self.remote.get_mut(&server.id) {
                                remote.sessions_requested = Some(Instant::now());
                                remote.sessions_loaded = false;
                                remote.sessions_retries = 0;
                                remote.sessions_error.clear();
                            }
                            self.controller.send(&server.id, json!({"op":"sessions"}));
                        }
                    });
                });
                ui.add_space(16.);
                label(ui, "Сессии", 16., p.text);
                ui.add_space(5.);
                let connected = self.connected(&server.id);
                let connecting = self.remote.get(&server.id).is_some_and(|r| r.connecting);
                egui::TopBottomPanel::bottom("connection-footer")
                    .frame(egui::Frame::NONE)
                    .show_inside(ui, |ui| {
                        ui.add_space(18.);
                        egui::Frame::new()
                            .fill(p.surface)
                            .corner_radius(16.)
                            .inner_margin(14.)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().interact_size.y = 18.;
                                ui.spacing_mut().item_spacing.y = 4.;
                                ui.horizontal(|ui| {
                                    let (r, _) = ui
                                        .allocate_exact_size(Vec2::splat(8.), egui::Sense::hover());
                                    ui.painter().circle_filled(
                                        r.center(),
                                        3.,
                                        if connected { GREEN } else { p.muted },
                                    );
                                    label(
                                        ui,
                                        if connected {
                                            "На связи"
                                        } else if connecting {
                                            "Подключаемся…"
                                        } else {
                                            "Нет подключения"
                                        },
                                        12.,
                                        p.muted,
                                    );
                                });
                                label(ui, &server.name, 14., p.text);
                                label(ui, format!("{}@{}", server.user, server.host), 11., p.muted);
                                if connected {
                                    ui.add_space(8.);
                                    ui.separator();
                                    ui.add_space(5.);
                                    if let Some(remote) = self.remote.get(&server.id) {
                                        limits::show(ui, &remote.usage, p, connected);
                                    }
                                }
                                if !connected && !connecting {
                                    ui.add_space(5.);
                                    if design::button(
                                        ui,
                                        "Подключиться",
                                        Some(Icon::Arrow),
                                        Kind::Secondary,
                                        p,
                                        Vec2::new(ui.available_width(), 36.),
                                    )
                                    .clicked()
                                    {
                                        self.connect(server.clone());
                                    }
                                }
                            });
                        ui.add_space(4.);
                    });
                let sessions = self
                    .remote
                    .get(&server.id)
                    .map(|r| r.sessions.clone())
                    .unwrap_or_default();
                let query = self.search.to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("session-list")
                    .show(ui, |ui| {
                        let mut count = 0;
                        for session in sessions {
                            if (!self.all_folders && session.cwd != server.cwd)
                                || !session.title.to_lowercase().contains(&query)
                            {
                                continue;
                            }
                            count += 1;
                            let selected = self.selected_chat.get(&server.id) == Some(&session.id);
                            let background = ui.painter().add(egui::Shape::Noop);
                            let row = egui::Frame::new()
                                .inner_margin(egui::Margin::symmetric(12, 9))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width().max(120.));
                                    ui.spacing_mut().item_spacing.y = 3.;
                                    ui.spacing_mut().interact_size.y = 18.;
                                    design::session_title(ui, &session.title, p);
                                    ui.add_space(1.);
                                    ui.horizontal(|ui| {
                                        label(ui, relative(session.modified), 12., p.muted);
                                        if self
                                            .chats
                                            .get(&key(&server.id, &session.id))
                                            .is_some_and(|c| c.busy)
                                        {
                                            design::dots(ui, ACCENT);
                                        }
                                    });
                                })
                                .response;
                            let response = ui.interact(
                                row.rect,
                                ui.id().with(&session.id),
                                egui::Sense::click(),
                            );
                            let t = ctx.animate_bool_with_time(
                                response.id.with("active"),
                                selected,
                                0.28,
                            );
                            let hover = ctx.animate_bool_with_time(
                                response.id.with("hover"),
                                response.hovered(),
                                0.23,
                            );
                            ui.painter().set(
                                background,
                                egui::Shape::rect_filled(
                                    row.rect,
                                    14.,
                                    design::mix(
                                        p.side,
                                        design::mix(p.surface, ACCENT, 0.08),
                                        t.max(hover * 0.65),
                                    ),
                                ),
                            );
                            if selected {
                                ui.painter().rect_filled(
                                    egui::Rect::from_center_size(
                                        egui::pos2(row.rect.left() + 3., row.rect.center().y),
                                        Vec2::new(2., 18.),
                                    ),
                                    2.,
                                    ACCENT,
                                );
                            }
                            if response.on_hover_text(&session.title).clicked() {
                                self.select_chat(session);
                            }
                            ui.add_space(4.);
                        }
                        if count == 0 {
                            if let Some(remote) = self.remote.get(&server.id) {
                                if !remote.sessions_loaded && connected {
                                    label(
                                        ui,
                                        if remote.sessions_error.is_empty() {
                                            "Загружаем сессии…"
                                        } else {
                                            &remote.sessions_error
                                        },
                                        12.,
                                        p.muted,
                                    );
                                }
                                if !remote.sessions.is_empty()
                                    && !self.all_folders
                                    && query.is_empty()
                                    && ui.button("Показать сессии из всех папок").clicked()
                                {
                                    self.all_folders = true;
                                }
                            }
                            ui.add_space(15.);
                            label(
                                ui,
                                if !connected {
                                    "Подключись, чтобы увидеть сессии"
                                } else if query.is_empty() {
                                    "Создай первую сессию в этой папке"
                                } else {
                                    "Нет подходящих разговоров"
                                },
                                13.,
                                p.muted,
                            );
                        }
                    });
            });
    }
    fn main_panel(&mut self, ctx: &egui::Context) {
        let p = design::animated_palette(ctx, self.settings.light);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.base))
            .show(ctx, |ui| {
                egui::TopBottomPanel::top("chat-header")
                    .frame(
                        egui::Frame::new()
                            .fill(p.base)
                            .inner_margin(egui::Margin::symmetric(24, 12)),
                    )
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            if let Some(server) = self.server() {
                                let (r, _) =
                                    ui.allocate_exact_size(Vec2::splat(18.), egui::Sense::hover());
                                design::icon(ui.painter(), r, Icon::Server, p.muted);
                                label(ui, &server.name, 14., p.text);
                                label(ui, "/", 14., p.muted);
                                let title = self
                                    .current_key()
                                    .and_then(|k| self.chats.get(&k))
                                    .map(|c| model::shorten(&c.title, 50))
                                    .unwrap_or_else(|| "Новая сессия".into());
                                label(ui, title, 14., p.muted);
                            } else {
                                label(ui, "Ваше пространство", 14., p.muted);
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if self.demo {
                                    label(ui, "ПРЕДПРОСМОТР", 10., p.muted);
                                }
                            });
                        });
                    });
                if !self.notice.is_empty() {
                    egui::TopBottomPanel::top("notice")
                        .frame(egui::Frame::new().fill(p.surface).inner_margin(16.))
                        .show_inside(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                label(ui, self.notice.clone(), 13., ACCENT);
                                if design::button(
                                    ui,
                                    "",
                                    Some(Icon::Close),
                                    Kind::Ghost,
                                    p,
                                    Vec2::splat(28.),
                                )
                                .clicked()
                                {
                                    self.notice.clear();
                                }
                            });
                        });
                }
                if self.server().is_none() {
                    ui.vertical_centered(|ui| {
                        ui.add_space((ui.available_height() * 0.25).max(30.));
                        design::mark(ui, 58.);
                        ui.add_space(28.);
                        label(ui, "Все проекты. Одно пространство.", 29., p.text);
                        ui.add_space(12.);
                        label(
                            ui,
                            "Подключи сервер и продолжай с того места, где остановился.",
                            15.,
                            p.muted,
                        );
                        ui.add_space(30.);
                        if design::button(
                            ui,
                            "Добавить сервер",
                            Some(Icon::Plus),
                            Kind::Primary,
                            p,
                            Vec2::new(220., 44.),
                        )
                        .clicked()
                        {
                            self.editor = Some(Editor {
                                server: Server::default(),
                                password: String::new(),
                                is_new: true,
                                error: String::new(),
                            });
                        }
                    });
                    return;
                }
                if self.current_key().is_none() {
                    self.new_chat();
                }
                let Some(k) = self.current_key() else {
                    return;
                };
                egui::TopBottomPanel::bottom("composer-panel")
                    .frame(
                        egui::Frame::new()
                            .fill(p.base)
                            .inner_margin(egui::Margin::symmetric(20, 10)),
                    )
                    .show_inside(ui, |ui| {
                        design::content(ui, 860., |ui| {
                            self.permission_card(ui, &k, p);
                            self.composer(ui, &k, p);
                        });
                    });
                let chat = self.chats.get(&k).unwrap();
                if chat.messages.is_empty() && !chat.loading {
                    let narrow = ui.available_width() < 560.;
                    egui::ScrollArea::vertical()
                        .id_salt("welcome")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space((ui.available_height() * 0.23).clamp(16., 120.));
                                label(ui, "С чего начнём?", if narrow { 28. } else { 34. }, p.text);
                                ui.add_space(12.);
                                label(
                                    ui,
                                    "Твои проекты, файлы и разговоры — под рукой.",
                                    15.,
                                    p.muted,
                                );
                                ui.add_space(24.);
                                design::content(ui, if narrow { 260. } else { 520. }, |ui| {
                                    let choices = |ui: &mut egui::Ui| {
                                        for text in [
                                            "Изучить проект",
                                            "Посмотреть изменения",
                                            "Продолжить задачу",
                                        ] {
                                            if design::button(
                                                ui,
                                                text,
                                                None,
                                                Kind::Secondary,
                                                p,
                                                Vec2::new(
                                                    if narrow { ui.available_width() } else { 0. },
                                                    40.,
                                                ),
                                            )
                                            .clicked()
                                            {
                                                self.chats.get_mut(&k).unwrap().draft = text.into();
                                            }
                                        }
                                    };
                                    if narrow {
                                        ui.vertical(choices);
                                    } else {
                                        ui.horizontal_wrapped(choices);
                                    }
                                });
                            });
                        });
                    return;
                }
                let chat = self.chats.get_mut(&k).unwrap();
                let fade = ctx.animate_bool_with_time_and_easing(
                    egui::Id::new(("chat-enter", &k)),
                    true,
                    0.30,
                    egui::emath::easing::cubic_in_out,
                );
                egui::ScrollArea::vertical()
                    .id_salt(("chat", &k))
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(24.);
                        design::content(ui, (ui.available_width() - 56.).min(800.), |ui| {
                            ui.set_opacity(fade);
                            if chat.loading {
                                ui.horizontal(|ui| {
                                    design::dots(ui, ACCENT);
                                    label(ui, "Открываем разговор", 13., p.muted);
                                });
                            }
                            let timeline = crate::timeline::items(&chat.messages);
                            let grouped_busy = chat.busy
                                && matches!(
                                    timeline.last(),
                                    Some(crate::timeline::Item::Actions(_))
                                );
                            for item in timeline {
                                let index = match item {
                                    crate::timeline::Item::Actions(range) => {
                                        let busy = chat.busy && range.end == chat.messages.len();
                                        crate::timeline::show(
                                            ui,
                                            &k,
                                            &chat.messages,
                                            range,
                                            busy,
                                            p,
                                        );
                                        continue;
                                    }
                                    crate::timeline::Item::Message(index) => index,
                                };
                                let message = &chat.messages[index];
                                ui.push_id(index, |ui| match message.role.as_str() {
                                    "user" => {
                                        ui.add_space(8.);
                                        egui::Frame::new()
                                            .fill(p.surface)
                                            .corner_radius(20.)
                                            .inner_margin(20.)
                                            .show(ui, |ui| {
                                                label(ui, "Вы", 11., p.muted);
                                                ui.add_space(3.);
                                                ui.add(
                                                    egui::Label::new(&message.text)
                                                        .selectable(true)
                                                        .wrap(),
                                                );
                                                if let Some(files) = chat.sent_images.get(&index) {
                                                    ui.horizontal_wrapped(|ui| {
                                                        for file in files {
                                                            attachment_view(ui, file, p, false);
                                                        }
                                                    });
                                                }
                                            });
                                        ui.add_space(28.);
                                    }
                                    _ => {
                                        ui.horizontal(|ui| {
                                            label(ui, "Ассистент", 12., p.muted);
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if design::button(
                                                        ui,
                                                        "",
                                                        Some(Icon::Copy),
                                                        Kind::Ghost,
                                                        p,
                                                        Vec2::splat(28.),
                                                    )
                                                    .on_hover_text("Скопировать ответ")
                                                    .clicked()
                                                    {
                                                        ui.ctx().copy_text(message.text.clone());
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(5.);
                                        let file_links = crate::artifacts::links(&message.text);
                                        for (destination, _) in &file_links {
                                            self.markdown.add_link_hook(destination.clone());
                                        }
                                        ui.scope(|ui| {
                                            ui.style_mut().wrap_mode =
                                                Some(egui::TextWrapMode::Wrap);
                                            ui.spacing_mut().item_spacing.y = 4.;
                                            ui.spacing_mut().interact_size.y = 20.;
                                            CommonMarkViewer::new().show(
                                                ui,
                                                &mut self.markdown,
                                                &message.text,
                                            );
                                        });
                                        for (destination, artifact) in &file_links {
                                            let clicked =
                                                self.markdown.remove_link_hook(destination)
                                                    == Some(true);
                                            if clicked {
                                                if let Some(server) =
                                                    self.settings.selected.as_deref()
                                                {
                                                    self.downloads.start(
                                                        &self.controller,
                                                        server,
                                                        &chat.cwd,
                                                        &artifact.path,
                                                    );
                                                }
                                            }
                                        }
                                        if let Some(server) = self.settings.selected.as_deref() {
                                            let connected = self.demo
                                                || self
                                                    .remote
                                                    .get(server)
                                                    .is_some_and(|r| r.connected);
                                            self.downloads.show(
                                                ui,
                                                &self.controller,
                                                server,
                                                &chat.cwd,
                                                &message.text,
                                                connected,
                                                p,
                                            );
                                        }
                                        ui.add_space(30.);
                                    }
                                });
                            }
                            if chat.busy && !grouped_busy {
                                ui.horizontal(|ui| {
                                    design::dots(ui, ACCENT);
                                    label(ui, &chat.status, 13., p.muted);
                                });
                                ui.add_space(18.);
                            }
                        });
                    });
            });
    }
    fn composer(&mut self, ui: &mut egui::Ui, k: &str, p: Palette) {
        let server = self.server().unwrap();
        let connected = self.connected(&server.id);
        let mut send = false;
        let mut attach = false;
        ui.horizontal(|ui| {
            let active=server.local_control && server.local_workspace.is_some();
            let auto=self.chats.get(k).is_some_and(|chat|chat.mode=="auto");
            let text=if active{if auto{"Этот ПК · Авто"}else{"Этот ПК · Вручную"}}else{"Подключить ПК"};
            let toggle=design::chip(ui,text,Icon::Computer,p,active).on_hover_text(if active{"ПК подключён. В «Авто» команды выполняются без подтверждения с правами пользователя Relay. Нажми, чтобы отключить."}else{"Подключить управление этим компьютером. Режим «Авто» выполняет команды и скачивает файлы без подтверждения."}).clicked();
            let disconnect=active && design::chip(ui,"",Icon::Close,p,false).on_hover_text("Отключить этот ПК").clicked();
            if toggle || disconnect {
                let path=if active{None}else{dirs::home_dir().or_else(||std::env::current_dir().ok()).map(|p|p.to_string_lossy().into_owned())};
                if let Some(profile)=self.settings.servers.iter_mut().find(|s|s.id==server.id){profile.local_control=path.is_some();profile.local_workspace=path.clone();}
                self.controller.send(&server.id,json!({"op":"_desktop_workspace","path":path}));
                for (key,chat) in &self.chats {if key.starts_with(&format!("{}/",server.id)){self.controller.send(&server.id,json!({"op":"_desktop_policy","sid":chat.sid,"auto":!active && chat.mode=="auto"}));}}
                self.save();
            }
        });
        ui.add_space(4.);
        let chat = self.chats.get_mut(k).unwrap();
        let previous = (chat.model.clone(), chat.mode.clone(), chat.effort.clone());
        if !chat.error.is_empty() {
            egui::Frame::new()
                .fill(p.surface)
                .corner_radius(12.)
                .inner_margin(12.)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        label(ui, chat.error.clone(), 12., ACCENT);
                        if design::button(
                            ui,
                            "",
                            Some(Icon::Close),
                            Kind::Ghost,
                            p,
                            Vec2::splat(28.),
                        )
                        .clicked()
                        {
                            chat.error.clear();
                        }
                    });
                });
            ui.add_space(10.);
        }
        let focused = ui.memory(|m| m.has_focus(egui::Id::new("prompt-field")));
        let focus = ui
            .ctx()
            .animate_bool_with_time(egui::Id::new("composer-focus"), focused, 0.18);
        egui::Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(
                1.0_f32,
                design::mix(p.line, ACCENT, focus * 0.65),
            ))
            .corner_radius(16.)
            .inner_margin(12.)
            .shadow(egui::epaint::Shadow {
                offset: [0, 5],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(14),
            })
            .show(ui, |ui| {
                if !chat.images.is_empty() {
                    let mut remove = None;
                    ui.horizontal_wrapped(|ui| {
                        for (index, file) in chat.images.iter().enumerate() {
                            if attachment_view(ui, file, p, true) {
                                remove = Some(index);
                            }
                        }
                    });
                    if let Some(index) = remove {
                        chat.images.remove(index);
                    }
                    ui.add_space(10.);
                }
                let response = egui::ScrollArea::vertical()
                    .id_salt("draft-scroll")
                    .max_height(132.)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut chat.draft)
                                .id(egui::Id::new("prompt-field"))
                                .frame(false)
                                .hint_text("Напиши задачу…")
                                .font(FontId::proportional(14.))
                                .desired_width(ui.available_width())
                                .desired_rows(1),
                        )
                    })
                    .inner;
                if response.has_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                {
                    send = true;
                }
                let compact =
                    ui.available_width() < if chat.mode == "manual" { 610. } else { 580. };
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    if design::button(
                        ui,
                        "Файлы",
                        Some(Icon::Attach),
                        Kind::Ghost,
                        p,
                        Vec2::new(100., 32.),
                    )
                    .on_hover_text("Выбор файлов, перетаскивание или Ctrl+V")
                    .clicked()
                    {
                        attach = true;
                    }
                    if !compact {
                        composer_selectors(ui, chat, p);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if chat.busy {
                            if design::button(
                                ui,
                                "",
                                Some(Icon::Stop),
                                Kind::Secondary,
                                p,
                                Vec2::splat(32.),
                            )
                            .on_hover_text("Остановить ответ")
                            .clicked()
                            {
                                self.controller
                                    .send(&server.id, json!({"op":"stop","sid":chat.sid}));
                                chat.status = "Останавливаем…".into();
                            }
                        } else {
                            let ready = (!chat.draft.trim().is_empty() || !chat.images.is_empty())
                                && !chat.loading;
                            ui.add_enabled_ui(ready || !connected, |ui| {
                                if design::button(
                                    ui,
                                    "",
                                    Some(if connected { Icon::Send } else { Icon::Arrow }),
                                    Kind::Primary,
                                    p,
                                    Vec2::splat(32.),
                                )
                                .on_hover_text(if connected {
                                    "Отправить · Enter"
                                } else {
                                    "Подключиться"
                                })
                                .clicked()
                                {
                                    send = true;
                                }
                            });
                        }
                    });
                });
                if compact {
                    ui.add_space(4.);
                    ui.horizontal_wrapped(|ui| composer_selectors(ui, chat, p));
                }
            });
        ui.add_space(9.);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                label(ui, "Enter — отправить · Shift+Enter — строка", 10., p.muted);
            });
        });
        let current = (chat.model.clone(), chat.mode.clone(), chat.effort.clone());
        if previous.1 != current.1 {
            self.controller.send(&server.id,json!({"op":"_desktop_policy","sid":chat.sid,"auto":server.local_control && current.1=="auto"}));
        }
        if previous != current {
            if let Some(s) = self.settings.servers.iter_mut().find(|s| s.id == server.id) {
                s.model = current.0;
                s.mode = current.1;
                s.effort = current.2;
            }
            self.save();
        }
        if send {
            self.send_prompt();
        }
        if attach {
            self.pick_images();
        }
    }
    fn permission_card(&mut self, ui: &mut egui::Ui, k: &str, p: Palette) {
        let Some(permission) = self
            .chats
            .get(k)
            .and_then(|c| c.permissions.first())
            .cloned()
        else {
            return;
        };
        let Some(server) = self.server() else {
            return;
        };
        let sid = self.chats[k].sid.clone();
        let mut decision = None;
        egui::Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.6)))
            .corner_radius(10.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                label(
                    ui,
                    if permission.tool == "AskUserQuestion" {
                        "Claude уточняет"
                    } else {
                        "Нужно твоё разрешение"
                    },
                    15.0,
                    p.text,
                );
                let mut ready = true;
                egui::ScrollArea::vertical()
                    .max_height(225.0)
                    .id_salt("permission-details")
                    .show(ui, |ui| {
                        if permission.tool == "AskUserQuestion" {
                            if let Some(questions) = permission.input["questions"].as_array() {
                                for question in questions {
                                    let text = question["question"].as_str().unwrap_or("");
                                    label(ui, text, 14.0, p.text);
                                    let a = self
                                        .answers
                                        .entry(format!("{}/{text}", permission.id))
                                        .or_default();
                                    if let Some(options) = question["options"].as_array() {
                                        for option in options {
                                            let option = option["label"].as_str().unwrap_or("");
                                            if question["multiSelect"].as_bool().unwrap_or(false) {
                                                let mut chosen: Vec<String> = a
                                                    .split(", ")
                                                    .filter(|s| !s.is_empty())
                                                    .map(str::to_owned)
                                                    .collect();
                                                let mut checked =
                                                    chosen.iter().any(|s| s == option);
                                                if ui.checkbox(&mut checked, option).changed() {
                                                    chosen.retain(|s| s != option);
                                                    if checked {
                                                        chosen.push(option.into());
                                                    }
                                                    *a = chosen.join(", ");
                                                }
                                            } else if ui
                                                .selectable_label(a == option, option)
                                                .clicked()
                                            {
                                                *a = option.into();
                                            }
                                        }
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(a)
                                            .hint_text("Свой ответ")
                                            .desired_width(f32::INFINITY),
                                    );
                                    ready &= !a.trim().is_empty();
                                }
                            }
                        } else {
                            label(ui, tool_name(&permission.tool), 12.0, p.muted);
                            let text = if permission.tool == "ExitPlanMode" {
                                "Перейти от плана к выполнению задачи?".into()
                            } else {
                                model::summary(&permission.input)
                            };
                            ui.add(
                                egui::Label::new(RichText::new(text).monospace().size(12.0))
                                    .selectable(true)
                                    .wrap(),
                            );
                        }
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            ready,
                            egui::Button::new(if permission.tool == "AskUserQuestion" {
                                "Ответить"
                            } else {
                                "Разрешить"
                            })
                            .fill(ACCENT),
                        )
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button("Отклонить").clicked() {
                        decision = Some(false);
                    }
                });
            });
        ui.add_space(10.0);
        if let Some(allow) = decision {
            let mut answers = serde_json::Map::new();
            if let Some(questions) = permission.input["questions"].as_array() {
                for q in questions {
                    if let Some(text) = q["question"].as_str() {
                        answers.insert(
                            text.into(),
                            json!(self
                                .answers
                                .remove(&format!("{}/{text}", permission.id))
                                .unwrap_or_default()),
                        );
                    }
                }
            }
            self.controller.send(&server.id,json!({"op":"permission","sid":sid,"id":permission.id,"allow":allow,"answers":answers}));
            self.chats.get_mut(k).unwrap().permissions.remove(0);
        }
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        let p = design::animated_palette(ctx, self.settings.light);
        if let Some((server, request)) = self.desktop_requests.first().cloned() {
            let mut decision = None;
            let profile = self.settings.servers.iter().find(|s| s.id == server);
            let name = profile.map(|s| s.name.as_str()).unwrap_or("Сервер");
            let workspace = profile
                .and_then(|s| s.local_workspace.as_deref())
                .unwrap_or("Не подключено");
            let command = request["tool"] == "run_local_command";
            let modal = egui::Modal::new(egui::Id::new("desktop-approval")).show(ctx, |ui| {
                ui.set_width(480.);
                label(
                    ui,
                    if command {
                        "Команда на этом компьютере"
                    } else {
                        "Файл с сервера"
                    },
                    20.,
                    p.text,
                );
                label(ui, format!("Запрос из {name}"), 12., p.muted);
                ui.add_space(8.);
                egui::ScrollArea::vertical()
                    .max_height(220.)
                    .show(ui, |ui| {
                        let text = if command {
                            request["input"]["command"].as_str().unwrap_or("")
                        } else {
                            request["input"]["server_path"].as_str().unwrap_or("")
                        };
                        ui.add(egui::Label::new(RichText::new(text).monospace().size(13.)).wrap());
                    });
                label(
                    ui,
                    if command {
                        format!(
                            "Папка: {workspace} · {} · до {} с",
                            std::env::consts::OS,
                            request["input"]["timeout_seconds"]
                                .as_u64()
                                .unwrap_or(60)
                                .clamp(1, 120)
                        )
                    } else {
                        format!(
                            "{} · место сохранения выберешь сам",
                            format!(
                                "{:.1} МБ",
                                request["input"]["size"].as_u64().unwrap_or(0) as f64 / 1048576.
                            )
                        )
                    },
                    12.,
                    p.muted,
                );
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    if ui.button("Отклонить").clicked() {
                        decision = Some(false);
                    }
                    if ui
                        .button(if command {
                            "Выполнить"
                        } else {
                            "Сохранить…"
                        })
                        .clicked()
                    {
                        decision = Some(true);
                    }
                });
            });
            if modal.should_close() {
                decision = Some(false);
            }
            if let Some(allow) = decision {
                self.desktop_requests.remove(0);
                if allow {
                    self.controller.approve_desktop(&server, &request);
                } else {
                    self.controller
                        .send(&server, json!({"op":"_desktop_deny","id":request["id"]}));
                }
            }
        }
        let visible =
            self.editor.is_some() || self.trust.is_some() || self.folder_open || self.close_confirm;
        let modal_t = ctx.animate_bool_with_time_and_easing(
            egui::Id::new("modal-fade"),
            visible,
            0.28,
            egui::emath::easing::cubic_in_out,
        );
        if let Some(mut editor) = self.editor.take() {
            let mut open = true;
            let mut submit = false;
            let mut delete = false;
            let mut cancel = false;
            let response = egui::Modal::new(egui::Id::new("server-editor"))
                .area(
                    egui::Modal::default_area(egui::Id::new("server-editor")).anchor(
                        egui::Align2::CENTER_CENTER,
                        Vec2::new(0., (1. - modal_t) * 9.),
                    ),
                )
                .frame(
                    egui::Frame::new()
                        .fill(p.surface)
                        .corner_radius(24.)
                        .inner_margin(28.)
                        .stroke(Stroke::new(1.0_f32, p.line)),
                )
                .backdrop_color(Color32::from_black_alpha((120. * modal_t) as u8))
                .show(ctx, |ui| {
                    ui.set_width(440.);
                    ui.set_opacity(modal_t);
                    ui.horizontal(|ui| {
                        label(
                            ui,
                            if editor.is_new {
                                "Новое подключение"
                            } else {
                                "Подключение к серверу"
                            },
                            23.,
                            p.text,
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if design::button(
                                ui,
                                "",
                                Some(Icon::Close),
                                Kind::Ghost,
                                p,
                                Vec2::splat(30.),
                            )
                            .clicked()
                            {
                                cancel = true;
                            }
                        });
                    });
                    label(ui, "Твои проекты и разговоры на этом сервере", 13., p.muted);
                    ui.add_space(20.);
                    label(ui, "Название", 12., p.muted);
                    design::field(
                        ui,
                        &mut editor.server.name,
                        "Например, рабочий сервер",
                        false,
                    );
                    ui.add_space(4.);
                    label(ui, "Адрес сервера", 12., p.muted);
                    design::field(ui, &mut editor.server.host, "IP-адрес или домен", false);
                    ui.add_space(4.);
                    ui.horizontal(|ui| {
                        let width = ui.available_width();
                        ui.allocate_ui_with_layout(
                            Vec2::new(width - 110., 70.),
                            Layout::top_down(Align::Min),
                            |ui| {
                                label(ui, "Пользователь", 12., p.muted);
                                design::field(ui, &mut editor.server.user, "root", false);
                            },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(100., 70.),
                            Layout::top_down(Align::Min),
                            |ui| {
                                label(ui, "Порт", 12., p.muted);
                                ui.add_sized(
                                    [ui.available_width(), 43.],
                                    egui::DragValue::new(&mut editor.server.port).range(1..=65535),
                                );
                            },
                        );
                    });
                    label(ui, "Пароль", 12., p.muted);
                    design::field(
                        ui,
                        &mut editor.password,
                        if editor.is_new {
                            "Пароль сервера"
                        } else {
                            "Сохранённый пароль"
                        },
                        true,
                    );
                    ui.add_space(4.);
                    label(ui, "Рабочая папка", 12., p.muted);
                    design::field(ui, &mut editor.server.cwd, "~", false);
                    ui.add_space(4.);
                    design::toggle(ui, &mut editor.server.remember, "Запомнить пароль", p)
                        .on_hover_text("Пароль сохраняется в хранилище системы");
                    if !editor.error.is_empty() {
                        ui.add_space(5.);
                        label(ui, &editor.error, 12., ACCENT);
                    }
                    ui.add_space(18.);
                    ui.horizontal(|ui| {
                        if !editor.is_new
                            && design::button(
                                ui,
                                "Удалить",
                                None,
                                Kind::Ghost,
                                p,
                                Vec2::new(82., 42.),
                            )
                            .clicked()
                        {
                            delete = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if design::button(
                                ui,
                                "Подключиться",
                                Some(Icon::Arrow),
                                Kind::Primary,
                                p,
                                Vec2::new(166., 42.),
                            )
                            .clicked()
                            {
                                submit = true;
                            }
                            if design::button(
                                ui,
                                "Отмена",
                                None,
                                Kind::Ghost,
                                p,
                                Vec2::new(94., 42.),
                            )
                            .clicked()
                            {
                                cancel = true;
                            }
                        });
                    });
                });
            if response.should_close() {
                open = false;
            }
            if delete {
                self.controller.disconnect(&editor.server.id);
                self.settings.servers.retain(|s| s.id != editor.server.id);
                storage::forget(&editor.server.id);
                if self.settings.selected.as_deref() == Some(&editor.server.id) {
                    self.settings.selected = self.settings.servers.first().map(|s| s.id.clone());
                }
                self.save();
                editor.password.zeroize();
            } else if submit {
                match editor.server.validate() {
                    Err(e) => {
                        editor.error = e.to_string();
                        self.editor = Some(editor);
                    }
                    Ok(()) => {
                        if let Some(old) = self
                            .settings
                            .servers
                            .iter()
                            .find(|s| s.id == editor.server.id)
                        {
                            if (old.host.as_str(), old.port, old.user.as_str())
                                != (
                                    editor.server.host.as_str(),
                                    editor.server.port,
                                    editor.server.user.as_str(),
                                )
                            {
                                editor.server.fingerprint = None;
                            }
                        }
                        if editor.password.is_empty() {
                            if let Some(password) = storage::password(&editor.server.id) {
                                editor.password = password;
                            } else {
                                editor.error = "Введи пароль сервера.".into();
                                self.editor = Some(editor);
                                return;
                            }
                        }
                        if editor.server.remember {
                            if let Err(e) = storage::remember(&editor.server.id, &editor.password) {
                                self.notice = e.to_string();
                            }
                        } else {
                            storage::forget(&editor.server.id);
                        }
                        self.settings.servers.retain(|s| s.id != editor.server.id);
                        self.settings.servers.push(editor.server.clone());
                        self.settings.selected = Some(editor.server.id.clone());
                        self.save();
                        self.establish(editor.server, std::mem::take(&mut editor.password));
                    }
                }
            } else if open && !cancel {
                self.editor = Some(editor);
            } else {
                editor.password.zeroize();
            }
        }
        if let Some(trust) = self.trust.take() {
            let mut accept = false;
            let mut deny = false;
            let modal = egui::Modal::new(egui::Id::new("host-trust"))
                .area(
                    egui::Modal::default_area(egui::Id::new("host-trust")).anchor(
                        egui::Align2::CENTER_CENTER,
                        Vec2::new(0., (1. - modal_t) * 9.),
                    ),
                )
                .frame(
                    egui::Frame::new()
                        .fill(p.surface)
                        .corner_radius(24.)
                        .inner_margin(28.),
                )
                .show(ctx, |ui| {
                    ui.set_width(460.);
                    ui.set_opacity(modal_t);
                    label(ui, "Первое подключение", 23., p.text);
                    ui.add_space(10.);
                    label(ui, "Запомнить ключ этого сервера?", 15., p.text);
                    if let Some(server) =
                        self.settings.servers.iter().find(|s| s.id == trust.server)
                    {
                        label(
                            ui,
                            format!("{} · {}:{}", server.name, server.host, server.port),
                            13.,
                            p.muted,
                        );
                    }
                    ui.add_space(15.);
                    egui::Frame::new()
                        .fill(p.base)
                        .corner_radius(12.)
                        .inner_margin(14.)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&trust.fingerprint).monospace().size(12.),
                                )
                                .wrap(),
                            );
                        });
                    ui.add_space(18.);
                    ui.horizontal(|ui| {
                        if design::button(ui, "Отмена", None, Kind::Ghost, p, Vec2::new(100., 42.))
                            .clicked()
                        {
                            deny = true;
                        }
                        if design::button(
                            ui,
                            "Запомнить и продолжить",
                            Some(Icon::Check),
                            Kind::Primary,
                            p,
                            Vec2::new(260., 42.),
                        )
                        .clicked()
                        {
                            accept = true;
                        }
                    });
                });
            if modal.should_close() {
                deny = true;
            }
            if accept {
                if let Some(s) = self
                    .settings
                    .servers
                    .iter_mut()
                    .find(|s| s.id == trust.server)
                {
                    s.fingerprint = Some(trust.fingerprint);
                }
                self.save();
                let _ = trust.answer.send(true);
            } else if deny {
                let _ = trust.answer.send(false);
            } else {
                self.trust = Some(trust);
            }
        }
        if self.folder_open {
            let mut open = true;
            let mut chosen = None;
            let mut browse = None;
            let modal = egui::Modal::new(egui::Id::new("folder-picker"))
                .area(
                    egui::Modal::default_area(egui::Id::new("folder-picker")).anchor(
                        egui::Align2::CENTER_CENTER,
                        Vec2::new(0., (1. - modal_t) * 9.),
                    ),
                )
                .frame(
                    egui::Frame::new()
                        .fill(p.surface)
                        .corner_radius(24.)
                        .inner_margin(26.),
                )
                .show(ctx, |ui| {
                    ui.set_width(540.);
                    ui.set_opacity(modal_t);
                    ui.horizontal(|ui| {
                        label(ui, "Рабочая папка", 23., p.text);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if design::button(
                                ui,
                                "",
                                Some(Icon::Close),
                                Kind::Ghost,
                                p,
                                Vec2::splat(30.),
                            )
                            .clicked()
                            {
                                open = false;
                            }
                        });
                    });
                    ui.add_space(16.);
                    ui.horizontal(|ui| {
                        if design::button(
                            ui,
                            "",
                            Some(Icon::Send),
                            Kind::Secondary,
                            p,
                            Vec2::splat(43.),
                        )
                        .on_hover_text("На уровень выше")
                        .clicked()
                        {
                            browse = Some(self.folder_parent.clone());
                        }
                        let width = ui.available_width() - 94.;
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 44.),
                            Layout::top_down(Align::Min),
                            |ui| {
                                let response = design::field(
                                    ui,
                                    &mut self.folder_path,
                                    "Путь на сервере",
                                    false,
                                );
                                if response.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    browse = Some(self.folder_path.clone());
                                }
                            },
                        );
                        if design::button(
                            ui,
                            "Открыть",
                            None,
                            Kind::Secondary,
                            p,
                            Vec2::new(85., 43.),
                        )
                        .clicked()
                        {
                            browse = Some(self.folder_path.clone());
                        }
                    });
                    ui.add_space(8.);
                    if design::toggle(ui, &mut self.folder_hidden, "Скрытые папки", p).clicked()
                    {
                        browse = Some(self.folder_path.clone());
                    }
                    if let Some(server) = self.server() {
                        ui.horizontal_wrapped(|ui| {
                            for path in server.folders.iter().take(5) {
                                if design::button(
                                    ui,
                                    &model::shorten(path, 22),
                                    Some(Icon::Folder),
                                    Kind::Ghost,
                                    p,
                                    Vec2::new(0., 32.),
                                )
                                .on_hover_text(path)
                                .clicked()
                                {
                                    browse = Some(path.clone());
                                }
                            }
                        });
                    }
                    ui.add_space(10.);
                    egui::ScrollArea::vertical()
                        .max_height(320.)
                        .show(ui, |ui| {
                            for folder in &self.folders {
                                if design::button(
                                    ui,
                                    &folder.name,
                                    Some(Icon::Folder),
                                    Kind::Ghost,
                                    p,
                                    Vec2::new(ui.available_width(), 38.),
                                )
                                .clicked()
                                {
                                    browse = Some(folder.path.clone());
                                }
                            }
                        });
                    ui.add_space(16.);
                    ui.add_enabled_ui(self.folder_path == self.folder_confirmed, |ui| {
                        if design::button(
                            ui,
                            "Работать в этой папке",
                            Some(Icon::Check),
                            Kind::Primary,
                            p,
                            Vec2::new(ui.available_width(), 44.),
                        )
                        .clicked()
                        {
                            chosen = Some(self.folder_path.clone());
                        }
                    });
                });
            if modal.should_close() {
                open = false;
            }
            self.folder_open = open;
            if let Some(path) = browse {
                self.folder_path = path;
                self.request_folders();
            }
            if let Some(path) = chosen {
                if let Some(s) = self
                    .settings
                    .servers
                    .iter_mut()
                    .find(|s| Some(&s.id) == self.settings.selected.as_ref())
                {
                    s.cwd = path.clone();
                    s.last_session = None;
                    s.folders.retain(|p| p != &path);
                    s.folders.insert(0, path);
                    s.folders.truncate(10);
                }
                self.save();
                self.new_chat();
                self.folder_open = false;
            }
        }
        if self.close_confirm {
            let modal=egui::Modal::new(egui::Id::new("close-confirm")).area(egui::Modal::default_area(egui::Id::new("close-confirm" )).anchor(egui::Align2::CENTER_CENTER,Vec2::new(0.,(1.-modal_t)*9.))).frame(egui::Frame::new().fill(p.surface).corner_radius(24.).inner_margin(28.)).show(ctx,|ui|{
            ui.set_width(440.);label(ui,"Задача ещё выполняется",22.,p.text);ui.add_space(12.);label(ui,"Можно оставить приложение открытым или остановить работу. Разговор сохранится.",14.,p.muted);ui.add_space(20.);ui.horizontal(|ui|{if design::button(ui,"Остаться",None,Kind::Secondary,p,Vec2::new(130.,42.)).clicked(){self.close_confirm=false;}if design::button(ui,"Остановить и закрыть",None,Kind::Primary,p,Vec2::new(240.,42.)).clicked(){ctx.send_viewport_cmd(egui::ViewportCommand::Close);self.close_confirm=false;for chat in self.chats.values_mut(){chat.busy=false;}}});
        });
            if modal.should_close() {
                self.close_confirm = false;
            }
        }
    }
    fn seed_demo(&mut self) {
        let s = Server {
            id: "demo".into(),
            name: "Production".into(),
            host: "192.168.1.10".into(),
            cwd: "/opt/atlas".into(),
            ..Default::default()
        };
        self.settings.servers = vec![
            s.clone(),
            Server {
                id: "stage".into(),
                name: "Stage".into(),
                host: "192.168.1.11".into(),
                ..Default::default()
            },
        ];
        self.settings.selected = Some(s.id.clone());
        self.remote.insert(
            s.id.clone(),
            Remote {
                connected: true,
                connecting: false,
                version: "Claude Code".into(),
                home: "/root".into(),
                usage: serde_json::from_value(json!({
                    "available":true,
                    "windows":[
                        {"id":"five_hour","utilization":38,"resets_at":chrono::Local::now().timestamp()+10500},
                        {"id":"seven_day","utilization":64,"resets_at":chrono::Local::now().timestamp()+198000}
                    ],
                    "updated_at":chrono::Local::now().timestamp()
                })).unwrap_or_default(),
                usage_requested: None,
                sessions_requested: None,
                sessions_loaded: true,
                sessions_retries: 0,
                sessions_error: String::new(),
                sessions: vec![
                    Session {
                        id: "demo-chat".into(),
                        title: "Упрощаем управление заказами".into(),
                        cwd: s.cwd.clone(),
                        modified: chrono::Local::now().timestamp() as f64,
                    },
                    Session {
                        id: "d2".into(),
                        title: "Новый интерфейс мастерской".into(),
                        cwd: s.cwd.clone(),
                        modified: chrono::Local::now().timestamp() as f64 - 4000.0,
                    },
                    Session {
                        id: "d3".into(),
                        title: "Проверка обновления сервера".into(),
                        cwd: s.cwd.clone(),
                        modified: chrono::Local::now().timestamp() as f64 - 90000.0,
                    },
                ],
            },
        );
        let mut c = Chat::new(
            "demo-chat".into(),
            s.cwd,
            "Упрощаем управление заказами".into(),
        );
        c.loaded = true;
        c.messages=vec![Message{role:"user".into(),text:"Давай сделаем управление заказами понятнее. Хочется выбрать работу, посмотреть условия и принять результат в одном окне.".into(),..Default::default()},Message{role:"assistant".into(),text:"Соберу всё в одном месте. Сначала посмотрю, как сейчас устроены заказы и приёмка работы.".into(),..Default::default()},Message{role:"tool".into(),tool:"Read".into(),input:json!({"file_path":"/opt/atlas/src/orders/board.rs"}),..Default::default()},Message{role:"assistant".into(),text:"Нашёл нужный экран. Предлагаю три небольших изменения:\n\n- **Условия рядом с заказом** — работа, оплата и срок видны сразу.\n- **Свои и свободные** — два понятных списка.\n- **Приёмка заказчиком** — оплата приходит после подтверждения.\n\nИсторию и текущие заявки сохраним. Начну с экрана списка.".into(),..Default::default()}];
        self.selected_chat.insert(s.id.clone(), c.sid.clone());
        self.chats.insert(key(&s.id, &c.sid), c);
    }
}
fn composer_selectors(ui: &mut egui::Ui, chat: &mut Chat, p: Palette) {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(8., 6.);
        ui.spacing_mut().interact_size.y = 30.;
        egui::ComboBox::from_id_salt("model-selector")
            .selected_text(
                RichText::new(match chat.model.as_str() {
                    "opus" => "Opus",
                    "sonnet" => "Sonnet",
                    _ => "Модель сервера",
                })
                .size(13.)
                .color(p.muted),
            )
            .width(116.)
            .show_ui(ui, |ui| {
                for (id, name) in [
                    ("default", "Модель сервера"),
                    ("sonnet", "Sonnet"),
                    ("opus", "Opus"),
                ] {
                    ui.selectable_value(&mut chat.model, id.into(), name);
                }
            });
        egui::ComboBox::from_id_salt("mode-selector")
            .selected_text(
                RichText::new(match chat.mode.as_str() {
                    "auto" => "Авто",
                    "acceptEdits" => "Правки файлов",
                    "plan" => "План",
                    _ => "С подтверждением",
                })
                .size(13.)
                .color(if chat.mode == "auto" { ACCENT } else { p.muted }),
            )
            .width(if chat.mode == "manual" { 154. } else { 102. })
            .show_ui(ui, |ui| {
                for (id, name) in [
                    ("auto", "Авто"),
                    ("manual", "С подтверждением"),
                    ("acceptEdits", "Правки файлов"),
                    ("plan", "Планирование"),
                ] {
                    ui.selectable_value(&mut chat.mode, id.into(), name);
                }
            });
        egui::ComboBox::from_id_salt("effort-selector")
            .selected_text(
                RichText::new(format!(
                    "Effort: {}",
                    if chat.effort == "default" {
                        "авто"
                    } else {
                        chat.effort.as_str()
                    }
                ))
                .size(13.)
                .color(p.muted),
            )
            .width(135.)
            .show_ui(ui, |ui| {
                label(ui, "Глубина размышлений", 12., p.muted);
                for (id, name) in [
                    ("default", "Автоматически"),
                    ("low", "Низкая · Low"),
                    ("medium", "Средняя · Medium"),
                    ("high", "Высокая · High"),
                    ("xhigh", "Очень высокая · xHigh"),
                    ("max", "Максимальная · Max"),
                ] {
                    ui.selectable_value(&mut chat.effort, id.into(), name);
                }
            })
            .response
            .on_hover_text("Применяется к следующему сообщению");
    });
}
fn attachment_view(ui: &mut egui::Ui, file: &Attachment, p: Palette, removable: bool) -> bool {
    let mut remove = false;
    egui::Frame::new()
        .fill(p.base)
        .corner_radius(12.)
        .inner_margin(10.)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(texture) = &file.texture {
                    ui.add(
                        egui::Image::new((texture.id(), texture.size_vec2()))
                            .max_size(Vec2::new(48., 40.))
                            .corner_radius(6.),
                    );
                } else {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(28., 34.), egui::Sense::hover());
                    design::icon(ui.painter(), r, Icon::File, ACCENT);
                }
                ui.vertical(|ui| {
                    label(ui, model::shorten(&file.name, 22), 12., p.text);
                    label(
                        ui,
                        if file.bytes >= 1024 * 1024 {
                            format!("{:.1} МБ", file.bytes as f32 / (1024. * 1024.))
                        } else {
                            format!("{} КБ", (file.bytes / 1024).max(1))
                        },
                        10.,
                        p.muted,
                    );
                });
                if removable
                    && design::button(
                        ui,
                        "",
                        Some(Icon::Close),
                        Kind::Ghost,
                        p,
                        Vec2::new(24., 28.),
                    )
                    .on_hover_text("Убрать файл")
                    .clicked()
                {
                    remove = true;
                }
            });
        });
    remove
}

fn relative(time: f64) -> String {
    let now = chrono::Local::now().timestamp();
    let seconds = (now - time as i64).max(0);
    if seconds < 3600 {
        format!("{} мин. назад", (seconds / 60).max(1))
    } else if seconds < 86400 {
        format!("{} ч. назад", seconds / 3600)
    } else {
        format!("{} дн. назад", seconds / 86400)
    }
}
fn tool_name(name: &str) -> &str {
    match name {
        "Read" => "Чтение файла",
        "Edit" | "MultiEdit" => "Изменение файла",
        "Write" => "Создание файла",
        "Bash" => "Команда на сервере",
        "mcp__relay_desktop__run_local_command" => "Команда на этом ПК",
        "mcp__relay_desktop__download_file" => "Скачивание на этот ПК",
        "Glob" | "Grep" => "Поиск по проекту",
        "WebSearch" | "WebFetch" => "Поиск в интернете",
        "ExitPlanMode" => "План готов",
        _ => name,
    }
}
fn reduce(c: &mut Chat, data: &Value) {
    match data["type"].as_str().unwrap_or("") {
        "stream_event" => {
            let e = &data["event"];
            let index = e["index"].as_u64().unwrap_or(0) as usize;
            match e["type"].as_str().unwrap_or("") {
                "message_start" => {
                    c.blocks.clear();
                }
                "content_block_start" => {
                    let block = &e["content_block"];
                    if block["type"] == "text" {
                        let at = c.messages.len();
                        c.messages.push(Message {
                            role: "assistant".into(),
                            text: block["text"].as_str().unwrap_or("").into(),
                            ..Default::default()
                        });
                        c.blocks.insert(index, at);
                    } else if block["type"] == "thinking" {
                        c.status = "Думает над ответом".into();
                    }
                }
                "content_block_delta" => {
                    if let Some(text) = e["delta"]["text"].as_str() {
                        if let Some(at) = c.blocks.get(&index) {
                            c.messages[*at].text.push_str(text);
                            c.status = "Пишет ответ".into();
                        }
                    }
                }
                _ => {}
            }
        }
        "assistant" => {
            if let Some(blocks) = data["message"]["content"].as_array() {
                for (index, b) in blocks.iter().enumerate() {
                    if b["type"] == "text" {
                        let text = b["text"].as_str().unwrap_or("");
                        if let Some(at) = c.blocks.get(&index) {
                            c.messages[*at].text = text.into();
                        } else if !text.is_empty() {
                            c.messages.push(Message {
                                role: "assistant".into(),
                                text: text.into(),
                                ..Default::default()
                            });
                        }
                    } else if b["type"] == "tool_use" {
                        let id = b["id"].as_str().unwrap_or("");
                        if !c
                            .messages
                            .iter()
                            .rev()
                            .take(20)
                            .any(|m| m.role == "tool" && m.tool_id == id)
                        {
                            c.messages.push(Message {
                                role: "tool".into(),
                                tool: b["name"].as_str().unwrap_or("Действие").into(),
                                input: b["input"].clone(),
                                tool_id: id.into(),
                                ..Default::default()
                            });
                        }
                        c.status = tool_name(b["name"].as_str().unwrap_or("Работает")).into();
                    }
                }
            }
        }
        "user" => {
            if let Some(blocks) = data["message"]["content"].as_array() {
                for b in blocks {
                    if b["type"] == "tool_result" {
                        let text = if let Some(s) = b["content"].as_str() {
                            s.to_owned()
                        } else {
                            b["content"]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|b| b["text"].as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_default()
                        };
                        c.messages.push(Message {
                            role: "tool_result".into(),
                            text: model::shorten(&text, 24000),
                            failed: b["is_error"].as_bool().unwrap_or(false),
                            tool_id: b["tool_use_id"].as_str().unwrap_or("").into(),
                            ..Default::default()
                        });
                    }
                }
            }
        }
        "result" => {
            c.busy = false;
            c.permissions.clear();
            c.status.clear();
            if data["is_error"].as_bool().unwrap_or(false) {
                c.error = data["errors"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_else(|| {
                        data["result"]
                            .as_str()
                            .unwrap_or("Claude не смог завершить задачу.")
                            .into()
                    });
            } else if !c
                .messages
                .iter()
                .skip(c.user_at + 1)
                .any(|m| m.role == "assistant" && !m.text.is_empty())
            {
                if let Some(text) = data["result"].as_str() {
                    c.messages.push(Message {
                        role: "assistant".into(),
                        text: text.into(),
                        ..Default::default()
                    });
                }
            }
        }
        _ => {}
    }
}
impl eframe::App for RelayApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.theme(ctx);
        self.poll();
        if !self.demo {
            if let Some(server) = self.settings.selected.as_ref() {
                if let Some(remote) = self.remote.get_mut(server).filter(|r| r.connected) {
                    if remote
                        .usage_requested
                        .is_none_or(|at| at.elapsed() >= Duration::from_secs(60))
                    {
                        remote.usage_requested = Some(Instant::now());
                        self.controller.send(server, json!({"op":"usage"}));
                    }
                    ctx.request_repaint_after(Duration::from_secs(60));
                }
            }
        }
        self.frames += 1;
        if !self.demo {
            for (server, remote) in &mut self.remote {
                if remote.connected
                    && !remote.sessions_loaded
                    && remote.sessions_retries < 3
                    && remote
                        .sessions_requested
                        .is_some_and(|at| at.elapsed() >= Duration::from_secs(12))
                {
                    remote.sessions_requested = Some(Instant::now());
                    remote.sessions_retries += 1;
                    remote.sessions_error = "Загрузка занимает больше времени. Повторяем…".into();
                    self.controller.send(server, json!({"op":"sessions"}));
                }
            }
            if self
                .remote
                .values()
                .any(|r| r.connected && !r.sessions_loaded)
            {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.chats.values().any(|c| c.busy) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_confirm = true;
        }
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        if !dropped.is_empty() {
            if self.current_key().is_none() {
                self.new_chat();
            }
            if let (Some(s), Some(k)) = (self.server(), self.current_key()) {
                let sid = self.chats[&k].sid.clone();
                self.load_images(
                    s.id,
                    sid,
                    dropped.into_iter().filter_map(|f| f.path).collect(),
                );
            }
        }
        if self.editor.is_none()
            && !self.folder_open
            && ctx.input(|i| i.events.iter().any(|event|matches!(event,egui::Event::Key{key:egui::Key::V,pressed:true,modifiers,..} if modifiers.command||modifiers.ctrl)))
        {
            self.paste_image();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N)) {
            self.new_chat();
        }
        self.rail(ctx);
        self.sidebar(ctx);
        self.main_panel(ctx);
        self.dialogs(ctx);
        if self.chats.values().any(|c| c.busy) || self.remote.values().any(|r| r.connecting) {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if self.screenshot.is_some() {
            let delay = std::env::args()
                .collect::<Vec<_>>()
                .windows(2)
                .find(|a| a[0] == "--screenshot-delay-ms")
                .and_then(|a| a[1].parse::<f64>().ok())
                .unwrap_or(450.)
                / 1000.;
            if self.capture_at == 0.0 {
                self.capture_at = ctx.input(|i| i.time) + delay;
            }
            if !self.capture_requested && ctx.input(|i| i.time) >= self.capture_at {
                self.capture_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = shot {
                let path = self.screenshot.take().unwrap();
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                let _ = image::save_buffer(
                    path,
                    &bytes,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint();
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streamed_and_final_text_are_not_duplicated() {
        let mut c = Chat::new("x".into(), "/tmp".into(), "t".into());
        reduce(
            &mut c,
            &json!({"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}),
        );
        reduce(
            &mut c,
            &json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Привет"}}}),
        );
        reduce(
            &mut c,
            &json!({"type":"assistant","message":{"content":[{"type":"text","text":"Привет"}]}}),
        );
        assert_eq!(c.messages.len(), 1);
        assert_eq!(c.messages[0].text, "Привет");
    }
}
