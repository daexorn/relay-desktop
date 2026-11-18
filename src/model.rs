use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const BACKENDS: [&str; 4] = ["claude", "codex", "agy", "api"];

pub fn valid_backend(b: &str) -> bool {
    matches!(b, "claude" | "codex" | "agy" | "api")
}

pub fn normalize_backend(b: &str) -> Option<&'static str> {
    match b.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claudecode" => Some("claude"),
        "codex" | "openai" => Some("codex"),
        "agy" | "antigravity" | "antigravity-cli" => Some("agy"),
        "api" | "custom" => Some("api"),
        _ => None,
    }
}

pub fn backend_label(b: &str) -> &'static str {
    match b {
        "codex" => "Codex",
        "agy" => "Antigravity (agy)",
        "api" => "Пользовательский API",
        _ => "Claude Code",
    }
}

pub fn backend_short_label(b: &str) -> &'static str {
    match b {
        "codex" => "Codex",
        "agy" => "AGY",
        "api" => "API",
        _ => "Claude Code",
    }
}

pub fn default_backend_models(b: &str) -> Vec<ModelInfo> {
    match b {
        "codex" => vec![
            ModelInfo {
                value: "default".into(),
                display_name: "По умолчанию (Codex)".into(),
                description: "Модель профиля Codex CLI на сервере".into(),
            },
            ModelInfo {
                value: "gpt-5.4".into(),
                display_name: "GPT-5.4".into(),
                description: "Основная агентная модель OpenAI Codex".into(),
            },
            ModelInfo {
                value: "gpt-5.3-codex".into(),
                display_name: "GPT-5.3 Codex".into(),
                description: "Специализированная модель для кода".into(),
            },
            ModelInfo {
                value: "gpt-5.4-mini".into(),
                display_name: "GPT-5.4 Mini".into(),
                description: "Быстрая модель для простых задач".into(),
            },
        ],
        "agy" => vec![
            ModelInfo {
                value: "default".into(),
                display_name: "По умолчанию (AGY)".into(),
                description: "Модель профиля Antigravity CLI на сервере".into(),
            },
            ModelInfo {
                value: "gemini-3.1-pro-preview".into(),
                display_name: "Gemini 3.1 Pro".into(),
                description: "Глубокое планирование и сложный код".into(),
            },
            ModelInfo {
                value: "gemini-3-flash-preview".into(),
                display_name: "Gemini 3 Flash".into(),
                description: "Быстрая агентная разработка".into(),
            },
            ModelInfo {
                value: "claude-sonnet-4-6".into(),
                display_name: "Claude Sonnet 4.6 (AGY)".into(),
                description: "Через Google Antigravity".into(),
            },
        ],
        _ => vec![],
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub cwd: String,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub remember: bool,
    #[serde(default)]
    pub last_session: Option<String>,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_model")]
    pub effort: String,
    #[serde(default)]
    pub local_workspace: Option<String>,
    #[serde(default)]
    pub local_control: bool,
    /// In-game tests on this PC: "auto", "ask" or "off".
    #[serde(default = "default_tests")]
    pub tests: String,
    #[serde(default)]
    pub api: ApiProfile,
    #[serde(default)]
    pub default_backend: String,
    #[serde(default = "default_model")]
    pub codex_model: String,
    #[serde(default = "default_model")]
    pub agy_model: String,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct ApiProfile {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub max_steps: u32,
    pub models: Vec<String>,
}
impl Default for ApiProfile {
    fn default() -> Self {
        Self { enabled: false, base_url: String::new(), model: String::new(), max_tokens: 8192, max_steps: 80, models: vec![] }
    }
}
impl ApiProfile {
    pub fn validate(&mut self) -> anyhow::Result<()> {
        self.base_url = self.base_url.trim().trim_end_matches('/').to_owned();
        self.model = self.model.trim().to_owned();
        anyhow::ensure!((self.base_url.starts_with("https://") || self.base_url.starts_with("http://")) && !self.base_url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '@' | '?' | '#')), "Укажи базовый URL API, например https://tooken.club/v1.");
        anyhow::ensure!(!self.model.is_empty() && self.model.len() <= 200 && !self.model.chars().any(char::is_control), "Укажи идентификатор модели API.");
        self.max_tokens = self.max_tokens.clamp(256, 65536);
        self.max_steps = self.max_steps.clamp(1, 256);
        Ok(())
    }
    pub fn request(&self, key: String) -> Value {
        serde_json::json!({"base_url":self.base_url,"model":self.model,"max_tokens":self.max_tokens,"max_steps":self.max_steps,"key":key})
    }
}
fn default_tests() -> String {
    "ask".into()
}
fn default_mode() -> String {
    "auto".into()
}
fn default_model() -> String {
    "default".into()
}
impl Default for Server {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            host: String::new(),
            port: 22,
            user: "root".into(),
            cwd: "~".into(),
            fingerprint: None,
            remember: true,
            last_session: None,
            folders: vec![],
            mode: default_mode(),
            model: default_model(),
            effort: default_model(),
            local_workspace: None,
            local_control: false,
            tests: default_tests(),
            api: ApiProfile::default(),
            default_backend: String::new(),
            codex_model: default_model(),
            agy_model: default_model(),
        }
    }
}
impl Server {
    pub fn effective_backend(&self) -> &str {
        if self.api.enabled && (self.default_backend.is_empty() || self.default_backend == "api") {
            "api"
        } else if valid_backend(&self.default_backend) && self.default_backend != "api" {
            &self.default_backend
        } else if self.api.enabled {
            "api"
        } else {
            "claude"
        }
    }

    pub fn set_default_backend(&mut self, backend: &str) {
        let normalized = normalize_backend(backend).unwrap_or("claude");
        self.default_backend = normalized.to_owned();
        self.api.enabled = normalized == "api";
    }

    pub fn model_for(&self, backend: &str) -> &str {
        match backend {
            "codex" => {
                if self.codex_model.is_empty() {
                    "default"
                } else {
                    &self.codex_model
                }
            }
            "agy" => {
                if self.agy_model.is_empty() {
                    "default"
                } else {
                    &self.agy_model
                }
            }
            "api" => &self.api.model,
            _ => {
                if self.model.is_empty() {
                    "default"
                } else {
                    &self.model
                }
            }
        }
    }

    pub fn set_model_for(&mut self, backend: &str, model: String) {
        match backend {
            "codex" => self.codex_model = model,
            "agy" => self.agy_model = model,
            "api" => self.api.model = model,
            _ => self.model = model,
        }
    }

    pub fn validate(&mut self) -> anyhow::Result<()> {
        self.host = self.host.trim().to_owned();
        self.user = self.user.trim().to_owned();
        self.name = self.name.trim().to_owned();
        anyhow::ensure!(
            !self.host.is_empty()
                && self.host.len() <= 253
                && !self
                    .host
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || c == '/'),
            "Укажи IP-адрес или имя сервера."
        );
        anyhow::ensure!(
            !self.user.is_empty()
                && !self
                    .user
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace()),
            "Укажи имя пользователя."
        );
        anyhow::ensure!(self.port > 0, "Порт должен быть от 1 до 65535.");
        if self.name.is_empty() {
            self.name = self.host.clone();
        }
        if self.cwd.trim().is_empty() {
            self.cwd = "~".into();
        }
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub servers: Vec<Server>,
    pub selected: Option<String>,
    #[serde(default)]
    pub light: bool,
    /// Shown on the team board and in agents' names; defaults to the OS user.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub cluster: Vec<AgentNode>,
    #[serde(default)]
    pub orchestrators: Vec<crate::cluster::Config>,
    #[serde(default)]
    pub prompt_snippets: Vec<crate::templates::Snippet>,
    /// Session folders folded in the sidebar (this computer only).
    #[serde(default)]
    pub collapsed_folders: Vec<String>,
    /// Per-conversation override; server API settings supply the default.
    #[serde(default)]
    pub api_sessions: std::collections::BTreeMap<String, bool>,
    /// Per-conversation backend override ("claude", "codex", "agy", "api").
    #[serde(default)]
    pub session_backends: std::collections::BTreeMap<String, String>,
}
/// Drafts and unsent queued messages, kept on disk so closing Relay loses nothing.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PendingStore {
    /// Keyed by "server/session".
    pub chats: std::collections::BTreeMap<String, PendingChat>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PendingChat {
    #[serde(default)]
    pub draft: String,
    #[serde(default)]
    pub queue: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: String,
    pub scale: f32,
    pub chat_width: f32,
    pub compact: bool,
    pub animations: bool,
    pub wrap_output: bool,
    pub open_errors: bool,
    pub send_on_enter: bool,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: String::new(),
            scale: 1.,
            chat_width: 820.,
            compact: true,
            animations: true,
            wrap_output: true,
            open_errors: false,
            send_on_enter: true,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct AgentNode {
    pub server: String,
    pub role: String,
    pub enabled: bool,
}
impl Settings {
    pub fn theme(&self) -> &str {
        if self.appearance.theme.is_empty() {
            if self.light {
                "paper"
            } else {
                "graphite"
            }
        } else {
            &self.appearance.theme
        }
    }
    pub fn actor(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.trim().to_owned();
        }
        std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "Relay".into())
    }

    pub fn session_backend(
        &self,
        server: &Server,
        chat_key: &str,
        remote_backend: Option<&str>,
    ) -> String {
        if self.api_sessions.get(chat_key) == Some(&true) {
            return "api".into();
        }
        if let Some(b) = self
            .session_backends
            .get(chat_key)
            .filter(|b| valid_backend(b) && *b != "api")
        {
            return b.clone();
        }
        if self.api_sessions.get(chat_key) == Some(&false) {
            return "claude".into();
        }
        if let Some(rb) = remote_backend.filter(|b| valid_backend(b)) {
            return rb.to_owned();
        }
        server.effective_backend().to_owned()
    }

    pub fn set_session_backend(&mut self, chat_key: String, backend: &str) {
        let b = normalize_backend(backend).unwrap_or("claude");
        self.api_sessions.insert(chat_key.clone(), b == "api");
        self.session_backends.insert(chat_key, b.to_owned());
    }
}
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_owned()))
        .unwrap_or_else(|_| "pc".into())
}

fn default_true() -> bool {
    true
}

/// Shared 40-theme catalog across Desktop GUI, Relay TUI, and Android (30 dark + 10 light):
/// `(id, label, is_light, [base, side, rail, surface, line, text, muted, accent])`.
pub const THEME_SPECS: [(&str, &str, bool, [u32; 8]); 40] = [
    ("graphite", "Графит", false, [0x10141a, 0x151b23, 0x0c1016, 0x1b242e, 0x2a3643, 0xe6edf3, 0x8d9cab, 0x83c4b3]),
    ("midnight", "Полночь", false, [0x0f1520, 0x141d2b, 0x0b111b, 0x1b2739, 0x2b3b52, 0xe5edf9, 0x95a5bd, 0x89aef1]),
    ("obsidian", "Обсидиан", false, [0x0d0f12, 0x12151a, 0x090b0e, 0x191d24, 0x2a303b, 0xedeff2, 0x9098a6, 0xe5b567]),
    ("tokyo", "Токио", false, [0x1a1b26, 0x16161e, 0x111219, 0x24283b, 0x343b58, 0xc0caf5, 0x7982a9, 0x7aa2f7]),
    ("catppuccin", "Мокко", false, [0x1e1e2e, 0x181825, 0x11111b, 0x313244, 0x45475a, 0xcdd6f4, 0x9399b2, 0xb4befe]),
    ("gruvbox", "Грувбокс", false, [0x1d2021, 0x282828, 0x161819, 0x32302f, 0x504945, 0xebdbb2, 0xa89984, 0xfabd2f]),
    ("kanagawa", "Канагава", false, [0x1f1f28, 0x181820, 0x121218, 0x2a2a37, 0x3a3a4e, 0xdcd7ba, 0x938aa9, 0xdca561]),
    ("everforest", "Тайга", false, [0x232a2e, 0x1e2428, 0x181d20, 0x2d353b, 0x404c51, 0xd3c6aa, 0x859289, 0xa7c080]),
    ("cyber", "Неон", false, [0x0b0e17, 0x101422, 0x070910, 0x171d30, 0x273252, 0xe6f1ff, 0x8496b8, 0x38e8ff]),
    ("aurora", "Аврора", false, [0x0d171c, 0x111f26, 0x091115, 0x182b34, 0x26424f, 0xe4f5f4, 0x88aeb0, 0x5eead4]),
    ("forest", "Хвоя", false, [0x111916, 0x17211c, 0x0c120f, 0x202e27, 0x2e4136, 0xe4eee8, 0x9aad9f, 0x83c6a9]),
    ("plum", "Слива", false, [0x1b1721, 0x16121c, 0x120e18, 0x292331, 0x3b3246, 0xeee7f5, 0xab9bb8, 0xc39bff]),
    ("ember", "Угли", false, [0x1d1816, 0x171311, 0x120f0d, 0x2a2320, 0x3d322d, 0xf3e9e4, 0xb0a097, 0xff8a5b]),
    ("ocean", "Океан", false, [0x0f1b1e, 0x0c1619, 0x091214, 0x172a2e, 0x24393e, 0xe2f2f2, 0x8fadaf, 0x4fd1c5]),
    ("nord", "Север", false, [0x2e3440, 0x292e39, 0x242933, 0x3b4252, 0x434c5e, 0xeceff4, 0x9aa5b8, 0x88c0d0]),
    ("rose", "Роза", false, [0x1e1619, 0x181114, 0x130d10, 0x2c2024, 0x402e34, 0xf6e8ec, 0xb79aa3, 0xf28ba8]),
    ("dracula", "Дракула", false, [0x282a36, 0x21222c, 0x1b1c24, 0x343746, 0x44475a, 0xf8f8f2, 0xa4a9c6, 0xbd93f9]),
    ("oled", "OLED", false, [0x000000, 0x050506, 0x000000, 0x121214, 0x232327, 0xf2f2f4, 0x8a8a93, 0xa78bfa]),
    ("monokai", "Монокай", false, [0x1e1f1c, 0x181916, 0x131411, 0x2b2c27, 0x3e4038, 0xf8f8f2, 0xa6a28c, 0xa6e22e]),
    ("ayu", "Аю Мираж", false, [0x1f2430, 0x191e2a, 0x141822, 0x272d3d, 0x384157, 0xcbccc6, 0x8a9199, 0xffcc66]),
    ("synthwave", "Синтвейв", false, [0x1a1426, 0x140f1e, 0x0f0b17, 0x261d38, 0x3b2d56, 0xfdf4ff, 0xb59ec9, 0xff5ec4]),
    ("cobalt", "Кобальт", false, [0x122338, 0x0e1c2d, 0x0a1522, 0x1a314d, 0x28476e, 0xeaf2fb, 0x8fb0d4, 0xffc600]),
    ("matrix", "Матрица", false, [0x080f0a, 0x060b07, 0x040805, 0x101f14, 0x1d3824, 0xd8f8df, 0x6ea87c, 0x39ff88]),
    ("vesper", "Веспер", false, [0x101010, 0x0c0c0c, 0x080808, 0x1b1b1b, 0x2c2c2c, 0xf5f5f5, 0x999999, 0xffc799]),
    ("poimandres", "Поймандрес", false, [0x1b1e28, 0x161922, 0x11141c, 0x252937, 0x353b4f, 0xe4f0fb, 0x8695b7, 0x5de4c7]),
    ("horizon", "Горизонт", false, [0x1c1e26, 0x16181f, 0x111319, 0x282a36, 0x3b3e4f, 0xe9ecf2, 0x9da3b4, 0xe95678]),
    ("zenburn", "Зенбёрн", false, [0x2b2b2b, 0x242424, 0x1d1d1d, 0x383838, 0x4b4b4b, 0xdcdccc, 0x9f9f91, 0x8cd0d3]),
    ("sakura_night", "Ночная сакура", false, [0x18131d, 0x130f17, 0x0e0b12, 0x241c2c, 0x382b44, 0xf7ebf4, 0xb29ab0, 0xff85b3]),
    ("arctic_void", "Космос", false, [0x090d16, 0x070a12, 0x05070d, 0x12192b, 0x212d4a, 0xeaf0ff, 0x8697bd, 0x7aa2f7]),
    ("copper", "Медь", false, [0x191412, 0x14100e, 0x0f0c0a, 0x261e1a, 0x3b2f29, 0xf4ebe6, 0xad998f, 0xe08b58]),
    ("paper", "Бумага", true, [0xfcfcfb, 0xf5f4f2, 0xedece9, 0xffffff, 0xe4e2de, 0x1e1e21, 0x78777e, 0x4a6fe3]),
    ("sand", "Песок", true, [0xfbf7f0, 0xf4eee3, 0xece4d6, 0xfffdf9, 0xe3d8c6, 0x2b241c, 0x857865, 0xc2703d]),
    ("mint", "Мята", true, [0xf6fbf8, 0xedf6f1, 0xe2efe8, 0xffffff, 0xd5e7dc, 0x17261e, 0x6c8478, 0x2f9e77]),
    ("latte", "Латте", true, [0xeff1f5, 0xe6e9ef, 0xdce0e8, 0xffffff, 0xccd0da, 0x2c2f3f, 0x6c6f85, 0x1e66f5]),
    ("solarized", "Солярис", true, [0xfdf6e3, 0xf4ecd8, 0xeee8d5, 0xfffbf0, 0xded6c1, 0x37474f, 0x657b83, 0x2aa198]),
    ("frost", "Иней", true, [0xf4f7fb, 0xeaf0f7, 0xdfe8f2, 0xffffff, 0xcfdbe8, 0x172133, 0x62728a, 0x2563eb]),
    ("lavender", "Лаванда", true, [0xf8f6fc, 0xf0ecf8, 0xe6e0f2, 0xffffff, 0xd8cfeb, 0x231d30, 0x6f6486, 0x7c3aed]),
    ("peach", "Персик", true, [0xfdf8f5, 0xf7eee8, 0xefe3db, 0xffffff, 0xe6d4c8, 0x2d211b, 0x826c60, 0xd96b43]),
    ("sage", "Шалфей", true, [0xf5f8f4, 0xebf1e9, 0xe0e8dd, 0xffffff, 0xcfdcd0, 0x1c261e, 0x637867, 0x3b875e]),
    ("sky_day", "Облака", true, [0xf5f9fd, 0xebf2f9, 0xdfeaf5, 0xffffff, 0xcbdced, 0x162434, 0x5e768f, 0x0284c7]),
];

pub fn theme_spec(name: &str) -> &'static (&'static str, &'static str, bool, [u32; 8]) {
    THEME_SPECS
        .iter()
        .find(|t| t.0 == name)
        .unwrap_or(&THEME_SPECS[0])
}

pub fn valid_theme(name: &str) -> bool {
    THEME_SPECS.iter().any(|t| t.0 == name)
}

#[derive(Clone, Default, Deserialize, Debug)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub modified: f64,
    /// Claude's own title when the team renamed the session.
    #[serde(default)]
    pub auto_title: String,
    /// When it was pinned (0 = not pinned); shared by everyone on the server.
    #[serde(default)]
    pub pinned: f64,
    #[serde(default)]
    pub folder: String,
    /// Claude is answering in it right now (the server host keeps it running without the app).
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub native_id: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "default_true")]
    pub unlocked: bool,
}

impl Session {
    pub fn is_locked_gate(&self) -> bool {
        self.locked && !self.unlocked
    }
}
/// A model exactly as the server's Claude Code lists it: `fable`, `opus`, a full id…
#[derive(Clone, Default, Deserialize, Debug, PartialEq)]
pub struct ModelInfo {
    pub value: String,
    #[serde(default, rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
}
/// A slash command of the server's Claude Code, for completion in the composer.
#[derive(Clone, Default, Deserialize, Debug, PartialEq)]
pub struct CommandInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub hint: String,
}
/// A named group of sessions, shared by everyone who connects to the server.
#[derive(Clone, Default, Deserialize, Debug)]
pub struct SessionFolder {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Default, Deserialize)]
pub struct Folder {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Default, Deserialize, Debug)]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub failed: bool,
    #[serde(default)]
    pub tool_id: String,
    /// Pictures of a user message from history: {media_type, data}; data is empty when the
    /// server left it out to keep the history small.
    #[serde(default)]
    pub images: Vec<Value>,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub actor: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub steer: bool,
    #[serde(default)]
    pub agent_name: String,
}

pub fn builtin_slash_commands() -> Vec<CommandInfo> {
    [
        ("usage", "Лимиты и токены выбранного агента", ""),
        ("cost", "Статистика токенов и вызовов инструментов сессии", ""),
        ("status", "Состояние сервера, CLI-агентов и текущей сессии", ""),
        ("clear", "Начать новую чистую сессию в текущей папке", ""),
        ("compact", "Сжать длинный контекст разговора", ""),
        ("backend", "Переключить агента: claude, codex, agy, api", "claude|codex|agy|api"),
        ("model", "Переключить модель активного агента", "<модель>"),
        ("mode", "Режим разрешений: auto, manual, edits, plan", "auto|manual|edits|plan"),
        ("effort", "Глубина рассуждений: auto, low, medium, high, max", "auto|low|medium|high|max"),
        ("lock", "Установить пароль или заблокировать сессию", ""),
        ("folder", "Открыть окно управления папками сессий", ""),
        ("radar", "Открыть живой радар агентов на сервере", ""),
        ("theme", "Открыть выбор из 40 тем оформления", "[тема]"),
        ("login", "Авторизовать агента через серверный браузер", "[claude|codex|agy]"),
        ("install", "Установить или обновить CLI на сервере", "[claude|codex|agy|all]"),
        ("stop", "Остановить текущий ответ агента", ""),
        ("help", "Показать список всех быстрых команд Relay", ""),
    ]
    .into_iter()
    .map(|(name, desc, hint)| CommandInfo {
        name: name.into(),
        description: desc.into(),
        hint: hint.into(),
    })
    .collect()
}

pub fn pretty_model_name(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("default") {
        return String::new();
    }
    let low = s.trim_start_matches("models/").to_ascii_lowercase();
    if low == "opus" {
        return "Opus".into();
    }
    if low == "sonnet" {
        return "Sonnet".into();
    }
    if low == "haiku" {
        return "Haiku".into();
    }
    if low.starts_with("claude-opus-4-7") {
        return "Opus 4.7".into();
    }
    if low.starts_with("claude-opus-4-6") {
        return "Opus 4.6".into();
    }
    if low.starts_with("claude-opus-4-5") {
        return "Opus 4.5".into();
    }
    if low.starts_with("claude-sonnet-4-6") {
        return "Sonnet 4.6".into();
    }
    if low.starts_with("claude-sonnet-4-5") {
        return "Sonnet 4.5".into();
    }
    if low.starts_with("claude-haiku-4-5") {
        return "Haiku 4.5".into();
    }
    if low == "gpt-5.4-mini" {
        return "GPT-5.4 Mini".into();
    }
    if low == "gpt-5.4" {
        return "GPT-5.4".into();
    }
    if low == "gpt-5.3-codex" {
        return "GPT-5.3 Codex".into();
    }
    if low.starts_with("gemini-3.1-pro") {
        return "Gemini 3.1 Pro".into();
    }
    if low.starts_with("gemini-3-flash") {
        return "Gemini 3 Flash".into();
    }
    shorten(s.trim_start_matches("models/"), 26)
}

pub fn mode_short_label(mode: &str) -> &'static str {
    match mode {
        "auto" => "Авто",
        "acceptEdits" => "Правки",
        "plan" => "План",
        "manual" => "Вручную",
        _ => "",
    }
}

pub fn effort_short_label(effort: &str) -> &'static str {
    match effort {
        "low" => "Быстро",
        "medium" => "Средне",
        "high" => "Глубоко",
        "max" | "xhigh" => "Макс",
        _ => "",
    }
}

fn step_word_ru(n: usize) -> &'static str {
    let m100 = n % 100;
    let m10 = n % 10;
    if (11..=14).contains(&m100) {
        "шагов"
    } else if m10 == 1 {
        "шаг"
    } else if (2..=4).contains(&m10) {
        "шага"
    } else {
        "шагов"
    }
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct MessageAttribution {
    pub backend: String,
    pub who: String,
    pub to_whom: String,
    pub how: String,
}

/// Computes exact per-message correspondence (`кто -> кому · как`) across user and assistant turns
/// so historical replies never get relabeled when switching the active composer backend.
pub fn compute_attributions(
    messages: &[Message],
    fallback_backend: &str,
    chat_busy: bool,
) -> Vec<MessageAttribution> {
    let default_b = if valid_backend(fallback_backend) {
        fallback_backend
    } else {
        "claude"
    };
    let mut out = vec![MessageAttribution::default(); messages.len()];
    let user_positions: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "user")
        .map(|(idx, _)| idx)
        .collect();

    for (u_pos, &u_idx) in user_positions.iter().enumerate() {
        let next_u = user_positions
            .get(u_pos + 1)
            .copied()
            .unwrap_or(messages.len());
        let umsg = &messages[u_idx];
        let reply_b = (u_idx + 1..next_u).find_map(|k| {
            let m = &messages[k];
            ((m.role == "assistant" || m.role == "tool") && valid_backend(&m.backend))
                .then(|| m.backend.as_str())
        });
        let turn_b = if valid_backend(&umsg.backend) {
            umsg.backend.as_str()
        } else if let Some(rb) = reply_b {
            rb
        } else {
            default_b
        };
        let sender = if umsg.text.starts_with("<relay-cluster-results>") {
            "Команда агентов".to_owned()
        } else if !umsg.actor.trim().is_empty() {
            umsg.actor.trim().to_owned()
        } else {
            "Вы".to_owned()
        };
        let target_agent = if !umsg.agent_name.trim().is_empty() {
            format!("{} ({})", umsg.agent_name.trim(), backend_short_label(turn_b))
        } else {
            backend_label(turn_b).to_owned()
        };
        let has_reply_in_span = (u_idx + 1..next_u)
            .any(|k| matches!(messages[k].role.as_str(), "assistant" | "tool"));
        let mut u_how = Vec::new();
        let u_mod = pretty_model_name(&umsg.model);
        if !u_mod.is_empty() {
            u_how.push(u_mod);
        }
        let u_mode = mode_short_label(&umsg.mode);
        if !u_mode.is_empty() {
            u_how.push(u_mode.into());
        }
        let u_eff = effort_short_label(&umsg.effort);
        if !u_eff.is_empty() {
            u_how.push(u_eff.into());
        }
        if umsg.steer {
            u_how.push("по ходу ответа".into());
        } else if !has_reply_in_span && u_pos + 1 < user_positions.len() {
            u_how.push("без отдельного ответа".into());
        } else if !has_reply_in_span && chat_busy && u_pos + 1 == user_positions.len() {
            u_how.push("в работе".into());
        }
        out[u_idx] = MessageAttribution {
            backend: turn_b.to_owned(),
            who: sender.clone(),
            to_whom: target_agent,
            how: u_how.join(" · "),
        };

        let prompt_snippet = {
            let first_line = umsg
                .text
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('<'))
                .unwrap_or("");
            shorten(first_line, 26)
        };
        let mut tool_steps = 0usize;
        for k in (u_idx + 1)..next_u {
            let child = &messages[k];
            if child.role == "tool" {
                tool_steps += 1;
                let tb = if valid_backend(&child.backend) {
                    child.backend.as_str()
                } else {
                    turn_b
                };
                out[k].backend = tb.to_owned();
            } else if child.role == "assistant" {
                let ab = if valid_backend(&child.backend) {
                    child.backend.as_str()
                } else {
                    turn_b
                };
                let who_label = if !child.agent_name.trim().is_empty() {
                    format!("{} · {}", child.agent_name.trim(), backend_short_label(ab))
                } else {
                    backend_label(ab).to_owned()
                };
                let recipient_user = if !child.actor.trim().is_empty() {
                    child.actor.trim()
                } else if !umsg.actor.trim().is_empty() {
                    umsg.actor.trim()
                } else {
                    ""
                };
                let to_whom = match (!recipient_user.is_empty(), !prompt_snippet.is_empty()) {
                    (true, true) => format!("{recipient_user} · на «{prompt_snippet}»"),
                    (true, false) => recipient_user.to_owned(),
                    (false, true) => format!("на «{prompt_snippet}»"),
                    (false, false) => String::new(),
                };
                let mut a_how = Vec::new();
                let m_name = pretty_model_name(if !child.model.is_empty() {
                    &child.model
                } else {
                    &umsg.model
                });
                if !m_name.is_empty() {
                    a_how.push(m_name);
                }
                let m_mode = mode_short_label(if !child.mode.is_empty() {
                    &child.mode
                } else {
                    &umsg.mode
                });
                if !m_mode.is_empty() {
                    a_how.push(m_mode.into());
                }
                let m_eff = effort_short_label(if !child.effort.is_empty() {
                    &child.effort
                } else {
                    &umsg.effort
                });
                if !m_eff.is_empty() {
                    a_how.push(m_eff.into());
                }
                if tool_steps > 0 {
                    a_how.push(format!("через {tool_steps} {}", step_word_ru(tool_steps)));
                } else {
                    a_how.push("прямой ответ".into());
                }
                out[k] = MessageAttribution {
                    backend: ab.to_owned(),
                    who: who_label,
                    to_whom,
                    how: a_how.join(" · "),
                };
            }
        }
    }

    // Handle any orphan assistant messages before the first user message
    if let Some(first_end) = user_positions.first().copied().or(Some(messages.len())) {
        for k in 0..first_end {
            if messages[k].role == "assistant" {
                let ab = if valid_backend(&messages[k].backend) {
                    messages[k].backend.as_str()
                } else {
                    default_b
                };
                let mut a_how = Vec::new();
                let m_name = pretty_model_name(&messages[k].model);
                if !m_name.is_empty() {
                    a_how.push(m_name);
                }
                a_how.push("прямой ответ".into());
                out[k] = MessageAttribution {
                    backend: ab.to_owned(),
                    who: backend_label(ab).to_owned(),
                    to_whom: String::new(),
                    how: a_how.join(" · "),
                };
            } else if messages[k].role == "tool" {
                let tb = if valid_backend(&messages[k].backend) {
                    messages[k].backend.as_str()
                } else {
                    default_b
                };
                out[k].backend = tb.to_owned();
            }
        }
    }
    out
}

#[derive(Clone)]
pub struct Permission {
    pub id: String,
    pub tool: String,
    pub input: Value,
}

pub fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.into();
    }
    format!(
        "{}…",
        s.chars().take(max.saturating_sub(1)).collect::<String>()
    )
}
pub fn summary(v: &Value) -> String {
    for field in [
        "command",
        "file_path",
        "path",
        "pattern",
        "task",
        "description",
        "query",
    ] {
        if let Some(s) = v[field].as_str() {
            return s.into();
        }
    }
    if v.is_null() {
        String::new()
    } else {
        serde_json::to_string_pretty(v).unwrap_or_default()
    }
}

/// Distinct visual signature RGB for each engine, tuned for dark and light themes.
pub fn backend_rgb(backend: &str, light: bool) -> (u8, u8, u8) {
    match backend {
        "codex" => if light { (22, 152, 115) } else { (62, 214, 168) },
        "agy" => if light { (110, 74, 224) } else { (164, 130, 255) },
        "api" => if light { (37, 108, 218) } else { (94, 168, 255) },
        _ => if light { (202, 98, 52) } else { (236, 138, 92) },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolPhase {
    Research,
    Edit,
    Terminal,
    Subagent,
    Desktop,
    Other,
}

impl ToolPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Research => "ПОИСК",
            Self::Edit => "ПРАВКА",
            Self::Terminal => "КОМАНДА",
            Self::Subagent => "СУБАГЕНТ",
            Self::Desktop => "ЭТОТ ПК",
            Self::Other => "ДЕЙСТВИЕ",
        }
    }

    pub fn human_action(self) -> &'static str {
        match self {
            Self::Research => "Изучает проект",
            Self::Edit => "Меняет код",
            Self::Terminal => "Выполняет команду",
            Self::Subagent => "Координирует агентов",
            Self::Desktop => "Работает на этом ПК",
            Self::Other => "Выполняет шаг",
        }
    }

    pub fn rgb(self, light: bool) -> (u8, u8, u8) {
        match self {
            Self::Research => if light { (34, 126, 198) } else { (92, 184, 246) },
            Self::Edit => if light { (32, 150, 102) } else { (98, 210, 158) },
            Self::Terminal => if light { (192, 124, 28) } else { (238, 178, 88) },
            Self::Subagent => if light { (122, 74, 218) } else { (176, 132, 255) },
            Self::Desktop => if light { (196, 84, 66) } else { (236, 134, 114) },
            Self::Other => if light { (94, 112, 144) } else { (142, 162, 194) },
        }
    }
}

pub fn classify_tool(tool: &str) -> ToolPhase {
    if tool.starts_with("mcp__relay_cluster__")
        || matches!(
            tool,
            "Task"
                | "Agent"
                | "spawn_agent"
                | "invoke_subagent"
                | "define_subagent"
                | "swarm"
                | "run_workflow"
                | "send_message"
        )
    {
        ToolPhase::Subagent
    } else if tool.starts_with("mcp__relay_desktop__") {
        ToolPhase::Desktop
    } else if matches!(
        tool,
        "Edit"
            | "MultiEdit"
            | "Write"
            | "NotebookEdit"
            | "edit_file"
            | "write_file"
            | "fileChange"
            | "apply_patch"
    ) {
        ToolPhase::Edit
    } else if matches!(
        tool,
        "Bash" | "SlashCommand" | "run_command" | "commandExecution"
    ) {
        ToolPhase::Terminal
    } else if matches!(
        tool,
        "Read"
            | "Glob"
            | "Grep"
            | "LS"
            | "WebSearch"
            | "WebFetch"
            | "read_file"
            | "grep_search"
            | "find_by_name"
            | "search_web"
            | "webSearch"
    ) {
        ToolPhase::Research
    } else {
        ToolPhase::Other
    }
}

pub fn tool_target_preview(m: &Message, max: usize) -> String {
    let i = &m.input;
    let get = |k: &str| i[k].as_str().unwrap_or("").trim();
    let raw = match m.tool.as_str() {
        "Bash" | "run_command" | "commandExecution" | "mcp__relay_desktop__run_local_command" => {
            let cmd = get("command");
            let cmd = if cmd.is_empty() { get("CommandLine") } else { cmd };
            if cmd.is_empty() {
                String::new()
            } else {
                format!("$ {}", cmd.lines().next().unwrap_or("").trim())
            }
        }
        "Read" | "Write" | "Edit" | "MultiEdit" | "read_file" | "write_file" | "edit_file" => {
            let p = get("file_path");
            let p = if p.is_empty() { get("Path") } else { p };
            p.rsplit(['/', '\\'])
                .find(|part| !part.is_empty())
                .unwrap_or(p)
                .to_owned()
        }
        "Grep" | "grep_search" => {
            let pat = get("pattern");
            let pat = if pat.is_empty() { get("Query") } else { pat };
            if pat.is_empty() { String::new() } else { format!("«{pat}»") }
        }
        "Glob" | "find_by_name" => {
            let pat = get("pattern");
            let pat = if pat.is_empty() { get("Pattern") } else { pat };
            pat.to_owned()
        }
        "WebSearch" | "search_web" => get("query").to_owned(),
        "WebFetch" => get("url").to_owned(),
        "Task" | "Agent" => {
            let sub = get("subagent_type");
            let desc = get("description");
            if !sub.is_empty() && !desc.is_empty() {
                format!("{sub} · {desc}")
            } else if !desc.is_empty() {
                desc.to_owned()
            } else {
                get("prompt").lines().next().unwrap_or("").to_owned()
            }
        }
        "invoke_subagent" => {
            let role = get("Role");
            if !role.is_empty() {
                role.to_owned()
            } else {
                get("TypeName").to_owned()
            }
        }
        "spawn_agent" => {
            let role = get("agent_type");
            let msg = get("message");
            if !role.is_empty() {
                role.to_owned()
            } else {
                msg.lines().next().unwrap_or("").to_owned()
            }
        }
        "mcp__relay_cluster__dispatch_task" => {
            let worker = get("worker");
            let title = get("title");
            let task = get("task");
            if !worker.is_empty() && !title.is_empty() {
                format!("{worker} -> {title}")
            } else if !worker.is_empty() && !task.is_empty() {
                format!("{worker} -> {}", task.lines().next().unwrap_or(""))
            } else {
                worker.to_owned()
            }
        }
        _ => {
            let s = summary(i);
            if s.starts_with('{') {
                String::new()
            } else {
                s.lines().next().unwrap_or("").trim().to_owned()
            }
        }
    };
    shorten(&raw, max)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubagentTrace {
    pub role: String,
    pub task: String,
    pub running: bool,
    pub failed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnTelemetry {
    pub total_actions: usize,
    pub done_actions: usize,
    pub running_actions: usize,
    pub failed_actions: usize,
    pub reads: usize,
    pub edits: usize,
    pub commands: usize,
    pub delegations: usize,
    pub active_phase: Option<ToolPhase>,
    pub active_headline: String,
    pub active_target: String,
    pub subagents: Vec<SubagentTrace>,
}

pub fn analyze_turn(
    messages: &[Message],
    start_from: usize,
    busy: bool,
    fallback_status: &str,
) -> TurnTelemetry {
    let slice = if start_from < messages.len() {
        &messages[start_from..]
    } else {
        &[]
    };
    struct Entry<'a> {
        call: &'a Message,
        done: bool,
        failed: bool,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut by_id: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut orphan_failures = 0usize;
    for msg in slice {
        if msg.role == "tool" {
            if !msg.tool_id.is_empty() {
                by_id.insert(msg.tool_id.as_str(), entries.len());
            }
            entries.push(Entry {
                call: msg,
                done: false,
                failed: false,
            });
        } else if msg.role == "tool_result" {
            let owner = if msg.tool_id.is_empty() {
                entries.iter().rposition(|e| !e.done)
            } else {
                by_id.get(msg.tool_id.as_str()).copied()
            };
            if let Some(idx) = owner {
                entries[idx].done = true;
                entries[idx].failed |= msg.failed;
            } else if msg.failed {
                orphan_failures += 1;
            }
        }
    }

    let mut out = TurnTelemetry {
        total_actions: entries.len() + orphan_failures,
        failed_actions: orphan_failures,
        ..Default::default()
    };

    for entry in &entries {
        let phase = classify_tool(&entry.call.tool);
        match phase {
            ToolPhase::Research => out.reads += 1,
            ToolPhase::Edit => out.edits += 1,
            ToolPhase::Terminal | ToolPhase::Desktop => out.commands += 1,
            ToolPhase::Subagent => out.delegations += 1,
            ToolPhase::Other => {}
        }
        let running = busy && !entry.done;
        if running {
            out.running_actions += 1;
            out.active_phase = Some(phase);
            let custom_desc = entry.call.input["description"]
                .as_str()
                .map(str::trim)
                .filter(|d| !d.is_empty());
            out.active_headline = custom_desc
                .map(|d| shorten(d, 58))
                .unwrap_or_else(|| phase.human_action().to_owned());
            out.active_target = tool_target_preview(entry.call, 64);
        } else {
            out.done_actions += 1;
        }
        if entry.failed {
            out.failed_actions += 1;
        }

        if phase == ToolPhase::Subagent
            && !matches!(
                entry.call.tool.as_str(),
                "mcp__relay_cluster__list_workers"
                    | "mcp__relay_cluster__cluster_status"
                    | "mcp__relay_cluster__wait_for_task"
                    | "mcp__relay_cluster__task_status"
            )
        {
            let i = &entry.call.input;
            let role = i["Role"]
                .as_str()
                .or_else(|| i["worker"].as_str())
                .or_else(|| i["subagent_type"].as_str())
                .or_else(|| i["agent_type"].as_str())
                .or_else(|| i["TypeName"].as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Субагент");
            let task = i["description"]
                .as_str()
                .or_else(|| i["title"].as_str())
                .or_else(|| i["task"].as_str())
                .or_else(|| i["Prompt"].as_str())
                .or_else(|| i["prompt"].as_str())
                .or_else(|| i["message"].as_str())
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .trim();
            out.subagents.push(SubagentTrace {
                role: shorten(role.trim(), 28),
                task: shorten(task, 54),
                running,
                failed: entry.failed,
            });
        }
    }

    if out.active_headline.is_empty() && busy {
        let streaming_text = slice
            .last()
            .is_some_and(|m| m.role == "assistant" && !m.text.trim().is_empty());
        out.active_headline = if streaming_text {
            "Формирует ответ".into()
        } else if !fallback_status.trim().is_empty() {
            shorten(fallback_status.trim(), 58)
        } else {
            "Анализирует задачу".into()
        };
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_profiles_migrate_without_losing_the_light_theme() {
        let s: Settings =
            serde_json::from_str(r#"{"servers":[],"selected":null,"light":true}"#).unwrap();
        assert_eq!(s.theme(), "paper");
        assert!(s.appearance.wrap_output);
        assert!(s.cluster.is_empty());
        let mut s = s;
        s.appearance.theme = "forest".into();
        let restored: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(restored.theme(), "forest");
    }
    #[test]
    fn unicode_truncation() {
        assert_eq!(shorten("Привет мир", 7), "Привет…");
    }
    #[test]
    fn profiles_contain_no_password() {
        let s = serde_json::to_string(&Server::default()).unwrap();
        assert!(!s.contains("password"));
    }
    #[test]
    fn existing_servers_gain_an_independent_disabled_api_profile() {
        let mut server = Server::default();
        server.api.enabled = true;server.api.base_url = "https://example.test/v1".into();server.api.model = "custom-model".into();
        let mut old = serde_json::to_value(&server).unwrap();old.as_object_mut().unwrap().remove("api");
        let restored: Server = serde_json::from_value(old).unwrap();
        assert!(!restored.api.enabled);assert!(restored.api.base_url.is_empty());
        assert_eq!(restored.host, server.host);assert_eq!(restored.cwd, server.cwd);
        let public = serde_json::to_string(&server).unwrap();
        assert!(!public.contains("api_key"));assert!(!public.contains("\"key\""));
        assert_eq!(server.api.request("fixture-only-key".into())["key"], "fixture-only-key");
    }
    #[test]
    fn api_profile_rejects_credentials_in_the_url_and_preserves_custom_model_ids() {
        let mut api = ApiProfile::default();api.model = "vendor/model-1".into();
        for bad in ["file:///tmp/model", "https://key@example.test/v1", "https://example.test/v1?key=secret", "https://example.test/v1\n"] {
            api.base_url = bad.into();
            // A trailing newline is trimmed; embedded whitespace must still be rejected.
            if bad.ends_with('\n') { api.base_url = "https://example.test/\nv1".into(); }
            assert!(api.validate().is_err());
        }
        api.base_url = "http://127.0.0.1:8000/v1/".into();api.validate().unwrap();
        assert_eq!(api.model,"vendor/model-1");assert_eq!(api.base_url,"http://127.0.0.1:8000/v1");
    }
    #[test]
    fn validates_addresses_without_shell_interpretation() {
        let mut s = Server {
            host: "127.0.0.1".into(),
            ..Default::default()
        };
        assert!(s.validate().is_ok());
        s.host = "host\ncommand".into();
        assert!(s.validate().is_err());
    }
    #[test]
    fn codex_and_agy_server_and_session_backends_round_trip() {
        let mut s = Server::default();
        assert_eq!(s.effective_backend(), "claude");
        s.set_default_backend("codex");
        s.set_model_for("codex", "gpt-5.4".into());
        assert_eq!(s.effective_backend(), "codex");
        assert!(!s.api.enabled);
        assert_eq!(s.model_for("codex"), "gpt-5.4");
        assert_eq!(s.model_for("claude"), "default");

        let mut settings = Settings::default();
        let chat = "srv/chat-1".to_owned();
        assert_eq!(settings.session_backend(&s, &chat, None), "codex");
        assert_eq!(settings.session_backend(&s, &chat, Some("agy")), "agy");
        settings.set_session_backend(chat.clone(), "agy");
        assert_eq!(settings.session_backend(&s, &chat, Some("claude")), "agy");
        assert!(!settings.api_sessions[&chat]);
        settings.set_session_backend(chat.clone(), "api");
        assert_eq!(settings.session_backend(&s, &chat, None), "api");
        assert!(settings.api_sessions[&chat]);
    }

    #[test]
    fn turn_telemetry_tracks_phases_parallel_tools_and_subagents() {
        let msgs = vec![
            Message {
                role: "user".into(),
                text: "Ship next-gen UI".into(),
                ..Default::default()
            },
            Message {
                role: "tool".into(),
                tool: "Grep".into(),
                tool_id: "t1".into(),
                input: serde_json::json!({"pattern": "draw_graph", "path": "src/"}),
                ..Default::default()
            },
            Message {
                role: "tool_result".into(),
                tool_id: "t1".into(),
                text: "src/app/cluster_view.rs".into(),
                ..Default::default()
            },
            Message {
                role: "tool".into(),
                tool: "invoke_subagent".into(),
                tool_id: "t2".into(),
                input: serde_json::json!({"Role": "TUI Engineer", "Prompt": "Build radar popup"}),
                ..Default::default()
            },
            Message {
                role: "tool".into(),
                tool: "Edit".into(),
                tool_id: "t3".into(),
                input: serde_json::json!({"file_path": "/opt/relay/src/timeline.rs", "description": "Обновляю конвейер шагов"}),
                ..Default::default()
            },
        ];
        let tel = analyze_turn(&msgs, 0, true, "Работает");
        assert_eq!(tel.total_actions, 3);
        assert_eq!(tel.done_actions, 1);
        assert_eq!(tel.running_actions, 2);
        assert_eq!(tel.reads, 1);
        assert_eq!(tel.edits, 1);
        assert_eq!(tel.delegations, 1);
        assert_eq!(tel.active_phase, Some(ToolPhase::Edit));
        assert_eq!(tel.active_headline, "Обновляю конвейер шагов");
        assert_eq!(tel.active_target, "timeline.rs");
        assert_eq!(tel.subagents.len(), 1);
        assert_eq!(tel.subagents[0].role, "TUI Engineer");
        assert!(tel.subagents[0].running);
        assert_ne!(backend_rgb("claude", false), backend_rgb("codex", false));
        assert_ne!(backend_rgb("agy", false), backend_rgb("api", false));
    }
}
