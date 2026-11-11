use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        }
    }
}
impl Server {
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
    fn validates_addresses_without_shell_interpretation() {
        let mut s = Server {
            host: "127.0.0.1".into(),
            ..Default::default()
        };
        assert!(s.validate().is_ok());
        s.host = "host\ncommand".into();
        assert!(s.validate().is_err());
    }
}
