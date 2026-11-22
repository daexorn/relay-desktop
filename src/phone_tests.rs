use crate::{connection::{Controller, Event}, model::Server, storage};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use std::{thread, time::Duration};
use zeroize::Zeroizing;

pub fn profile(id: &str) -> Result<Server> {
    let settings = storage::load()?;
    let server = settings.servers.into_iter().find(|s| s.id == id || s.name == id)
        .context("Профиль Relay не найден")?;
    ensure!(server.remember && server.fingerprint.is_some(), "Подключись к серверу в Relay и сохрани пароль");
    Ok(server)
}

pub fn pair(server: &Server) -> Result<String> {
    let name = format!("phone-tests:{}", server.id);
    if let Some(existing) = storage::password(&name) { return Ok(existing); }
    let secret = uuid::Uuid::new_v4().simple().to_string();
    storage::remember(&name, &secret)?;
    Ok(secret)
}

/// Launch the paired worker independently of the current Relay window.
pub fn start_from_ui(server: &Server) -> Result<String> {
    let server = profile(&server.id)?;
    ensure!(storage::password(&server.id).is_some(), "Сохрани пароль подключения в профиле сервера.");
    let secret = pair(&server)?;
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.args(["--phone-tests", &server.id]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn()?;
    thread::spawn(move || { let _ = child.wait(); });
    Ok(secret)
}

#[cfg(target_os = "linux")]
pub fn login_entry(id: &str) -> Result<std::path::PathBuf> {
    // The name is stable even for an imported profile with a non-UUID id.
    use sha2::Digest;
    let name = format!("relay-phone-{:x}.desktop", sha2::Sha256::digest(id.as_bytes()));
    Ok(dirs::config_dir().context("Не найдена папка настроек")?.join("autostart").join(name))
}

#[cfg(target_os = "linux")]
pub fn set_login_start(id: &str, enabled: bool) -> Result<()> {
    let file = login_entry(id)?;
    if !enabled {
        if file.exists() { std::fs::remove_file(file)?; }
        return Ok(());
    }
    fn argument(value: &str) -> Result<String> {
        ensure!(!value.contains(['\n', '\r']), "Недопустимый путь автозапуска");
        let mut result = String::from("\"");
        for ch in value.chars() {
            if matches!(ch, '\\' | '"' | '$' | '`') { result.push('\\'); }
            if ch == '%' { result.push('%'); }
            result.push(ch);
        }
        result.push('"');
        Ok(result.replace('\\', "\\\\"))
    }
    let exe = std::env::current_exe()?;
    let exec = argument(exe.to_str().context("Недопустимый путь Relay")?)?;
    let profile = argument(id)?;
    std::fs::create_dir_all(file.parent().context("Нет папки автозапуска")?)?;
    std::fs::write(file, format!("[Desktop Entry]\nType=Application\nName=Relay — тесты с телефона\nExec={exec} --phone-tests {profile}\nTerminal=false\n"))?;
    Ok(())
}

pub fn serve(server: Server) -> Result<()> {
    use sha2::Digest;
    let dir = storage::directory();
    std::fs::create_dir_all(&dir)?;
    let lock_path = dir.join(format!("worker-{:x}.lock", sha2::Sha256::digest(server.id.as_bytes())));
    let lock = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(lock_path)?;
    match lock.try_lock() {
        Ok(()) => {},
        Err(std::fs::TryLockError::WouldBlock) => return Ok(()),
        Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
    }
    let (mut controller, rx) = Controller::headless();
    let mut delay = 1;
    loop {
        let credentials = storage::password(&format!("phone-tests:{}", server.id))
            .zip(storage::password(&server.id));
        let Some((key, password)) = credentials else {
            // The desktop keyring can become available after the worker starts.
            thread::sleep(Duration::from_secs(delay));
            delay = (delay * 2).min(30);
            continue;
        };
        controller.connect_with_worker(server.clone(), password, Some(Zeroizing::new(key)));
        loop {
            if let Event::Epoch { server: id, generation, event } = rx.recv()? {
                if !controller.is_current(&id, generation) { continue; }
                match *event {
                    Event::Data { value, .. } if value["type"] == "hello" => { delay = 1; }
                    Event::Data { value, .. } if value["type"] == "game_test_status" => {
                        controller.send(&id, json!({"op":"phone_progress","id":value["id"],"message":value["message"]}));
                    }
                    Event::Trust { answer, .. } => { let _ = answer.send(false); }
                    Event::Error { message, .. } => eprintln!("{message}"),
                    Event::Disconnected { .. } => break,
                    _ => {}
                }
            }
        }
        thread::sleep(Duration::from_secs(delay));
        delay = (delay * 2).min(30);
    }
}

#[derive(Default)]
pub struct Gate {
    nonce: Option<String>,
    active: Option<Value>,
    next_mod: usize,
}
impl Gate {
    pub fn challenge(&mut self) -> Result<Value> {
        ensure!(self.active.is_none(), "Тест уже идёт");
        let nonce = uuid::Uuid::new_v4().to_string();
        self.nonce = Some(nonce.clone());
        Ok(json!({"op":"phone_challenge","nonce":nonce}))
    }

    pub fn approve(&mut self, frame: &Value, key: &str) -> Result<Value> {
        let payload = frame["payload"].as_str().context("Отсутствует запрос телефона")?;
        let tag = STANDARD.decode(frame["signature"].as_str().unwrap_or(""))?;
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key.as_bytes())?;
        mac.update(payload.as_bytes());
        mac.verify_slice(&tag).context("Код сопряжения телефона и ПК различается")?;
        let request: Value = serde_json::from_str(payload)?;
        let nonce = self.nonce.take().context("Подтверждение уже использовано")?;
        ensure!(request["nonce"] == nonce && request["type"] == "desktop_request", "Подтверждение устарело");
        ensure!(matches!(request["tool"].as_str(), Some("minecraft_test" | "run_local_command")), "Неизвестный вид теста");
        self.active = Some(request.clone());
        Ok(request)
    }

    pub fn permits(&mut self, frame: &Value) -> Result<bool> {
        let Some(job) = &self.active else { return Ok(false); };
        if frame["id"] != job["id"] { return Ok(false); }
        match frame["type"].as_str() {
            Some("desktop_end") if frame.get("error").is_none() => {
                let hash = job["input"]["mods"][self.next_mod]["sha256"].as_str().context("Нет подписанной суммы мода")?;
                ensure!(frame["sha256"] == hash, "Мод изменился после подтверждения");
                self.next_mod += 1;
                Ok(true)
            }
            Some("desktop_cancel" | "desktop_chunk" | "desktop_end") => Ok(true),
            _ => bail!("Неизвестное событие теста"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_signature_binds_command_and_one_connection() {
        let mut gate = Gate::default();
        let nonce = gate.challenge().unwrap()["nonce"].clone();
        let request = json!({"nonce":nonce,"type":"desktop_request","tool":"run_local_command","id":"fixture","input":{"command":"cargo test"}});
        let payload = request.to_string();
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(b"fixture-pair").unwrap();
        mac.update(payload.as_bytes());
        let frame = json!({"payload":payload,"signature":STANDARD.encode(mac.finalize().into_bytes())});
        let mut changed = frame.clone();
        changed["payload"] = json!(payload.replace("cargo test", "cargo build"));
        assert!(gate.approve(&changed, "fixture-pair").is_err());
        assert!(gate.approve(&frame, "wrong-key").is_err());
        assert_eq!(gate.approve(&frame, "fixture-pair").unwrap(), request);
        assert!(gate.approve(&frame,"fixture-pair").is_err());
        let mut next = Gate::default();
        next.challenge().unwrap();
        assert!(next.approve(&frame,"fixture-pair").is_err());
    }
}
