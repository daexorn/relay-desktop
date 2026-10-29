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
}

#[derive(Clone, Default, Deserialize, Debug)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub modified: f64,
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
