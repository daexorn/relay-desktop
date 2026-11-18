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
}
