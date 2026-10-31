use crate::model::Server;
use anyhow::{Context, Result};
use eframe::egui;
use russh::{client, ChannelMsg};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::BufRead,
    sync::{mpsc, Arc},
    time::Duration,
};
use tokio::sync::{mpsc as async_mpsc, oneshot};
use zeroize::Zeroizing;

pub enum Event {
    Epoch {
        server: String,
        generation: u64,
        event: Box<Event>,
    },
    Trust {
        server: String,
        fingerprint: String,
        answer: oneshot::Sender<bool>,
    },
    Data {
        server: String,
        value: Value,
    },
    Error {
        server: String,
        message: String,
    },
    Disconnected {
        server: String,
    },
    Files {
        server: String,
        sid: String,
        paths: Vec<std::path::PathBuf>,
    },
    Images {
        server: String,
        sid: String,
        images: Vec<crate::images::Attachment>,
        errors: Vec<String>,
    },
    Workspace {
        server: String,
        path: Option<String>,
    },
    Password {
        server: Server,
        password: Option<String>,
    },
}
#[derive(Clone)]
pub struct Bus {
    pub tx: mpsc::Sender<Event>,
    pub ctx: egui::Context,
    epoch: Option<(String, u64)>,
}
impl Bus {
    pub fn send(&self, event: Event) {
        let event = if let Some((server, generation)) = &self.epoch {
            Event::Epoch {
                server: server.clone(),
                generation: *generation,
                event: Box::new(event),
            }
        } else {
            event
        };
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}
pub struct Controller {
    pub runtime: tokio::runtime::Runtime,
    channels: HashMap<String, async_mpsc::UnboundedSender<Value>>,
    pub bus: Bus,
    generations: HashMap<String, u64>,
}
impl Controller {
    pub fn new(ctx: egui::Context) -> (Self, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("async runtime");
        (
            Self {
                runtime,
                channels: HashMap::new(),
                generations: HashMap::new(),
                bus: Bus {
                    tx,
                    ctx,
                    epoch: None,
                },
            },
            rx,
        )
    }
    pub fn connect(&mut self, server: Server, password: String) {
        self.disconnect(&server.id);
        let (tx, rx) = async_mpsc::unbounded_channel();
        self.channels.insert(server.id.clone(), tx);
        let mut bus = self.bus.clone();
        bus.epoch = Some((server.id.clone(), self.generations[&server.id]));
        self.runtime.spawn(async move {
            let id = server.id.clone();
            if let Err(error) = run(server, Zeroizing::new(password), bus.clone(), rx).await {
                bus.send(Event::Error {
                    server: id.clone(),
                    message: format!("{error:#}"),
                });
            }
            bus.send(Event::Disconnected { server: id });
        });
    }
    pub fn send(&self, server: &str, value: Value) -> bool {
        self.channels
            .get(server)
            .is_some_and(|c| c.send(value).is_ok())
    }
    pub fn approve_desktop(&self, server: &str, request: &Value) {
        let Some(sender) = self.channels.get(server).cloned() else {
            return;
        };
        let id = request["id"].as_str().unwrap_or("").to_owned();
        if request["tool"] == "download_file" {
            let name = crate::desktop::file_name(
                request["input"]["name"].as_str().unwrap_or("download.bin"),
            );
            let workspace = request["local_workspace"]
                .as_str()
                .map(std::path::PathBuf::from);
            self.runtime.spawn(async move {
                let mut dialog = rfd::AsyncFileDialog::new();
                if let Some(workspace) = workspace {
                    dialog = dialog.set_directory(workspace);
                }
                let target = dialog
                    .set_title("Сохранить файл с сервера")
                    .set_file_name(name)
                    .save_file()
                    .await;
                let value = match target {
                    Some(target) => json!({"op":"_desktop_approve","id":id,"target":target.path()}),
                    None => json!({"op":"_desktop_deny","id":id}),
                };
                let _ = sender.send(value);
            });
        } else {
            let _ = sender.send(json!({"op":"_desktop_approve","id":id}));
        }
    }
    pub fn disconnect(&mut self, server: &str) {
        if let Some(sender) = self.channels.remove(server) {
            let _ = sender.send(json!({"op":"_disconnect"}));
        }
        *self.generations.entry(server.into()).or_default() += 1;
    }
    pub fn is_current(&self, server: &str, generation: u64) -> bool {
        self.generations.get(server).copied() == Some(generation)
    }
}
struct Handler {
    server: String,
    expected: Option<String>,
    bus: Bus,
}
impl client::Handler for Handler {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let fingerprint = key
            .public_key()
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string();
        if let Some(expected) = &self.expected {
            if expected == &fingerprint {
                return Ok(true);
            }
            self.bus.send(Event::Error{server:self.server.clone(),message:format!("Ключ сервера изменился. Ожидался {expected}, получен {fingerprint}. Проверь сервер перед изменением подключения.")});
            return Ok(false);
        }
        let (tx, rx) = oneshot::channel();
        self.bus.send(Event::Trust {
            server: self.server.clone(),
            fingerprint,
            answer: tx,
        });
        Ok(tokio::time::timeout(Duration::from_secs(180), rx)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or(false))
    }
}
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
async fn run(
    server: Server,
    password: Zeroizing<String>,
    bus: Bus,
    mut requests: async_mpsc::UnboundedReceiver<Value>,
) -> Result<()> {
    let config = Arc::new(client::Config {
        inactivity_timeout: None,
        keepalive_interval: Some(Duration::from_secs(20)),
        keepalive_max: 3,
        ..Default::default()
    });
    let stream = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::net::TcpStream::connect((server.host.as_str(), server.port)),
    )
    .await
    .context("Сервер не ответил за 15 секунд.")?
    .context("Не удалось подключиться. Проверь адрес и порт.")?;
    let handler = Handler {
        server: server.id.clone(),
        expected: server.fingerprint.clone(),
        bus: bus.clone(),
    };
    let mut session = client::connect_stream(config, stream, handler)
        .await
        .context("Не удалось установить защищённое подключение.")?;
    let auth = tokio::time::timeout(
        Duration::from_secs(25),
        session.authenticate_password(&server.user, password.as_str()),
    )
    .await
    .context("Сервер не ответил при входе.")??;
    anyhow::ensure!(auth.success(), "Сервер не принял логин или пароль.");
    drop(password);
    let mut channel = session.channel_open_session().await?;
    channel
        .exec(
            true,
            format!(
                "python3 -u -c {}",
                shell_quote(&format!(
                    "DESKTOP_SOURCE = {}\n{}",
                    serde_json::to_string(include_str!("../bridge/desktop_mcp.py"))?,
                    include_str!("../bridge/relay_bridge.py")
                ))
            ),
        )
        .await?;
    let (local_tx, mut local_rx) = async_mpsc::unbounded_channel::<Value>();
    let mut desktop = crate::desktop::Desktop::new(local_tx, bus.clone(), server.id.clone());
    let mut buffer = Vec::new();
    let mut stderr = String::new();
    let mut greeted = false;
    let deadline = tokio::time::sleep(Duration::from_secs(35));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            Some(value)=local_rx.recv()=>{let mut encoded=serde_json::to_vec(&value)?;encoded.push(b'\n');channel.data(&encoded[..]).await?;},
            _=&mut deadline, if !greeted => anyhow::bail!("Не удалось запустить помощник. На сервере нужны Python 3 и Claude Code."),
            request=requests.recv()=>match request {
                Some(value)=>{if value["op"]=="_disconnect"{let _=channel.eof().await;break;}if desktop.local(&value){continue;}let mut encoded=serde_json::to_vec(&value)?;encoded.push(b'\n');channel.data(&encoded[..]).await?;},
                None=>{let _=channel.eof().await;break;}
            },
            incoming=channel.wait()=>match incoming {
                Some(ChannelMsg::Data{data})=>{
                    buffer.extend_from_slice(&data);
                    anyhow::ensure!(buffer.len()<32*1024*1024,"Ответ сервера слишком большой.");
                    while let Some(end)=buffer.iter().position(|b|*b==b'\n') {
                        let line:Vec<_>=buffer.drain(..=end).collect();
                        let value:Value=serde_json::from_slice(&line).context("Сервер вернул некорректный ответ.")?;
                        if value["type"]=="hello" {greeted=true;}
                        if !desktop.incoming(&value){bus.send(Event::Data{server:server.id.clone(),value});}
                    }
                },
                Some(ChannelMsg::ExtendedData{data,..})=>{if stderr.len()<4000 {stderr.push_str(&String::from_utf8_lossy(&data));}},
                Some(ChannelMsg::ExitStatus{exit_status}) if exit_status!=0=>anyhow::bail!("Не удалось запустить помощник на сервере. {}",stderr.trim()),
                Some(ChannelMsg::Close)|None=>break,
                _=>{}
            }
        }
    }
    let _ = session
        .disconnect(russh::Disconnect::ByApplication, "Relay disconnected", "en")
        .await;
    Ok(())
}
/// Credentials come from stdin, never argv or files.
pub fn probe() -> Result<()> {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let value: Value = serde_json::from_str(&line)?;
    let profile: Server = serde_json::from_value(value["server"].clone())?;
    anyhow::ensure!(
        profile.fingerprint.is_some(),
        "Probe requires a known fingerprint"
    );
    let ctx = egui::Context::default();
    let (mut controller, rx) = Controller::new(ctx);
    let password = value["password"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            if value["use_keyring"] == true {
                crate::storage::password(&profile.id)
            } else {
                None
            }
        })
        .context("Probe credentials unavailable")?;
    controller.connect(profile.clone(), password);
    line.clear();
    let exercise = value["exercise"].as_bool().unwrap_or(false);
    let desktop_probe = value["desktop_probe"].as_bool().unwrap_or(false);
    let mut desktop_exercised = false;
    let sid = value["sid"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let prompt = value["prompt"]
        .as_str()
        .unwrap_or("Reply with the single word READY. Do not use tools.")
        .to_owned();
    let images = value["images"].as_array().cloned().unwrap_or_default();
    let files = value["files"].as_array().cloned().unwrap_or_default();
    let mode = value["mode"].as_str().unwrap_or("plan").to_owned();
    let mut reply = String::new();
    let mut count = 0;
    let until = std::time::Instant::now() + Duration::from_secs(150);
    while std::time::Instant::now() < until {
        match rx.recv_timeout(Duration::from_secs(1)).map(|e| match e {
            Event::Epoch { event, .. } => *event,
            event => event,
        }) {
            Ok(Event::Data { value, .. }) if value["type"] == "hello" => {
                controller.send(&profile.id, json!({"op":"sessions"}));
            }
            Ok(Event::Data { value, .. }) if value["type"] == "sessions" => {
                count = value["sessions"].as_array().map(Vec::len).unwrap_or(0);
                if !exercise {
                    println!("{}", json!({"ok":true,"sessions":count}));
                    controller.disconnect(&profile.id);
                    return Ok(());
                }
                if desktop_probe {
                    controller.send(
                        &profile.id,
                        json!({"op":"_desktop_workspace","path":std::env::temp_dir()}),
                    );
                }
                controller.send(&profile.id,json!({"op":"prompt","sid":sid,"cwd":"/tmp","mode":mode,"model":"default","text":prompt,"images":images,"files":files,"desktop_enabled":desktop_probe,"desktop_context":{"os":std::env::consts::OS,"workspace":std::env::temp_dir()}}));
            }
            Ok(Event::Data { value: frame, .. }) if frame["type"] == "desktop_request" => {
                // Only this fixed harmless probe command can bypass the test UI.
                if desktop_probe
                    && frame["tool"] == "run_local_command"
                    && frame["input"]["command"] == "printf RELAY_DESKTOP_PROBE"
                {
                    desktop_exercised = true;
                    controller.approve_desktop(&profile.id, &frame);
                } else {
                    controller.send(&profile.id, json!({"op":"_desktop_deny","id":frame["id"]}));
                }
            }
            Ok(Event::Data { value: frame, .. }) if frame["type"] == "claude" => {
                let data = &frame["data"];
                if data["type"] == "assistant" {
                    if let Some(blocks) = data["message"]["content"].as_array() {
                        for b in blocks {
                            if let Some(text) = b["text"].as_str() {
                                reply.push_str(text);
                            }
                        }
                    }
                }
                if data["type"] == "result" {
                    anyhow::ensure!(
                        !data["is_error"].as_bool().unwrap_or(false),
                        "Claude returned an error: {}",
                        data
                    );
                    anyhow::ensure!(
                        !desktop_probe || desktop_exercised,
                        "Claude did not call the desktop probe tool"
                    );
                    println!(
                        "{}",
                        json!({"ok":true,"sessions":count,"sid":sid,"response":reply,"desktop_exercised":desktop_exercised})
                    );
                    controller.disconnect(&profile.id);
                    return Ok(());
                }
            }
            Ok(Event::Data { value: frame, .. }) if frame["type"] == "permission" => {
                controller.send(
                    &profile.id,
                    json!({"op":"permission","sid":frame["sid"],"id":frame["id"],"allow":false}),
                );
            }
            Ok(Event::Data { value: frame, .. })
                if frame["type"] == "error" || frame["type"] == "fatal" =>
            {
                anyhow::bail!("{}", frame["message"])
            }
            Ok(Event::Data { value: frame, .. }) if frame["type"] == "run_closed" => {
                anyhow::bail!("Claude closed: {}", frame["detail"])
            }
            Ok(Event::Error { message, .. }) => anyhow::bail!(message),
            _ => {}
        }
    }
    anyhow::bail!("Transport probe timed out")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_all_shell_metacharacters() {
        assert_eq!(shell_quote("hello'\n$(echo x)"), "'hello'\\''\n$(echo x)'");
    }
}
