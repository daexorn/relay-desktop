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
    ("/menu", "Открыть панель серверов, сессий, агентов и лимитов"),
    ("/radar", "Живой радар агентов и топология сессий (F6)"),
    ("/agents", "Открыть радар активных агентов и субагентов (F6)"),
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
    ("/install", "Установить или обновить claude, codex, agy на сервере"),
    ("/api", "Настроить API для сервера"),
    ("/lock", "Защитить сессию паролем или закрыть (/lock change — сменить)"),
    ("/unlock", "Разблокировать сессию паролем"),
    ("/unlock-remove", "Снять парольную защиту с сессии"),
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
    ("/theme", "Выбрать тему оформления (24 темы)"),
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
    pub installing: BTreeSet<String>,
    pub usage: Value,
    pub home: String,
    pub version: String,
    pub error: String,
    requested: Option<Instant>,
    retries: u8,
    last_refresh: Option<Instant>,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Picker {
    Hosts,
    Sessions,
    Models,
    Backends,
    Install,
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
#[derive(Clone)]
pub enum WorkspaceAction {
    Host(String),
    AddHost,
    NewSession,
    LockSession,
    UnlockSession,
    RemoveLock,
    Session(Session),
    Agent(String),
    OpenInstall,
}
pub struct WorkspaceEntry {
    pub section: usize,
    pub label: String,
    pub detail: String,
    pub active: bool,
    pub action: WorkspaceAction,
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
    SessionLockSet {
        sid: String,
        has_existing: bool,
        stage: u8,
        current_pw: zeroize::Zeroizing<String>,
    },
    SessionUnlock {
        sid: String,
    },
    SessionLockRemove {
        sid: String,
    },
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
    Workspace {
        section: usize,
        index: usize,
    },
    Radar {
        cursor: usize,
    },
    Help {
        scroll: usize,
    },
}
#[derive(Clone)]
pub enum Hit {
    Hosts,
    Sessions,
    Workspace,
    Radar,
    WorkspaceTab(usize),
    WorkspaceItem(usize),
    RadarItem(usize),
}

pub struct RadarNode {
    pub sid: String,
    pub session: Option<Session>,
    pub title: String,
    pub backend: String,
    pub cwd: String,
    pub running: bool,
    pub locked_gate: bool,
    pub is_current: bool,
    pub loaded_in_mem: bool,
    pub telemetry: relay_desktop::model::TurnTelemetry,
    pub last_tool: String,
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
    pub fn active_backend_missing(&self) -> bool {
        let Some(c) = self.chat() else {
            return false;
        };
        if !matches!(c.backend.as_str(), "claude" | "codex" | "agy") {
            return false;
        }
        self.active
            .as_ref()
            .and_then(|h| self.remote.get(h))
            .is_some_and(|r| {
                r.connected
                    && r.backends.is_object()
                    && r.backends
                        .get(&c.backend)
                        .is_some_and(|b| b["installed"] == false)
            })
    }
    pub fn open_workspace(&mut self) {
        self.popup = Some(Popup::Workspace {
            section: 0,
            index: 0,
        });
    }
    pub fn open_radar(&mut self) {
        self.popup = Some(Popup::Radar { cursor: 0 });
    }
    pub fn active_agents_count(&self) -> usize {
        let Some(host) = self.active.as_deref() else {
            return 0;
        };
        let mut active_ids = BTreeSet::new();
        if let Some(r) = self.remote.get(host) {
            for s in &r.sessions {
                if s.running {
                    active_ids.insert(s.id.clone());
                }
            }
        }
        let prefix = format!("{host}/");
        for (key, c) in &self.chats {
            if let Some(sid) = key.strip_prefix(&prefix) {
                if c.busy {
                    active_ids.insert(sid.to_owned());
                }
            }
        }
        active_ids.len()
    }
    pub fn active_turn_telemetry(&self) -> Option<relay_desktop::model::TurnTelemetry> {
        let c = self.chat()?;
        if c.is_locked_gate() {
            return None;
        }
        let start = if c.user_at < c.messages.len()
            && c.messages.get(c.user_at).is_some_and(|m| m.role == "user")
        {
            c.user_at
        } else {
            c.messages
                .iter()
                .rposition(|m| m.role == "user")
                .unwrap_or(0)
        };
        Some(relay_desktop::model::analyze_turn(
            &c.messages,
            start,
            c.busy,
            &c.status,
        ))
    }
    pub fn radar_nodes(&self) -> Vec<RadarNode> {
        let Some(host) = self.active.as_deref() else {
            return Vec::new();
        };
        let server = self.server().unwrap_or_default();
        let current_sid = self.selected.get(host).map(String::as_str);
        let mut sessions = self
            .remote
            .get(host)
            .map(|r| r.sessions.clone())
            .unwrap_or_default();
        sessions.sort_by(|a, b| {
            let a_run =
                a.running || self.chats.get(&chat_key(host, &a.id)).is_some_and(|c| c.busy);
            let b_run =
                b.running || self.chats.get(&chat_key(host, &b.id)).is_some_and(|c| c.busy);
            b_run
                .cmp(&a_run)
                .then_with(|| (b.pinned > 0.).cmp(&(a.pinned > 0.)))
                .then_with(|| b.modified.total_cmp(&a.modified))
        });

        let mut nodes = Vec::new();
        let mut seen = BTreeSet::new();
        for s in sessions {
            seen.insert(s.id.clone());
            let key = chat_key(host, &s.id);
            let chat = self.chats.get(&key);
            let running = s.running || chat.is_some_and(|c| c.busy);
            let locked_gate = chat.map_or_else(|| s.is_locked_gate(), Chat::is_locked_gate);
            let backend = chat
                .map(|c| c.backend.clone())
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| {
                    self.settings.session_backend(
                        &server,
                        &key,
                        Some(s.backend.as_str()).filter(|b| !b.is_empty()),
                    )
                });
            let (telemetry, last_tool) = if locked_gate {
                (Default::default(), String::new())
            } else if let Some(c) = chat {
                let start = if c.user_at < c.messages.len()
                    && c.messages.get(c.user_at).is_some_and(|m| m.role == "user")
                {
                    c.user_at
                } else {
                    c.messages
                        .iter()
                        .rposition(|m| m.role == "user")
                        .unwrap_or(0)
                };
                let tel = relay_desktop::model::analyze_turn(
                    &c.messages,
                    start,
                    running,
                    &c.status,
                );
                let last = c
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "tool")
                    .map(|m| {
                        let prev = relay_desktop::model::tool_target_preview(m, 44);
                        if prev.is_empty() {
                            m.tool.clone()
                        } else {
                            format!("{} {}", m.tool, prev)
                        }
                    })
                    .unwrap_or_default();
                (tel, last)
            } else {
                let mut tel = relay_desktop::model::TurnTelemetry::default();
                if running {
                    tel.active_headline = "Выполняет задачу на сервере".into();
                }
                (tel, String::new())
            };
            nodes.push(RadarNode {
                sid: s.id.clone(),
                title: chat
                    .map(|c| c.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| s.title.clone()),
                cwd: chat
                    .map(|c| c.cwd.clone())
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| s.cwd.clone()),
                backend,
                running,
                locked_gate,
                is_current: current_sid == Some(s.id.as_str()),
                loaded_in_mem: chat.is_some_and(|c| c.loaded || !c.messages.is_empty()),
                telemetry,
                last_tool,
                session: Some(s),
            });
        }

        if let Some(sid) = current_sid {
            if !seen.contains(sid) {
                if let Some(c) = self.chats.get(&chat_key(host, sid)) {
                    let locked_gate = c.is_locked_gate();
                    let start = c
                        .messages
                        .iter()
                        .rposition(|m| m.role == "user")
                        .unwrap_or(0);
                    let telemetry = if locked_gate {
                        Default::default()
                    } else {
                        relay_desktop::model::analyze_turn(
                            &c.messages,
                            start,
                            c.busy,
                            &c.status,
                        )
                    };
                    let last_tool = if locked_gate {
                        String::new()
                    } else {
                        c.messages
                            .iter()
                            .rev()
                            .find(|m| m.role == "tool")
                            .map(|m| {
                                let prev = relay_desktop::model::tool_target_preview(m, 44);
                                if prev.is_empty() {
                                    m.tool.clone()
                                } else {
                                    format!("{} {}", m.tool, prev)
                                }
                            })
                            .unwrap_or_default()
                    };
                    let node = RadarNode {
                        sid: sid.to_owned(),
                        session: None,
                        title: c.title.clone(),
                        backend: c.backend.clone(),
                        cwd: c.cwd.clone(),
                        running: c.busy,
                        locked_gate,
                        is_current: true,
                        loaded_in_mem: true,
                        telemetry,
                        last_tool,
                    };
                    if c.busy {
                        nodes.insert(0, node);
                    } else {
                        nodes.push(node);
                    }
                }
            }
        }
        nodes
    }
    fn activate_radar(&mut self, index: usize) {
        let nodes = self.radar_nodes();
        let Some(node) = nodes.get(index) else {
            return;
        };
        if let Some(session) = node.session.clone() {
            self.open_session(session);
        } else if let Some(host) = self.active.clone() {
            self.selected.insert(host.clone(), node.sid.clone());
            self.store.last.insert(host, node.sid.clone());
            self.dirty = true;
        }
    }
    pub fn workspace_items(&self) -> Vec<WorkspaceEntry> {
        let mut out = vec![];
        for s in self.servers() {
            let is_active = self.active.as_deref() == Some(&s.id);
            let connected = self.remote.get(&s.id).is_some_and(|r| r.connected);
            let eff = s.effective_backend();
            out.push(WorkspaceEntry {
                section: 0,
                label: format!("{} {}", if connected { "●" } else { "○" }, s.name),
                detail: format!(
                    "{}@{}:{} · {}{}",
                    s.user,
                    s.host,
                    s.port,
                    relay_desktop::model::backend_short_label(eff),
                    if connected {
                        " · на связи"
                    } else {
                        " · не подключён"
                    }
                ),
                active: is_active,
                action: WorkspaceAction::Host(s.id),
            });
        }
        out.push(WorkspaceEntry {
            section: 0,
            label: "＋ Добавить сервер".into(),
            detail: "Ctrl+A · новый SSH-профиль".into(),
            active: false,
            action: WorkspaceAction::AddHost,
        });

        out.push(WorkspaceEntry {
            section: 1,
            label: "＋ Новая сессия".into(),
            detail: "Ctrl+N · чистый диалог на сервере".into(),
            active: false,
            action: WorkspaceAction::NewSession,
        });
        if let Some(c) = self.chat() {
            if c.locked && !c.unlocked {
                out.push(WorkspaceEntry {
                    section: 1,
                    label: "🔒 Разблокировать текущую сессию".into(),
                    detail: "/unlock · ввести пароль для открытия истории".into(),
                    active: false,
                    action: WorkspaceAction::UnlockSession,
                });
            } else if c.locked {
                out.push(WorkspaceEntry {
                    section: 1,
                    label: "🔓 Заблокировать сессию снова".into(),
                    detail: "/lock · скрыть историю до ввода пароля".into(),
                    active: false,
                    action: WorkspaceAction::LockSession,
                });
                out.push(WorkspaceEntry {
                    section: 1,
                    label: "🔓 Снять пароль с сессии".into(),
                    detail: "/unlock-remove · отключить парольную защиту".into(),
                    active: false,
                    action: WorkspaceAction::RemoveLock,
                });
            } else {
                out.push(WorkspaceEntry {
                    section: 1,
                    label: "🔓 Защитить сессию паролем".into(),
                    detail: "/lock · приватная сессия с PBKDF2-защитой".into(),
                    active: false,
                    action: WorkspaceAction::LockSession,
                });
            }
        }
        let mut sessions = self
            .active
            .as_ref()
            .and_then(|h| self.remote.get(h))
            .map(|r| r.sessions.clone())
            .unwrap_or_default();
        sessions.sort_by(|a, b| {
            b.pinned
                .total_cmp(&a.pinned)
                .then_with(|| b.modified.total_cmp(&a.modified))
        });
        for s in sessions {
            let is_current = self.chat().is_some_and(|c| c.sid == s.id);
            let lock_icon = if s.locked && !s.unlocked {
                "🔒 "
            } else if s.locked {
                "🔓 "
            } else {
                ""
            };
            let pin_icon = if s.pinned > 0. { "◆ " } else { "" };
            out.push(WorkspaceEntry {
                section: 1,
                label: format!(
                    "{}{}{}{}",
                    lock_icon,
                    pin_icon,
                    s.title,
                    if s.running { " · работает" } else { "" }
                ),
                detail: format!(
                    "{} · {}",
                    relay_desktop::model::backend_short_label(&s.backend),
                    s.cwd
                ),
                active: is_current,
                action: WorkspaceAction::Session(s),
            });
        }

        let active_backend = self
            .chat()
            .map(|c| c.backend.clone())
            .or_else(|| self.server().map(|s| s.effective_backend().to_owned()))
            .unwrap_or_else(|| "claude".into());
        let r = self.active.as_ref().and_then(|h| self.remote.get(h));
        for id in ["claude", "codex", "agy"] {
            let is_active = active_backend == id;
            let running = r.is_some_and(|rem| {
                rem.installing.contains(id) || rem.backends[id]["running"] == true
            });
            let detail = if running {
                "⟳ Устанавливается / обновляется на сервере…".into()
            } else if let Some(rem) = r.filter(|rem| rem.backends.is_object()) {
                if rem.backends[id]["installed"] == true {
                    let ver = rem.backends[id]["version"].as_str().unwrap_or("");
                    let v = if ver.is_empty() {
                        "установлен".into()
                    } else {
                        format!("установлен ({ver})")
                    };
                    if is_active {
                        format!("{v} · активен · Enter — обновить CLI")
                    } else {
                        format!("{v} · Enter — переключить агент")
                    }
                } else {
                    "не установлен на сервере · Enter — установить CLI".into()
                }
            } else if is_active {
                "активен в сессии".into()
            } else {
                "Enter — переключить агент сессии".into()
            };
            out.push(WorkspaceEntry {
                section: 2,
                label: relay_desktop::model::backend_label(id).into(),
                detail,
                active: is_active,
                action: WorkspaceAction::Agent(id.into()),
            });
        }
        out.push(WorkspaceEntry {
            section: 2,
            label: "Пользовательский API".into(),
            detail: if active_backend == "api" {
                "активен · Enter — настроить URL и ключ (/api)".into()
            } else {
                "Enter — переключить на API или настроить (/api)".into()
            },
            active: active_backend == "api",
            action: WorkspaceAction::Agent("api".into()),
        });
        out.push(WorkspaceEntry {
            section: 2,
            label: "⬇ Установить / обновить CLI на сервере".into(),
            detail: "/install · claude, codex, agy или все сразу".into(),
            active: false,
            action: WorkspaceAction::OpenInstall,
        });
        out
    }
    fn activate_workspace(&mut self, action: WorkspaceAction) {
        match action {
            WorkspaceAction::Host(id) => self.select_host(id),
            WorkspaceAction::AddHost => self.command("/add"),
            WorkspaceAction::NewSession => self.new_chat(),
            WorkspaceAction::LockSession => self.command("/lock"),
            WorkspaceAction::UnlockSession => self.command("/unlock"),
            WorkspaceAction::RemoveLock => self.command("/unlock-remove"),
            WorkspaceAction::Session(s) => self.open_session(s),
            WorkspaceAction::Agent(id) => {
                if id == "api" {
                    if self.server().is_some_and(|s| s.api.base_url.is_empty())
                        || self.chat().is_some_and(|c| c.backend == "api")
                    {
                        self.command("/api");
                    } else {
                        self.command("/backend api");
                    }
                } else {
                    let not_installed = self
                        .active
                        .as_ref()
                        .and_then(|h| self.remote.get(h))
                        .is_some_and(|r| {
                            r.backends.is_object() && r.backends[&id]["installed"] == false
                        });
                    let already_active = self.chat().is_some_and(|c| c.backend == id);
                    if not_installed || (already_active && self.connected()) {
                        self.command(&format!("/install {id}"));
                    } else {
                        self.command(&format!("/backend {id}"));
                    }
                }
            }
            WorkspaceAction::OpenInstall => self.picker(Picker::Install),
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
                    .map(|s| {
                        let lock_icon = if s.locked && !s.unlocked {
                            "🔒 "
                        } else if s.locked {
                            "🔓 "
                        } else {
                            ""
                        };
                        Item {
                            id: s.id,
                            label: format!(
                                "{}{}{}{}",
                                lock_icon,
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
                        }
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
                            if rem.installing.contains(id) || rem.backends[id]["running"] == true {
                                "Устанавливается на сервере…".into()
                            } else if rem.backends.is_object() {
                                if rem.backends[id]["installed"] == true {
                                    let ver = rem.backends[id]["version"].as_str().unwrap_or("");
                                    if ver.is_empty() {
                                        "Установлен на сервере".into()
                                    } else {
                                        format!("Установлен · {ver}")
                                    }
                                } else {
                                    "Не найден на сервере · /install".into()
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
            Picker::Install => {
                let r = self.active.as_ref().and_then(|h| self.remote.get(h));
                let mut rows: Vec<Item> = [
                    ("claude", "Claude Code"),
                    ("codex", "OpenAI Codex CLI"),
                    ("agy", "Antigravity CLI (agy)"),
                ]
                .into_iter()
                .map(|(id, label)| {
                    let detail = if let Some(rem) = r {
                        let running =
                            rem.installing.contains(id) || rem.backends[id]["running"] == true;
                        if running {
                            "⟳ Устанавливается / обновляется на сервере…".into()
                        } else if rem.backends.is_object() {
                            if rem.backends[id]["installed"] == true {
                                let ver = rem.backends[id]["version"].as_str().unwrap_or("");
                                if ver.is_empty() {
                                    "Установлен · Enter — обновить".into()
                                } else {
                                    format!("Установлен ({ver}) · Enter — обновить")
                                }
                            } else {
                                "Не установлен на сервере · Enter — установить".into()
                            }
                        } else {
                            "Enter — установить или обновить на сервере".into()
                        }
                    } else {
                        "Сначала подключись к серверу (/connect)".into()
                    };
                    Item {
                        id: id.into(),
                        label: label.into(),
                        detail,
                    }
                })
                .collect();
                let all_detail = if let Some(rem) = r {
                    if !rem.installing.is_empty() {
                        format!(
                            "В процессе: {}",
                            rem.installing.iter().cloned().collect::<Vec<_>>().join(", ")
                        )
                    } else if rem.backends.is_object() {
                        let missing: Vec<&str> = ["claude", "codex", "agy"]
                            .into_iter()
                            .filter(|a| rem.backends[*a]["installed"] != true)
                            .collect();
                        if missing.is_empty() {
                            "Все CLI установлены · Enter — обновить все".into()
                        } else {
                            format!("Не установлены: {} · Enter — установить", missing.join(", "))
                        }
                    } else {
                        "Установить недостающие CLI-агенты на сервере".into()
                    }
                } else {
                    "Установить все недостающие CLI-агенты на сервере".into()
                };
                rows.push(Item {
                    id: "all".into(),
                    label: "Установить все недостающие".into(),
                    detail: all_detail,
                });
                rows
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
            Picker::Themes => relay_desktop::model::THEME_SPECS
                .iter()
                .map(|&(id, label, is_light, _)| Item {
                    id: id.into(),
                    label: format!(
                        "{}{}",
                        label,
                        if self.theme() == id { " · активна" } else { "" }
                    ),
                    detail: format!(
                        "{} · {id}",
                        if is_light {
                            "Светлая тема"
                        } else {
                            "Тёмная тема"
                        }
                    ),
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
            c.locked = session.locked;
            c.unlocked = session.unlocked;
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
        let is_gate = session.is_locked_gate();
        let sid = session.id.clone();
        self.ensure_chat(&host, &sid, Some(&session));
        self.selected.insert(host.clone(), sid.clone());
        self.store.last.insert(host.clone(), sid.clone());
        self.dirty = true;
        let c = self.chats.get_mut(&chat_key(&host, &sid)).unwrap();
        c.title = session.title;
        c.cwd = session.cwd;
        c.locked = session.locked;
        c.unlocked = session.unlocked;
        if is_gate {
            c.locked = true;
            c.unlocked = false;
            c.loading = false;
            c.loaded = false;
            c.messages.clear();
            c.permissions.clear();
            self.ask_field(Field::SessionUnlock { sid }, String::new());
            return;
        }
        if !c.loaded && !c.loading && self.remote.get(&host).is_some_and(|r| r.connected) {
            c.loading = true;
            c.history_revision = c.revision;
            c.history_messages = c.messages.len();
            self.controller
                .send(&host, json!({"op":"history","sid":sid}));
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
            .filter(|(_, c)| {
                !c.busy
                    && !c.loading
                    && c.loaded
                    && !c.paused
                    && !c.is_locked_gate()
                    && !c.queue.is_empty()
            })
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
                r.installing.clear();
                for (key, c) in &mut self.chats {
                    if key.starts_with(&format!("{server}/")) {
                        c.busy = false;
                        c.paused = true;
                        c.loading = false;
                        c.permissions.clear();
                        c.recovery = c.inflight.take().or(c.recovery.take());
                        if c.locked {
                            c.unlocked = false;
                            c.loaded = false;
                            c.messages.clear();
                        }
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
                        c.locked = s.locked;
                        c.unlocked = s.unlocked;
                        if c.is_locked_gate() {
                            c.messages.clear();
                            c.permissions.clear();
                            c.loading = false;
                            c.loaded = false;
                        }
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
                let busy = value["busy"] == true;
                if let Some(r) = self.remote.get_mut(host) {
                    if let Some(s) = r.sessions.iter_mut().find(|s| s.id == sid) {
                        s.running = busy;
                    }
                }
                if let Some(c) = self.chats.get_mut(&key) {
                    c.busy = busy;
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
                        if let Some(r) = self.remote.get_mut(host) {
                            if let Some(s) = r.sessions.iter_mut().find(|s| s.id == sid) {
                                s.running = false;
                            }
                        }
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
                if let Some(r) = self.remote.get_mut(host) {
                    if let Some(s) = r.sessions.iter_mut().find(|s| s.id == sid) {
                        s.running = false;
                    }
                }
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
            "session_locked" => {
                let msg = value["message"]
                    .as_str()
                    .unwrap_or("Сессия защищена паролем. Введи пароль, чтобы открыть историю.")
                    .to_owned();
                if let Some(r) = self.remote.get_mut(host) {
                    if let Some(s) = r.sessions.iter_mut().find(|s| s.id == sid) {
                        s.locked = true;
                        s.unlocked = false;
                    }
                }
                if let Some(c) = self.chats.get_mut(&key) {
                    c.locked = true;
                    c.unlocked = false;
                    c.loading = false;
                    c.loaded = false;
                    c.busy = false;
                    c.messages.clear();
                    c.permissions.clear();
                }
                self.notice = msg;
                if self.popup.is_none()
                    && self.active.as_deref() == Some(host)
                    && self.selected.get(host).is_some_and(|s| s == sid)
                {
                    self.ask_field(Field::SessionUnlock { sid: sid.into() }, String::new());
                }
            }
            "session_unlock_result" => {
                if value["ok"] == true {
                    if let Some(r) = self.remote.get_mut(host) {
                        if let Some(s) = r.sessions.iter_mut().find(|s| s.id == sid) {
                            s.locked = true;
                            s.unlocked = true;
                        }
                    }
                    if let Some(c) = self.chats.get_mut(&key) {
                        c.locked = true;
                        c.unlocked = true;
                        c.error.clear();
                        c.loading = true;
                        c.history_revision = c.revision;
                        c.history_messages = c.messages.len();
                    }
                    if matches!(
                        &self.popup,
                        Some(Popup::Field {
                            kind: Field::SessionUnlock { sid: s },
                            ..
                        }) if s == sid
                    ) {
                        self.popup = None;
                    }
                    self.notice = "Сессия разблокирована".into();
                    self.controller
                        .send(host, json!({"op":"history","sid":sid}));
                } else {
                    let msg = value["message"]
                        .as_str()
                        .unwrap_or("Неверный пароль сессии.")
                        .to_owned();
                    if let Some(c) = self.chats.get_mut(&key) {
                        c.unlocked = false;
                        c.loading = false;
                        c.messages.clear();
                    }
                    if self.active.as_deref() == Some(host)
                        && self.selected.get(host).is_some_and(|s| s == sid)
                    {
                        self.popup = Some(Popup::Field {
                            kind: Field::SessionUnlock { sid: sid.into() },
                            editor: Editor::default(),
                            error: msg.clone(),
                        });
                    }
                    self.notice = msg;
                }
            }
            "session_lock_changed" => {
                let locked = value["locked"] == true;
                let unlocked = if value["unlocked"].is_boolean() {
                    value["unlocked"] == true
                } else {
                    !locked
                };
                if let Some(r) = self.remote.get_mut(host) {
                    if let Some(s) = r.sessions.iter_mut().find(|s| s.id == sid) {
                        s.locked = locked;
                        s.unlocked = unlocked;
                    }
                }
                if let Some(c) = self.chats.get_mut(&key) {
                    c.locked = locked;
                    c.unlocked = unlocked;
                    if locked && !unlocked {
                        c.messages.clear();
                        c.permissions.clear();
                        c.loading = false;
                        c.loaded = false;
                        c.busy = false;
                    }
                }
                self.notice = if locked && !unlocked {
                    "Сессия заблокирована паролем".into()
                } else if locked {
                    "Пароль сессии установлен".into()
                } else {
                    "Пароль сессии снят".into()
                };
                self.dirty = true;
            }
            "agent_install" => {
                let agent = value["agent"].as_str().unwrap_or("");
                let running = value["running"] == true;
                let r = self.remote.entry(host.into()).or_default();
                if !agent.is_empty() {
                    if running {
                        r.installing.insert(agent.into());
                    } else {
                        r.installing.remove(agent);
                    }
                    if r.backends.is_object() {
                        if let Some(obj) = r.backends.get_mut(agent).and_then(Value::as_object_mut)
                        {
                            obj.insert("running".into(), json!(running));
                            if let Some(inst) = value["installed"].as_bool() {
                                obj.insert("installed".into(), json!(inst));
                            }
                            if let Some(ver) = value["version"].as_str().filter(|s| !s.is_empty()) {
                                obj.insert("version".into(), json!(ver));
                            }
                        }
                    }
                }
                if let Some(msg) = value["message"].as_str().filter(|s| !s.is_empty()) {
                    self.notice = msg.into();
                }
                if !running && value["ok"] == true && matches!(agent, "claude" | "codex" | "agy") {
                    self.controller
                        .send(host, json!({"op":"catalog","backend":agent}));
                }
            }
            "backends_status" => {
                let r = self.remote.entry(host.into()).or_default();
                r.backends = value["backends"].clone();
                for agent in ["claude", "codex", "agy"] {
                    if r.backends[agent]["running"] == true {
                        r.installing.insert(agent.into());
                    } else if r.backends[agent].is_object() {
                        r.installing.remove(agent);
                    }
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
        let locked_gate = self.chats[&key].is_locked_gate();
        let sid = self.chats[&key].sid.clone();
        let text = self.chats[&key].editor.text.trim().to_owned();
        if text.starts_with('/') && !text.contains('\n') {
            let (cmd, _) = text.split_once(char::is_whitespace).unwrap_or((&text, ""));
            if locked_gate
                && !matches!(
                    cmd,
                    "/unlock"
                        | "/hosts"
                        | "/sessions"
                        | "/new"
                        | "/help"
                        | "/quit"
                        | "/exit"
                        | "/menu"
                        | "/panel"
                        | "/radar"
                        | "/agents"
                        | "/theme"
                        | "/connect"
                        | "/disconnect"
                        | "/add"
                        | "/refresh"
                        | "/install"
                )
            {
                self.chats.get_mut(&key).unwrap().editor.take();
                self.notice = "Сессия защищена паролем. Введи пароль через /unlock.".into();
                self.ask_field(Field::SessionUnlock { sid }, String::new());
                self.dirty = true;
                return;
            }
            self.chats.get_mut(&key).unwrap().editor.take();
            self.command(&text);
            self.dirty = true;
            return;
        }
        if locked_gate {
            self.notice = "Сессия защищена паролем. Введи пароль для доступа к истории.".into();
            self.ask_field(Field::SessionUnlock { sid }, String::new());
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
            "/menu"|"/panel"=>self.open_workspace(),
            "/radar"|"/agents"=>self.open_radar(),
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
            "/install" => {
                if arg.is_empty() {
                    self.picker(Picker::Install);
                } else {
                    let target = if arg.eq_ignore_ascii_case("all") {
                        Some("all")
                    } else {
                        relay_desktop::model::normalize_backend(arg)
                            .filter(|b| matches!(*b, "claude" | "codex" | "agy"))
                    };
                    let Some(target) = target else {
                        self.notice = "Укажи CLI: /install claude, /install codex, /install agy или /install all".into();
                        self.dirty = true;
                        return;
                    };
                    let Some(host) = self.active.clone() else {
                        self.notice = "Сначала выбери сервер: F2".into();
                        return;
                    };
                    if self.demo {
                        self.notice = "Демонстрация: установка CLI на сервер отключена".into();
                        return;
                    }
                    if !self.connected() {
                        self.notice = "Сначала подключись к серверу: /connect".into();
                        return;
                    }
                    let installed = self
                        .remote
                        .get(&host)
                        .is_some_and(|r| {
                            if target == "all" {
                                r.backends.is_object()
                                    && ["claude", "codex", "agy"]
                                        .iter()
                                        .all(|a| r.backends[*a]["installed"] == true)
                            } else {
                                r.backends[target]["installed"] == true
                            }
                        });
                    let action = if installed { "update" } else { "install" };
                    let r = self.remote.entry(host.clone()).or_default();
                    if target == "all" {
                        let missing: Vec<&str> = ["claude", "codex", "agy"]
                            .into_iter()
                            .filter(|a| r.backends[*a]["installed"] != true)
                            .collect();
                        for a in if missing.is_empty() {
                            vec!["claude", "codex", "agy"]
                        } else {
                            missing
                        } {
                            r.installing.insert(a.into());
                        }
                    } else {
                        r.installing.insert(target.into());
                    }
                    self.controller.send(
                        &host,
                        json!({"op":"agent_install","agent":target,"action":action}),
                    );
                    self.notice = format!(
                        "{} {} на сервере…",
                        if action == "update" {
                            "Обновление"
                        } else {
                            "Установка"
                        },
                        if target == "all" {
                            "CLI-агентов"
                        } else {
                            relay_desktop::model::backend_label(target)
                        }
                    );
                }
            }
            "/lock" => {
                let Some(c) = self.chat() else {
                    self.notice = "Сначала открой или создай сессию".into();
                    return;
                };
                let sid = c.sid.clone();
                let locked = c.locked;
                let unlocked = c.unlocked;
                if locked && unlocked && arg.is_empty() {
                    if let Some(c) = self.chat_mut() {
                        c.unlocked = false;
                        c.loaded = false;
                        c.loading = false;
                        c.messages.clear();
                        c.permissions.clear();
                    }
                    if let Some(h) = self.active.clone() {
                        if let Some(r) = self.remote.get_mut(&h) {
                            if let Some(s) = r.sessions.iter_mut().find(|s| s.id == sid) {
                                s.unlocked = false;
                            }
                        }
                        self.controller
                            .send(&h, json!({"op":"session_relock","sid":sid}));
                    }
                    self.notice = "Сессия заблокирована".into();
                } else if !locked || arg == "change" {
                    if locked && !unlocked {
                        self.notice = "Сначала разблокируй сессию через /unlock".into();
                        self.ask_field(Field::SessionUnlock { sid }, String::new());
                    } else {
                        self.ask_field(
                            Field::SessionLockSet {
                                sid,
                                has_existing: locked,
                                stage: 0,
                                current_pw: zeroize::Zeroizing::new(String::new()),
                            },
                            String::new(),
                        );
                    }
                } else if locked && !unlocked {
                    self.notice = "Сессия уже закрыта паролем. Введи /unlock.".into();
                    self.ask_field(Field::SessionUnlock { sid }, String::new());
                } else {
                    self.notice =
                        "Используй /lock для закрытия сессии или /lock change для смены пароля."
                            .into();
                }
            }
            "/unlock" => {
                if let Some(c) = self.chat() {
                    let sid = c.sid.clone();
                    self.ask_field(Field::SessionUnlock { sid }, String::new());
                } else {
                    self.notice = "Сначала выбери сессию: F3".into();
                }
            }
            "/unlock-remove" => {
                if let Some(c) = self.chat() {
                    let sid = c.sid.clone();
                    self.ask_field(Field::SessionLockRemove { sid }, String::new());
                } else {
                    self.notice = "Сначала выбери сессию: F3".into();
                }
            }
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
            "/theme" => {
                if arg.is_empty() {
                    self.picker(Picker::Themes);
                } else if relay_desktop::model::valid_theme(arg) {
                    self.store.theme = arg.into();
                    let &(_, label, _, _) = relay_desktop::model::theme_spec(arg);
                    self.notice = format!("Тема оформления: {label} ({arg})");
                } else {
                    self.notice =
                        "Неизвестная тема. Нажми F4 или введи /theme для выбора из 24 тем.".into();
                }
            }
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
                    KeyCode::F(5) => self.open_workspace(),
                    KeyCode::F(6) => self.open_radar(),
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
                    KeyCode::Char('b') if ctrl => self.open_workspace(),
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
                            Hit::Hosts => self.command("/hosts"),
                            Hit::Sessions => self.command("/sessions"),
                            Hit::Workspace => self.open_workspace(),
                            Hit::Radar => self.open_radar(),
                            Hit::WorkspaceTab(_) | Hit::WorkspaceItem(_) | Hit::RadarItem(_) => {}
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
            Picker::Install => self.command(&format!("/install {id}")),
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
                    let is_secret = matches!(
                        kind,
                        Field::Api { stage: 2, .. }
                            | Field::SessionLockSet { .. }
                            | Field::SessionUnlock { .. }
                            | Field::SessionLockRemove { .. }
                    );
                    editor.insert(&if matches!(kind, Field::Prompt) {
                        text
                    } else if is_secret {
                        text.replace('\n', "")
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
                        let is_lock_pw = matches!(
                            kind,
                            Field::SessionLockSet { .. }
                                | Field::SessionUnlock { .. }
                                | Field::SessionLockRemove { .. }
                        );
                        let raw = if is_lock_pw {
                            editor.take()
                        } else {
                            editor.text.trim().to_owned()
                        };
                        let value = zeroize::Zeroizing::new(raw);
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
            Popup::Workspace { section, index } => {
                let items = self.workspace_items();
                if cancel
                    || key.is_some_and(|k| {
                        k.code == KeyCode::F(5) || (ctrl && k.code == KeyCode::Char('b'))
                    })
                {
                    keep = false;
                } else if let Input::Mouse(mouse) = event {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            *index = index.saturating_sub(1);
                            if let Some(it) = items.get(*index) {
                                *section = it.section;
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            *index = (*index + 1).min(items.len().saturating_sub(1));
                            if let Some(it) = items.get(*index) {
                                *section = it.section;
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
                                    Hit::Workspace => keep = false,
                                    Hit::WorkspaceTab(sec) => {
                                        if let Some(pos) =
                                            items.iter().position(|it| it.section == sec)
                                        {
                                            *section = sec;
                                            *index = pos;
                                        }
                                    }
                                    Hit::WorkspaceItem(idx) => {
                                        if let Some(item) = items.get(idx) {
                                            self.activate_workspace(item.action.clone());
                                            keep = false;
                                        }
                                    }
                                    Hit::Hosts => {
                                        self.command("/hosts");
                                        keep = false;
                                    }
                                    Hit::Sessions => {
                                        self.command("/sessions");
                                        keep = false;
                                    }
                                    Hit::Radar => {
                                        self.open_radar();
                                        keep = false;
                                    }
                                    Hit::RadarItem(_) => {}
                                }
                            }
                        }
                        _ => {}
                    }
                } else if let Some(key) = key {
                    if key.code == KeyCode::F(6) {
                        self.open_radar();
                        keep = false;
                    } else if ctrl && key.code == KeyCode::Char('a') {
                        self.command("/add");
                        keep = false;
                    } else if ctrl && key.code == KeyCode::Char('n') {
                        self.new_chat();
                        keep = false;
                    } else {
                        match key.code {
                            KeyCode::Up => {
                                *index = index.saturating_sub(1);
                                if let Some(it) = items.get(*index) {
                                    *section = it.section;
                                }
                            }
                            KeyCode::Down => {
                                *index = (*index + 1).min(items.len().saturating_sub(1));
                                if let Some(it) = items.get(*index) {
                                    *section = it.section;
                                }
                            }
                            KeyCode::Tab => {
                                let next_sec = (*section + 1) % 3;
                                if let Some(pos) =
                                    items.iter().position(|it| it.section == next_sec)
                                {
                                    *section = next_sec;
                                    *index = pos;
                                }
                            }
                            KeyCode::BackTab => {
                                let prev_sec = (*section + 2) % 3;
                                if let Some(pos) =
                                    items.iter().position(|it| it.section == prev_sec)
                                {
                                    *section = prev_sec;
                                    *index = pos;
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(item) =
                                    items.get((*index).min(items.len().saturating_sub(1)))
                                {
                                    self.activate_workspace(item.action.clone());
                                    keep = false;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Popup::Radar { cursor } => {
                let nodes = self.radar_nodes();
                if cancel || key.is_some_and(|k| k.code == KeyCode::F(6)) {
                    keep = false;
                } else if let Input::Mouse(mouse) = event {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => *cursor = cursor.saturating_sub(1),
                        MouseEventKind::ScrollDown => {
                            *cursor = (*cursor + 1).min(nodes.len().saturating_sub(1))
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
                                    Hit::Radar => keep = false,
                                    Hit::RadarItem(idx) => {
                                        self.activate_radar(idx);
                                        keep = false;
                                    }
                                    Hit::Workspace => {
                                        self.open_workspace();
                                        keep = false;
                                    }
                                    Hit::Hosts => {
                                        self.command("/hosts");
                                        keep = false;
                                    }
                                    Hit::Sessions => {
                                        self.command("/sessions");
                                        keep = false;
                                    }
                                    Hit::WorkspaceTab(_) | Hit::WorkspaceItem(_) => {}
                                }
                            }
                        }
                        _ => {}
                    }
                } else if let Some(key) = key {
                    let alt = key.modifiers.contains(KeyModifiers::ALT);
                    if key.code == KeyCode::F(5) || (ctrl && key.code == KeyCode::Char('b')) {
                        self.open_workspace();
                        keep = false;
                    } else if ctrl && key.code == KeyCode::Char('n') {
                        self.new_chat();
                        keep = false;
                    } else if ctrl && key.code == KeyCode::Char('r') {
                        self.refresh();
                    } else {
                        match key.code {
                            KeyCode::Up => *cursor = cursor.saturating_sub(1),
                            KeyCode::Char('k') if !ctrl && !alt => {
                                *cursor = cursor.saturating_sub(1)
                            }
                            KeyCode::Down => {
                                *cursor = (*cursor + 1).min(nodes.len().saturating_sub(1))
                            }
                            KeyCode::Char('j') if !ctrl && !alt => {
                                *cursor = (*cursor + 1).min(nodes.len().saturating_sub(1))
                            }
                            KeyCode::Home => *cursor = 0,
                            KeyCode::End => *cursor = nodes.len().saturating_sub(1),
                            KeyCode::Enter => {
                                if !nodes.is_empty() {
                                    self.activate_radar((*cursor).min(nodes.len() - 1));
                                    keep = false;
                                }
                            }
                            _ => {}
                        }
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
            Field::SessionLockSet {
                sid,
                has_existing,
                stage,
                current_pw,
            } => {
                anyhow::ensure!(
                    !value.is_empty() && value.len() <= 256 && !value.contains('\0'),
                    "Пароль сессии должен содержать от 1 до 256 символов"
                );
                if *has_existing && *stage == 0 {
                    return Ok(Some((
                        Field::SessionLockSet {
                            sid: sid.clone(),
                            has_existing: true,
                            stage: 1,
                            current_pw: zeroize::Zeroizing::new(value.to_owned()),
                        },
                        String::new(),
                    )));
                }
                let host = self.active.clone().context("Выбери сервер")?;
                let mut payload = json!({
                    "op": "session_lock",
                    "sid": sid,
                    "remove": false,
                    "password": value,
                    "actor": self.settings.actor(),
                });
                if *has_existing {
                    payload["current_password"] = json!(current_pw.as_str());
                }
                self.controller.send(&host, payload);
                self.notice = "Установка парольной защиты сессии…".into();
            }
            Field::SessionUnlock { sid } => {
                anyhow::ensure!(
                    !value.is_empty() && value.len() <= 256 && !value.contains('\0'),
                    "Введи пароль сессии"
                );
                let host = self.active.clone().context("Выбери сервер")?;
                self.controller
                    .send(&host, json!({"op":"session_unlock","sid":sid,"password":value}));
                self.notice = "Проверка пароля сессии…".into();
            }
            Field::SessionLockRemove { sid } => {
                anyhow::ensure!(
                    !value.is_empty() && value.len() <= 256 && !value.contains('\0'),
                    "Введи текущий пароль сессии"
                );
                let host = self.active.clone().context("Выбери сервер")?;
                self.controller.send(
                    &host,
                    json!({
                        "op": "session_lock",
                        "sid": sid,
                        "remove": true,
                        "current_password": value,
                        "actor": self.settings.actor(),
                    }),
                );
                self.notice = "Снятие парольной защиты сессии…".into();
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
        let s1 = Session {
            id: "11111111-1111-4111-8111-111111111111".into(),
            title: "Оптимизация атласа".into(),
            cwd: "/opt/project".into(),
            pinned: 1.,
            modified: 1700000300.,
            backend: "claude".into(),
            ..Default::default()
        };
        let s2 = Session {
            id: "22222222-2222-4222-8222-222222222222".into(),
            title: "Новый интерфейс лаунчера".into(),
            cwd: "/opt/launcher".into(),
            modified: 1700000400.,
            running: true,
            backend: "codex".into(),
            ..Default::default()
        };
        let s3 = Session {
            id: "44444444-4444-4444-8444-444444444444".into(),
            title: "Аудит ключей релизов".into(),
            cwd: "/opt/security".into(),
            modified: 1700000200.,
            backend: "agy".into(),
            locked: true,
            unlocked: false,
            ..Default::default()
        };
        self.remote.insert(
            "demo".into(),
            Remote {
                connected: true,
                loaded: true,
                home: "/root".into(),
                version: "Claude Code".into(),
                backends: json!({
                    "claude": {"installed": true, "version": "2.1.12", "running": false, "label": "Claude Code"},
                    "codex": {"installed": true, "version": "0.98.0", "running": false, "label": "OpenAI Codex CLI"},
                    "agy": {"installed": false, "version": "", "running": false, "label": "Antigravity CLI (agy)"},
                    "api": {"installed": true, "version": "", "running": false, "label": "Пользовательский API"}
                }),
                usage: json!({
                    "available": true,
                    "windows": [
                        {"id": "five_hour", "label": "5 часов", "utilization": 38.},
                        {"id": "seven_day", "label": "Неделя", "utilization": 64.}
                    ]
                }),
                sessions: vec![s1.clone(), s2.clone(), s3],
                ..Default::default()
            },
        );

        // Preload live Codex session telemetry for the Radar Observatory
        self.ensure_chat("demo", &s2.id, Some(&s2));
        if let Some(bg) = self.chats.get_mut(&chat_key("demo", &s2.id)) {
            bg.loaded = true;
            bg.loading = false;
            bg.busy = false;
            bg.backend = "codex".into();
            bg.model = "gpt-5.4".into();
            bg.mode = "auto".into();
            bg.messages = vec![
                relay_desktop::model::Message {
                    role: "user".into(),
                    text: "Собери новый UI лаунчера и прогони интеграционные тесты".into(),
                    ..Default::default()
                },
                relay_desktop::model::Message {
                    role: "tool".into(),
                    tool: "Read".into(),
                    tool_id: "bg1".into(),
                    input: json!({"file_path": "/opt/launcher/src/theme.rs"}),
                    ..Default::default()
                },
                relay_desktop::model::Message {
                    role: "tool_result".into(),
                    tool_id: "bg1".into(),
                    text: "pub struct LauncherTheme".into(),
                    ..Default::default()
                },
                relay_desktop::model::Message {
                    role: "tool".into(),
                    tool: "spawn_agent".into(),
                    tool_id: "bg2".into(),
                    input: json!({"agent_type": "UI Verifier", "message": "Проверить сетку карточек и контраст"}),
                    ..Default::default()
                },
                relay_desktop::model::Message {
                    role: "tool".into(),
                    tool: "Bash".into(),
                    tool_id: "bg3".into(),
                    input: json!({"command": "cargo test --workspace --locked", "description": "Прогоняю тесты лаунчера"}),
                    ..Default::default()
                },
            ];
            bg.user_at = 0;
        }

        self.open_session(s1);
        let c = self.chat_mut().unwrap();
        c.loaded = true;
        c.loading = false;
        c.paused = false;
        c.mode = "auto".into();
        c.model = "opus".into();
        c.effort = "high".into();
        c.messages = vec![
            relay_desktop::model::Message {
                role: "user".into(),
                text: "Проверь изменения в атласе и запусти тесты".into(),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "assistant".into(),
                text: "Сверю привязку координат карты, поручу субагенту аудит береговой линии и прогоню тесты.".into(),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool".into(),
                tool: "Grep".into(),
                tool_id: "step1".into(),
                input: json!({"pattern": "AtlasCoordinates", "path": "src/"}),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool_result".into(),
                tool_id: "step1".into(),
                text: "src/AtlasRenderer.java:84\nsrc/CoastlineTest.java:19".into(),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool".into(),
                tool: "Read".into(),
                tool_id: "step2".into(),
                input: json!({"file_path": "/opt/project/src/AtlasRenderer.java"}),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool_result".into(),
                tool_id: "step2".into(),
                text: "public final class AtlasRenderer { ... }".into(),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool".into(),
                tool: "invoke_subagent".into(),
                tool_id: "step3".into(),
                input: json!({"Role": "Coastline Verifier", "Prompt": "Сверить пиксельные границы у побережья и облаков"}),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool_result".into(),
                tool_id: "step3".into(),
                text: "Контуры берега и тумана совпадают без зазоров.".into(),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool".into(),
                tool: "Edit".into(),
                tool_id: "step4".into(),
                input: json!({
                    "file_path": "/opt/project/src/AtlasRenderer.java",
                    "old_string": "int margin = region.rawOffset();\nrenderCoast(margin);",
                    "new_string": "int margin = region.pixelAlignedOffset();\nrenderCoast(margin);"
                }),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool_result".into(),
                tool_id: "step4".into(),
                text: "Updated AtlasRenderer.java".into(),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool".into(),
                tool: "Bash".into(),
                tool_id: "test".into(),
                input: json!({"command": "./gradlew test --no-daemon"}),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "tool_result".into(),
                tool_id: "test".into(),
                text: "AtlasCoordinatesTest > alignsBothMaps PASSED\nCoastlineTest > followsMapPixels PASSED\nFogRendererTest > coversEasternMountains PASSED\nBUILD SUCCESSFUL in 12s · 18 tests completed, 0 failed".into(),
                ..Default::default()
            },
            relay_desktop::model::Message {
                role: "assistant".into(),
                text: "**Все 18 тестов прошли.** Смещение границ исправлено; облака закрывают край гор.\n\n| Проверка | Результат |\n| --- | --- |\n| Координаты | Совпадают |\n| Границы | По пикселям карты |\n| Облака | Плавный переход |\n\nИзменения в `AtlasRenderer.java`. Сервер перезапускать не потребовалось.".into(),
                ..Default::default()
            },
        ];
        c.user_at = 0;
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

    #[test]
    fn workspace_popup_opens_navigates_sections_and_activates_items() {
        let (mut app, mut outgoing) = connected();
        app.input(key(KeyCode::F(5), KeyModifiers::NONE));
        assert!(matches!(
            app.popup,
            Some(Popup::Workspace {
                section: 0,
                index: 0
            })
        ));
        app.input(key(KeyCode::F(5), KeyModifiers::NONE));
        assert!(app.popup.is_none());

        app.input(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert!(matches!(app.popup, Some(Popup::Workspace { .. })));
        app.input(key(KeyCode::Tab, KeyModifiers::NONE));
        assert!(matches!(
            app.popup,
            Some(Popup::Workspace { section: 1, .. })
        ));
        app.input(key(KeyCode::Tab, KeyModifiers::NONE));
        assert!(matches!(
            app.popup,
            Some(Popup::Workspace { section: 2, .. })
        ));

        // First item in section 2 is Claude Code (active & installed) -> Down goes to Codex (installed, inactive) -> Enter switches backend!
        app.input(key(KeyCode::Down, KeyModifiers::NONE));
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.popup.is_none());
        assert_eq!(app.chat().unwrap().backend, "codex");
        let cat = outgoing.try_recv().unwrap();
        assert_eq!(cat["op"], "catalog");
        assert_eq!(cat["backend"], "codex");

        // Re-open via /menu, jump to section 2, navigate to Antigravity (installed == false) -> Enter triggers agent_install!
        app.command("/menu");
        app.input(key(KeyCode::Tab, KeyModifiers::NONE));
        app.input(key(KeyCode::Tab, KeyModifiers::NONE));
        app.input(key(KeyCode::Down, KeyModifiers::NONE));
        app.input(key(KeyCode::Down, KeyModifiers::NONE));
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        let install_req = outgoing.try_recv().unwrap();
        assert_eq!(install_req["op"], "agent_install");
        assert_eq!(install_req["agent"], "agy");
        assert_eq!(install_req["action"], "install");
    }

    #[test]
    fn theme_picker_lists_all_24_themes_with_light_and_dark_details() {
        let mut app = App::new(true).unwrap();
        app.demo();
        let items = app.items(Picker::Themes, "");
        assert_eq!(items.len(), 24);
        assert!(items.iter().any(|i| i.id == "graphite" && i.detail.contains("Тёмная тема")));
        assert!(items.iter().any(|i| i.id == "paper" && i.detail.contains("Светлая тема")));
        assert!(items.iter().any(|i| i.id == "frost" && i.detail.contains("Светлая тема")));
        assert!(items.iter().any(|i| i.id == "oled" && i.detail.contains("Тёмная тема")));

        for &(id, _, _, _) in &relay_desktop::model::THEME_SPECS {
            app.command(&format!("/theme {id}"));
            assert_eq!(app.theme(), id);
        }
        app.command("/theme nonexistent-theme");
        assert_eq!(app.theme(), "frost");
    }

    #[test]
    fn private_session_lock_relock_unlock_and_message_wiping() {
        let (mut app, mut outgoing) = connected();
        let sid = app.chat().unwrap().sid.clone();
        app.chat_mut().unwrap().messages.push(relay_desktop::model::Message {
            role: "assistant".into(),
            text: "Приватные данные".into(),
            ..Default::default()
        });

        // 1. Set lock on unlocked session
        app.command("/lock");
        assert!(matches!(
            app.popup,
            Some(Popup::Field {
                kind: Field::SessionLockSet {
                    has_existing: false,
                    stage: 0,
                    ..
                },
                ..
            })
        ));
        app.input(Input::Paste("my-secret-pass\n".into()));
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        let set_pkt = outgoing.try_recv().unwrap();
        assert_eq!(set_pkt["op"], "session_lock");
        assert_eq!(set_pkt["sid"], sid);
        assert_eq!(set_pkt["remove"], false);
        assert_eq!(set_pkt["password"], "my-secret-pass");
        assert!(app.popup.is_none());

        app.data(
            "demo",
            json!({"type":"session_lock_changed","sid":sid,"locked":true,"unlocked":true}),
        );
        assert!(app.chat().unwrap().locked);
        assert!(app.chat().unwrap().unlocked);

        // 2. /lock on unlocked session immediately relocks and wipes messages!
        app.command("/lock");
        let relock_pkt = outgoing.try_recv().unwrap();
        assert_eq!(relock_pkt["op"], "session_relock");
        assert_eq!(relock_pkt["sid"], sid);
        assert!(app.chat().unwrap().is_locked_gate());
        assert!(app.chat().unwrap().messages.is_empty());

        // 3. Regular prompt submission is blocked while locked and opens SessionUnlock
        app.chat_mut().unwrap().editor = Editor::new("Секретный вопрос".into());
        app.submit();
        assert!(outgoing.try_recv().is_err());
        assert_eq!(app.chat().unwrap().editor.text, "Секретный вопрос");
        assert!(matches!(
            app.popup,
            Some(Popup::Field {
                kind: Field::SessionUnlock { .. },
                ..
            })
        ));
        app.popup = None;

        // Safe global commands still work while locked
        app.chat_mut().unwrap().editor = Editor::new("/help".into());
        app.submit();
        assert!(matches!(app.popup, Some(Popup::Help { .. })));
        app.popup = None;

        // 4. Unlocking requests history upon ok=true
        app.command("/unlock");
        app.input(Input::Paste("my-secret-pass".into()));
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        let unlock_pkt = outgoing.try_recv().unwrap();
        assert_eq!(unlock_pkt["op"], "session_unlock");
        assert_eq!(unlock_pkt["sid"], sid);
        assert_eq!(unlock_pkt["password"], "my-secret-pass");

        app.data(
            "demo",
            json!({"type":"session_unlock_result","sid":sid,"ok":true}),
        );
        assert!(app.chat().unwrap().unlocked);
        assert!(app.chat().unwrap().loading);
        let hist_pkt = outgoing.try_recv().unwrap();
        assert_eq!(hist_pkt["op"], "history");
        assert_eq!(hist_pkt["sid"], sid);

        // 5. /unlock-remove sends remove=true with current_password
        app.command("/unlock-remove");
        app.input(Input::Paste("my-secret-pass".into()));
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        let rem_pkt = outgoing.try_recv().unwrap();
        assert_eq!(rem_pkt["op"], "session_lock");
        assert_eq!(rem_pkt["remove"], true);
        assert_eq!(rem_pkt["current_password"], "my-secret-pass");

        // 6. Opening a locked session from session list opens SessionUnlock instead of history
        let locked_session = Session {
            id: "33333333-3333-4333-8333-333333333333".into(),
            title: "Закрытая сессия".into(),
            cwd: "/opt/secret".into(),
            locked: true,
            unlocked: false,
            ..Default::default()
        };
        app.open_session(locked_session);
        assert!(outgoing.try_recv().is_err());
        assert!(app.chat().unwrap().is_locked_gate());
        assert!(matches!(
            app.popup,
            Some(Popup::Field {
                kind: Field::SessionUnlock { .. },
                ..
            })
        ));

        // 7. Store never contains passwords
        let serialized = serde_json::to_string(&app.store).unwrap();
        assert!(!serialized.contains("my-secret-pass"));
    }

    #[test]
    fn cli_installer_command_and_inbound_events_track_backend_status() {
        let (mut app, mut outgoing) = connected();
        app.command("/install");
        assert!(matches!(
            app.popup,
            Some(Popup::Picker {
                kind: Picker::Install,
                ..
            })
        ));
        let items = app.items(Picker::Install, "");
        assert_eq!(items.len(), 4);
        assert!(items.iter().any(|i| i.id == "claude" && i.detail.contains("2.1.12")));
        assert!(items.iter().any(|i| i.id == "agy" && i.detail.contains("Не установлен")));
        assert!(items.iter().any(|i| i.id == "all"));
        app.popup = None;

        // Uninstalled agent -> action: "install"
        app.command("/install agy");
        let pkt1 = outgoing.try_recv().unwrap();
        assert_eq!(pkt1["op"], "agent_install");
        assert_eq!(pkt1["agent"], "agy");
        assert_eq!(pkt1["action"], "install");
        assert!(app.remote["demo"].installing.contains("agy"));

        // Installed agent -> action: "update"
        app.command("/install claude");
        let pkt2 = outgoing.try_recv().unwrap();
        assert_eq!(pkt2["op"], "agent_install");
        assert_eq!(pkt2["agent"], "claude");
        assert_eq!(pkt2["action"], "update");

        // Switch active chat to agy while still uninstalled -> active_backend_missing() is true
        app.command("/backend agy");
        let _ = outgoing.try_recv();
        assert!(app.active_backend_missing());

        // Inbound agent_install progress & completion + backends_status
        app.data(
            "demo",
            json!({"type":"agent_install","agent":"agy","running":true,"message":"Устанавливаем Antigravity CLI (agy) на сервере…"}),
        );
        assert!(app.notice.contains("Устанавливаем Antigravity"));
        assert!(app.remote["demo"].installing.contains("agy"));

        app.data(
            "demo",
            json!({"type":"agent_install","agent":"agy","running":false,"ok":true,"installed":true,"version":"1.4.0","message":"Antigravity CLI (agy) готов (1.4.0)."}),
        );
        assert!(!app.remote["demo"].installing.contains("agy"));
        assert!(app.notice.contains("готов (1.4.0)"));
        assert!(!app.active_backend_missing());
        let cat = outgoing.try_recv().unwrap();
        assert_eq!(cat["op"], "catalog");
        assert_eq!(cat["backend"], "agy");

        app.data(
            "demo",
            json!({"type":"backends_status","backends":{"claude":{"installed":true,"version":"2.1.15","running":false},"codex":{"installed":true,"version":"0.99.0","running":false},"agy":{"installed":true,"version":"1.4.0","running":false}}}),
        );
        assert_eq!(app.remote["demo"].backends["claude"]["version"], "2.1.15");
    }

    #[test]
    fn radar_popup_opens_via_f6_and_commands_navigates_j_k_and_jumps_on_enter() {
        let mut app = App::new(true).unwrap();
        app.demo();
        assert_eq!(app.active_agents_count(), 1);

        let nodes = app.radar_nodes();
        assert_eq!(nodes.len(), 3);
        // Running Codex session is prioritized first on the radar
        assert_eq!(nodes[0].sid, "22222222-2222-4222-8222-222222222222");
        assert!(nodes[0].running);
        assert_eq!(nodes[0].backend, "codex");
        assert_eq!(nodes[0].telemetry.active_headline, "Прогоняю тесты лаунчера");
        assert_eq!(nodes[0].telemetry.subagents.len(), 1);
        assert_eq!(nodes[0].telemetry.subagents[0].role, "UI Verifier");

        assert_eq!(nodes[1].sid, "11111111-1111-4111-8111-111111111111");
        assert!(nodes[1].is_current);
        assert_eq!(nodes[1].telemetry.total_actions, 5);
        assert_eq!(nodes[1].telemetry.reads, 2);
        assert_eq!(nodes[1].telemetry.edits, 1);
        assert_eq!(nodes[1].telemetry.commands, 1);
        assert_eq!(nodes[1].telemetry.delegations, 1);

        assert!(nodes[2].locked_gate);
        assert_eq!(nodes[2].telemetry.total_actions, 0);

        // Open via F6 and toggle closed with F6
        app.input(key(KeyCode::F(6), KeyModifiers::NONE));
        assert!(matches!(app.popup, Some(Popup::Radar { cursor: 0 })));
        app.input(key(KeyCode::F(6), KeyModifiers::NONE));
        assert!(app.popup.is_none());

        // Open via /radar, navigate with j / k and Down / Up, select running session 0 via Enter
        app.command("/radar");
        assert!(matches!(app.popup, Some(Popup::Radar { cursor: 0 })));
        app.input(key(KeyCode::Char('j'), KeyModifiers::NONE));
        assert!(matches!(app.popup, Some(Popup::Radar { cursor: 1 })));
        app.input(key(KeyCode::Char('k'), KeyModifiers::NONE));
        assert!(matches!(app.popup, Some(Popup::Radar { cursor: 0 })));
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.popup.is_none());
        assert_eq!(
            app.chat().unwrap().sid,
            "22222222-2222-4222-8222-222222222222"
        );
        assert_eq!(app.chat().unwrap().backend, "codex");

        // Open via /agents and select locked session (index 2) -> opens SessionUnlock prompt
        app.command("/agents");
        app.input(key(KeyCode::Down, KeyModifiers::NONE));
        app.input(key(KeyCode::Down, KeyModifiers::NONE));
        assert!(matches!(app.popup, Some(Popup::Radar { cursor: 2 })));
        app.input(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            app.popup,
            Some(Popup::Field {
                kind: Field::SessionUnlock { .. },
                ..
            })
        ));
    }
}
