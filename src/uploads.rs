use crate::{
    connection::{Bus, Event},
    images::MAX_ATTACHMENT_BYTES,
};
use anyhow::{Context, Result};
use russh::{client, Channel, ChannelMsg};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use tokio::{io::AsyncReadExt, sync::oneshot};

type SshChannel = Channel<client::Msg>;

pub fn needed(request: &Value) -> bool {
    matches!(request["op"].as_str(), Some("prompt" | "_upload_probe"))
        && request["files"].as_array().is_some_and(|files| {
            files
                .iter()
                .any(|f| f["type"] == "file" && f["path"].is_string())
        })
}

pub fn failure(bus: &Bus, server: &str, sid: &str, message: impl ToString) {
    bus.send(Event::Data {
        server: server.into(),
        value: json!({
            "type":"error", "op":"prompt", "sid":sid, "upload_failed":true,
            "message":message.to_string()
        }),
    });
}

pub async fn run(
    mut channel: SshChannel,
    request: Value,
    bus: Bus,
    server: String,
    mut cancelled: oneshot::Receiver<()>,
) -> Result<Value> {
    let result = tokio::select! {
        result = transfer(&mut channel, request, &bus, &server) => result,
        _ = &mut cancelled => Err(anyhow::anyhow!("Отправка файлов отменена.")),
    };
    let _ = channel.close().await;
    result
}

async fn reply(channel: &mut SshChannel) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mut buffer = Vec::new();
        let mut stderr = String::new();
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    buffer.extend_from_slice(&data);
                    anyhow::ensure!(
                        buffer.len() <= 32768,
                        "Некорректный ответ при загрузке файла."
                    );
                    if let Some(end) = buffer.iter().position(|b| *b == b'\n') {
                        return serde_json::from_slice(&buffer[..end])
                            .context("Не удалось подтвердить передачу файла.");
                    }
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    if stderr.len() < 2000 {
                        stderr.push_str(&String::from_utf8_lossy(&data));
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) if exit_status != 0 => {
                    anyhow::bail!("Не удалось загрузить файл. {}", stderr.trim());
                }
                Some(ChannelMsg::Close | ChannelMsg::Eof) | None => {
                    anyhow::bail!("Передача файла прервалась. {}", stderr.trim());
                }
                _ => {}
            }
        }
    })
    .await
    .context("Сервер не подтвердил передачу файла за 60 секунд.")?
}

async fn write_json(channel: &SshChannel, value: Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    channel.data(&bytes[..]).await?;
    Ok(())
}

async fn transfer(
    channel: &mut SshChannel,
    mut request: Value,
    bus: &Bus,
    server: &str,
) -> Result<Value> {
    let sid = request["sid"]
        .as_str()
        .context("Нет сессии для вложений.")?
        .to_owned();
    let sources = request["files"]
        .as_array()
        .context("Некорректные вложения.")?;
    anyhow::ensure!(
        !sources.is_empty() && sources.len() <= 8,
        "До восьми файлов в сообщении."
    );
    let mut files = Vec::new();
    let mut headers = Vec::new();
    let mut total = 0u64;
    for source in sources {
        let path = source["path"].as_str().context("Прикрепи файл ещё раз.")?;
        let file = tokio::fs::File::open(path)
            .await
            .context("Файл больше не доступен на этом ПК.")?;
        let metadata = file.metadata().await?;
        anyhow::ensure!(
            metadata.is_file() && metadata.len() <= MAX_ATTACHMENT_BYTES as u64,
            "Можно отправлять обычные файлы до 512 МБ."
        );
        anyhow::ensure!(
            source["bytes"].as_u64() == Some(metadata.len()),
            "Файл изменился после выбора. Прикрепи его ещё раз."
        );
        total += metadata.len();
        headers.push(json!({"name":source["name"], "bytes":metadata.len()}));
        files.push((file, metadata));
    }
    anyhow::ensure!(
        total <= MAX_ATTACHMENT_BYTES as u64,
        "Общий размер файлов — до 512 МБ."
    );
    let upload_id = uuid::Uuid::new_v4().to_string();
    channel
        .exec(
            true,
            format!(
                "python3 -u -c {}",
                crate::connection::shell_quote(include_str!("../bridge/attachments.py"))
            ),
        )
        .await?;
    write_json(
        channel,
        json!({"sid":sid, "upload_id":upload_id, "files":headers}),
    )
    .await?;
    let ready = reply(channel).await?;
    anyhow::ensure!(
        ready["ready"] == true,
        "{}",
        ready["message"]
            .as_str()
            .unwrap_or("Сервер не готов принять файлы.")
    );

    let mut buffer = vec![0u8; 256 * 1024];
    let mut digests = Vec::new();
    let mut sent = 0u64;
    let mut update = Instant::now();
    for (index, (mut file, metadata)) in files.into_iter().enumerate() {
        let mut digest = Sha256::new();
        let mut remaining = metadata.len();
        while remaining > 0 {
            let wanted = buffer.len().min(remaining as usize);
            let size = file.read(&mut buffer[..wanted]).await?;
            anyhow::ensure!(size > 0, "Файл изменился во время отправки.");
            digest.update(&buffer[..size]);
            channel.data(&buffer[..size]).await?;
            remaining -= size as u64;
            sent += size as u64;
            if sent == size as u64
                || update.elapsed() >= Duration::from_millis(200)
                || sent == total
            {
                bus.send(Event::Data {
                    server: server.into(),
                    value: json!({
                    "type":"upload_progress", "sid":sid, "sent":sent, "total":total, "upload_id":upload_id,
                        "name":headers[index]["name"]
                    }),
                });
                update = Instant::now();
            }
        }
        let mut extra = [0u8; 1];
        anyhow::ensure!(
            file.read(&mut extra).await? == 0
                && file.metadata().await?.modified().ok() == metadata.modified().ok(),
            "Файл изменился во время отправки. Прикрепи его ещё раз."
        );
        digests.push(format!("{:x}", digest.finalize()));
    }
    write_json(channel, json!({"sha256":digests})).await?;
    let result = reply(channel).await?;
    anyhow::ensure!(
        result["ok"] == true,
        "{}",
        result["message"]
            .as_str()
            .unwrap_or("Сервер не подтвердил файлы.")
    );
    let saved = result["files"]
        .as_array()
        .context("Нет подтверждения загрузки файлов.")?;
    anyhow::ensure!(saved.len() == headers.len(), "Переданы не все вложения.");
    for (index, file) in saved.iter().enumerate() {
        anyhow::ensure!(
            file["upload_id"] == upload_id
                && file["index"] == index
                && file["bytes"] == headers[index]["bytes"]
                && file["sha256"] == digests[index]
                && file["type"] == "uploaded_file",
            "Размер или контрольная сумма вложения не совпали."
        );
    }
    request["files"] = result["files"].clone();
    Ok(request)
}
