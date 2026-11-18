use super::{
    conversation::{Chat, Prompt, SavedChat},
    editor::Editor,
};
use anyhow::{Context, Result};
use crossterm::event::{
    Event as Input, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use ratatui::layout::Rect;
use relay_desktop::{
    attachments,
    connection::{Controller, Event},
    model::{ApiProfile, CommandInfo, ModelInfo, Permission, Server, Session, Settings},
    storage,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "Клавиши и команды"),
    ("/hosts", "Выбрать сервер"),
    ("/connect", "Подключиться или переподключиться"),
    ("/disconnect", "Отключиться от сервера"),
    ("/add", "Добавить сервер"),
    ("/sessions", "Найти сессию"),
    ("/resume", "Открыть сессию по ID"),
    ("/new", "Новая сессия"),
    ("/cd", "Рабочая папка на сервере"),
    ("/model", "Выбрать модель"),
    ("/mode", "auto, manual, acceptEdits, plan"),
    ("/effort", "default, low, medium, high, xhigh, max"),
    ("/backend", "claude, codex, agy или api в этой сессии"),
    ("/api", "Настроить API для сервера"),
    ("/attach", "Прикрепить файл с компьютера"),
    ("/clear-files", "Убрать вложения"),
    ("/download", "Скачать файл с сервера"),
    ("/stop", "Остановить текущий ответ"),
    ("/queue", "Посмотреть очередь сообщений"),
    ("/pause", "Приостановить очередь"),
    ("/continue", "Продолжить очередь"),
    (
        "/recover",
        "Вернуть сообщение с неизвестным статусом в черновик",
    ),
    ("/rename", "Переименовать сессию"),
    ("/pin", "Закрепить или открепить сессию"),
    ("/delete", "Удалить сессию с подтверждением"),
    ("/prompt", "Инструкции для этой сессии"),
    ("/theme", "graphite, midnight, paper"),
    ("/refresh", "Обновить сессии и лимиты"),
    ("/login", "Как войти в Claude, Codex или AGY на сервере"),
    ("/quit", "Выйти из Relay TUI"),
];

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub selected: Option<String>,
    pub last: BTreeMap<String, String>,
    pub cwd: BTreeMap<String, String>,
    pub chats: BTreeMap<String, SavedChat>,
    pub trusted: BTreeMap<String, String>,
    pub servers: Vec<Server>,
    pub api: BTreeMap<String, ApiProfile>,
    pub theme: String,
}
impl Store {
    fn load() -> Result<Self> {
        let path = storage::directory().join("tui-state.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(&path)?;
        anyhow::ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "Состояние TUI превышает 64 МБ."
        );
        serde_json::from_slice(&bytes)
            .context("Не удалось прочитать tui-state.json. Файл сохранён без изменений.")
    }
    fn save(&self) -> Result<()> {
        self.save_to(&storage::directory())
    }
    fn save_to(&self, dir: &std::path::Path) -> Result<()> {
        use std::io::Write;
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let bytes = serde_json::to_vec(self)?;
        anyhow::ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "Состояние TUI превышает 64 МБ."
        );
        let temp = dir.join(format!("tui-{}.pending", uuid::Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        let target = dir.join("tui-state.json");
        #[cfg(windows)]
        if target.exists() {
            std::fs::copy(&target, dir.join("tui-state.backup.json"))?;
            std::fs::remove_file(&target)?;
        }
        if let Err(e) = std::fs::rename(&temp, &target) {
            let _ = std::fs::remove_file(temp);
            return Err(e.into());
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Remote {
    pub connected: bool,
    pub connecting: bool,
    pub sessions: Vec<Session>,
    pub loaded: bool,
    pub models: Vec<ModelInfo>,
    pub codex_models: Vec<ModelInfo>,
    pub agy_models: Vec<ModelInfo>,
    pub commands: Vec<CommandInfo>,
    pub codex_commands: Vec<CommandInfo>,
    pub agy_commands: Vec<CommandInfo>,
    pub backends: Value,
    pub usage: Value,
    pub home: String,
    pub version: String,
    pub error: String,
    requested: Option<Instant>,
    retries: u8,
    last_refresh: Option<Instant>,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Picker {
    Hosts,
    Sessions,
    Models,
    Backends,
    Themes,
    Modes,
    Efforts,
    Commands,
}
pub struct Item {
    pub id: String,
    pub label: String,
    pub detail: String,
}
pub enum Field {
    Attach,
    Cwd,
    Rename,
    Prompt,
    Resume,
    DownloadRemote,
    DownloadLocal { remote: String },
    Add { server: Server, stage: u8 },
    Api { profile: ApiProfile, stage: u8 },
}
pub enum Confirmation {
    Quit,
    Delete {
        server: String,
        sid: String,
    },
    Download {
        server: String,
        remote: String,
        target: PathBuf,
    },
}
pub enum Popup {
    Picker {
        kind: Picker,
        search: Editor,
        index: usize,
    },
    Password {
        server: Server,
        editor: Editor,
    },
    Trust {
        server: String,
        fingerprint: String,
        answer: tokio::sync::oneshot::Sender<bool>,
    },
    Field {
        kind: Field,
        editor: Editor,
        error: String,
    },
    Confirm {
        message: String,
        action: Confirmation,
    },
    Permission {
        server: String,
        sid: String,
        permission: Permission,
        answers: Vec<Editor>,
        selected: usize,
        error: String,
    },
    Queue {
        key: String,
        index: usize,
    },
    Help {
        scroll: usize,
    },
}
#[derive(Clone)]
pub enum Hit {
    Host(String),
    Session(Session),
    New,
    Hosts,
    Sessions,
}

pub struct App {
    pub controller: Controller,
    rx: mpsc::Receiver<Event>,
    pub settings: Settings,
    pub store: Store,
    pub remote: BTreeMap<String, Remote>,
    pub chats: BTreeMap<String, Chat>,
    pub active: Option<String>,
    pub selected: BTreeMap<String, String>,
    pub popup: Option<Popup>,
    pending_trust: VecDeque<(String, String, tokio::sync::oneshot::Sender<bool>)>,
    pub notice: String,
    pub quit: bool,
    pub demo: bool,
    pub read_only: bool,
    pub sidebar: bool,
    pub hits: Vec<(Rect, Hit)>,
    pub chat_rect: Rect,
    pub dirty: bool,
    last_save: Instant,
    history: Vec<String>,
    history_at: Option<usize>,
    removed: BTreeSet<String>,
    dir_request: Option<(String, u64)>,
    counter: u64,
}
pub fn chat_key(server: &str, sid: &str) -> String {
    format!("{server}/{sid}")
}
fn trust_key(server: &Server) -> String {
    format!(
        "{}@{}:{}#{}",
        server.user, server.host, server.port, server.id
    )
}
pub fn path(value: &str) -> PathBuf {
    let value = value.trim().trim_matches('"').trim_matches('\'');
    if value == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    } else if let Some(rest) = value.strip_prefix("~/") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest)
    } else {
        PathBuf::from(value)
    }
}

impl App {
    pub fn new(demo: bool) -> Result<Self> {
        let settings = if demo {
            Settings::default()
        } else {
            storage::load()?
        };
        let store = if demo {
            Store::default()
        } else {
            Store::load()?
        };
        let (controller, rx) = Controller::headless();
        let active = store
            .selected
            .clone()
            .filter(|id| {
                settings
                    .servers
                    .iter()
                    .chain(store.servers.iter())
                    .any(|s| &s.id == id)
            })
            .or_else(|| settings.selected.clone())
            .or_else(|| settings.servers.first().map(|s| s.id.clone()))
            .or_else(|| store.servers.first().map(|s| s.id.clone()));
        let selected = store.last.clone();
        Ok(Self {
            controller,
            rx,
            settings,
            store,
            remote: BTreeMap::new(),
            chats: BTreeMap::new(),
            active,
            selected,
            popup: None,
            pending_trust: VecDeque::new(),
            notice: String::new(),
            quit: false,
            demo,
            read_only: false,
            sidebar: true,
            hits: vec![],
            chat_rect: Rect::default(),
            dirty: false,
            last_save: Instant::now(),
            history: vec![],
            history_at: None,
            removed: BTreeSet::new(),
            dir_request: None,
            counter: 0,
        })
    }
    pub fn servers(&self) -> Vec<Server> {
        let mut servers = self.settings.servers.clone();
        for server in &self.store.servers {
            if !servers.iter().any(|s| s.id == server.id) {
                servers.push(server.clone());
            }
        }
        for server in &mut servers {
            if let Some(api) = self.store.api.get(&server.id) {
                server.api = api.clone();
            }
        }
        servers
    }
    pub fn server(&self) -> Option<Server> {
        self.servers()
            .into_iter()
            .find(|s| Some(&s.id) == self.active.as_ref())
    }
    pub fn key(&self) -> Option<String> {
        let host = self.active.as_ref()?;
        Some(chat_key(host, self.selected.get(host)?))
    }
    pub fn chat(&self) -> Option<&Chat> {
        self.key().and_then(|k| self.chats.get(&k))
    }
    pub fn chat_mut(&mut self) -> Option<&mut Chat> {
        self.key().and_then(|k| self.chats.get_mut(&k))
    }
    pub fn connected(&self) -> bool {
        self.active
            .as_ref()
            .and_then(|h| self.remote.get(h))
            .is_some_and(|r| r.connected)
    }
    pub fn theme(&self) -> &str {
        if self.store.theme.is_empty() {
            self.settings.theme()
        } else {
            &self.store.theme
        }
    }
    pub fn picker(&mut self, kind: Picker) {
        self.popup = Some(Popup::Picker {
            kind,
            search: Editor::default(),
            index: 0,
        });
    }
    pub fn items(&self, kind: Picker, query: &str) -> Vec<Item> {
        let items: Vec<Item> = match kind {
            Picker::Hosts => self
                .servers()
                .into_iter()
                .map(|s| {
                    let eff = s.effective_backend();
                    let agent_tag = if eff == "claude" {
                        String::new()
                    } else {
                        format!(" · {}", relay_desktop::model::backend_short_label(eff))
                    };
                    Item {
                        id: s.id.clone(),
                        label: s.name,
                        detail: format!(
                            "{}@{}:{}{}{}",
                            s.user,
                            s.host,
                            s.port,
                            agent_tag,
                            if self.remote.get(&s.id).is_some_and(|r| r.connected) {
                                " · на связи"
                            } else {
                                ""
                            }
                        ),
                    }
                })
                .collect(),
            Picker::Sessions => {
                let mut rows = self
                    .active
                    .as_ref()
                    .and_then(|h| self.remote.get(h))
                    .map(|r| r.sessions.clone())
                    .unwrap_or_default();
                rows.sort_by(|a, b| {
                    b.pinned
                        .total_cmp(&a.pinned)
                        .then_with(|| b.modified.total_cmp(&a.modified))
                });
                rows.into_iter()
                    .map(|s| Item {
                        id: s.id,
                        label: format!(
                            "{}{}{}",
                            if s.pinned > 0. { "◆ " } else { "" },
                            s.title,
                            if s.running {
                                " · работает"
                            } else {
                                ""
                            }
                        ),
                        detail: format!(
                            "{} · {}",
                            s.cwd,
                            relay_desktop::model::backend_short_label(&s.backend)
                        ),
                    })
                    .collect()
            }
            Picker::Backends => {
                let r = self.active.as_ref().and_then(|h| self.remote.get(h));
                relay_desktop::model::BACKENDS
                    .into_iter()
                    .map(|id| {
                        let status = if id == "api" {
                            "Свой OpenAI-совместимый URL и ключ (/api)".to_owned()
                        } else if let Some(rem) = r {
                            if rem.backends.is_object() {
                                if rem.backends[id]["installed"] == true {
                                    let ver = rem.backends[id]["version"].as_str().unwrap_or("");
                                    if ver.is_empty() {
                                        "Установлен на сервере".into()
                                    } else {
                                        format!("Установлен · {ver}")
                                    }
                                } else {
                                    "Не найден на сервере".into()
                                }
                            } else {
                                "CLI на сервере".into()
                            }
                        } else {
                            "CLI на сервере".into()
                        };
                        Item {
                            id: id.into(),
                            label: relay_desktop::model::backend_label(id).into(),
                            detail: status,
                        }
                    })
                    .collect()
            }
            Picker::Models => {
                let active_backend = self.chat().map(|c| c.backend.as_str()).unwrap_or("claude");
                let mut items = vec![Item {
                    id: "default".into(),
                    label: "По умолчанию".into(),
                    detail: "Выбор сервера / текущего профиля".into(),
                }];
                match active_backend {
                    "api" => {
                        if let Some(server) = self.server() {
                            for model in std::iter::once(&server.api.model)
                                .chain(server.api.models.iter())
                                .filter(|m| !m.is_empty())
                            {
                                if !items.iter().any(|i| i.id == *model) {
                                    items.push(Item {
                                        id: model.clone(),
                                        label: model.clone(),
                                        detail: "API".into(),
                                    });
                                }
                            }
                        }
                    }
                    "codex" | "agy" => {
                        let remote_models = self
                            .active
                            .as_ref()
                            .and_then(|h| self.remote.get(h))
                            .map(|r| {
                                if active_backend == "codex" {
                                    r.codex_models.clone()
                                } else {
                                    r.agy_models.clone()
                                }
                            })
                            .unwrap_or_default();
                        let list = if remote_models.is_empty() {
                            relay_desktop::model::default_backend_models(active_backend)
                        } else {
                            remote_models
                        };
                        for model in list {
                            if !items.iter().any(|i| i.id == model.value) {
                                items.push(Item {
                                    id: model.value.clone(),
                                    label: if model.display_name.is_empty() {
                                        model.value
                                    } else {
                                        model.display_name
                                    },
                                    detail: model.description,
                                });
                            }
                        }
                    }
                    _ => {
                        if let Some(r) = self.active.as_ref().and_then(|h| self.remote.get(h)) {
                            for model in &r.models {
                                if !items.iter().any(|i| i.id == model.value) {
                                    items.push(Item {
                                        id: model.value.clone(),
                                        label: if model.display_name.is_empty() {
                                            model.value.clone()
                                        } else {
                                            model.display_name.clone()
                                        },
                                        detail: model.description.clone(),
                                    });
                                }
                            }
                        }
                    }
                }
                items
            }
            Picker::Themes => ["graphite", "midnight", "paper"]
                .into_iter()
                .map(|s| Item {
                    id: s.into(),
                    label: match s {
                        "graphite" => "Графит · мята",
                        "midnight" => "Полночь · голубой",
                        _ => "Бумага · светлая",
                    }
                    .into(),
                    detail: String::new(),
                })
                .collect(),
            Picker::Modes => [
                ("manual", "С подтверждениями"),
                ("auto", "Авто"),
                ("acceptEdits", "Принимать правки"),
                ("plan", "Планирование"),
            ]
            .into_iter()
            .map(|(id, label)| Item {
                id: id.into(),
                label: label.into(),
                detail: id.into(),
            })
            .collect(),
            Picker::Efforts => ["default", "low", "medium", "high", "xhigh", "max"]
                .into_iter()
                .map(|s| Item {
                    id: s.into(),
                    label: s.into(),
                    detail: String::new(),
                })
                .collect(),
            Picker::Commands => {
                let mut items: Vec<_> = COMMANDS
                    .iter()
                    .map(|(c, d)| Item {
                        id: (*c).into(),
                        label: (*c).into(),
                        detail: (*d).into(),
                    })
                    .collect();
                if let Some(r) = self.active.as_ref().and_then(|h| self.remote.get(h)) {
                    let remote_cmds = match self.chat().map(|c| c.backend.as_str()).unwrap_or("claude") {
                        "codex" => &r.codex_commands,
                        "agy" => &r.agy_commands,
                        _ => &r.commands,
                    };
                    for c in remote_cmds {
                        let id = format!("/{}", c.name.trim_start_matches('/'));
                        if !items.iter().any(|i| i.id == id) {
                            items.push(Item {
                                id: id.clone(),
                                label: id,
                                detail: c.description.clone(),
                            });
                        }
                    }
                }
                items
            }
        };
        let query = query.to_lowercase();
        items
            .into_iter()
            .filter(|i| {
                format!("{} {} {}", i.id, i.label, i.detail)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect()
    }
    pub fn select_host(&mut self, id: String) {
        if !self.servers().iter().any(|s| s.id == id) {
            self.notice = "Сервер не найден".into();
            return;
        }
        self.active = Some(id.clone());
        self.store.selected = Some(id.clone());
        self.dirty = true;
        if let Some(sid) = self.selected.get(&id).cloned() {
            self.ensure_chat(&id, &sid, None);
        }
        if !self
            .remote
            .get(&id)
            .is_some_and(|r| r.connected || r.connecting)
        {
            self.connect();
        }
    }
    pub fn connect(&mut self) {
        let Some(mut server) = self.server() else {
            self.picker(Picker::Hosts);
            return;
        };
        if self.demo {
            self.notice = "Демонстрация: сеть и запросы к моделям отключены".into();
            return;
        }
        if self.remote.get(&server.id).is_some_and(|r| r.connecting) {
            return;
        }
        server.fingerprint = server
            .fingerprint
            .clone()
            .or_else(|| self.store.trusted.get(&trust_key(&server)).cloned());
        let r = self.remote.entry(server.id.clone()).or_default();
        r.connecting = true;
        r.connected = false;
        r.error.clear();
        for (key, c) in &mut self.chats {
            if key.starts_with(&format!("{}/", server.id)) {
                c.busy = false;
                c.paused = true;
                c.loading = false;
                c.permissions.clear();
                c.recovery = c.inflight.take().or(c.recovery.take());
            }
        }
        let bus = self.controller.bus.clone();
        self.controller.runtime.spawn_blocking(move || {
            let password = storage::password(&server.id);
            bus.send(Event::Password { server, password });
        });
    }
    fn connect_password(&mut self, server: Server, password: String) {
        self.controller.connect(server, password);
    }
    fn ensure_chat(&mut self, host: &str, sid: &str, session: Option<&Session>) {
        let key = chat_key(host, sid);
        if self.chats.contains_key(&key) {
            return;
        }
        let Some(server) = self.servers().into_iter().find(|s| s.id == host) else {
            return;
        };
        let backend = self.settings.session_backend(
            &server,
            &key,
            session.map(|s| s.backend.as_str()).filter(|b| !b.is_empty()),
        );
        let default_model = if backend == "api" {
            "default".into()
        } else {
            server.model_for(&backend).to_owned()
        };
        let saved = self.store.chats.remove(&key).unwrap_or_else(|| SavedChat {
            model: default_model,
            mode: server.mode.clone(),
            effort: server.effort.clone(),
            backend,
            ..Default::default()
        });
        let cwd = session
            .map(|s| s.cwd.clone())
            .or_else(|| self.store.cwd.get(host).cloned())
            .unwrap_or(server.cwd);
        let mut c = Chat::new(sid.into(), cwd, saved);
        if let Some(session) = session {
            c.cwd = session.cwd.clone();
            c.title = session.title.clone();
            c.busy = session.running;
        }
        if c.recovery.is_some() {
            self.notice="Осталось сообщение с неизвестным статусом. Проверь историю; /recover вернёт его в черновик.".into();
        }
        self.chats.insert(key, c);
    }
    pub fn new_chat(&mut self) {
        let Some(host) = self.active.clone() else {
            self.picker(Picker::Hosts);
            return;
        };
        let sid = uuid::Uuid::new_v4().to_string();
        self.ensure_chat(&host, &sid, None);
        if let Some(c) = self.chats.get_mut(&chat_key(&host, &sid)) {
            c.loaded = true;
            c.paused = false;
        }
        self.selected.insert(host.clone(), sid.clone());
        self.store.last.insert(host, sid);
        self.dirty = true;
    }
    pub fn open_session(&mut self, session: Session) {
        let Some(host) = self.active.clone() else {
            return;
        };
        self.ensure_chat(&host, &session.id, Some(&session));
        self.selected.insert(host.clone(), session.id.clone());
        self.store.last.insert(host.clone(), session.id.clone());
        self.dirty = true;
        let c = self.chats.get_mut(&chat_key(&host, &session.id)).unwrap();
        c.title = session.title;
        c.cwd = session.cwd;
        if !c.loaded && !c.loading && self.remote.get(&host).is_some_and(|r| r.connected) {
            c.loading = true;
            c.history_revision = c.revision;
            c.history_messages = c.messages.len();
            self.controller
                .send(&host, json!({"op":"history","sid":session.id}));
        }
    }
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        for _ in 0..256 {
            let Ok(event) = self.rx.try_recv() else {
                break;
            };
            changed = true;
            self.event(event);
        }
        let mut requests = vec![];
        for (host, r) in &mut self.remote {
            if !r.connected {
                continue;
            }
            if !r.loaded
                && r.requested
                    .is_some_and(|t| t.elapsed() > Duration::from_secs(12))
                && r.retries < 3
            {
                r.retries += 1;
                r.requested = Some(Instant::now());
                requests.push((host.clone(), json!({"op":"sessions"})));
            }
            if r.last_refresh
                .is_none_or(|t| t.elapsed() > Duration::from_secs(60))
            {
                r.last_refresh = Some(Instant::now());
                requests.push((host.clone(), json!({"op":"sessions"})));
                requests.push((host.clone(), json!({"op":"usage"})));
            }
        }
        for (host, v) in requests {
            self.controller.send(&host, v);
        }
        let ready: Vec<_> = self
            .chats
            .iter()
            .filter(|(_, c)| !c.busy && !c.loading && c.loaded && !c.paused && !c.queue.is_empty())
            .map(|(k, _)| k.clone())
            .collect();
        for key in ready {
            if let Some((host, _)) = key.split_once('/') {
                if self.remote.get(host).is_some_and(|r| r.connected) {
                    let prompt = self.chats.get_mut(&key).unwrap().queue.pop_front().unwrap();
                    self.dispatch(&key, prompt);
                    changed = true;
                }
            }
        }
        if self.popup.is_none() {
            if let Some((server, fingerprint, answer)) = self.pending_trust.pop_front() {
                self.popup = Some(Popup::Trust {
                    server,
                    fingerprint,
                    answer,
                });
                changed = true;
            } else if let Some((key, c)) =
                self.chats.iter().find(|(_, c)| !c.permissions.is_empty())
            {
                let p = c.permissions[0].clone();
                let (server, sid) = key.split_once('/').unwrap();
                let count = p.input["questions"].as_array().map_or(0, Vec::len);
                self.popup = Some(Popup::Permission {
                    server: server.into(),
                    sid: sid.into(),
                    permission: p,
                    answers: (0..count).map(|_| Editor::default()).collect(),
                    selected: 0,
                    error: String::new(),
                });
                changed = true;
            }
        }
        if self.dirty && self.last_save.elapsed() > Duration::from_secs(1) {
            if let Err(e) = self.persist() {
                self.notice = format!("Не удалось сохранить черновик: {e}");
            }
            self.last_save = Instant::now();
        }
        changed
    }
    pub fn persist(&mut self) -> Result<()> {
        if self.demo || self.read_only {
            return Ok(());
        }
        for (key, c) in &self.chats {
            self.store.chats.insert(key.clone(), c.saved());
        }
        self.store.selected = self.active.clone();
        self.store.save()?;
        self.dirty = false;
        Ok(())
    }
    fn event(&mut self, event: Event) {
        match event {
            Event::Epoch {
                server,
                generation,
                event,
            } => {
                if self.controller.is_current(&server, generation) {
                    self.event(*event)
                }
            }
            Event::Trust {
                server,
                fingerprint,
                answer,
            } => self.pending_trust.push_back((server, fingerprint, answer)),
            Event::Password { server, password } => {
                if !self.remote.get(&server.id).is_some_and(|r| r.connecting) {
                    return;
                }
                if let Some(password) = password {
                    self.connect_password(server, password);
                } else {
                    self.popup = Some(Popup::Password {
                        server,
                        editor: Editor::default(),
                    });
                }
            }
            Event::Data { server, value } => self.data(&server, value),
            Event::Error { server, message } => {
                self.notice = message.clone();
                let r = self.remote.entry(server).or_default();
                r.error = message;
            }
            Event::Disconnected { server } => {
                let r = self.remote.entry(server.clone()).or_default();
                r.connected = false;
                r.connecting = false;
                for (key, c) in &mut self.chats {
                    if key.starts_with(&format!("{server}/")) {
                        c.busy = false;
                        c.paused = true;
                        c.loading = false;
                        c.permissions.clear();
                        c.recovery = c.inflight.take().or(c.recovery.take());
                    }
                }
                self.pending_trust.retain(|(h, _, _)| h != &server);
                if matches!(&self.popup,Some(Popup::Permission{server:h,..}|Popup::Trust{server:h,..}) if h==&server)
                {
                    self.popup = None;
                }
                self.dirty = true;
            }
            _ => {}
        }
    }
    fn data(&mut self, host: &str, value: Value) {
        let kind = value["type"].as_str().unwrap_or("");
        let sid = value["sid"].as_str().unwrap_or("");
        let key = chat_key(host, sid);
        if self.removed.contains(&key) {
            return;
        }
        match kind {
            "hello" => {
                let r = self.remote.entry(host.into()).or_default();
                r.connected = true;
                r.connecting = false;
                r.error.clear();
                r.home = value["home"].as_str().unwrap_or("~").into();
                r.version = value["version"].as_str().unwrap_or("").into();
                r.backends = value["backends"].clone();
                r.loaded = false;
                r.requested = Some(Instant::now());
                r.retries = 0;
                r.last_refresh = Some(Instant::now());
                self.controller
                    .send(host, json!({"op":"_desktop_workspace","path":null}));
                self.controller
                    .send(host, json!({"op":"_tests_policy","mode":"off"}));
                self.controller.send(host, json!({"op":"sessions"}));
                self.controller.send(host, json!({"op":"usage"}));
                if let Some(b) = self
                    .selected
                    .get(host)
                    .and_then(|sid| self.chats.get(&chat_key(host, sid)))
                    .map(|c| c.backend.clone())
                    .filter(|b| matches!(b.as_str(), "codex" | "agy"))
                {
                    self.controller.send(host, json!({"op":"catalog","backend":b}));
                }
                // A disconnected draft is kept, but an existing conversation always reloads its server history.
                if let Some(sid) = self.selected.get(host).cloned() {
                    if let Some(c) = self.chats.get_mut(&chat_key(host, &sid)) {
                        c.loading = true;
                        c.history_revision = c.revision;
                        c.history_messages = c.messages.len();
                        self.controller
                            .send(host, json!({"op":"history","sid":sid}));
                    }
                }
            }
            "sessions" => {
                let Some(rows) = value["sessions"].as_array() else {
                    return;
                };
                let sessions: Vec<Session> = rows
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect();
                let r = self.remote.entry(host.into()).or_default();
                r.sessions = sessions.clone();
                r.loaded = true;
                r.requested = None;
                for s in &sessions {
                    if let Some(c) = self.chats.get_mut(&chat_key(host, &s.id)) {
                        c.title = s.title.clone();
                    }
                }
                if self.active.as_deref() == Some(host) && self.chat().is_none() {
                    let last = self
                        .selected
                        .get(host)
                        .cloned()
                        .or_else(|| self.server().and_then(|s| s.last_session));
                    if let Some(s) = sessions
                        .iter()
                        .find(|s| Some(&s.id) == last.as_ref())
                        .cloned()
                    {
                        self.open_session(s);
                    } else {
                        self.new_chat();
                    }
                }
            }
            "catalog" => {
                let r = self.remote.entry(host.into()).or_default();
                let models = serde_json::from_value(value["models"].clone()).unwrap_or_default();
                let commands = serde_json::from_value(value["commands"].clone()).unwrap_or_default();
                match value["backend"].as_str().unwrap_or("claude") {
                    "codex" => {
                        r.codex_models = models;
                        r.codex_commands = commands;
                    }
                    "agy" => {
                        r.agy_models = models;
                        r.agy_commands = commands;
                    }
                    _ => {
                        r.models = models;
                        r.commands = commands;
                    }
                }
            }
            "model_changed" => {
                if let Some(c) = self.chats.get_mut(&key) {
                    c.model = value["model"].as_str().unwrap_or("default").into();
                    self.dirty = true;
                }
            }
            "mode_changed" => {
                if let Some(c) = self.chats.get_mut(&key) {
                    c.mode = value["mode"].as_str().unwrap_or("manual").into();
                    self.dirty = true;
                }
            }
            "usage" => self.remote.entry(host.into()).or_default().usage = value.clone(),
            "history" => {
                if let Some(c) = self.chats.get_mut(&key) {
                    let messages: Vec<relay_desktop::model::Message> =
                        serde_json::from_value(value["messages"].clone()).unwrap_or_default();
                    if c.history_revision == c.revision {
                        c.messages = messages;
                        c.revision += 1;
                        c.blocks.clear();
                        c.user_at = c
                            .messages
                            .iter()
                            .rposition(|m| m.role == "user")
                            .unwrap_or(0);
                    } else if !c.busy {
                        c.loading = true;
                        c.history_revision = c.revision;
                        c.history_messages = c.messages.len();
                        self.controller
                            .send(host, json!({"op":"history","sid":sid}));
                        return;
                    } else {
                        c.merge_history(messages);
                    }
                    c.loading = false;
                    c.loaded = true;
                }
            }
            "run_state" => {
                if !sid.is_empty() {
                    self.ensure_chat(host, sid, None);
                }
                if let Some(c) = self.chats.get_mut(&key) {
                    c.busy = value["busy"] == true;
                    if c.busy {
                        if let Some(b) = value["backend"]
                            .as_str()
                            .filter(|b| relay_desktop::model::valid_backend(b))
                        {
                            c.backend = b.into();
                        }
                        c.status = "Работает на сервере".into();
                    } else {
                        c.status.clear();
                    }
                }
            }
            "claude" => {
                if !sid.is_empty() {
                    self.ensure_chat(host, sid, None);
                }
                if let Some(c) = self.chats.get_mut(&key) {
                    c.reduce(&value["data"]);
                    if value["data"]["type"] == "result" {
                        self.controller.send(host, json!({"op":"sessions"}));
                        if c.history_dirty {
                            c.history_dirty = false;
                            c.loading = true;
                            c.history_revision = c.revision;
                            c.history_messages = c.messages.len();
                            self.controller
                                .send(host, json!({"op":"history","sid":sid}));
                        }
                    }
                    self.dirty = true;
                }
            }
            "permission" => {
                self.ensure_chat(host, sid, None);
                if let Some(c) = self.chats.get_mut(&key) {
                    let id = value["id"].as_str().unwrap_or("");
                    if !c.permissions.iter().any(|p| p.id == id) {
                        c.permissions.push(Permission {
                            id: id.into(),
                            tool: value["tool"].as_str().unwrap_or("Действие").into(),
                            input: value["input"].clone(),
                        });
                    }
                }
            }
            "permission_cancelled" => {
                if let Some(c) = self.chats.get_mut(&key) {
                    c.permissions.retain(|p| value["id"] != p.id);
                }
                if matches!(&self.popup,Some(Popup::Permission{permission,..}) if value["id"]==permission.id)
                {
                    self.popup = None;
                }
            }
            "run_closed" => {
                if let Some(c) = self.chats.get_mut(&key) {
                    c.busy = false;
                    c.paused = true;
                    c.permissions.clear();
                    c.recovery = c.inflight.take().or(c.recovery.take());
                    c.status.clear();
                    c.error = value["detail"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Ответ остановлен. История сохранена.")
                        .into();
                    self.dirty = true;
                }
            }
            "upload_progress" => {
                if let Some(c) = self.chats.get_mut(&key) {
                    c.status = format!(
                        "Передача файлов · {} / {} МБ",
                        value["sent"].as_u64().unwrap_or(0) / 1024 / 1024,
                        value["total"].as_u64().unwrap_or(0) / 1024 / 1024
                    );
                }
            }
            "upload_complete" => {
                if let Some(c) = self.chats.get_mut(&key) {
                    c.status = "Думает над ответом".into();
                }
            }
            "error" | "fatal" => {
                let message = value["message"]
                    .as_str()
                    .unwrap_or("Ошибка сервера")
                    .to_owned();
                if let Some(c) = self.chats.get_mut(&key) {
                    c.error = message;
                    if value["op"] == "prompt" {
                        c.busy = false;
                        c.paused = true;
                        c.recovery = c.inflight.take().or(c.recovery.take());
                    }
                    if value["op"] == "history" {
                        c.loading = false;
                    }
                    self.dirty = true;
                } else {
                    self.notice = message;
                }
            }
            "notice" | "desktop_status" => {
                self.notice = value["message"].as_str().unwrap_or("").into()
            }
            "desktop_request" => {
                self.controller
                    .send(host, json!({"op":"_desktop_deny","id":value["id"]}));
                self.notice = "Локальный ПК не подключён в этой TUI-сессии".into();
            }
            "cluster_request" => {
                self.controller.send(host,json!({"op":"cluster_reply","id":value["id"],"sid":value["sid"],"run":value["run"],"result":{"ok":false,"error":"Управление кластером доступно в графическом Relay. Открой там сессию руководителя."}}));
            }
            "artifact_status" => {
                self.notice = if value["stage"] == "saved" {
                    format!(
                        "Файл сохранён: {}",
                        value["local_path"].as_str().unwrap_or("")
                    )
                } else {
                    format!(
                        "Файл · {} {}",
                        value["stage"].as_str().unwrap_or(""),
                        value["message"].as_str().unwrap_or("")
                    )
                }
            }
            "session_deleted" => {
                self.chats.remove(&key);
                self.store.chats.remove(&key);
                self.removed.insert(key);
                if let Some(r) = self.remote.get_mut(host) {
                    r.sessions.retain(|s| s.id != sid);
                }
                if self.selected.get(host).is_some_and(|s| s == sid) {
                    self.selected.remove(host);
                    self.store.last.remove(host);
                    if self.active.as_deref() == Some(host) {
                        self.new_chat();
                    }
                }
                self.dirty = true;
            }
            "folders" => {
                if self
                    .dir_request
                    .as_ref()
                    .is_some_and(|(h, n)| h == host && value["request"].as_u64() == Some(*n))
                {
                    self.dir_request = None;
                    let cwd = value["path"].as_str().unwrap_or("~").to_owned();
                    self.store.cwd.insert(host.into(), cwd);
                    if self.active.as_deref() == Some(host) {
                        self.new_chat();
                    }
                    self.dirty = true;
                }
            }
            _ => {}
        }
        if kind == "run_closed"
            || kind == "session_deleted"
            || kind == "claude" && value["data"]["type"] == "result"
            || kind == "error" && value["op"] == "prompt"
        {
            if matches!(&self.popup,Some(Popup::Permission{server,sid:s,..}) if server==host&&s==sid)
            {
                self.popup = None;
            }
        }
    }
    fn api_key_id(&self, host: &str) -> String {
        if self.store.api.contains_key(host) {
            format!("tui:{host}")
        } else {
            host.into()
        }
    }
    fn dispatch(&mut self, key: &str, prompt: Prompt) {
        let Some((host, _)) = key.split_once('/') else {
            return;
        };
        if self.demo {
            self.notice = "Демонстрация: сообщение не отправлено".into();
            self.chats.get_mut(key).unwrap().queue.push_front(prompt);
            self.chats.get_mut(key).unwrap().paused = true;
            return;
        }
        let result = (|| -> Result<Value> {
            anyhow::ensure!(
                self.remote.get(host).is_some_and(|r| r.connected),
                "Сначала подключись к серверу: /connect"
            );
            let c = &self.chats[key];
            let mut request = c.request(&prompt, &self.settings.actor());
            let files = prompt
                .files
                .iter()
                .map(|p| attachments::from_file(p))
                .collect::<Result<Vec<_>>>()?;
            attachments::validate(&files)?;
            request["files"] = json!(files
                .iter()
                .filter(|f| f.source["type"] == "file")
                .map(|f| f.source.clone())
                .collect::<Vec<_>>());
            request["images"] = json!(files
                .iter()
                .filter(|f| f.source["type"] != "file")
                .map(|f| f.source.clone())
                .collect::<Vec<_>>());
            if c.backend == "api" {
                let mut profile = self
                    .servers()
                    .into_iter()
                    .find(|s| s.id == host)
                    .context("Сервер не найден")?
                    .api;
                profile.validate()?;
                let secret = storage::api_key(&self.api_key_id(host))
                    .context("Нет API-ключа. Настрой его через /api.")?;
                request["backend"] = json!("api");
                request["api"] = profile.request(secret);
            }
            Ok(request)
        })();
        match result {
            Ok(request) => {
                if !self.controller.send(host, request) {
                    let c = self.chats.get_mut(key).unwrap();
                    c.queue.push_front(prompt);
                    c.paused = true;
                    c.error = "Соединение потеряно; сообщение оставлено в очереди".into();
                    return;
                }
                let c = self.chats.get_mut(key).unwrap();
                c.user_at = c.messages.len();
                c.revision += 1;
                c.blocks.clear();
                c.messages.push(relay_desktop::model::Message {
                    role: "user".into(),
                    text: if prompt.files.is_empty() {
                        prompt.text.clone()
                    } else {
                        format!(
                            "{}\n\n{}",
                            prompt.text,
                            prompt
                                .files
                                .iter()
                                .map(|p| format!(
                                    "▧ {}",
                                    p.file_name().unwrap_or_default().to_string_lossy()
                                ))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    },
                    ..Default::default()
                });
                c.inflight = Some(prompt);
                c.busy = true;
                c.paused = false;
                c.scroll = 0;
                c.error.clear();
                c.status = "Думает над ответом".into();
            }
            Err(error) => {
                let c = self.chats.get_mut(key).unwrap();
                c.queue.push_front(prompt);
                c.paused = true;
                c.error = error.to_string();
            }
        }
        self.dirty = true;
    }
    pub fn submit(&mut self) {
        let Some(key) = self.key() else {
            self.notice = "Выбери сервер: F2 или /add".into();
            return;
        };
        let text = self.chats[&key].editor.text.trim().to_owned();
        if text.starts_with('/') && !text.contains('\n') {
            self.chats.get_mut(&key).unwrap().editor.take();
            self.command(&text);
            self.dirty = true;
            return;
        }
        if text.is_empty() && self.chats[&key].files.is_empty() {
            return;
        }
        if self.chats[&key].loading {
            self.notice = "Сначала дождись загрузки истории".into();
            return;
        }
        let c = self.chats.get_mut(&key).unwrap();
        if c.queue.len() >= 32 {
            self.notice = "В очереди уже 32 сообщения".into();
            return;
        }
        c.editor.take();
        let prompt = Prompt {
            text: text.clone(),
            files: std::mem::take(&mut c.files),
        };
        self.history.push(text);
        if self.history.len() > 100 {
            self.history.remove(0);
        }
        self.history_at = None;
        if c.busy || !c.queue.is_empty() {
            c.queue.push_back(prompt);
            self.notice = format!("В очереди: {} · /queue", c.queue.len());
        } else {
            self.dispatch(&key, prompt);
        }
        self.dirty = true;
    }
    fn stop(&mut self) {
        let Some(host) = self.active.clone() else {
            return;
        };
        let Some(c) = self.chat_mut() else {
            return;
        };
        c.paused = true;
        let sid = c.sid.clone();
        self.controller.send(&host, json!({"op":"stop","sid":sid}));
        self.dirty = true;
    }
    fn ask_field(&mut self, kind: Field, text: String) {
        self.popup = Some(Popup::Field {
            kind,
            editor: Editor::new(text),
            error: String::new(),
        });
    }
    pub fn command(&mut self, line: &str) {
        let (command, arg) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let arg = arg.trim();
        match command {
            "/help"=>self.popup=Some(Popup::Help{scroll:0}),
            "/hosts"=>{if !self.demo{if let Ok(settings)=storage::load(){self.settings=settings;}}self.picker(Picker::Hosts);},
            "/connect"=>if arg.is_empty(){self.connect();}else{let servers=self.servers();let found:Vec<_>=servers.iter().filter(|s|s.id==arg||s.name.eq_ignore_ascii_case(arg)||s.host==arg).collect();if found.len()==1{self.select_host(found[0].id.clone());}else{self.notice="Имя не найдено или неоднозначно. Выбери сервер через F2.".into();}},
            "/disconnect"=>if let Some(host)=self.active.clone(){self.controller.disconnect(&host);self.event(Event::Disconnected{server:host});},
            "/add"=>self.ask_field(Field::Add{server:Server::default(),stage:0},String::new()),
            "/sessions"=>{self.refresh();self.popup=Some(Popup::Picker{kind:Picker::Sessions,search:Editor::new(arg.into()),index:0});},
            "/resume"=>if arg.is_empty(){self.ask_field(Field::Resume,String::new());}else if uuid::Uuid::parse_str(arg).is_err(){self.notice="Укажи UUID сессии".into();}else if let Some(session)=self.active.as_ref().and_then(|h|self.remote.get(h)).and_then(|r|r.sessions.iter().find(|s|s.id==arg)).cloned(){self.open_session(session);}else{self.notice="Сессия не найдена в списке. /refresh обновит список.".into();},
            "/new"=>self.new_chat(),
            "/cd"=>if arg.is_empty(){self.ask_field(Field::Cwd,self.chat().map(|c|c.cwd.clone()).unwrap_or_else(||"~".into()));}else{self.set_cwd(arg);},
            "/model"=>if arg.is_empty(){self.picker(Picker::Models);}else if arg.len()<=160&&!arg.chars().any(char::is_control){if let Some(c)=self.chat_mut(){c.model=arg.into();}self.control("model",arg);},
            "/mode"=>if arg.is_empty(){self.picker(Picker::Modes);}else if ["auto","manual","acceptEdits","plan"].contains(&arg){if let Some(c)=self.chat_mut(){c.mode=arg.into();}self.control("mode",arg);}else{self.notice="Режим: auto, manual, acceptEdits или plan".into();},
            "/effort"=>if arg.is_empty(){self.picker(Picker::Efforts);}else if ["default","low","medium","high","xhigh","max"].contains(&arg){if let Some(c)=self.chat_mut(){c.effort=arg.into();}self.notice="Глубина применится к следующему ответу".into();}else{self.notice="Глубина: default, low, medium, high, xhigh, max".into();},
            "/backend" => {
                if arg.is_empty() {
                    self.picker(Picker::Backends);
                } else if let Some(b) = relay_desktop::model::normalize_backend(arg) {
                    if self.chat().is_some_and(|c| c.busy) {
                        self.notice = "Дождись завершения ответа перед сменой агента".into();
                    } else {
                        let next_model = self
                            .server()
                            .map(|s| {
                                if b == "api" {
                                    "default".into()
                                } else {
                                    s.model_for(b).to_owned()
                                }
                            })
                            .unwrap_or_else(|| "default".into());
                        if let Some(c) = self.chat_mut() {
                            c.backend = b.into();
                            c.model = next_model;
                        }
                        if matches!(b, "codex" | "agy") {
                            if let Some(h) = &self.active {
                                self.controller.send(h, json!({"op":"catalog","backend":b}));
                            }
                        }
                        self.notice = format!(
                            "Агент сессии: {}",
                            relay_desktop::model::backend_label(b)
                        );
                    }
                } else {
                    self.notice =
                        "Укажи агент: /backend claude, /backend codex, /backend agy или /backend api"
                            .into();
                }
            }
            "/api"=>if let Some(s)=self.server(){self.ask_field(Field::Api{profile:s.api.clone(),stage:0},s.api.base_url);},
            "/attach"=>if arg.is_empty(){self.ask_field(Field::Attach,String::new());}else{if let Err(e)=self.attach(arg){self.notice=e.to_string();}},
            "/clear-files"=>if let Some(c)=self.chat_mut(){c.files.clear();},
            "/download"=>if arg.is_empty(){self.ask_field(Field::DownloadRemote,String::new());}else{self.download_field(arg.into());},
            "/stop"=>self.stop(),
            "/queue"=>if let Some(key)=self.key(){self.popup=Some(Popup::Queue{key,index:0});},
            "/pause"=>if let Some(c)=self.chat_mut(){c.paused=true;},
            "/continue"=>if self.connected(){if let Some(c)=self.chat_mut(){c.paused=false;c.error.clear();}},
            "/recover"=>if let Some(c)=self.chat_mut(){if c.editor.text.is_empty()&&c.files.is_empty(){if let Some(prompt)=c.recovery.take(){c.editor=Editor::new(prompt.text);c.files=prompt.files;self.notice="Проверь историю перед отправкой: сервер мог уже принять это сообщение".into();}}else{self.notice="Сначала освободи текущий черновик".into();}},
            "/rename"=>if arg.is_empty(){self.ask_field(Field::Rename,self.chat().map(|c|c.title.clone()).unwrap_or_default());}else{self.meta(json!({"title":arg}));},
            "/pin"=>{let pinned=self.active.as_ref().and_then(|h|self.remote.get(h)).and_then(|r|r.sessions.iter().find(|s|self.chat().is_some_and(|c|c.sid==s.id))).is_some_and(|s|s.pinned>0.);self.meta(json!({"pinned":!pinned}));},
            "/delete"=>if let (Some(server),Some(c))=(self.active.clone(),self.chat()) {if c.busy{self.notice="Сначала останови ответ: /stop".into();}else{self.popup=Some(Popup::Confirm{message:format!("Удалить сессию «{}» с сервера?",c.title),action:Confirmation::Delete{server,sid:c.sid.clone()}});}},
            "/prompt"=>if arg.is_empty(){self.ask_field(Field::Prompt,self.chat().map(|c|c.custom_prompt.clone()).unwrap_or_default());}else if let Some(c)=self.chat_mut(){c.custom_prompt=arg.into();},
            "/theme"=>if arg.is_empty(){self.picker(Picker::Themes);}else if ["graphite","midnight","paper"].contains(&arg){self.store.theme=arg.into();}else{self.notice="Темы: graphite, midnight, paper".into();},
            "/refresh"=>self.refresh(),
            "/login" => {
                let target = if arg.is_empty() {
                    self.chat().map(|c| c.backend.as_str()).unwrap_or("claude")
                } else {
                    relay_desktop::model::normalize_backend(arg).unwrap_or("claude")
                };
                self.notice = match target {
                    "codex" => "Вход в Codex: нажми «Войти в Codex» в графическом Relay или выполни `codex login --device-auth` (или `codex login --with-api-key`) по SSH на сервере.".into(),
                    "agy" => "Вход в AGY: нажми «Войти в AGY» в графическом Relay или выполни `agy acp --login` по SSH на сервере.".into(),
                    _ => "Вход общий с Relay: открой «Войти в Claude / Codex / AGY» в графическом Relay или выполни `claude auth login` (либо `codex login`, `agy acp --login`) по SSH на сервере.".into(),
                };
            }
            "/quit"|"/exit"=>self.request_quit(),
            _=>{
                if let Some(key)=self.key(){let prompt=Prompt{text:line.into(),files:vec![]};if self.chats[&key].busy{self.chats.get_mut(&key).unwrap().queue.push_back(prompt);}else{self.dispatch(&key,prompt);}}
                else{self.notice="Выбери сервер: F2".into();}
            },
        }
        self.dirty = true;
    }
    fn set_cwd(&mut self, cwd: &str) {
        if !self.connected() {
            self.notice = "Сначала подключись к серверу".into();
            return;
        }
        self.counter += 1;
        let host = self.active.clone().unwrap();
        self.dir_request = Some((host.clone(), self.counter));
        self.controller.send(
            &host,
            json!({"op":"folders","path":cwd,"request":self.counter}),
        );
    }
    fn control(&self, action: &str, value: &str) {
        if let (Some(h), Some(c)) = (&self.active, self.chat()) {
            if c.busy {
                self.controller.send(
                    h,
                    json!({"op":"control","sid":c.sid,"action":action,"value":value}),
                );
            }
        }
    }
    fn meta(&self, mut value: Value) {
        if let (Some(h), Some(c)) = (&self.active, self.chat()) {
            value["op"] = json!("session_meta");
            value["sid"] = json!(c.sid);
            value["actor"] = json!(self.settings.actor());
            self.controller.send(h, value);
        }
    }
    fn refresh(&self) {
        if let Some(host) = &self.active {
            self.controller.send(host, json!({"op":"sessions"}));
            self.controller.send(host, json!({"op":"usage"}));
        }
    }
    fn attach(&mut self, value: &str) -> Result<()> {
        let path = path(value).canonicalize().context("Файл не найден")?;
        let c = self.chat_mut().context("Сначала создай сессию")?;
        anyhow::ensure!(
            c.files.len() < attachments::MAX_ATTACHMENTS,
            "До восьми вложений."
        );
        anyhow::ensure!(path.is_file(), "Выбери обычный файл.");
        let size = std::fs::metadata(&path)?.len();
        let total = c
            .files
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum::<u64>();
        anyhow::ensure!(
            total + size <= attachments::MAX_ATTACHMENT_BYTES as u64,
            "Вложения в сообщении — до 512 МБ."
        );
        if !c.files.contains(&path) {
            c.files.push(path);
        }
        Ok(())
    }
    fn download_field(&mut self, remote: String) {
        let name =
            relay_desktop::desktop::file_name(remote.rsplit('/').next().unwrap_or("download.bin"));
        let target = dirs::download_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(name);
        self.ask_field(
            Field::DownloadLocal { remote },
            target.to_string_lossy().into(),
        );
    }
    fn download(&mut self, server: &str, remote: &str, target: PathBuf) {
        let cwd = self.chat().map(|c| c.cwd.as_str()).unwrap_or("~");
        if self.demo {
            self.notice = "Демонстрация: файл не скачивается".into();
            return;
        }
        if let Some(parent) = target.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.notice = format!("Не удалось открыть папку назначения: {e}");
                return;
            }
        }
        if self.controller.send(server,json!({"op":"_artifact_download","id":uuid::Uuid::new_v4().to_string(),"path":remote,"cwd":cwd,"target":target})){self.notice="Скачивание файла с сервера…".into();}else{self.notice="Сначала подключись к серверу".into();}
    }
    pub fn request_quit(&mut self) {
        if self.chats.values().any(|c| {
            c.busy || !c.queue.is_empty() || !c.editor.text.is_empty() || !c.files.is_empty()
        }) {
            self.popup=Some(Popup::Confirm{message:"Выйти? Черновики и очередь сохранятся. Текущая работа продолжится на сервере; очередь при следующем входе будет на паузе.".into(),action:Confirmation::Quit});
        } else {
            self.quit = true;
        }
    }
    pub fn input(&mut self, event: Input) {
        if matches!(
            event,
            Input::Key(KeyEvent {
                kind: KeyEventKind::Release,
                ..
            })
        ) {
            return;
        }
        if self.popup.is_some() {
            self.popup_input(event);
            return;
        }
        match event {
            Input::Paste(text) => {
                if let Some(c) = self.chat_mut() {
                    c.editor.insert(&text);
                }
                self.dirty = true;
            }
            Input::Key(key) => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::F(1) => self.popup = Some(Popup::Help { scroll: 0 }),
                    KeyCode::F(2) => self.command("/hosts"),
                    KeyCode::F(3) => self.command("/sessions"),
                    KeyCode::F(4) => self.picker(Picker::Themes),
                    KeyCode::Char('q') if ctrl => self.request_quit(),
                    KeyCode::Char('c') if ctrl => {
                        if self.chat().is_some_and(|c| c.busy) {
                            self.stop();
                        } else if self.chat().is_some_and(|c| !c.editor.text.is_empty()) {
                            if let Some(c) = self.chat_mut() {
                                c.editor.take();
                            }
                        } else {
                            self.request_quit();
                        }
                    }
                    KeyCode::Char('d')
                        if ctrl && self.chat().is_none_or(|c| c.editor.text.is_empty()) =>
                    {
                        self.request_quit()
                    }
                    KeyCode::Char('n') if ctrl => self.new_chat(),
                    KeyCode::Char('r') if ctrl => self.connect(),
                    KeyCode::Char('b') if ctrl => self.sidebar = !self.sidebar,
                    KeyCode::Char('o') if ctrl => {
                        if let Some(c) = self.chat_mut() {
                            c.expanded = !c.expanded;
                        }
                    }
                    KeyCode::Char('l') if ctrl => {
                        self.notice.clear();
                        if let Some(c) = self.chat_mut() {
                            c.scroll = 0;
                        }
                    }
                    KeyCode::Char('p') if ctrl => self.history_move(-1),
                    KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::ALT) => {
                        self.history_move(1)
                    }
                    KeyCode::PageUp => {
                        if let Some(c) = self.chat_mut() {
                            c.scroll = c.scroll.saturating_add(12);
                        }
                    }
                    KeyCode::PageDown => {
                        if let Some(c) = self.chat_mut() {
                            c.scroll = c.scroll.saturating_sub(12);
                        }
                    }
                    KeyCode::Home if ctrl => {
                        if let Some(c) = self.chat_mut() {
                            c.scroll = usize::MAX;
                        }
                    }
                    KeyCode::End if ctrl => {
                        if let Some(c) = self.chat_mut() {
                            c.scroll = 0;
                        }
                    }
                    KeyCode::Esc => {
                        self.notice.clear();
                        if let Some(c) = self.chat_mut() {
                            c.scroll = 0;
                        }
                    }
                    KeyCode::Tab => {
                        if let Some(c) = self.chat() {
                            let prefix = c.editor.text.clone();
                            if prefix.starts_with('/') && !prefix.contains(' ') {
                                let choices: Vec<_> = COMMANDS
                                    .iter()
                                    .filter(|(cmd, _)| cmd.starts_with(&prefix))
                                    .collect();
                                if choices.len() == 1 {
                                    if let Some(c) = self.chat_mut() {
                                        c.editor = Editor::new(format!("{} ", choices[0].0));
                                    }
                                } else {
                                    self.popup = Some(Popup::Picker {
                                        kind: Picker::Commands,
                                        search: Editor::new(prefix),
                                        index: 0,
                                    });
                                }
                            }
                        }
                    }
                    KeyCode::Enter
                        if key
                            .modifiers
                            .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
                    {
                        if let Some(c) = self.chat_mut() {
                            c.editor.insert("\n");
                        }
                    }
                    KeyCode::Char('j') if ctrl => {
                        if let Some(c) = self.chat_mut() {
                            c.editor.insert("\n");
                        }
                    }
                    KeyCode::Enter => self.submit(),
                    _ => {
                        if self.chat().is_none() {
                            if self.active.is_some() {
                                self.new_chat();
                            } else if let KeyCode::Char('/') = key.code {
                                self.picker(Picker::Commands);
                                return;
                            }
                        }
                        if let Some(c) = self.chat_mut() {
                            edit(&mut c.editor, key);
                        }
                    }
                }
                self.dirty = true;
            }
            Input::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    if let Some(c) = self.chat_mut() {
                        c.scroll = c.scroll.saturating_add(3);
                    }
                }
                MouseEventKind::ScrollDown => {
                    if let Some(c) = self.chat_mut() {
                        c.scroll = c.scroll.saturating_sub(3);
                    }
                }
                MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                    let position = ratatui::layout::Position::new(mouse.column, mouse.row);
                    if let Some(hit) = self
                        .hits
                        .iter()
                        .find(|(r, _)| r.contains(position))
                        .map(|(_, h)| h.clone())
                    {
                        match hit {
                            Hit::Host(id) => self.select_host(id),
                            Hit::Session(s) => self.open_session(s),
                            Hit::New => self.new_chat(),
                            Hit::Hosts => self.command("/hosts"),
                            Hit::Sessions => self.command("/sessions"),
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    fn history_move(&mut self, direction: i32) {
        if self.history.is_empty() {
            return;
        }
        let next = if direction < 0 {
            self.history_at
                .unwrap_or(self.history.len())
                .saturating_sub(1)
        } else {
            (self.history_at.unwrap_or(self.history.len()) + 1).min(self.history.len())
        };
        self.history_at = Some(next);
        let text = self.history.get(next).cloned().unwrap_or_default();
        if let Some(c) = self.chat_mut() {
            c.editor = Editor::new(text);
        }
    }
    fn choose(&mut self, kind: Picker, id: String) {
        match kind {
            Picker::Hosts => self.select_host(id),
            Picker::Sessions => {
                if let Some(s) = self
                    .active
                    .as_ref()
                    .and_then(|h| self.remote.get(h))
                    .and_then(|r| r.sessions.iter().find(|s| s.id == id))
                    .cloned()
                {
                    self.open_session(s);
                }
            }
            Picker::Models => self.command(&format!("/model {id}")),
            Picker::Backends => self.command(&format!("/backend {id}")),
            Picker::Themes => self.command(&format!("/theme {id}")),
            Picker::Modes => self.command(&format!("/mode {id}")),
            Picker::Efforts => self.command(&format!("/effort {id}")),
            Picker::Commands => {
                if let Some(c) = self.chat_mut() {
                    c.editor = Editor::new(format!("{id} "));
                } else {
                    self.command(&id);
                }
            }
        }
    }
    fn popup_input(&mut self, event: Input) {
        let Some(mut popup) = self.popup.take() else {
            return;
        };
        let mut keep = true;
        let key = if let Input::Key(key) = event {
            Some(key)
        } else {
            None
        };
        let ctrl = key.is_some_and(|k| k.modifiers.contains(KeyModifiers::CONTROL));
        let cancel =
            key.is_some_and(|k| k.code == KeyCode::Esc || (ctrl && k.code == KeyCode::Char('c')));
        match &mut popup {
            Popup::Picker {
                kind,
                search,
                index,
            } => {
                if cancel {
                    keep = false;
                } else if let Input::Paste(text) = event {
                    search.insert(&text.replace('\n', " "));
                    *index = 0;
                } else if key.is_some_and(|k| {
                    *kind == Picker::Hosts
                        && k.modifiers.contains(KeyModifiers::CONTROL)
                        && k.code == KeyCode::Char('a')
                }) {
                    self.ask_field(
                        Field::Add {
                            server: Server::default(),
                            stage: 0,
                        },
                        String::new(),
                    );
                    keep = false;
                } else if let Some(key) = key {
                    let items = self.items(*kind, &search.text);
                    match key.code {
                        KeyCode::Up => *index = index.saturating_sub(1),
                        KeyCode::Down => *index = (*index + 1).min(items.len().saturating_sub(1)),
                        KeyCode::Enter => {
                            if let Some(item) =
                                items.get((*index).min(items.len().saturating_sub(1)))
                            {
                                self.choose(*kind, item.id.clone());
                                keep = false;
                            }
                        }
                        _ => {
                            edit(search, key);
                            *index = 0;
                        }
                    }
                }
            }
            Popup::Password { server, editor } => {
                if cancel {
                    self.remote.entry(server.id.clone()).or_default().connecting = false;
                    keep = false;
                } else if let Input::Paste(text) = event {
                    editor.insert(&text.replace('\n', ""));
                } else if let Some(key) = key {
                    if key.code == KeyCode::Enter {
                        let password = editor.take();
                        self.connect_password(server.clone(), password);
                        keep = false;
                    } else {
                        edit(editor, key);
                    }
                }
            }
            Popup::Trust {
                server,
                fingerprint,
                ..
            } => {
                if cancel || (ctrl && key.is_some_and(|k| k.code == KeyCode::Char('n'))) {
                    keep = false;
                } else if ctrl && key.is_some_and(|k| k.code == KeyCode::Char('y')) {
                    if let Some(profile) = self.servers().into_iter().find(|s| s.id == *server) {
                        self.store
                            .trusted
                            .insert(trust_key(&profile), fingerprint.clone());
                        self.dirty = true;
                    }
                    let Popup::Trust { answer, .. } = popup else {
                        unreachable!()
                    };
                    let _ = answer.send(true);
                    return;
                }
            }
            Popup::Confirm { action, .. } => {
                if cancel || (ctrl && key.is_some_and(|k| k.code == KeyCode::Char('n'))) {
                    keep = false;
                } else if ctrl && key.is_some_and(|k| k.code == KeyCode::Char('y')) {
                    match action {
                        Confirmation::Quit => self.quit = true,
                        Confirmation::Delete { server, sid } => {
                            self.controller
                                .send(server, json!({"op":"delete_session","sid":sid}));
                        }
                        Confirmation::Download {
                            server,
                            remote,
                            target,
                        } => self.download(server, remote, target.clone()),
                    }
                    keep = false;
                }
            }
            Popup::Field {
                kind,
                editor,
                error,
            } => {
                if cancel {
                    keep = false;
                } else if let Input::Paste(text) = event {
                    editor.insert(&if matches!(kind, Field::Prompt) {
                        text
                    } else {
                        text.replace('\n', " ")
                    });
                } else if let Some(key) = key {
                    if key.code == KeyCode::Enter
                        && !(matches!(kind, Field::Prompt)
                            && key
                                .modifiers
                                .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT))
                    {
                        let value = zeroize::Zeroizing::new(editor.text.trim().to_owned());
                        match self.field_value(kind, &value) {
                            Ok(Some((next, text))) => {
                                self.ask_field(next, text);
                                keep = false;
                            }
                            Ok(None) => keep = false,
                            Err(e) => *error = e.to_string(),
                        }
                    } else if key.code == KeyCode::Enter || ctrl && key.code == KeyCode::Char('j') {
                        if matches!(kind, Field::Prompt) {
                            editor.insert("\n");
                        }
                    } else {
                        edit(editor, key);
                    }
                }
            }
            Popup::Permission {
                server,
                sid,
                permission,
                answers,
                selected,
                error,
            } => {
                if cancel || ctrl && key.is_some_and(|k| k.code == KeyCode::Char('n')) {
                    self.controller.send(
                        server,
                        json!({"op":"permission","sid":sid,"id":permission.id,"allow":false}),
                    );
                    self.remove_permission(server, sid, &permission.id);
                    keep = false;
                } else if ctrl && key.is_some_and(|k| k.code == KeyCode::Char('y')) {
                    if answers.iter().any(|a| a.text.trim().is_empty()) {
                        *error = "Заполни ответы на все вопросы; Tab переключает поля".into();
                    } else {
                        let mut result = serde_json::Map::new();
                        if let Some(questions) = permission.input["questions"].as_array() {
                            for (question, answer) in questions.iter().zip(answers.iter()) {
                                result.insert(
                                    question["question"].as_str().unwrap_or("").into(),
                                    json!(answer.text),
                                );
                            }
                        }
                        self.controller.send(server,json!({"op":"permission","sid":sid,"id":permission.id,"allow":true,"answers":result}));
                        self.remove_permission(server, sid, &permission.id);
                        keep = false;
                    }
                } else if !answers.is_empty() {
                    if let Input::Paste(text) = event {
                        answers[*selected].insert(&text.replace('\n', " "));
                    } else if let Some(key) = key {
                        match key.code {
                            KeyCode::Tab => *selected = (*selected + 1) % answers.len(),
                            KeyCode::BackTab => {
                                *selected = (*selected + answers.len() - 1) % answers.len()
                            }
                            _ => {
                                edit(&mut answers[*selected], key);
                            }
                        }
                    }
                }
            }
            Popup::Queue {
                key: chat_key,
                index,
            } => {
                if cancel {
                    keep = false;
                } else if let Some(key) = key {
                    if let Some(c) = self.chats.get_mut(chat_key) {
                        match key.code {
                            KeyCode::Up => *index = index.saturating_sub(1),
                            KeyCode::Down => {
                                *index = (*index + 1).min(c.queue.len().saturating_sub(1))
                            }
                            KeyCode::Char('d') | KeyCode::Delete => {
                                c.queue.remove(*index);
                                *index = (*index).min(c.queue.len().saturating_sub(1));
                            }
                            KeyCode::Char('p') => c.paused = true,
                            KeyCode::Char('r') => {
                                c.paused = false;
                                keep = false;
                            }
                            KeyCode::Char('e') => {
                                if c.editor.text.is_empty() && c.files.is_empty() {
                                    if let Some(prompt) = c.queue.remove(*index) {
                                        c.editor = Editor::new(prompt.text);
                                        c.files = prompt.files;
                                        keep = false;
                                    }
                                }
                            }
                            _ => {}
                        }
                        self.dirty = true;
                    }
                }
            }
            Popup::Help { scroll } => {
                if cancel || key.is_some_and(|k| k.code == KeyCode::F(1)) {
                    keep = false;
                } else if let Some(key) = key {
                    match key.code {
                        KeyCode::Down => *scroll += 1,
                        KeyCode::PageDown => *scroll += 10,
                        KeyCode::Up => *scroll = scroll.saturating_sub(1),
                        KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                        _ => {}
                    }
                }
            }
        }
        if keep && self.popup.is_none() {
            self.popup = Some(popup);
        }
    }
    fn remove_permission(&mut self, server: &str, sid: &str, id: &str) {
        if let Some(c) = self.chats.get_mut(&chat_key(server, sid)) {
            c.permissions.retain(|p| p.id != id);
        }
    }
    fn field_value(&mut self, field: &mut Field, value: &str) -> Result<Option<(Field, String)>> {
        match field {
            Field::Attach => self.attach(value)?,
            Field::Cwd => self.set_cwd(value),
            Field::Rename => {
                anyhow::ensure!(!value.is_empty(), "Введи название");
                self.meta(json!({"title":value}));
            }
            Field::Prompt => {
                anyhow::ensure!(value.len() <= 96000, "Инструкции — до 96 КБ");
                if let Some(c) = self.chat_mut() {
                    c.custom_prompt = value.into();
                }
            }
            Field::Resume => self.command(&format!("/resume {value}")),
            Field::DownloadRemote => {
                anyhow::ensure!(!value.is_empty(), "Укажи путь на сервере");
                self.download_field(value.into());
            }
            Field::DownloadLocal { remote } => {
                anyhow::ensure!(!value.is_empty(), "Укажи путь на компьютере");
                let target = path(value);
                let target = if target.is_absolute() {
                    target
                } else {
                    std::env::current_dir()?.join(target)
                };
                anyhow::ensure!(target.file_name().is_some(), "Укажи имя файла");
                let server = self.active.clone().context("Выбери сервер")?;
                if target.exists() {
                    self.popup = Some(Popup::Confirm {
                        message: format!("Заменить существующий файл {}?", target.display()),
                        action: Confirmation::Download {
                            server,
                            remote: remote.clone(),
                            target,
                        },
                    });
                } else {
                    self.download(&server, remote, target);
                }
            }
            Field::Add { server, stage } => {
                anyhow::ensure!(!value.is_empty(), "Заполни поле");
                match *stage {
                    0 => server.name = value.into(),
                    1 => server.host = value.into(),
                    2 => server.user = value.into(),
                    3 => {
                        server.port = value
                            .parse()
                            .context("Порт должен быть числом от 1 до 65535")?
                    }
                    _ => server.cwd = value.into(),
                }
                if *stage < 4 {
                    let next = *stage + 1;
                    let text = match next {
                        2 => "root",
                        3 => "22",
                        4 => "~",
                        _ => "",
                    }
                    .into();
                    return Ok(Some((
                        Field::Add {
                            server: server.clone(),
                            stage: next,
                        },
                        text,
                    )));
                }
                server.validate()?;
                self.store.servers.push(server.clone());
                self.select_host(server.id.clone());
            }
            Field::Api { profile, stage } => {
                anyhow::ensure!(!value.is_empty(), "Заполни поле");
                match *stage {
                    0 => {
                        profile.base_url = value.into();
                        return Ok(Some((
                            Field::Api {
                                profile: profile.clone(),
                                stage: 1,
                            },
                            profile.model.clone(),
                        )));
                    }
                    1 => {
                        profile.model = value.into();
                        profile.validate()?;
                        return Ok(Some((
                            Field::Api {
                                profile: profile.clone(),
                                stage: 2,
                            },
                            String::new(),
                        )));
                    }
                    _ => {
                        let host = self.active.clone().context("Выбери сервер")?;
                        if !self.demo {
                            storage::remember_api_key(&format!("tui:{host}"), value)?;
                        }
                        profile.enabled = true;
                        self.store.api.insert(host, profile.clone());
                        if let Some(c) = self.chat_mut() {
                            if !c.busy {
                                c.backend = "api".into();
                                c.model = "default".into();
                            }
                        }
                        self.notice =
                            "API настроен; ключ сохранён отдельно от состояния TUI".into();
                    }
                }
            }
        }
        self.dirty = true;
        Ok(None)
    }
    pub fn demo(&mut self) {
        let server = Server {
            id: "demo".into(),
            name: "Production".into(),
            host: "server.example".into(),
            cwd: "/opt/project".into(),
            ..Default::default()
        };
        self.settings.servers = vec![server];
        self.active = Some("demo".into());
        self.store.theme = "graphite".into();
        self.remote.insert("demo".into(),Remote{connected:true,loaded:true,home:"/root".into(),version:"Claude Code".into(),usage:json!({"available":true,"windows":[{"id":"five_hour","label":"5 часов","utilization":38.},{"id":"seven_day","label":"Неделя","utilization":64.}]}),sessions:vec![Session{id:"11111111-1111-4111-8111-111111111111".into(),title:"Оптимизация атласа".into(),cwd:"/opt/project".into(),pinned:1.,..Default::default()},Session{id:"22222222-2222-4222-8222-222222222222".into(),title:"Новый интерфейс лаунчера".into(),cwd:"/opt/launcher".into(),..Default::default()}],..Default::default()});
        let session = self.remote["demo"].sessions[0].clone();
        self.open_session(session);
        let c = self.chat_mut().unwrap();
        c.loaded = true;
        c.loading = false;
        c.paused = false;
        c.mode = "auto".into();
        c.model = "opus".into();
        c.effort = "high".into();
        c.messages=vec![relay_desktop::model::Message{role:"user".into(),text:"Проверь изменения в атласе и запусти тесты".into(),..Default::default()},relay_desktop::model::Message{role:"assistant".into(),text:"Проверю привязку карты и границы. Сначала посмотрю изменения, затем прогоню тесты.\n\n### Что проверяю\n1. Совпадение координат атласа и фракционной карты.\n2. Рельеф, острова и границы у побережья.\n3. Облака над восточной частью карты.".into(),..Default::default()},relay_desktop::model::Message{role:"tool".into(),tool:"Bash".into(),tool_id:"test".into(),input:json!({"command":"./gradlew test --no-daemon"}),..Default::default()},relay_desktop::model::Message{role:"tool_result".into(),tool_id:"test".into(),text:"AtlasCoordinatesTest > alignsBothMaps PASSED\nCoastlineTest > followsMapPixels PASSED\nFogRendererTest > coversEasternMountains PASSED\n\nBUILD SUCCESSFUL in 12s\n18 tests completed, 0 failed".into(),..Default::default()},relay_desktop::model::Message{role:"assistant".into(),text:"**Все 18 тестов прошли.** Смещение границ исправлено; облака закрывают край гор.\n\n| Проверка | Результат |\n| --- | --- |\n| Координаты | Совпадают |\n| Границы | По пикселям карты |\n| Облака | Плавный переход |\n\nИзменения в `AtlasRenderer.java`. Сервер перезапускать не потребовалось.".into(),..Default::default()}];
    }
}

pub fn edit(editor: &mut Editor, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Left => editor.left(),
        KeyCode::Right => editor.right(),
        KeyCode::Up => editor.vertical(-1),
        KeyCode::Down => editor.vertical(1),
        KeyCode::Home => editor.home(),
        KeyCode::End => editor.end(),
        KeyCode::Backspace if ctrl => editor.word_back(),
        KeyCode::Backspace => editor.backspace(),
        KeyCode::Delete => editor.delete(),
        KeyCode::Char('a') if ctrl => editor.home(),
        KeyCode::Char('e') if ctrl => editor.end(),
        KeyCode::Char('u') if ctrl => editor.kill_line(),
        KeyCode::Char('w') if ctrl => editor.word_back(),
        KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
            editor.insert(&c.to_string())
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn connected() -> (App, tokio::sync::mpsc::UnboundedReceiver<Value>) {
        let mut app = App::new(true).unwrap();
        app.demo();
        app.demo = false;
        app.read_only = true;
        app.remote.get_mut("demo").unwrap().last_refresh = Some(Instant::now());
        let outgoing = app.controller.mock_host("demo");
        app.chat_mut().unwrap().messages.clear();
        app.chat_mut().unwrap().revision = 0;
        (app, outgoing)
    }
    fn key(code: KeyCode, modifiers: KeyModifiers) -> Input {
        Input::Key(KeyEvent::new(code, modifiers))
    }
    #[test]
    fn adding_messages_during_work_keeps_the_run_and_sends_after_completion() {
        let (mut app, mut outgoing) = connected();
        app.chat_mut().unwrap().editor = Editor::new("Первая задача".into());
        app.submit();
        let first = outgoing.try_recv().unwrap();
        assert_eq!(first["op"], "prompt");
        assert_eq!(first["text"], "Первая задача");
        app.chat_mut().unwrap().editor = Editor::new("И ещё вторая".into());
        app.submit();
        assert!(outgoing.try_recv().is_err());
        assert_eq!(app.chat().unwrap().queue.len(), 1);
        let sid = app.chat().unwrap().sid.clone();
        app.data(
            "demo",
            json!({"type":"claude","sid":sid,"data":{"type":"result","result":"Готово"}}),
        );
        app.tick();
        let packets: Vec<_> = std::iter::from_fn(|| outgoing.try_recv().ok()).collect();
        let prompts: Vec<_> = packets.iter().filter(|p| p["op"] == "prompt").collect();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0]["text"], "И ещё вторая");
        assert_eq!(prompts[0]["sid"], first["sid"]);
        assert!(app.chat().unwrap().queue.is_empty());
    }
    #[test]
    fn interrupted_connections_preserve_drafts_and_do_not_replay_prompts() {
        let (mut app, mut outgoing) = connected();
        app.chat_mut().unwrap().editor = Editor::new("Отправлено".into());
        app.submit();
        outgoing.try_recv().unwrap();
        app.chat_mut().unwrap().editor = Editor::new("Черновик".into());
        app.event(Event::Disconnected {
            server: "demo".into(),
        });
        app.tick();
        let c = app.chat().unwrap();
        assert!(c.paused);
        assert!(!c.busy);
        assert_eq!(c.editor.text, "Черновик");
        assert_eq!(c.recovery.as_ref().unwrap().text, "Отправлено");
        assert!(outgoing.try_recv().is_err());
    }
    #[test]
    fn paste_cannot_run_a_slash_command_or_send_a_message() {
        let (mut app, mut outgoing) = connected();
        app.input(Input::Paste("/stop\nпроверь код".into()));
        assert!(outgoing.try_recv().is_err());
        assert_eq!(app.chat().unwrap().editor.text, "/stop\nпроверь код");
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        let packet = outgoing.try_recv().unwrap();
        assert_eq!(packet["op"], "prompt");
        assert_eq!(packet["text"], "/stop\nпроверь код");
        assert_eq!(packet["desktop_enabled"], false);
    }
    #[test]
    fn permissions_require_a_deliberate_shortcut_and_are_bound_to_the_exact_session() {
        let (mut app, mut outgoing) = connected();
        let sid = app.chat().unwrap().sid.clone();
        app.data("demo",json!({"type":"permission","sid":sid,"id":"permission-1","tool":"Bash","input":{"command":"pwd"}}));
        app.tick();
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        app.input(key(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(outgoing.try_recv().is_err());
        assert!(app.popup.is_some());
        app.input(key(KeyCode::Char('y'), KeyModifiers::CONTROL));
        let packet = outgoing.try_recv().unwrap();
        assert_eq!(packet["op"], "permission");
        assert_eq!(packet["sid"], sid);
        assert_eq!(packet["id"], "permission-1");
        assert_eq!(packet["allow"], true);
        assert!(app.chat().unwrap().permissions.is_empty());
    }
    #[test]
    fn questions_return_each_answer_under_its_own_question() {
        let (mut app, mut outgoing) = connected();
        let sid = app.chat().unwrap().sid.clone();
        app.data("demo",json!({"type":"permission","sid":sid,"id":"q","tool":"AskUserQuestion","input":{"questions":[{"question":"Где?"},{"question":"Когда?"}]}}));
        app.tick();
        app.input(Input::Paste("На сервере".into()));
        app.input(key(KeyCode::Tab, KeyModifiers::NONE));
        app.input(Input::Paste("Позже".into()));
        app.input(key(KeyCode::Char('y'), KeyModifiers::CONTROL));
        let packet = outgoing.try_recv().unwrap();
        assert_eq!(
            packet["answers"],
            json!({"Где?":"На сервере","Когда?":"Позже"})
        );
    }
    #[test]
    fn cancelling_key_trust_never_accepts_it() {
        let (mut app, _) = connected();
        let (answer, mut result) = tokio::sync::oneshot::channel();
        app.event(Event::Trust {
            server: "demo".into(),
            fingerprint: "SHA256:test".into(),
            answer,
        });
        app.tick();
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(result.try_recv().is_err());
        app.input(key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            result.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
        assert!(app.store.trusted.is_empty());
    }
    #[test]
    fn history_for_another_host_and_stale_generations_do_not_change_active_chat() {
        let (mut app, _) = connected();
        let sid = app.chat().unwrap().sid.clone();
        app.chat_mut().unwrap().editor = Editor::new("Живой черновик".into());
        app.event(Event::Epoch{server:"demo".into(),generation:999,event:Box::new(Event::Data{server:"demo".into(),value:json!({"type":"history","sid":sid,"messages":[{"role":"assistant","text":"stale"}]})})});
        app.data("other",json!({"type":"history","sid":sid,"messages":[{"role":"assistant","text":"wrong host"}]}));
        assert!(app.chat().unwrap().messages.is_empty());
        assert_eq!(app.chat().unwrap().editor.text, "Живой черновик");
    }
    #[test]
    fn tui_save_preserves_gui_settings_and_restricts_drafts() {
        let dir = std::env::temp_dir().join(format!("relay-tui-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let gui = dir.join("settings.json");
        std::fs::write(&gui, b"original-gui-config").unwrap();
        let mut store = Store::default();
        store.chats.insert(
            "host/session".into(),
            SavedChat {
                draft: "Важный черновик".into(),
                ..Default::default()
            },
        );
        store.save_to(&dir).unwrap();
        assert_eq!(std::fs::read(&gui).unwrap(), b"original-gui-config");
        let saved: Store =
            serde_json::from_slice(&std::fs::read(dir.join("tui-state.json")).unwrap()).unwrap();
        assert_eq!(saved.chats["host/session"].draft, "Важный черновик");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.join("tui-state.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn adding_a_host_is_available_without_any_graphical_profile() {
        let mut app = App::new(true).unwrap();
        app.picker(Picker::Hosts);
        app.input(key(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(matches!(
            app.popup,
            Some(Popup::Field {
                kind: Field::Add { stage: 0, .. },
                ..
            })
        ));
    }
    #[test]
    fn a_tui_api_override_never_replaces_the_desktop_providers_key() {
        let mut app = App::new(true).unwrap();
        app.demo();
        assert_eq!(app.api_key_id("demo"), "demo");
        app.store.api.insert(
            "demo".into(),
            ApiProfile {
                base_url: "https://second.example/v1".into(),
                model: "second-model".into(),
                ..Default::default()
            },
        );
        assert_eq!(app.api_key_id("demo"), "tui:demo");
        assert_eq!(app.api_key_id("other"), "other");
        assert!(app.settings.servers[0].api.base_url.is_empty());
        assert!(!serde_json::to_string(&app.store)
            .unwrap()
            .contains("api_key"));
    }

    #[test]
    fn tui_switches_between_codex_agy_claude_and_api_with_catalogs() {
        let (mut app, mut outgoing) = connected();
        app.command("/backend");
        assert!(matches!(
            app.popup,
            Some(Popup::Picker {
                kind: Picker::Backends,
                ..
            })
        ));
        app.popup = None;

        app.command("/backend codex");
        assert_eq!(app.chat().unwrap().backend, "codex");
        let cat_req = outgoing.try_recv().unwrap();
        assert_eq!(cat_req["op"], "catalog");
        assert_eq!(cat_req["backend"], "codex");

        app.data(
            "demo",
            json!({"type":"catalog","backend":"codex","models":[{"value":"gpt-5.4","displayName":"GPT-5.4","description":"Codex"}]}),
        );
        let items = app.items(Picker::Models, "");
        assert!(items.iter().any(|i| i.id == "gpt-5.4"));

        app.command("/model gpt-5.4");
        app.chat_mut().unwrap().editor = Editor::new("Run Codex check".into());
        app.submit();
        let prompt = outgoing.try_recv().unwrap();
        assert_eq!(prompt["op"], "prompt");
        assert_eq!(prompt["backend"], "codex");
        assert_eq!(prompt["model"], "gpt-5.4");

        let sid = app.chat().unwrap().sid.clone();
        app.data(
            "demo",
            json!({"type":"claude","sid":sid,"data":{"type":"result","result":"ok"}}),
        );
        let _ = outgoing.try_recv();

        app.command("/backend agy");
        assert_eq!(app.chat().unwrap().backend, "agy");
        let cat_agy = outgoing.try_recv().unwrap();
        assert_eq!(cat_agy["backend"], "agy");
        app.command("/login agy");
        assert!(app.notice.contains("agy acp --login"));
    }
}
