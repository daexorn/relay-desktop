use crate::connection::{Bus, Event};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc::UnboundedSender;

struct Download {
    target: PathBuf,
    temp: PathBuf,
    file: File,
    size: u64,
    sequence: u64,
    hash: Sha256,
}
impl Drop for Download {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temp);
    }
}
struct TestJob {
    request: Value,
    dir: PathBuf,
    next: usize,
    cancel: Arc<AtomicBool>,
}
struct ArtifactRequest {
    source: String,
    target: Option<PathBuf>,
}
pub struct Desktop {
    pending: HashMap<String, Value>,
    downloads: HashMap<String, Download>,
    artifacts: HashMap<String, ArtifactRequest>,
    workspace: Option<PathBuf>,
    policies: HashMap<String, bool>,
    requests: UnboundedSender<Value>,
    bus: Bus,
    server: String,
    commands: HashMap<String, Arc<AtomicBool>>,
    workers: Vec<std::thread::JoinHandle<()>>,
    /// In-game tests: "auto", "ask" or "off", chosen in the composer.
    tests: String,
    jobs: HashMap<String, TestJob>,
}
impl Desktop {
    pub fn new(requests: UnboundedSender<Value>, bus: Bus, server: String) -> Self {
        Self {
            pending: HashMap::new(),
            downloads: HashMap::new(),
            artifacts: HashMap::new(),
            workspace: None,
            policies: HashMap::new(),
            requests,
            bus,
            server,
            commands: HashMap::new(),
            workers: vec![],
            tests: "ask".into(),
            jobs: HashMap::new(),
        }
    }
    fn test_status(&self, id: &str, stage: &str, message: impl Into<String>, result: Value) {
        let purpose = self.jobs.get(id).map(|j| j.request["input"]["purpose"].clone());
        self.bus.send(Event::Data {
            server: self.server.clone(),
            value: json!({"type":"game_test_status","id":id,"stage":stage,"message":message.into(),"result":result,"purpose":purpose}),
        });
    }
    /// Asks the server for the next mod jar of a test, or starts the game once all arrived.
    fn advance_test(&mut self, id: &str) {
        let Some(job) = self.jobs.get(id) else {
            return;
        };
        let mods = job.request["input"]["mods"].as_array().cloned().unwrap_or_default();
        if job.next < mods.len() {
            let name = file_name(mods[job.next]["name"].as_str().unwrap_or("mod.jar"));
            let message = format!("Передаём моды · {}/{}", job.next + 1, mods.len());
            let index = job.next;
            match self.begin(job.dir.join("mods").join(name)) {
                Ok(download) => {
                    self.downloads.insert(id.to_owned(), download);
                    let _ = self
                        .requests
                        .send(json!({"op":"desktop_download","id":id,"index":index}));
                    self.test_status(id, "transfer", message, Value::Null);
                }
                Err(error) => self.fail_test(id, error),
            }
            return;
        }
        let input = &job.request["input"];
        let test = crate::gametest::Job {
            version: input["minecraft_version"].as_str().unwrap_or("1.21.8").into(),
            loader: input["loader_version"].as_str().unwrap_or("").into(),
            scenario: input["scenario"].as_str().unwrap_or("").into(),
            timeout: Duration::from_secs(input["timeout_seconds"].as_u64().unwrap_or(300).clamp(60, 900)),
            mods: job.dir.join("mods"),
            out: job.dir.join("out"),
        };
        let cancel = job.cancel.clone();
        let purpose = job.request["input"]["purpose"].clone();
        let requests = self.requests.clone();
        let bus = self.bus.clone();
        let server = self.server.clone();
        let id = id.to_owned();
        let worker_id = id.clone();
        self.workers.retain(|worker| !worker.is_finished());
        self.workers.push(std::thread::spawn(move || {
            let id = worker_id;
            let status = |stage: &str, message: String, result: Value| {
                bus.send(Event::Data {
                    server: server.clone(),
                    value: json!({"type":"game_test_status","id":id,"stage":stage,"message":message,"result":result,"purpose":purpose}),
                });
            };
            let mut result = match crate::gametest::run(&test, &cancel, &|m| status("running", m, Value::Null)) {
                Ok(v) => v,
                Err(e) => json!({"ok":false,"message":format!("{e:#}")}),
            };
            status("upload", "Отправляем результаты на сервер".into(), Value::Null);
            let mut sent = 0u64;
            let mut names = vec![];
            for path in crate::gametest::results(&test.out) {
                let Ok(body) = fs::read(&path) else { continue };
                if sent + body.len() as u64 > 60 * 1024 * 1024 {
                    break;
                }
                sent += body.len() as u64;
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                for (sequence, chunk) in body.chunks(128 * 1024).enumerate() {
                    let _ = requests.send(json!({"op":"desktop_upload","id":id,"name":name,"sequence":sequence,"data":STANDARD.encode(chunk)}));
                }
                let _ = requests.send(json!({"op":"desktop_upload_end","id":id,"name":name}));
                names.push(name);
            }
            result["uploaded"] = json!(names);
            result["local_folder"] = json!(test.out);
            status("done", String::new(), result.clone());
            let _ = requests.send(json!({"op":"desktop_result","id":id,"result":result}));
        }));
        if let Some(job) = self.jobs.get_mut(&id) {
            job.next = usize::MAX;
        }
    }
    fn fail_test(&mut self, id: &str, error: impl ToString) {
        self.test_status(id, "done", error.to_string(), json!({"ok":false,"message":error.to_string()}));
        self.jobs.remove(id);
        self.downloads.remove(id);
        self.fail(id, error);
    }
    fn result(&self, id: &str, result: Value) {
        let _ = self
            .requests
            .send(json!({"op":"desktop_result","id":id,"result":result}));
    }
    fn complete(&mut self, id: &str, result: Value) {
        if let Some(request) = self.artifacts.remove(id) {
            let path = request.source;
            let ok = result["ok"].as_bool().unwrap_or(false);
            self.bus.send(Event::Data{server:self.server.clone(),value:json!({"type":"artifact_status","id":id,"path":path,"stage":if ok{"saved"}else{"failed"},"message":result["message"],"local_path":result["path"]})});
            let _ = self.requests.send(json!({"op":"file_cancel","id":id}));
        } else {
            self.result(id, result);
        }
    }
    fn fail(&self, id: &str, message: impl ToString) {
        self.result(id, json!({"ok":false,"message":message.to_string()}));
    }
    /// Only messages from the in-process UI queue can enter this method.
    pub fn local(&mut self, value: &Value) -> bool {
        let id = value["id"].as_str().unwrap_or("");
        match value["op"].as_str().unwrap_or("") {
            "_artifact_download" => {
                if uuid::Uuid::parse_str(id).is_ok()
                    && self.artifacts.len() < 8
                    && !self.artifacts.contains_key(id)
                {
                    self.artifacts.insert(
                        id.into(),
                        ArtifactRequest {
                            source: value["path"].as_str().unwrap_or("").into(),
                            target: value["target"].as_str().map(PathBuf::from),
                        },
                    );
                    let _ = self.requests.send(
                        json!({"op":"file_info","id":id,"path":value["path"],"cwd":value["cwd"]}),
                    );
                } else {
                    self.bus.send(Event::Data{server:self.server.clone(),value:json!({"type":"artifact_status","id":id,"stage":"failed","message":"Уже скачивается несколько файлов. Повтори чуть позже."})});
                }
            }
            "_desktop_workspace" => {
                let workspace = value["path"]
                    .as_str()
                    .and_then(|p| Path::new(p).canonicalize().ok())
                    .filter(|p| p.is_dir());
                if self.workspace != workspace {
                    for id in self.pending.keys() {
                        self.fail(id, "Local connection changed");
                    }
                    self.commands
                        .values()
                        .for_each(|cancel| cancel.store(true, Ordering::Relaxed));
                    self.commands.clear();
                    self.pending.clear();
                    let artifacts = &self.artifacts;
                    self.downloads.retain(|id, _| artifacts.contains_key(id));
                    self.policies.clear();
                }
                self.workspace = workspace;
            }
            "_desktop_policy" => {
                if let Some(sid) = value["sid"].as_str().filter(|sid| !sid.is_empty()) {
                    let auto = value["auto"].as_bool().unwrap_or(false);
                    self.policies.insert(sid.into(), auto);
                    if auto && self.workspace.is_some() {
                        let pending: Vec<_> = self
                            .pending
                            .iter()
                            .filter(|(_, req)| req["sid"] == sid)
                            .map(|(id, _)| id.clone())
                            .collect();
                        for id in pending {
                            self.bus.send(Event::Data {
                                server: self.server.clone(),
                                value: json!({"type":"desktop_cancel","id":id}),
                            });
                            self.approve_auto(&id);
                        }
                    }
                }
            }
            "_tests_policy" => {
                if let Some(mode) = value["mode"].as_str().filter(|m| ["auto", "ask", "off"].contains(m)) {
                    self.tests = mode.into();
                }
            }
            "_game_test_stop" => {
                if let Some(job) = self.jobs.get(id) {
                    job.cancel.store(true, Ordering::Relaxed);
                    if job.next != usize::MAX {
                        self.fail_test(id, "Тест остановлен пользователем");
                    }
                }
            }
            "_desktop_deny" => {
                if self.pending.remove(id).is_some() {
                    self.fail(id, "The user declined this action in Relay.");
                }
            }
            "_desktop_approve" => {
                if let Some(request) = self.pending.remove(id) {
                    if request["tool"] == "minecraft_test" {
                        let dir = crate::gametest::root().join("runs").join(id);
                        let _ = fs::remove_dir_all(&dir);
                        if let Err(error) = fs::create_dir_all(dir.join("mods")) {
                            self.fail(id, error);
                            return true;
                        }
                        self.jobs.insert(
                            id.to_owned(),
                            TestJob { request, dir, next: 0, cancel: Arc::new(AtomicBool::new(false)) },
                        );
                        self.advance_test(id);
                        return true;
                    }
                    if self.workspace.is_none() {
                        self.fail(id, "Local access is disabled");
                        return true;
                    }
                    if request["tool"] == "run_local_command" {
                        let cwd = self.workspace.clone().unwrap();
                        let command = request["input"]["command"]
                            .as_str()
                            .unwrap_or("")
                            .to_owned();
                        let timeout = request["input"]["timeout_seconds"]
                            .as_u64()
                            .unwrap_or(60)
                            .clamp(1, 120);
                        let requests = self.requests.clone();
                        let bus = self.bus.clone();
                        let server = self.server.clone();
                        let id = id.to_owned();
                        self.commands
                            .retain(|_, cancel| !cancel.load(Ordering::Relaxed));
                        let closed = Arc::new(AtomicBool::new(false));
                        self.commands.insert(id.clone(), closed.clone());
                        self.workers.retain(|worker| !worker.is_finished());
                        let worker = std::thread::spawn(move || {
                            let result =
                                match run(&command, &cwd, Duration::from_secs(timeout), &closed) {
                                    Ok(v) => v,
                                    Err(e) => json!({"ok":false,"message":e.to_string()}),
                                };
                            closed.store(true, Ordering::Relaxed);
                            bus.send(Event::Data {
                                server,
                                value: json!({"type":"desktop_completed","id":id,"result":result}),
                            });
                            let _ = requests
                                .send(json!({"op":"desktop_result","id":id,"result":result}));
                        });
                        self.workers.push(worker);
                    } else {
                        let target = value["target"].as_str().map(PathBuf::from);
                        match target.and_then(|target| self.begin(target).ok()) {
                            Some(download) => {
                                self.downloads.insert(id.to_owned(), download);
                                let _ =
                                    self.requests.send(json!({"op":"desktop_download","id":id}));
                            }
                            None => self.fail(
                                id,
                                "Не удалось создать временный файл рядом с выбранным назначением.",
                            ),
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }
    fn approve_auto(&mut self, id: &str) {
        let Some(request) = self.pending.get(id).cloned() else {
            return;
        };
        let target = if request["tool"] == "download_file" {
            let result = (|| -> Result<PathBuf> {
                let destination = request["input"]["destination"].as_str();
                let target = if let Some(path) = destination {
                    anyhow::ensure!(
                        !path.trim().is_empty()
                            && path.len() <= 4096
                            && !path.contains('\0')
                            && !path.ends_with('/')
                            && !path.ends_with('\\'),
                        "Укажи путь к файлу"
                    );
                    let path = PathBuf::from(path);
                    if path.is_absolute() {
                        path
                    } else {
                        self.workspace.as_ref().context("ПК отключён")?.join(path)
                    }
                } else {
                    dirs::download_dir()
                        .or_else(|| self.workspace.clone())
                        .context("Не найдена папка загрузок")?
                        .join(file_name(
                            request["input"]["name"].as_str().unwrap_or("download.bin"),
                        ))
                };
                anyhow::ensure!(target.file_name().is_some(), "Укажи имя файла");
                if let Ok(metadata) = target.symlink_metadata() {
                    anyhow::ensure!(metadata.is_file(), "Назначение должно быть обычным файлом");
                }
                fs::create_dir_all(target.parent().context("Нет папки назначения")?)?;
                Ok(target)
            })();
            match result {
                Ok(path) => Some(path),
                Err(error) => {
                    self.pending.remove(id);
                    self.fail(id, error);
                    return;
                }
            }
        } else {
            None
        };
        self.local(&json!({"op":"_desktop_approve","id":id,"target":target}));
    }
    fn begin(&self, target: PathBuf) -> Result<Download> {
        if let Ok(metadata) = target.symlink_metadata() {
            anyhow::ensure!(metadata.is_file(), "Назначение должно быть обычным файлом");
        }
        let parent = target.parent().context("У файла нет папки")?;
        let temp = parent.join(format!(".relay-{}.part", uuid::Uuid::new_v4()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        Ok(Download {
            target,
            temp,
            file,
            size: 0,
            sequence: 0,
            hash: Sha256::new(),
        })
    }
    /// Automatic execution requires a policy set through the local UI queue
    /// for this conversation. Other requests wait for explicit approval.
    pub fn incoming(&mut self, value: &Value) -> bool {
        let id = value["id"].as_str().unwrap_or("");
        match value["type"].as_str().unwrap_or("") {
            "artifact_status" => true,
            "artifact_info" => {
                if !self.artifacts.contains_key(id) || self.downloads.contains_key(id) {
                    return true;
                }
                let result = (|| -> Result<Download> {
                    anyhow::ensure!(
                        value["error"].as_str().is_none(),
                        "{}",
                        value["error"].as_str().unwrap_or("Ошибка скачивания")
                    );
                    anyhow::ensure!(
                        value["size"]
                            .as_u64()
                            .is_some_and(|size| size <= 512 * 1024 * 1024),
                        "Неверный размер файла"
                    );
                    let target = if let Some(target) = self
                        .artifacts
                        .get(id)
                        .and_then(|request| request.target.clone())
                    {
                        target
                    } else {
                        let folder = dirs::download_dir()
                            .or_else(dirs::home_dir)
                            .context("Не найдена папка загрузок")?;
                        fs::create_dir_all(&folder)?;
                        folder.join(file_name(
                            value["name"].as_str().context("Нет имени файла")?,
                        ))
                    };
                    self.begin(target)
                })();
                match result {
                    Ok(download) => {
                        self.downloads.insert(id.into(), download);
                        let _ = self.requests.send(json!({"op":"file_stream","id":id}));
                    }
                    Err(error) => {
                        self.complete(id, json!({"ok":false,"message":error.to_string()}))
                    }
                }
                true
            }
            "desktop_request" => {
                let valid = uuid::Uuid::parse_str(id).is_ok()
                    && !self.pending.contains_key(id)
                    && !self.downloads.contains_key(id)
                    && !self.commands.contains_key(id)
                    && self.pending.len() + self.downloads.len() < 8
                    && (self.workspace.is_some() || value["tool"] == "minecraft_test")
                    && match value["tool"].as_str() {
                        Some("minecraft_test") => {
                            self.tests != "off"
                                && self.jobs.len() < 2
                                && value["input"]["scenario"].as_str().is_some_and(|s| s.len() <= 65536)
                                && value["input"]["mods"].as_array().is_some_and(|mods| {
                                    (1..=16).contains(&mods.len())
                                        && mods.iter().all(|m| {
                                            m["size"].as_u64().is_some_and(|s| s <= 256 * 1024 * 1024)
                                                && m["name"].as_str().is_some_and(|n| file_name(n) == n && n.ends_with(".jar"))
                                        })
                                })
                        }
                        Some("download_file") => value["input"]["size"]
                            .as_u64()
                            .is_some_and(|s| s <= 512 * 1024 * 1024),
                        Some("run_local_command") => {
                            value["input"]["command"].as_str().is_some_and(|s| {
                                !s.trim().is_empty() && s.len() <= 16000 && !s.contains('\0')
                            })
                        }
                        _ => false,
                    };
                if valid {
                    self.pending.insert(id.to_owned(), value.clone());
                    let mut request = value.clone();
                    request["local_workspace"] = json!(self.workspace);
                    let auto = if request["tool"] == "minecraft_test" {
                        self.tests == "auto"
                    } else {
                        request["sid"]
                            .as_str()
                            .and_then(|sid| self.policies.get(sid))
                            .copied()
                            .unwrap_or(false)
                    };
                    if auto {
                        self.approve_auto(id);
                    } else {
                        self.bus.send(Event::Data {
                            server: self.server.clone(),
                            value: request,
                        });
                    }
                } else if value["tool"] == "minecraft_test" && self.tests == "off" {
                    self.fail(id, "The user turned in-game tests off in Relay. Continue without them and say what stays unverified.");
                } else {
                    self.fail(id,"Local access is off or the request is invalid. Ask the user to choose a local workspace in Relay.");
                }
                true
            }
            "desktop_cancel" => {
                self.pending.remove(id);
                if let Some(job) = self.jobs.remove(id) {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                if let Some(cancel) = self.commands.remove(id) {
                    cancel.store(true, Ordering::Relaxed);
                }
                self.downloads.remove(id);
                false
            }
            "desktop_chunk" => {
                let result = (|| -> Result<()> {
                    let download = self.downloads.get_mut(id).context("No approved download")?;
                    anyhow::ensure!(
                        value["sequence"].as_u64() == Some(download.sequence),
                        "Нарушен порядок частей файла"
                    );
                    let encoded = value["data"].as_str().context("Нет данных")?;
                    anyhow::ensure!(encoded.len() <= 180000, "Часть файла слишком большая");
                    let body = STANDARD.decode(encoded)?;
                    download.size += body.len() as u64;
                    anyhow::ensure!(download.size <= 512 * 1024 * 1024, "Файл слишком большой");
                    download.file.write_all(&body)?;
                    download.hash.update(&body);
                    download.sequence += 1;
                    Ok(())
                })();
                if let Err(error) = result {
                    self.downloads.remove(id);
                    if self.jobs.contains_key(id) {
                        self.fail_test(id, error);
                    } else {
                        self.complete(id, json!({"ok":false,"message":error.to_string()}));
                    }
                }
                true
            }
            "desktop_end" => {
                if let Some(mut download) = self.downloads.remove(id) {
                    let result = (|| -> Result<Value> {
                        anyhow::ensure!(
                            value["error"].as_str().is_none(),
                            "{}",
                            value["error"].as_str().unwrap_or("Ошибка передачи")
                        );
                        anyhow::ensure!(
                            value["size"].as_u64() == Some(download.size),
                            "Размер файла не совпал"
                        );
                        let hash = format!("{:x}", download.hash.clone().finalize());
                        anyhow::ensure!(
                            value["sha256"].as_str() == Some(hash.as_str()),
                            "Контрольная сумма файла не совпала"
                        );
                        download.file.flush()?;
                        download.file.sync_all()?;
                        if let Ok(metadata) = download.target.symlink_metadata() {
                            anyhow::ensure!(
                                metadata.is_file(),
                                "Назначение изменилось во время передачи"
                            );
                        }
                        let backup = if download.target.symlink_metadata().is_ok() {
                            let backup = download
                                .target
                                .with_file_name(format!(".relay-backup-{}", uuid::Uuid::new_v4()));
                            fs::rename(&download.target, &backup)?;
                            Some(backup)
                        } else {
                            None
                        };
                        if let Err(error) = fs::rename(&download.temp, &download.target) {
                            if let Some(backup) = &backup {
                                let _ = fs::rename(backup, &download.target);
                            }
                            return Err(error.into());
                        }
                        Ok(
                            json!({"ok":true,"path":download.target,"size":download.size,"sha256":hash,"previous_file_backup":backup}),
                        )
                    })();
                    if self.jobs.contains_key(id) {
                        match result {
                            Ok(_) => {
                                if let Some(job) = self.jobs.get_mut(id) {
                                    job.next += 1;
                                }
                                self.advance_test(id);
                            }
                            Err(error) => self.fail_test(id, error),
                        }
                        return true;
                    }
                    match result {
                        Ok(result) => {
                            if !self.artifacts.contains_key(id) {
                                self.bus.send(Event::Data{server:self.server.clone(),value:json!({"type":"desktop_status","message":format!("Скачан файл: {}",download.target.file_name().unwrap_or_default().to_string_lossy())})});
                            }
                            self.complete(id, result);
                        }
                        Err(error) => {
                            self.complete(id, json!({"ok":false,"message":error.to_string()}))
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        self.commands
            .values()
            .for_each(|cancel| cancel.store(true, Ordering::Relaxed));
        let until = Instant::now() + Duration::from_secs(3);
        for worker in self.workers.drain(..) {
            while !worker.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    truncated: bool,
}
async fn capture(
    mut stream: impl tokio::io::AsyncRead + Unpin,
    buffer: Arc<std::sync::Mutex<Capture>>,
) {
    use tokio::io::AsyncReadExt;
    let mut part = [0u8; 4096];
    while let Ok(count) = stream.read(&mut part).await {
        if count == 0 {
            break;
        }
        let mut output = buffer.lock().unwrap();
        let keep = count.min(16384usize.saturating_sub(output.bytes.len()));
        output.bytes.extend_from_slice(&part[..keep]);
        output.truncated |= keep < count;
    }
}
fn run(command: &str, cwd: &Path, timeout: Duration, closed: &AtomicBool) -> Result<Value> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        anyhow::ensure!(!closed.load(Ordering::Relaxed),"Connection closed");
        #[cfg(windows)] let mut process={let mut c=tokio::process::Command::new("powershell.exe");c.args(["-NoLogo","-NoProfile","-NonInteractive","-Command",&format!("[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); $OutputEncoding = [Console]::OutputEncoding; {command}")]);use std::os::windows::process::CommandExt;c.as_std_mut().creation_flags(0x08000000|0x00000200);c};
        #[cfg(unix)] let mut process={let mut c=tokio::process::Command::new("/bin/sh");c.args(["-c",command]);use std::os::unix::process::CommandExt;c.as_std_mut().process_group(0);c};
        let mut child=process.current_dir(cwd).kill_on_drop(true).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn()?;
        let output=Arc::new(std::sync::Mutex::new(Capture::default()));let errors=Arc::new(std::sync::Mutex::new(Capture::default()));
        let mut out=tokio::spawn(capture(child.stdout.take().unwrap(),output.clone()));let mut err=tokio::spawn(capture(child.stderr.take().unwrap(),errors.clone()));
        let started=Instant::now();let mut timed_out=false;let mut cancelled=false;
        let status=loop {
            if let Some(status)=child.try_wait()?{break status;}
            timed_out=started.elapsed()>timeout;cancelled=closed.load(Ordering::Relaxed);
            if timed_out||cancelled {
                if let Some(pid)=child.id(){
                    #[cfg(unix)] {let _=std::process::Command::new("kill").args(["-KILL","--",&format!("-{pid}")]).stderr(std::process::Stdio::null()).status();}
                    #[cfg(windows)] {let _=std::process::Command::new("taskkill.exe").args(["/PID",&pid.to_string(),"/T","/F"]).output();}
                }
                let _=child.kill().await;break child.wait().await?;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        // Detached applications may inherit pipes. Close our readers rather
        // than waiting forever or killing a deliberately launched application.
        let mut pipes_truncated=false;
        for task in [&mut out,&mut err] {
            if tokio::time::timeout(Duration::from_millis(250),&mut *task).await.is_err(){pipes_truncated=true;task.abort();let _=task.await;}
        }
        let stdout=output.lock().unwrap();let stderr=errors.lock().unwrap();
        Ok(json!({"ok":status.success() && !timed_out && !cancelled,"exit_code":status.code(),"stdout":String::from_utf8_lossy(&stdout.bytes),"stderr":String::from_utf8_lossy(&stderr.bytes),"output_truncated":stdout.truncated||stderr.truncated||pipes_truncated,"timed_out":timed_out,"cancelled":cancelled,"workspace":cwd,"os":std::env::consts::OS}))
    })
}

pub fn file_name(value: &str) -> String {
    let name: String = value
        .chars()
        .filter(|c| !c.is_control() && !"/\\:".contains(*c))
        .take(180)
        .collect();
    if name.trim_matches('.').is_empty() {
        "download.bin".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_filename_has_no_path() {
        assert_eq!(file_name("../x\\bad:name\n.jar"), "..xbadname.jar");
        assert_eq!(file_name(".."), "download.bin");
    }
    #[test]
    fn transfers_require_approval_and_verify_the_hash_before_replacing() {
        let (controller, _events) =
            crate::connection::Controller::new(eframe::egui::Context::default());
        let (sender, mut outgoing) = tokio::sync::mpsc::unbounded_channel();
        let mut manager = Desktop::new(sender, controller.bus.clone(), "test".into());
        let folder = std::env::temp_dir().join(format!("relay-transfer-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&folder).unwrap();
        let target = folder.join("mod.jar");
        fs::write(&target, b"old mod").unwrap();
        manager.local(&json!({"op":"_desktop_workspace","path":folder}));
        let id = uuid::Uuid::new_v4().to_string();
        let request =
            json!({"type":"desktop_request","id":id,"tool":"download_file","input":{"size":7}});
        manager.incoming(&request);
        manager.incoming(&json!({"type":"desktop_chunk","id":id,"sequence":0,"data":STANDARD.encode(b"new mod")}));
        assert_eq!(fs::read(&target).unwrap(), b"old mod");
        assert!(manager.downloads.is_empty());
        manager.local(&json!({"op":"_desktop_approve","id":id,"target":target}));
        manager.incoming(&json!({"type":"desktop_chunk","id":id,"sequence":0,"data":STANDARD.encode(b"new mod")}));
        manager.incoming(&json!({"type":"desktop_end","id":id,"size":7,"sha256":"invalid"}));
        assert_eq!(fs::read(&target).unwrap(), b"old mod");
        let id = uuid::Uuid::new_v4().to_string();
        manager.incoming(
            &json!({"type":"desktop_request","id":id,"tool":"download_file","input":{"size":7}}),
        );
        manager.local(&json!({"op":"_desktop_approve","id":id,"target":target}));
        manager.incoming(&json!({"type":"desktop_chunk","id":id,"sequence":0,"data":STANDARD.encode(b"new mod")}));
        let hash = format!("{:x}", Sha256::digest(b"new mod"));
        manager.incoming(&json!({"type":"desktop_end","id":id,"size":7,"sha256":hash}));
        assert_eq!(fs::read(&target).unwrap(), b"new mod");
        assert!(manager.downloads.is_empty());
        assert!(std::iter::from_fn(|| outgoing.try_recv().ok())
            .any(|event| event["result"]["ok"] == true));
        drop(manager);
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn commands_never_run_from_network_events_without_local_approval() {
        let (controller, _) = crate::connection::Controller::new(eframe::egui::Context::default());
        let (sender, mut outgoing) = tokio::sync::mpsc::unbounded_channel();
        let mut manager = Desktop::new(sender, controller.bus.clone(), "test".into());
        let id = uuid::Uuid::new_v4().to_string();
        let request = json!({"type":"desktop_request","id":id,"tool":"run_local_command","input":{"command":"echo example"}});
        manager.incoming(&request);
        assert_eq!(outgoing.try_recv().unwrap()["result"]["ok"], false);
        assert!(manager.commands.is_empty());
        manager.local(&json!({"op":"_desktop_workspace","path":std::env::temp_dir()}));
        manager.incoming(&request);
        assert!(manager.commands.is_empty());
        assert_eq!(manager.pending.len(), 1);
        manager.local(&json!({"op":"_desktop_deny","id":id}));
        assert!(manager.pending.is_empty());
        assert!(manager.commands.is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn automatic_commands_require_a_local_policy_for_the_exact_session() {
        let (controller, events) =
            crate::connection::Controller::new(eframe::egui::Context::default());
        let (sender, mut outgoing) = tokio::sync::mpsc::unbounded_channel();
        let mut manager = Desktop::new(sender, controller.bus.clone(), "test".into());
        manager.local(&json!({"op":"_desktop_workspace","path":std::env::temp_dir()}));
        let id = uuid::Uuid::new_v4().to_string();
        manager.incoming(&json!({"type":"desktop_request","sid":"A","id":id,"tool":"run_local_command","auto":true,"input":{"command":"printf AUTO_PC"}}));
        assert!(manager.commands.is_empty());
        assert_eq!(manager.pending.len(), 1);
        manager.local(&json!({"op":"_desktop_policy","sid":"B","auto":true}));
        assert!(manager.commands.is_empty());
        manager.local(&json!({"op":"_desktop_policy","sid":"A","auto":true}));
        assert!(manager.pending.is_empty());
        let until = Instant::now() + Duration::from_secs(3);
        let result = loop {
            if let Ok(result) = outgoing.try_recv() {
                break result;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(result["result"]["stdout"], "AUTO_PC");
        assert!(events
            .try_iter()
            .any(|e| matches!(e,Event::Data{value,..} if value["type"]=="desktop_completed")));
    }
    #[test]
    fn automatic_download_can_target_an_absolute_path_outside_starting_directory() {
        let (controller, events) =
            crate::connection::Controller::new(eframe::egui::Context::default());
        let (sender, mut outgoing) = tokio::sync::mpsc::unbounded_channel();
        let mut manager = Desktop::new(sender, controller.bus.clone(), "test".into());
        let folder = std::env::temp_dir().join(format!("relay-auto-{}", uuid::Uuid::new_v4()));
        let start = folder.join("start");
        fs::create_dir_all(&start).unwrap();
        manager.local(&json!({"op":"_desktop_workspace","path":start}));
        manager.local(&json!({"op":"_desktop_policy","sid":"A","auto":true}));
        let target = folder.join("other/mods/example.jar");
        let id = uuid::Uuid::new_v4().to_string();
        manager.incoming(&json!({"type":"desktop_request","sid":"A","id":id,"tool":"download_file","input":{"name":"example.jar","size":3,"destination":target}}));
        assert_eq!(outgoing.try_recv().unwrap()["op"], "desktop_download");
        assert!(!events
            .try_iter()
            .any(|e| matches!(e,Event::Data{value,..} if value["type"]=="desktop_request")));
        manager.incoming(
            &json!({"type":"desktop_chunk","id":id,"sequence":0,"data":STANDARD.encode(b"jar")}),
        );
        manager.incoming(&json!({"type":"desktop_end","id":id,"size":3,"sha256":format!("{:x}",Sha256::digest(b"jar"))}));
        assert_eq!(fs::read(&target).unwrap(), b"jar");
        drop(manager);
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn card_download_works_without_pc_control_and_reports_saved_only_after_verification() {
        let (controller, events) =
            crate::connection::Controller::new(eframe::egui::Context::default());
        let (sender, mut outgoing) = tokio::sync::mpsc::unbounded_channel();
        let mut manager = Desktop::new(sender, controller.bus.clone(), "test".into());
        let folder = std::env::temp_dir().join(format!("relay-card-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&folder).unwrap();
        let target = folder.join("mod.jar");
        let id = uuid::Uuid::new_v4().to_string();
        manager.local(&json!({"op":"_artifact_download","id":id,"path":"/server/mod.jar","cwd":"/server","target":target}));
        assert!(manager.workspace.is_none());
        assert!(manager.policies.is_empty());
        assert_eq!(outgoing.try_recv().unwrap()["op"], "file_info");
        manager.incoming(&json!({"type":"artifact_info","id":id,"name":"mod.jar","size":3}));
        assert_eq!(outgoing.try_recv().unwrap()["op"], "file_stream");
        manager.incoming(
            &json!({"type":"desktop_chunk","id":id,"sequence":0,"data":STANDARD.encode(b"jar")}),
        );
        assert!(!target.exists());
        assert!(!events.try_iter().any(|e|matches!(e,Event::Data{value,..} if value["type"]=="artifact_status" && value["stage"]=="saved")));
        manager.incoming(&json!({"type":"desktop_end","id":id,"size":3,"sha256":format!("{:x}",Sha256::digest(b"jar"))}));
        assert_eq!(fs::read(&target).unwrap(), b"jar");
        assert!(manager.artifacts.is_empty());
        assert!(events.try_iter().any(|e|matches!(e,Event::Data{value,..} if value["type"]=="artifact_status" && value["stage"]=="saved" && value["local_path"]==target.to_string_lossy().as_ref())));
        drop(manager);
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn missing_artifact_is_an_error_and_unsolicited_info_is_ignored() {
        let (controller, events) =
            crate::connection::Controller::new(eframe::egui::Context::default());
        let (sender, _) = tokio::sync::mpsc::unbounded_channel();
        let mut manager = Desktop::new(sender, controller.bus.clone(), "test".into());
        let id = uuid::Uuid::new_v4().to_string();
        manager.incoming(&json!({"type":"artifact_info","id":id,"name":"mod.jar","size":3}));
        assert!(manager.downloads.is_empty());
        manager.local(
            &json!({"op":"_artifact_download","id":id,"path":"/missing/mod.jar","cwd":"/server"}),
        );
        manager.incoming(&json!({"type":"artifact_info","id":id,"error":"Файл не найден"}));
        assert!(manager.artifacts.is_empty());
        assert!(manager.downloads.is_empty());
        assert!(events.try_iter().any(|e|matches!(e,Event::Data{value,..} if value["stage"]=="failed" && value["message"]=="Файл не найден")));
    }
    #[cfg(windows)]
    #[test]
    fn windows_uses_powershell_and_returns_output() {
        let v = run(
            "Write-Output RELAY_LOCAL_WINDOWS",
            &std::env::temp_dir(),
            Duration::from_secs(15),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(v["exit_code"], 0);
        assert!(v["stdout"]
            .as_str()
            .unwrap()
            .contains("RELAY_LOCAL_WINDOWS"));
    }
    #[cfg(unix)]
    #[test]
    fn command_has_a_real_exit_code_and_output() {
        let v = run(
            "printf relay; printf problem >&2; exit 7",
            &std::env::temp_dir(),
            Duration::from_secs(2),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(v["stdout"], "relay");
        assert_eq!(v["stderr"], "problem");
        assert_eq!(v["exit_code"], 7);
        assert_eq!(v["ok"], false);
    }
    #[cfg(unix)]
    #[test]
    fn command_timeout_terminates_children() {
        let at = Instant::now();
        let v = run(
            "sleep 20",
            &std::env::temp_dir(),
            Duration::from_millis(60),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(v["timed_out"], true);
        assert!(at.elapsed() < Duration::from_secs(3));
    }
}
