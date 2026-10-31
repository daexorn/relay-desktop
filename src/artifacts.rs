use crate::{
    connection::Controller,
    model,
    ui::{self as design, Icon, Kind, Palette},
};
use eframe::egui::{self, RichText, Vec2};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct Artifact {
    pub path: String,
    pub name: String,
}

fn candidate(value: &str, link: bool) -> Option<Artifact> {
    let value = value.trim().trim_matches(['`', '"', '\'', '<', '>']);
    if value.len() > 4096
        || value.contains(['\n', '\r', '\0'])
        || value.contains("://") && !value.starts_with("file://")
    {
        return None;
    }
    let value = value.strip_prefix("file://").unwrap_or(value);
    if value.contains(':') || value.starts_with('\\') || value.as_bytes().get(1) == Some(&b':') {
        return None;
    }
    let path = if link {
        let mut bytes = Vec::new();
        let source = value.as_bytes();
        let mut at = 0;
        while at < source.len() {
            if source[at] == b'%' && at + 2 < source.len() {
                if let (Some(a), Some(b)) = (
                    (source[at + 1] as char).to_digit(16),
                    (source[at + 2] as char).to_digit(16),
                ) {
                    bytes.push((a * 16 + b) as u8);
                    at += 3;
                    continue;
                }
            }
            bytes.push(source[at]);
            at += 1;
        }
        String::from_utf8(bytes).ok()?
    } else {
        value.to_owned()
    };
    if path.contains(['\n', '\r', '\0', ':', '\\'])
        || path.ends_with('/')
        || !path.contains('/') && path.chars().any(char::is_whitespace)
    {
        return None;
    }
    let lower = path.to_ascii_lowercase();
    if ![
        ".jar", ".zip", ".7z", ".tar.gz", ".pdf", ".docx", ".pptx", ".xlsx", ".bbmodel", ".png",
        ".jpg", ".jpeg", ".webp",
    ]
    .iter()
    .any(|suffix| lower.ends_with(suffix))
    {
        return None;
    }
    let name = path.rsplit('/').next()?.to_owned();
    Some(Artifact { path, name })
}

pub fn extract(text: &str) -> Vec<Artifact> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut in_link = false;
    let mut add = |value: &str, link: bool| {
        if let Some(artifact) = candidate(value, link) {
            if seen.insert(artifact.path.clone()) {
                found.push(artifact);
            }
        }
    };
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                in_link = true;
                add(&dest_url, true);
            }
            Event::End(TagEnd::Link) => in_link = false,
            Event::Code(code) if !in_link => add(&code, false),
            Event::Text(text) if !in_link => {
                for word in text.split_whitespace() {
                    let word = word.trim_matches(['(', ')', '[', ']', ',', ';']);
                    if word.starts_with('/') || word.starts_with("~/") || text.trim() == word {
                        add(word, false);
                    }
                }
            }
            _ => {}
        }
    }
    found.truncate(4);
    found
}

pub fn links(text: &str) -> Vec<(String, Artifact)> {
    Parser::new(text)
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                candidate(&dest_url, true).map(|artifact| (dest_url.into_string(), artifact))
            }
            _ => None,
        })
        .take(4)
        .collect()
}

#[derive(Clone, Default)]
struct Status {
    stage: String,
    message: String,
    local_path: String,
}
#[derive(Default)]
pub struct Downloads {
    states: HashMap<(String, String, String), Status>,
    requests: HashMap<String, (String, String, String)>,
}
impl Downloads {
    pub fn start(&mut self, controller: &Controller, server: &str, cwd: &str, path: &str) {
        let key = (server.to_owned(), cwd.to_owned(), path.to_owned());
        if self
            .states
            .get(&key)
            .is_some_and(|status| status.stage == "loading")
        {
            return;
        }
        let status = if let Some(id) = controller.download_artifact(server, path, cwd) {
            self.requests.insert(id, key.clone());
            Status {
                stage: "loading".into(),
                ..Default::default()
            }
        } else {
            Status {
                stage: "failed".into(),
                message: "Подключись к серверу и повтори скачивание.".into(),
                ..Default::default()
            }
        };
        self.states.insert(key, status);
    }

    pub fn event(&mut self, server: &str, value: &Value) {
        let Some(id) = value["id"].as_str() else {
            return;
        };
        let Some(key) = self
            .requests
            .get(id)
            .filter(|(s, _, _)| s == server)
            .cloned()
        else {
            return;
        };
        let stage = value["stage"].as_str().unwrap_or("failed").to_owned();
        self.states.insert(
            key,
            Status {
                stage: stage.clone(),
                message: value["message"].as_str().unwrap_or("").into(),
                local_path: value["local_path"].as_str().unwrap_or("").into(),
            },
        );
        if stage == "saved" || stage == "failed" {
            self.requests.remove(id);
        }
    }
    pub fn disconnected(&mut self, server: &str) {
        let keys: Vec<_> = self
            .requests
            .iter()
            .filter(|(_, key)| key.0 == server)
            .map(|(id, key)| (id.clone(), key.clone()))
            .collect();
        for (id, key) in keys {
            self.requests.remove(&id);
            self.states.insert(
                key,
                Status {
                    stage: "failed".into(),
                    message: "Соединение прервалось. Можно повторить скачивание.".into(),
                    ..Default::default()
                },
            );
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        controller: &Controller,
        server: &str,
        cwd: &str,
        text: &str,
        connected: bool,
        p: Palette,
    ) {
        for artifact in extract(text) {
            let key = (server.to_owned(), cwd.to_owned(), artifact.path.clone());
            let status = self.states.get(&key).cloned().unwrap_or_default();
            let busy = status.stage == "loading";
            ui.add_space(8.);
            egui::Frame::new()
                .fill(p.surface)
                .corner_radius(12.)
                .inner_margin(12.)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::splat(24.), egui::Sense::hover());
                        design::icon(ui.painter(), rect, Icon::File, p.muted);
                        ui.vertical(|ui| {
                            ui.set_width((ui.available_width() - 110.).max(80.));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(model::shorten(&artifact.name, 55))
                                        .size(13.)
                                        .color(p.text),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&artifact.path);
                            let subtitle = if busy {
                                "Скачивается…"
                            } else if status.stage == "saved" {
                                "Файл сохранён"
                            } else {
                                "Файл с сервера · в «Загрузки»"
                            };
                            ui.label(RichText::new(subtitle).size(11.).color(p.muted))
                                .on_hover_text(&status.local_path);
                        });
                        ui.add_enabled_ui(connected && !busy, |ui| {
                            if design::button(
                                ui,
                                if status.stage == "failed" {
                                    "Повторить"
                                } else {
                                    "Скачать"
                                },
                                None,
                                Kind::Secondary,
                                p,
                                Vec2::new(98., 32.),
                            )
                            .on_hover_text(if connected {
                                "Скачать по SSH без включения управления ПК"
                            } else {
                                "Сначала подключись к серверу"
                            })
                            .clicked()
                            {
                                self.start(controller, server, cwd, &artifact.path);
                            }
                        });
                    });
                    if status.stage == "failed" {
                        ui.label(
                            RichText::new(&status.message)
                                .size(12.)
                                .color(egui::Color32::from_rgb(204, 133, 138)),
                        );
                    }
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_server_artifacts_and_decodes_markdown_paths() {
        let result = extract(
            "Готово: [Мод](/opt/Project%20Name/build/libs/mod.jar). Ещё `/tmp/report.pdf`.",
        );
        assert_eq!(
            result,
            vec![
                Artifact {
                    path: "/opt/Project Name/build/libs/mod.jar".into(),
                    name: "mod.jar".into()
                },
                Artifact {
                    path: "/tmp/report.pdf".into(),
                    name: "report.pdf".into()
                }
            ]
        );
    }
    #[test]
    fn local_windows_paths_and_external_links_are_not_ssh_artifacts() {
        assert!(extract("`C:\\Games\\mods\\mod.jar` [zip](https://example.com/a.zip)").is_empty());
    }
    #[test]
    fn duplicate_paths_are_one_card_and_unknown_schemes_are_rejected() {
        assert_eq!(extract("[jar](/tmp/a.jar) `/tmp/a.jar`").len(), 1);
        assert_eq!(extract("[a.jar](/tmp/a.jar)").len(), 1);
        assert!(extract("[jar](ssh://host/a.jar) [jar](javascript:evil.jar)").is_empty());
        assert!(extract("Выполни `java -jar mod.jar`").is_empty());
        assert_eq!(
            links("[mod.jar](/tmp/Project%20Name/mod.jar)")[0].1.path,
            "/tmp/Project Name/mod.jar"
        );
        assert!(links("[mod](https://example.com/mod.jar)").is_empty());
    }
}
