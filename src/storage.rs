use crate::model::Settings;
use anyhow::{Context, Result};
use std::{fs, path::PathBuf};

pub fn directory() -> PathBuf {
    std::env::var_os("RELAY_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("relay-desktop")
        })
}
pub fn load() -> Result<Settings> {
    let p = directory().join("settings.json");
    if !p.exists() {
        return Ok(Settings::default());
    }
    serde_json::from_slice(&fs::read(&p)?)
        .context("Не удалось прочитать сохранённые серверы. Файл оставлен на месте.")
}
pub fn save(settings: &Settings) -> Result<()> {
    let dir = directory();
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.join("settings.json");
    let temp = dir.join("settings.pending");
    fs::write(&temp, serde_json::to_vec_pretty(settings)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
    }
    // Windows cannot atomically rename over an existing file; keep a recovery copy.
    #[cfg(windows)]
    if path.exists() {
        fs::copy(&path, dir.join("settings.backup.json"))?;
        fs::remove_file(&path)?;
    }
    fs::rename(temp, path)?;
    Ok(())
}
pub fn save_private_json(name: &str, value: &impl serde::Serialize) -> Result<()> {
    anyhow::ensure!(name == "cluster-sessions.json" || name == "pending-messages.json", "Unknown store");
    let dir = directory();
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    anyhow::ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "История кластера превышает 64 МБ."
    );
    let path = dir.join(name);
    let temp = dir.join(format!("{name}.tmp"));
    fs::write(&temp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    if path.exists() {
        fs::copy(&path, dir.join(format!("{name}.backup")))?;
        fs::remove_file(&path)?;
    }
    fs::rename(temp, path)?;
    Ok(())
}
/// A store written by save_private_json; missing or unreadable gives the default.
pub fn load_private_json<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    fs::read(directory().join(name))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
fn entry(id: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new("dev.relay.desktop", id)?)
}
pub fn password(id: &str) -> Option<String> {
    entry(id).ok()?.get_password().ok()
}
pub fn remember(id: &str, password: &str) -> Result<()> {
    entry(id)?.set_password(password).context("Хранилище паролей системы недоступно. Подключение работает, пароль останется только до закрытия приложения.")
}
pub fn forget(id: &str) {
    if let Ok(e) = entry(id) {
        let _ = e.delete_credential();
    }
}
