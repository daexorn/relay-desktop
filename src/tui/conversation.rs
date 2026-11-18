use super::editor::Editor;
use relay_desktop::model::{Message, Permission};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Prompt {
    pub text: String,
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedChat {
    pub draft: String,
    pub files: Vec<PathBuf>,
    pub queue: VecDeque<Prompt>,
    pub recovery: Option<Prompt>,
    pub model: String,
    pub mode: String,
    pub effort: String,
    pub backend: String,
    pub cwd: String,
    pub custom_prompt: String,
}

pub struct Chat {
    pub sid: String,
    pub title: String,
    pub cwd: String,
    pub editor: Editor,
    pub files: Vec<PathBuf>,
    pub queue: VecDeque<Prompt>,
    pub recovery: Option<Prompt>,
    pub inflight: Option<Prompt>,
    pub paused: bool,
    pub messages: Vec<Message>,
    pub permissions: Vec<Permission>,
    pub busy: bool,
    pub loading: bool,
    pub loaded: bool,
    pub locked: bool,
    pub unlocked: bool,
    pub status: String,
    pub error: String,
    pub model: String,
    pub mode: String,
    pub effort: String,
    pub backend: String,
    pub custom_prompt: String,
    pub blocks: BTreeMap<usize, usize>,
    pub user_at: usize,
    pub revision: u64,
    pub history_revision: u64,
    pub history_messages: usize,
    pub history_dirty: bool,
    pub scroll: usize,
    pub expanded: bool,
}
impl Chat {
    pub fn new(sid: String, cwd: String, saved: SavedChat) -> Self {
        Self {
            sid,
            title: "Новая сессия".into(),
            cwd: if saved.cwd.is_empty() { cwd } else { saved.cwd },
            editor: Editor::new(saved.draft),
            files: saved.files,
            queue: saved.queue,
            recovery: saved.recovery,
            inflight: None,
            paused: true,
            messages: vec![],
            permissions: vec![],
            busy: false,
            loading: false,
            loaded: false,
            locked: false,
            unlocked: true,
            status: String::new(),
            error: String::new(),
            model: if saved.model.is_empty() {
                "default".into()
            } else {
                saved.model
            },
            mode: if saved.mode.is_empty() {
                "manual".into()
            } else {
                saved.mode
            },
            effort: if saved.effort.is_empty() {
                "default".into()
            } else {
                saved.effort
            },
            backend: if saved.backend.is_empty() {
                "claude".into()
            } else {
                saved.backend
            },
            custom_prompt: saved.custom_prompt,
            blocks: BTreeMap::new(),
            user_at: 0,
            revision: 0,
            history_revision: 0,
            history_messages: 0,
            history_dirty: false,
            scroll: 0,
            expanded: false,
        }
    }
    pub fn is_locked_gate(&self) -> bool {
        self.locked && !self.unlocked
    }
    pub fn saved(&self) -> SavedChat {
        SavedChat {
            draft: self.editor.text.clone(),
            files: self.files.clone(),
            queue: self.queue.clone(),
            recovery: self.inflight.clone().or_else(|| self.recovery.clone()),
            model: self.model.clone(),
            mode: self.mode.clone(),
            effort: self.effort.clone(),
            backend: self.backend.clone(),
            cwd: self.cwd.clone(),
            custom_prompt: self.custom_prompt.clone(),
        }
    }
    pub fn reduce(&mut self, data: &Value) {
        self.revision += 1;
        match data["type"].as_str().unwrap_or("") {
            "stream_event" => {
                let e = &data["event"];
                let index = e["index"].as_u64().unwrap_or(0) as usize;
                match e["type"].as_str().unwrap_or("") {
                    "message_start" => self.blocks.clear(),
                    "content_block_start" => {
                        if e["content_block"]["type"] == "text" {
                            self.blocks.insert(index, self.messages.len());
                            self.messages.push(Message {
                                role: "assistant".into(),
                                text: e["content_block"]["text"].as_str().unwrap_or("").into(),
                                ..Default::default()
                            });
                        } else if e["content_block"]["type"] == "thinking" {
                            self.status = "Думает над ответом".into();
                        }
                    }
                    "content_block_delta" => {
                        if let Some(text) = e["delta"]["text"].as_str() {
                            if let Some(&at) = self.blocks.get(&index) {
                                if let Some(message) = self.messages.get_mut(at) {
                                    message.text.push_str(text);
                                    self.status = "Пишет ответ".into();
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            "assistant" => {
                if let Some(blocks) = data["message"]["content"].as_array() {
                    for b in blocks {
                        if b["type"] == "text" {
                            let text = b["text"].as_str().unwrap_or("");
                            if let Some((index, at)) = self
                                .blocks
                                .iter()
                                .find(|(_, at)| {
                                    self.messages
                                        .get(**at)
                                        .is_some_and(|m| text.starts_with(&m.text))
                                })
                                .map(|(i, a)| (*i, *a))
                            {
                                self.blocks.remove(&index);
                                self.messages[at].text = text.into();
                            } else if !text.is_empty() && text.trim() != "No response requested." {
                                self.messages.push(Message {
                                    role: "assistant".into(),
                                    text: text.into(),
                                    ..Default::default()
                                });
                            }
                        } else if b["type"] == "tool_use" {
                            let id = b["id"].as_str().unwrap_or("");
                            if !self
                                .messages
                                .iter()
                                .rev()
                                .take(30)
                                .any(|m| m.role == "tool" && m.tool_id == id)
                            {
                                self.messages.push(Message {
                                    role: "tool".into(),
                                    tool: b["name"].as_str().unwrap_or("Действие").into(),
                                    input: b["input"].clone(),
                                    tool_id: id.into(),
                                    ..Default::default()
                                });
                            }
                            self.status =
                                tool_name(b["name"].as_str().unwrap_or("Работает")).into();
                        }
                    }
                }
            }
            "user" => {
                if let Some(blocks) = data["message"]["content"].as_array() {
                    for b in blocks.iter().filter(|b| b["type"] == "tool_result") {
                        let text = b["content"].as_str().map(str::to_owned).unwrap_or_else(|| {
                            b["content"]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|t| t["text"].as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_default()
                        });
                        let id = b["tool_use_id"].as_str().unwrap_or("");
                        if let Some(message) = self
                            .messages
                            .iter_mut()
                            .rev()
                            .find(|m| m.role == "tool_result" && m.tool_id == id)
                        {
                            message.text = relay_desktop::model::shorten(&text, 24000);
                            message.failed = b["is_error"] == true;
                        } else {
                            self.messages.push(Message {
                                role: "tool_result".into(),
                                text: relay_desktop::model::shorten(&text, 24000),
                                tool_id: id.into(),
                                failed: b["is_error"] == true,
                                ..Default::default()
                            });
                        }
                    }
                }
            }
            "result" => {
                self.busy = false;
                self.permissions.clear();
                self.status.clear();
                self.inflight = None;
                self.blocks.clear();
                if data["is_error"] == true {
                    self.paused = true;
                    self.error = data["errors"]
                        .as_array()
                        .map(|e| {
                            e.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| {
                            data["result"]
                                .as_str()
                                .unwrap_or("Не удалось завершить ответ.")
                                .into()
                        });
                } else if !self
                    .messages
                    .iter()
                    .skip(self.user_at + 1)
                    .any(|m| m.role == "assistant" && !m.text.is_empty())
                {
                    if let Some(text) = data["result"].as_str().filter(|s| !s.is_empty()) {
                        self.messages.push(Message {
                            role: "assistant".into(),
                            text: text.into(),
                            ..Default::default()
                        });
                    }
                }
            }
            _ => {}
        }
    }
    pub fn request(&self, prompt: &Prompt, actor: &str) -> Value {
        json!({"op":"prompt","backend":self.backend,"sid":self.sid,"cwd":self.cwd,"text":prompt.text,
            "model":self.model,"mode":self.mode,"effort":self.effort,"actor":actor,
            "custom_prompt":self.custom_prompt,"desktop_enabled":false,"images":[],"files":[]})
    }
    /// The history snapshot and the stream can cross in transit. Keep only the
    /// live tail, matching tool IDs / overlapping text and remapping stream indices.
    pub fn merge_history(&mut self, mut history: Vec<Message>) {
        let first = history
            .iter()
            .rposition(|m| m.role == "user")
            .map_or(0, |i| i + 1);
        let mut remap = BTreeMap::new();
        let mut used = std::collections::BTreeSet::new();
        for (old_index, message) in self.messages.iter().enumerate() {
            if old_index < self.history_messages && !self.blocks.values().any(|at| *at == old_index)
            {
                continue;
            }
            let found = history
                .iter()
                .enumerate()
                .rev()
                .find(|(at, other)| {
                    if used.contains(at) || other.role != message.role {
                        return false;
                    }
                    if matches!(message.role.as_str(), "tool" | "tool_result") {
                        return !message.tool_id.is_empty() && other.tool_id == message.tool_id;
                    }
                    *at >= first
                        && message.role == "assistant"
                        && if message.text.is_empty() {
                            other.text.is_empty()
                        } else {
                            other.text.starts_with(&message.text)
                                || message.text.starts_with(&other.text)
                        }
                })
                .map(|(i, _)| i);
            let at = if let Some(at) = found {
                if message.role != "assistant" || message.text.len() > history[at].text.len() {
                    history[at] = message.clone();
                }
                at
            } else {
                history.push(message.clone());
                history.len() - 1
            };
            used.insert(at);
            remap.insert(old_index, at);
        }
        self.blocks.retain(|_, at| {
            if let Some(new) = remap.get(at) {
                *at = *new;
                true
            } else {
                false
            }
        });
        self.messages = history;
        self.user_at = self
            .messages
            .iter()
            .rposition(|m| m.role == "user")
            .unwrap_or(0);
        self.revision += 1;
        self.history_dirty = true;
    }
}
pub fn tool_name(name: &str) -> &str {
    match name {
        "Bash" => "Команда на сервере",
        "Read" => "Чтение файла",
        "Write" => "Создание файла",
        "Edit" => "Изменение файла",
        "Grep" | "Glob" => "Поиск в проекте",
        "WebSearch" | "WebFetch" => "Поиск в интернете",
        "ExitPlanMode" => "План готов",
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streamed_text_is_replaced_by_final_message_not_duplicated() {
        let mut c = Chat::new("id".into(), "/tmp".into(), SavedChat::default());
        c.messages.push(Message {
            role: "user".into(),
            text: "Привет".into(),
            ..Default::default()
        });
        for e in [
            json!({"type":"message_start"}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"text":"Прив"}}),
        ] {
            c.reduce(&json!({"type":"stream_event","event":e}));
        }
        c.reduce(&json!({"type":"assistant","message":{"content":[{"type":"text","text":"Привет мир"},{"type":"tool_use","id":"t","name":"Bash","input":{"command":"pwd"}}]}}));
        c.reduce(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"/tmp"}]}}));
        c.reduce(&json!({"type":"result","result":"Привет мир"}));
        assert_eq!(
            c.messages.iter().filter(|m| m.role == "assistant").count(),
            1
        );
        assert_eq!(c.messages[1].text, "Привет мир");
        assert_eq!(c.messages.last().unwrap().text, "/tmp");
    }
    #[test]
    fn reopening_never_automatically_replays_uncertain_messages_or_queue() {
        let mut c = Chat::new("id".into(), "/tmp".into(), SavedChat::default());
        c.inflight = Some(Prompt {
            text: "Опубликуй".into(),
            files: vec![],
        });
        c.queue.push_back(Prompt {
            text: "Проверь".into(),
            files: vec![],
        });
        let restored = Chat::new("id".into(), "/tmp".into(), c.saved());
        assert!(restored.paused);
        assert!(!restored.busy);
        assert!(restored.inflight.is_none());
        assert_eq!(restored.recovery.unwrap().text, "Опубликуй");
        assert_eq!(restored.queue.len(), 1);
    }
    #[test]
    fn history_merges_only_live_tail_without_duplicating_previous_turns() {
        let mut c = Chat::new("id".into(), "/tmp".into(), SavedChat::default());
        let old = Message {
            role: "assistant".into(),
            text: "Предыдущий ответ".into(),
            ..Default::default()
        };
        let user = Message {
            role: "user".into(),
            text: "Продолжай".into(),
            ..Default::default()
        };
        c.messages = vec![old.clone(), user.clone()];
        c.history_messages = 2;
        c.reduce(&json!({"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"text","text":"Текущий"}}}));
        c.merge_history(vec![
            old,
            user,
            Message {
                role: "assistant".into(),
                text: "Текущий ответ".into(),
                ..Default::default()
            },
        ]);
        assert_eq!(c.messages.len(), 3);
        assert_eq!(c.messages[2].text, "Текущий ответ");
        assert_eq!(c.blocks[&1], 2);
        c.reduce(&json!({"type":"assistant","message":{"content":[{"type":"text","text":"Текущий ответ завершён"}]}}));
        assert_eq!(c.messages.len(), 3);
        assert_eq!(c.messages[2].text, "Текущий ответ завершён");
    }
    #[test]
    fn lock_flags_are_never_persisted_in_saved_chat() {
        let mut c = Chat::new("id".into(), "/tmp".into(), SavedChat::default());
        assert!(!c.locked);
        assert!(c.unlocked);
        assert!(!c.is_locked_gate());
        c.locked = true;
        c.unlocked = false;
        assert!(c.is_locked_gate());
        let raw = serde_json::to_string(&c.saved()).unwrap();
        assert!(!raw.contains("locked"));
        assert!(!raw.contains("unlocked"));
    }
}
