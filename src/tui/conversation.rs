use super::{
    editor::Editor,
    markdown::{self, clean},
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use relay_desktop::model::{classify_tool, tool_target_preview, Message, Permission, ToolPhase};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
};
use unicode_width::UnicodeWidthStr;

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
        let b_backend = data["relay_backend"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.backend)
            .to_owned();
        let b_model = data["relay_model"]
            .as_str()
            .or_else(|| data["message"]["model"].as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.model)
            .to_owned();
        let b_mode = data["relay_mode"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.mode)
            .to_owned();
        let b_effort = data["relay_effort"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.effort)
            .to_owned();
        let b_actor = data["relay_actor"].as_str().unwrap_or("").to_owned();
        let b_agent = data["relay_agent_name"].as_str().unwrap_or("").to_owned();
        let stamp = |m: &mut Message| {
            if m.backend.is_empty() {
                m.backend = b_backend.clone();
            }
            if m.model.is_empty() {
                m.model = b_model.clone();
            }
            if m.mode.is_empty() {
                m.mode = b_mode.clone();
            }
            if m.effort.is_empty() {
                m.effort = b_effort.clone();
            }
            if m.actor.is_empty() && !b_actor.is_empty() {
                m.actor = b_actor.clone();
            }
            if m.agent_name.is_empty() && !b_agent.is_empty() {
                m.agent_name = b_agent.clone();
            }
        };
        match data["type"].as_str().unwrap_or("") {
            "stream_event" => {
                let e = &data["event"];
                let index = e["index"].as_u64().unwrap_or(0) as usize;
                match e["type"].as_str().unwrap_or("") {
                    "message_start" => self.blocks.clear(),
                    "content_block_start" => {
                        if e["content_block"]["type"] == "text" {
                            self.blocks.insert(index, self.messages.len());
                            let mut msg = Message {
                                role: "assistant".into(),
                                text: e["content_block"]["text"].as_str().unwrap_or("").into(),
                                ..Default::default()
                            };
                            stamp(&mut msg);
                            self.messages.push(msg);
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
                                stamp(&mut self.messages[at]);
                            } else if !text.is_empty() && text.trim() != "No response requested." {
                                let mut msg = Message {
                                    role: "assistant".into(),
                                    text: text.into(),
                                    ..Default::default()
                                };
                                stamp(&mut msg);
                                self.messages.push(msg);
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
                                let mut msg = Message {
                                    role: "tool".into(),
                                    tool: b["name"].as_str().unwrap_or("Действие").into(),
                                    input: b["input"].clone(),
                                    tool_id: id.into(),
                                    ..Default::default()
                                };
                                stamp(&mut msg);
                                self.messages.push(msg);
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
        "Bash" | "run_command" | "commandExecution" => "Команда на сервере",
        "Read" | "read_file" => "Чтение файла",
        "Write" | "write_file" => "Создание файла",
        "Edit" | "MultiEdit" | "edit_file" | "apply_patch" => "Изменение файла",
        "Grep" | "Glob" | "grep_search" | "find_by_name" => "Поиск в проекте",
        "WebSearch" | "WebFetch" | "search_web" => "Поиск в интернете",
        "Task" | "Agent" | "invoke_subagent" | "spawn_agent" | "swarm" => "Делегирование субагенту",
        "ExitPlanMode" => "План готов",
        _ => name,
    }
}

#[derive(Clone, Copy)]
pub struct TranscriptTheme {
    pub light: bool,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub red: Color,
    pub green: Color,
}

pub fn engine_badge(backend: &str) -> &'static str {
    match backend {
        "codex" => "◈ Codex",
        "agy" => "◈ AGY",
        "api" => "◈ API",
        _ => "◈ Claude",
    }
}

pub fn assistant_header(backend: &str) -> &'static str {
    match backend {
        "codex" => "◈ Codex",
        "agy" => "◈ Antigravity",
        "api" => "◈ API",
        _ => "◈ Claude Code",
    }
}

pub fn steps_word(n: usize) -> &'static str {
    if n % 10 == 1 && n % 100 != 11 {
        "шаг"
    } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) {
        "шага"
    } else {
        "шагов"
    }
}

fn phase_color(phase: ToolPhase, light: bool) -> Color {
    let (r, g, b) = phase.rgb(light);
    Color::Rgb(r, g, b)
}

pub fn subagent_delegation_summary(m: &Message) -> (String, String) {
    let i = &m.input;
    let role = i["Role"]
        .as_str()
        .or_else(|| i["worker"].as_str())
        .or_else(|| i["subagent_type"].as_str())
        .or_else(|| i["agent_type"].as_str())
        .or_else(|| i["TypeName"].as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Субагент");
    let task = i["description"]
        .as_str()
        .or_else(|| i["title"].as_str())
        .or_else(|| i["task"].as_str())
        .or_else(|| i["Prompt"].as_str())
        .or_else(|| i["prompt"].as_str())
        .or_else(|| i["message"].as_str())
        .unwrap_or("")
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    (
        relay_desktop::model::shorten(&clean(role.trim()), 36),
        relay_desktop::model::shorten(&clean(task), 76),
    )
}

fn collect_pair_diff(old_s: &str, new_s: &str, out: &mut Vec<(bool, String)>) {
    let old_lines: Vec<&str> = old_s.lines().collect();
    let new_lines: Vec<&str> = new_s.lines().collect();
    let mut prefix = 0usize;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    let mut old_end = old_lines.len();
    let mut new_end = new_lines.len();
    while old_end > prefix
        && new_end > prefix
        && old_lines[old_end - 1] == new_lines[new_end - 1]
    {
        old_end -= 1;
        new_end -= 1;
    }
    for &line in &old_lines[prefix..old_end] {
        out.push((false, clean(line)));
    }
    for &line in &new_lines[prefix..new_end] {
        out.push((true, clean(line)));
    }
}

pub fn extract_edit_diff(m: &Message) -> Vec<(bool, String)> {
    let i = &m.input;
    let mut out = Vec::new();
    if let Some(edits) = i["edits"].as_array() {
        for item in edits {
            let old_s = item["old_string"]
                .as_str()
                .or_else(|| item["Target"].as_str())
                .unwrap_or("");
            let new_s = item["new_string"]
                .as_str()
                .or_else(|| item["Replacement"].as_str())
                .unwrap_or("");
            if !old_s.is_empty() || !new_s.is_empty() {
                collect_pair_diff(old_s, new_s, &mut out);
            }
        }
    } else {
        let old_s = i["old_string"].as_str().or_else(|| i["Target"].as_str());
        let new_s = i["new_string"]
            .as_str()
            .or_else(|| i["Replacement"].as_str());
        if old_s.is_some() || new_s.is_some() {
            collect_pair_diff(old_s.unwrap_or(""), new_s.unwrap_or(""), &mut out);
        } else if let Some(patch) = i["patch"].as_str().or_else(|| i["diff"].as_str()) {
            for line in patch.lines() {
                if let Some(rest) = line.strip_prefix('+').filter(|_| !line.starts_with("+++")) {
                    out.push((true, clean(rest)));
                } else if let Some(rest) =
                    line.strip_prefix('-').filter(|_| !line.starts_with("---"))
                {
                    out.push((false, clean(rest)));
                }
            }
        }
    }
    out
}

fn wrap_prefixed(
    first_prefix: &str,
    cont_prefix: &str,
    prefix_style: Style,
    body: Line<'static>,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let pfx_w = first_prefix.width().max(cont_prefix.width());
    if width <= pfx_w + 3 {
        let mut combined = vec![Span::styled(first_prefix.to_owned(), prefix_style)];
        combined.extend(body.spans);
        return markdown::wrap(Line::from(combined), width);
    }
    let body_width = width.saturating_sub(pfx_w).max(1);
    markdown::wrap(body, body_width)
        .into_iter()
        .enumerate()
        .map(|(idx, part)| {
            let pfx = if idx == 0 { first_prefix } else { cont_prefix };
            let mut spans = vec![Span::styled(pfx.to_owned(), prefix_style)];
            spans.extend(part.spans);
            Line::from(spans)
        })
        .collect()
}

pub fn render_transcript(
    messages: &[Message],
    backend: &str,
    busy: bool,
    expanded: bool,
    width: usize,
    theme: TranscriptTheme,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut rows = vec![];
    let mut by_id: BTreeMap<&str, &Message> = BTreeMap::new();
    let mut positional: BTreeMap<usize, &Message> = BTreeMap::new();
    let mut open_tools: Vec<usize> = Vec::new();
    for (idx, m) in messages.iter().enumerate() {
        if m.role == "tool" {
            if m.tool_id.is_empty() {
                open_tools.push(idx);
            }
        } else if m.role == "tool_result" {
            if !m.tool_id.is_empty() {
                by_id.insert(m.tool_id.as_str(), m);
            } else if let Some(owner) = open_tools.pop() {
                positional.insert(owner, m);
            }
        }
    }

    let attributions = relay_desktop::model::compute_attributions(messages, backend, busy);
    let mut i = 0usize;
    while i < messages.len() {
        let message = &messages[i];
        let attr = &attributions[i];
        match message.role.as_str() {
            "user" => {
                let mut hdr_spans = vec![Span::styled(
                    format!("› {}", if attr.who.is_empty() { "Вы" } else { &attr.who }),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )];
                if !attr.to_whom.is_empty() {
                    let (br, bg, bb) = relay_desktop::model::backend_rgb(&attr.backend, theme.light);
                    hdr_spans.push(Span::styled(" → ", Style::default().fg(theme.muted)));
                    hdr_spans.push(Span::styled(
                        attr.to_whom.clone(),
                        Style::default().fg(Color::Rgb(br, bg, bb)),
                    ));
                }
                if !attr.how.is_empty() {
                    hdr_spans.push(Span::styled(
                        format!(" · {}", attr.how),
                        Style::default().fg(theme.muted),
                    ));
                }
                rows.extend(markdown::wrap(Line::from(hdr_spans), width));
                rows.extend(markdown::render(
                    &message.text,
                    width,
                    theme.text,
                    theme.accent,
                    theme.muted,
                ));
                rows.push(Line::default());
                i += 1;
            }
            "assistant" => {
                if !message.text.is_empty() {
                    let (er, eg, eb) =
                        relay_desktop::model::backend_rgb(&attr.backend, theme.light);
                    let mut hdr_spans = vec![Span::styled(
                        format!("{} · {}", assistant_header(&attr.backend), attr.who),
                        Style::default()
                            .fg(Color::Rgb(er, eg, eb))
                            .add_modifier(Modifier::BOLD),
                    )];
                    if !attr.to_whom.is_empty() {
                        hdr_spans.push(Span::styled(
                            format!(" → {}", attr.to_whom),
                            Style::default().fg(theme.muted),
                        ));
                    }
                    if !attr.how.is_empty() {
                        hdr_spans.push(Span::styled(
                            format!(" · {}", attr.how),
                            Style::default().fg(theme.muted),
                        ));
                    }
                    rows.extend(markdown::wrap(Line::from(hdr_spans), width));
                    rows.extend(markdown::render(
                        &message.text,
                        width,
                        theme.text,
                        theme.accent,
                        theme.muted,
                    ));
                    rows.push(Line::default());
                }
                i += 1;
            }
            "tool" => {
                let mut group = vec![(i, message)];
                let mut j = i + 1;
                while j < messages.len() {
                    let next = &messages[j];
                    if next.role == "tool" {
                        group.push((j, next));
                        j += 1;
                    } else if next.role == "tool_result"
                        || (next.role == "assistant" && next.text.trim().is_empty())
                    {
                        j += 1;
                    } else {
                        break;
                    }
                }
                rows.extend(render_pipeline_group(
                    &group,
                    &by_id,
                    &positional,
                    busy,
                    expanded,
                    width,
                    theme,
                ));
                rows.push(Line::default());
                i = j;
            }
            _ => {
                i += 1;
            }
        }
    }
    rows
}

fn render_pipeline_group(
    group: &[(usize, &Message)],
    by_id: &BTreeMap<&str, &Message>,
    positional: &BTreeMap<usize, &Message>,
    busy: bool,
    expanded: bool,
    width: usize,
    theme: TranscriptTheme,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    let n = group.len();
    let mut reads = 0usize;
    let mut edits = 0usize;
    let mut commands = 0usize;
    let mut delegations = 0usize;
    for &(_, m) in group {
        match classify_tool(&m.tool) {
            ToolPhase::Research => reads += 1,
            ToolPhase::Edit => edits += 1,
            ToolPhase::Terminal | ToolPhase::Desktop => commands += 1,
            ToolPhase::Subagent => delegations += 1,
            ToolPhase::Other => {}
        }
    }

    let mut head_spans = vec![
        Span::styled("╭─ ", Style::default().fg(theme.border)),
        Span::styled(
            "◈ Конвейер агента",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {} {}", n, steps_word(n)),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
    ];
    let counters: &[(usize, &str, ToolPhase)] = &[
        (reads, "🔍", ToolPhase::Research),
        (edits, "✎", ToolPhase::Edit),
        (commands, "$_", ToolPhase::Terminal),
        (delegations, "◈", ToolPhase::Subagent),
    ];
    let mut any_tag = false;
    for &(count, icon, ph) in counters {
        if count > 0 {
            head_spans.push(Span::styled(
                if any_tag { "  " } else { "   " },
                Style::default().fg(theme.muted),
            ));
            head_spans.push(Span::styled(
                format!("{icon}{count}"),
                Style::default()
                    .fg(phase_color(ph, theme.light))
                    .add_modifier(Modifier::BOLD),
            ));
            any_tag = true;
        }
    }
    head_spans.push(Span::styled(
        if expanded {
            "   [Ctrl+O свернуть]"
        } else {
            "   [Ctrl+O подробно]"
        },
        Style::default().fg(theme.muted),
    ));
    rows.extend(markdown::wrap(Line::from(head_spans), width));

    let border_style = Style::default().fg(theme.border);
    for (step_idx, &(msg_idx, tool_msg)) in group.iter().enumerate() {
        let is_last = step_idx + 1 == n;
        let result = if !tool_msg.tool_id.is_empty() {
            by_id.get(tool_msg.tool_id.as_str()).copied()
        } else {
            positional.get(&msg_idx).copied()
        };
        let failed = result.is_some_and(|r| r.failed);
        let running = result.is_none();
        let phase = classify_tool(&tool_msg.tool);
        let p_color = phase_color(phase, theme.light);

        let (glyph, glyph_color) = if running {
            (if busy { "◉ " } else { "⚙ " }, p_color)
        } else if failed {
            ("✕ ", theme.red)
        } else {
            ("✓ ", theme.green)
        };

        let mut step_spans = vec![
            Span::styled(
                glyph,
                Style::default()
                    .fg(glyph_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("[{}] ", phase.label()),
                Style::default().fg(p_color).add_modifier(Modifier::BOLD),
            ),
        ];

        if phase == ToolPhase::Subagent {
            let (role, task) = subagent_delegation_summary(tool_msg);
            step_spans.push(Span::styled(
                format!("◈ Субагент · {role}"),
                Style::default().fg(p_color).add_modifier(Modifier::BOLD),
            ));
            if !task.is_empty() {
                step_spans.push(Span::styled(" -> ", Style::default().fg(theme.muted)));
                step_spans.push(Span::styled(task, Style::default().fg(theme.text)));
            }
        } else {
            let preview = clean(&tool_target_preview(tool_msg, 120));
            let target_str = match tool_msg.tool.as_str() {
                "Grep" | "grep_search" if !preview.is_empty() => format!("Grep {preview}"),
                "Glob" | "find_by_name" if !preview.is_empty() => format!("Glob {preview}"),
                _ if !preview.is_empty() => preview,
                _ => clean(tool_name(&tool_msg.tool)),
            };
            step_spans.push(Span::styled(
                target_str,
                Style::default().fg(if failed { theme.red } else { theme.text }),
            ));
            if let Some(desc) = tool_msg.input["description"]
                .as_str()
                .map(clean)
                .filter(|d| !d.trim().is_empty())
            {
                step_spans.push(Span::styled(
                    format!("  · {}", desc.trim()),
                    Style::default().fg(theme.muted),
                ));
            }
        }

        let (branch, cont_branch) = if is_last {
            ("│  ╰─ ", "│     ")
        } else {
            ("│  ├─ ", "│  │  ")
        };
        rows.extend(wrap_prefixed(
            branch,
            cont_branch,
            border_style,
            Line::from(step_spans),
            width,
        ));

        let sub_spine = if is_last { "│       " } else { "│  │    " };
        let diffs = if phase == ToolPhase::Edit {
            extract_edit_diff(tool_msg)
        } else {
            Vec::new()
        };
        if !diffs.is_empty() {
            let limit = if expanded { 48 } else { 6 };
            for &(added, ref dline) in diffs.iter().take(limit) {
                let sign = if added { "+" } else { "-" };
                let d_color = if added { theme.green } else { theme.red };
                rows.extend(wrap_prefixed(
                    sub_spine,
                    sub_spine,
                    border_style,
                    Line::styled(format!("{sign} {dline}"), Style::default().fg(d_color)),
                    width,
                ));
            }
            if diffs.len() > limit {
                rows.extend(wrap_prefixed(
                    sub_spine,
                    sub_spine,
                    border_style,
                    Line::styled(
                        format!(
                            "… ещё {} строк правок · Ctrl+O подробно",
                            diffs.len() - limit
                        ),
                        Style::default().fg(theme.muted),
                    ),
                    width,
                ));
            }
        }

        if expanded && phase != ToolPhase::Edit {
            if let Some(full_cmd) = tool_msg.input["command"]
                .as_str()
                .or_else(|| tool_msg.input["CommandLine"].as_str())
                .filter(|c| c.lines().count() > 1)
            {
                for cmd_line in clean(full_cmd).lines().skip(1) {
                    rows.extend(wrap_prefixed(
                        sub_spine,
                        sub_spine,
                        border_style,
                        Line::styled(cmd_line.to_owned(), Style::default().fg(theme.text)),
                        width,
                    ));
                }
            }
        }

        let show_output = if let Some(res) = result {
            !res.text.trim().is_empty()
                && (expanded
                    || failed
                    || matches!(
                        phase,
                        ToolPhase::Terminal | ToolPhase::Desktop | ToolPhase::Other
                    )
                    || (phase == ToolPhase::Edit && diffs.is_empty()))
        } else {
            false
        };

        if show_output {
            let res = result.unwrap();
            let cleaned = clean(&res.text);
            let out_lines: Vec<&str> = cleaned
                .lines()
                .filter(|l| expanded || !l.trim().is_empty())
                .collect();
            let count = if expanded {
                out_lines.len()
            } else {
                out_lines.len().min(4)
            };
            if count < out_lines.len() {
                rows.extend(wrap_prefixed(
                    sub_spine,
                    sub_spine,
                    border_style,
                    Line::styled(
                        format!(
                            "… ещё {} строк · Ctrl+O развернуть",
                            out_lines.len() - count
                        ),
                        Style::default().fg(theme.muted),
                    ),
                    width,
                ));
            }
            for &line in out_lines.iter().skip(out_lines.len() - count) {
                let line_color = if failed {
                    theme.red
                } else if line.starts_with('+') && !line.starts_with("+++") {
                    theme.green
                } else if line.starts_with('-') && !line.starts_with("---") {
                    theme.red
                } else {
                    theme.muted
                };
                rows.extend(wrap_prefixed(
                    sub_spine,
                    sub_spine,
                    border_style,
                    Line::styled(line.to_owned(), Style::default().fg(line_color)),
                    width,
                ));
            }
        }
    }

    rows.extend(markdown::wrap(
        Line::styled("╰────", Style::default().fg(theme.border)),
        width,
    ));
    rows
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

    #[test]
    fn connected_pipeline_renders_spine_phases_subagents_diffs_and_engine_headers() {
        let theme = TranscriptTheme {
            light: false,
            border: Color::DarkGray,
            text: Color::White,
            muted: Color::Gray,
            accent: Color::Cyan,
            red: Color::Red,
            green: Color::Green,
        };
        let messages = vec![
            Message {
                role: "user".into(),
                text: "Обнови рендер".into(),
                ..Default::default()
            },
            Message {
                role: "assistant".into(),
                text: "Запускаю конвейер проверки.".into(),
                ..Default::default()
            },
            Message {
                role: "tool".into(),
                tool: "Grep".into(),
                tool_id: "t1".into(),
                input: json!({"pattern": "AtlasCoordinates"}),
                ..Default::default()
            },
            Message {
                role: "tool_result".into(),
                tool_id: "t1".into(),
                text: "src/AtlasRenderer.java:42".into(),
                ..Default::default()
            },
            Message {
                role: "tool".into(),
                tool: "invoke_subagent".into(),
                tool_id: "t2".into(),
                input: json!({"Role": "Coastline Verifier", "Prompt": "Проверь береговую линию"}),
                ..Default::default()
            },
            Message {
                role: "tool_result".into(),
                tool_id: "t2".into(),
                text: "Границы совпадают".into(),
                ..Default::default()
            },
            Message {
                role: "tool".into(),
                tool: "Edit".into(),
                tool_id: "t3".into(),
                input: json!({
                    "file_path": "/opt/project/AtlasRenderer.java",
                    "old_string": "int offset = rawMargin();\nrender();",
                    "new_string": "int offset = pixelAlignedMargin();\nrender();"
                }),
                ..Default::default()
            },
            Message {
                role: "tool_result".into(),
                tool_id: "t3".into(),
                text: "Updated".into(),
                ..Default::default()
            },
            Message {
                role: "tool".into(),
                tool: "Bash".into(),
                tool_id: "t4".into(),
                input: json!({"command": "cargo test --locked"}),
                ..Default::default()
            },
        ];

        for (backend, expected_head) in [
            ("claude", "◈ Claude Code"),
            ("codex", "◈ Codex"),
            ("agy", "◈ Antigravity"),
            ("api", "◈ API"),
        ] {
            let lines = render_transcript(&messages, backend, true, false, 110, theme);
            assert!(lines.iter().all(|l| l.width() <= 110));
            let text = lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains(expected_head));
            assert!(text.contains("╭─ ◈ Конвейер агента · 4 шага"));
            assert!(text.contains("🔍1"));
            assert!(text.contains("✎1"));
            assert!(text.contains("$_1"));
            assert!(text.contains("◈1"));
            assert!(text.contains("[Ctrl+O подробно]"));
            assert!(text.contains("│  ├─ ✓ [ПОИСК]"));
            assert!(text.contains("│  ├─ ✓ [СУБАГЕНТ] ◈ Субагент · Coastline Verifier -> Проверь береговую линию"));
            assert!(text.contains("│  ├─ ✓ [ПРАВКА] AtlasRenderer.java"));
            assert!(text.contains("- int offset = rawMargin();"));
            assert!(text.contains("+ int offset = pixelAlignedMargin();"));
            assert!(text.contains("│  ╰─ ◉ [КОМАНДА] $ cargo test --locked"));
            assert!(text.contains("╰────"));
        }

        // Also ensure narrow widths (e.g. 28 cols on a 32x10 terminal) never overflow or panic
        let narrow = render_transcript(&messages, "claude", true, true, 28, theme);
        assert!(narrow.iter().all(|l| l.width() <= 28));
    }
}
