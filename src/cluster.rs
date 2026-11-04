use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub server: String,
    pub cwd: String,
    pub model: String,
    pub effort: String,
    pub prompt: String,
    pub enabled: bool,
}
impl Default for Agent {
    fn default() -> Self {
        Self {
            id: id(),
            name: "Исполнитель".into(),
            server: String::new(),
            cwd: String::new(),
            model: "default".into(),
            effort: "default".into(),
            prompt: String::new(),
            enabled: true,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Config {
    pub id: String,
    pub name: String,
    pub leader: Agent,
    pub workers: Vec<Agent>,
    pub shared_prompt: String,
    pub max_parallel: usize,
    pub max_tasks: usize,
    pub mode: String,
    pub auto_results: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            id: id(),
            name: "Команда разработки".into(),
            leader: Agent {
                name: "Руководитель".into(),
                ..Default::default()
            },
            workers: vec![],
            shared_prompt: String::new(),
            max_parallel: 2,
            max_tasks: 32,
            mode: "auto".into(),
            auto_results: true,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Назови кластер.".into());
        }
        if self.leader.server.is_empty() {
            return Err("Выбери хост руководителя.".into());
        }
        let workers: Vec<_> = self.workers.iter().filter(|a| a.enabled).collect();
        if workers.is_empty() || self.workers.len() > 12 {
            return Err("Добавь от 1 до 12 исполнителей.".into());
        }
        if !(1..=8).contains(&self.max_parallel) || !(1..=128).contains(&self.max_tasks) {
            return Err("Проверь ограничения задач.".into());
        }
        let mut ids = std::collections::HashSet::new();
        for a in std::iter::once(&self.leader).chain(workers) {
            if a.server.is_empty() || a.name.trim().is_empty() || !ids.insert(&a.id) {
                return Err(
                    "У каждого агента должны быть имя, хост и отдельный идентификатор.".into(),
                );
            }
            if a.prompt.len() > 65536 {
                return Err("Промпт агента слишком длинный.".into());
            }
        }
        if self.shared_prompt.len() > 65536 {
            return Err("Общий промпт слишком длинный.".into());
        }
        if !["auto", "manual", "acceptEdits", "plan"].contains(&self.mode.as_str()) {
            return Err("Неизвестный режим.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Queued,
    Running,
    Waiting,
    Cancelling,
    Done,
    Failed,
    Cancelled,
    Interrupted,
}
impl TaskState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
    pub fn active(self) -> bool {
        matches!(self, Self::Running | Self::Waiting | Self::Cancelling)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "В очереди",
            Self::Running => "В работе",
            Self::Waiting => "Ждёт ответа",
            Self::Cancelling => "Останавливается",
            Self::Done => "Ответ получен",
            Self::Failed => "Ошибка",
            Self::Cancelled => "Отменена",
            Self::Interrupted => "Прервана",
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Task {
    pub id: String,
    pub request: String,
    pub agent: Agent,
    pub title: String,
    pub instructions: String,
    pub depends_on: Vec<String>,
    pub sid: String,
    pub state: TaskState,
    pub result: String,
    pub error: String,
    pub delivered: bool,
    pub created: f64,
    pub finished: Option<f64>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct JournalEntry {
    pub time: f64,
    pub message: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Run {
    pub id: String,
    pub config: Config,
    pub leader_sid: String,
    pub title: String,
    pub paused: bool,
    pub created: f64,
    pub tasks: Vec<Task>,
    pub journal: Vec<JournalEntry>,
    #[serde(default)]
    pub pending_config: Option<Config>,
}
impl Run {
    pub fn new(config: Config, title: String) -> Self {
        Self {
            id: id(),
            config,
            leader_sid: id(),
            title,
            paused: false,
            created: now(),
            tasks: vec![],
            journal: vec![],
            pending_config: None,
        }
    }
    pub fn log(&mut self, message: impl Into<String>) {
        self.journal.push(JournalEntry {
            time: now(),
            message: message.into().chars().take(1000).collect(),
        });
        if self.journal.len() > 250 {
            self.journal.remove(0);
        }
    }
    pub fn dispatch(&mut self, request: &str, args: &Value) -> Result<Value, String> {
        if let Some(t) = self.tasks.iter().find(|t| t.request == request) {
            return Ok(self.task_json(&t.id, false).unwrap());
        }
        if self.paused {
            return Err("Кластер на паузе. Дождись возобновления пользователем.".into());
        }
        if self.tasks.len() >= self.config.max_tasks {
            return Err(
                "Достигнут лимит задач этого запуска. Передай пользователю текущие результаты."
                    .into(),
            );
        }
        let worker = args["worker"]
            .as_str()
            .ok_or("Укажи worker из list_workers.")?;
        let agent = self
            .config
            .workers
            .iter()
            .find(|a| a.id == worker && a.enabled)
            .cloned()
            .ok_or("Исполнитель не входит в этот кластер.")?;
        let instructions = args["task"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 128 * 1024)
            .ok_or("Текст задачи: от 1 до 131072 байт.")?
            .to_owned();
        let title = args["title"]
            .as_str()
            .unwrap_or(&instructions)
            .chars()
            .take(180)
            .collect::<String>();
        let deps = match args.get("depends_on") {
            Some(Value::Array(items)) if items.len() <= 32 => items
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or("Некорректная зависимость.")
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => vec![],
            _ => return Err("depends_on должен быть списком задач.".into()),
        };
        if deps
            .iter()
            .any(|dep| !self.tasks.iter().any(|t| &t.id == dep))
        {
            return Err("Зависимость не найдена в этой сессии кластера.".into());
        }
        let sid = self
            .tasks
            .iter()
            .rev()
            .find(|t| {
                t.agent.id == agent.id && t.agent.server == agent.server && t.agent.cwd == agent.cwd
            })
            .map(|t| t.sid.clone())
            .unwrap_or_else(id);
        let task = Task {
            id: id(),
            request: request.into(),
            agent,
            title,
            instructions,
            depends_on: deps,
            sid,
            state: TaskState::Queued,
            result: String::new(),
            error: String::new(),
            delivered: false,
            created: now(),
            finished: None,
        };
        let task_id = task.id.clone();
        self.log(format!(
            "Руководитель → {}: {}",
            task.agent.name, task.title
        ));
        self.tasks.push(task);
        Ok(self.task_json(&task_id, false).unwrap())
    }
    pub fn runnable(&mut self, online: &std::collections::HashSet<String>) -> Vec<String> {
        if self.paused {
            return vec![];
        }
        let failed: std::collections::HashSet<_> = self
            .tasks
            .iter()
            .filter(|t| t.state.terminal() && t.state != TaskState::Done)
            .map(|t| t.id.clone())
            .collect();
        for t in &mut self.tasks {
            if t.state == TaskState::Queued && t.depends_on.iter().any(|d| failed.contains(d)) {
                t.state = TaskState::Failed;
                t.error = "Зависимая задача завершилась без результата.".into();
                t.finished = Some(now());
            }
        }
        let done: std::collections::HashSet<_> = self
            .tasks
            .iter()
            .filter(|t| t.state == TaskState::Done)
            .map(|t| t.id.clone())
            .collect();
        let mut agents: std::collections::HashSet<_> = self
            .tasks
            .iter()
            .filter(|t| t.state.active())
            .map(|t| t.agent.id.clone())
            .collect();
        let slots = self
            .config
            .max_parallel
            .saturating_sub(self.tasks.iter().filter(|t| t.state.active()).count());
        let mut selected = vec![];
        for t in &self.tasks {
            if selected.len() >= slots {
                break;
            }
            if t.state == TaskState::Queued
                && online.contains(&t.agent.server)
                && !agents.contains(&t.agent.id)
                && t.depends_on.iter().all(|d| done.contains(d))
            {
                selected.push(t.id.clone());
                agents.insert(t.agent.id.clone());
            }
        }
        selected
    }
    pub fn task_json(&self, id: &str, include_result: bool) -> Option<Value> {
        self.tasks.iter().find(|t|t.id==id).map(|t| {
            let mut value=json!({"ok":!matches!(t.state,TaskState::Failed|TaskState::Interrupted),"job":t.id,"worker":t.agent.id,"worker_name":t.agent.name,"title":t.title,
                "state":t.state,"finished":t.state.terminal(),"depends_on":t.depends_on,"error":t.error});
            if include_result{value["result"]=json!(t.result);}
            value
        })
    }
    pub fn complete(&mut self, id: &str, result: &str, error: &str) {
        if let Some(t) = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id && t.state.active())
        {
            let cancelled = t.state == TaskState::Cancelling;
            t.result = result.chars().take(32000).collect();
            t.error = error.chars().take(4000).collect();
            t.state = if cancelled {
                TaskState::Cancelled
            } else if error.is_empty() {
                TaskState::Done
            } else {
                TaskState::Failed
            };
            t.finished = Some(now());
            let message = format!("{}: {} — {}", t.agent.name, t.title, t.state.label());
            self.log(message);
        }
    }
    pub fn recover(&mut self) {
        self.paused = true;
        for t in &mut self.tasks {
            if t.state.active() {
                t.state = TaskState::Interrupted;
                t.error="Соединение предыдущего запуска Relay завершено. Проверь сохранённую сессию исполнителя.".into();
                t.finished = Some(now());
            }
        }
    }
    pub fn apply_pending(&mut self) {
        let Some(config) = self.pending_config.take() else {
            return;
        };
        for task in &mut self.tasks {
            if task.state != TaskState::Queued {
                continue;
            }
            match config
                .workers
                .iter()
                .find(|a| a.id == task.agent.id && a.enabled)
            {
                Some(a) if a.server == task.agent.server && a.cwd == task.agent.cwd => {
                    task.agent = a.clone()
                }
                _ => {
                    task.state = TaskState::Cancelled;
                    task.error="Состав команды изменён пользователем. Требуется новое назначение руководителем.".into();
                    task.finished = Some(now());
                }
            }
        }
        self.config = config;
        self.log("Новые промпты и настройки применены к сессии");
    }
}
#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    pub runs: Vec<Run>,
    #[serde(default)]
    pub selected: Option<String>,
    #[serde(skip)]
    pub dirty: bool,
    #[serde(skip)]
    pub revision: u64,
    #[serde(skip)]
    pub save_disabled: bool,
    #[cfg(test)]
    #[serde(skip)]
    pub memory_only: bool,
}
impl Store {
    pub fn load() -> Result<Self, String> {
        let path = crate::storage::directory().join("cluster-sessions.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        if path.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
            return Err("История кластера превышает 64 МБ.".into());
        }
        let mut store: Self =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("История кластера: {e}"))?;
        for run in &mut store.runs {
            run.recover();
        }
        store.dirty = true;
        Ok(store)
    }
    pub fn save(&mut self) -> Result<(), String> {
        #[cfg(test)]
        if self.memory_only {
            if self.dirty {
                self.revision = self.revision.wrapping_add(1);
            }
            self.dirty = false;
            return Ok(());
        }
        if !self.dirty {
            return Ok(());
        }
        if self.save_disabled {
            return Err(
                "История кластера не прочитана. Существующий файл оставлен на месте.".into(),
            );
        }
        crate::storage::save_private_json("cluster-sessions.json", self)
            .map_err(|e| e.to_string())?;
        self.revision = self.revision.wrapping_add(1);
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_sessions_keep_instructions_and_results_but_do_not_restart_work() {
        let mut r = fixture();
        r.config.shared_prompt = "Use [данные_айпи_(Production)]".into();
        r.dispatch("a", &json!({"worker":"one","task":"first"}))
            .unwrap();
        r.dispatch("b", &json!({"worker":"two","task":"second"}))
            .unwrap();
        r.tasks[0].state = TaskState::Running;
        r.tasks[1].state = TaskState::Running;
        let done = r.tasks[1].id.clone();
        r.complete(&done, "Saved result", "");
        let mut restored: Run = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        restored.recover();
        assert_eq!(
            restored.config.shared_prompt,
            "Use [данные_айпи_(Production)]"
        );
        assert_eq!(restored.tasks[1].result, "Saved result");
        assert_eq!(restored.tasks[0].state, TaskState::Interrupted);
        assert!(restored
            .runnable(&std::collections::HashSet::from(["a".into(), "b".into()]))
            .is_empty());
    }
    #[test]
    fn changing_the_team_updates_queued_instructions_and_cancels_removed_workers() {
        let mut r = fixture();
        for (id, w) in [("a", "one"), ("b", "two")] {
            r.dispatch(id, &json!({"worker":w,"task":"work"})).unwrap();
        }
        let mut updated = r.config.clone();
        updated.workers[0].prompt = "New constraints".into();
        updated.workers[1].enabled = false;
        r.pending_config = Some(updated);
        r.apply_pending();
        assert_eq!(r.tasks[0].agent.prompt, "New constraints");
        assert_eq!(r.tasks[1].state, TaskState::Cancelled);
    }
    use std::collections::HashSet;
    fn fixture() -> Run {
        Run::new(
            Config {
                leader: Agent {
                    server: "a".into(),
                    ..Default::default()
                },
                workers: vec![
                    Agent {
                        id: "one".into(),
                        server: "a".into(),
                        ..Default::default()
                    },
                    Agent {
                        id: "two".into(),
                        server: "b".into(),
                        ..Default::default()
                    },
                ],
                max_parallel: 2,
                ..Default::default()
            },
            "task".into(),
        )
    }
    #[test]
    fn leader_dispatches_only_configured_workers_and_requests_are_idempotent() {
        let mut r = fixture();
        assert!(r
            .dispatch("x", &json!({"worker":"alien","task":"do it"}))
            .is_err());
        let a = r
            .dispatch("x", &json!({"worker":"one","task":"build"}))
            .unwrap();
        let b = r
            .dispatch("x", &json!({"worker":"one","task":"build"}))
            .unwrap();
        assert_eq!(a["job"], b["job"]);
        assert_eq!(r.tasks.len(), 1);
    }
    #[test]
    fn scheduling_respects_host_connections_agent_locks_and_global_slots() {
        let mut r = fixture();
        for (q, w) in [("1", "one"), ("2", "one"), ("3", "two")] {
            r.dispatch(q, &json!({"worker":w,"task":"work"})).unwrap();
        }
        let all = HashSet::from(["a".into(), "b".into()]);
        assert_eq!(r.runnable(&all).len(), 2);
        r.tasks[0].state = TaskState::Running;
        r.config.max_parallel = 1;
        assert!(r.runnable(&all).is_empty());
        r.config.max_parallel = 2;
        assert_eq!(r.runnable(&all), vec![r.tasks[2].id.clone()]);
        assert!(r.runnable(&HashSet::from(["a".into()])).is_empty());
    }
    #[test]
    fn dependency_results_reach_the_leader_and_failed_dependencies_do_not_run() {
        let mut r = fixture();
        let a = r
            .dispatch("a", &json!({"worker":"one","task":"implement"}))
            .unwrap()["job"]
            .as_str()
            .unwrap()
            .to_owned();
        r.dispatch(
            "b",
            &json!({"worker":"two","task":"review","depends_on":[a]}),
        )
        .unwrap();
        r.tasks[0].state = TaskState::Running;
        r.complete(&a, "changed files and checks", "");
        assert_eq!(
            r.task_json(&a, true).unwrap()["result"],
            "changed files and checks"
        );
        assert_eq!(r.runnable(&HashSet::from(["b".into()])).len(), 1);
        let mut failed = fixture();
        failed
            .dispatch("a", &json!({"worker":"one","task":"a"}))
            .unwrap();
        let id = failed.tasks[0].id.clone();
        failed
            .dispatch("b", &json!({"worker":"two","task":"b","depends_on":[id]}))
            .unwrap();
        failed.tasks[0].state = TaskState::Failed;
        failed.runnable(&HashSet::from(["b".into()]));
        assert_eq!(failed.tasks[1].state, TaskState::Failed);
    }
    #[test]
    fn recovery_does_not_repeat_running_tasks() {
        let mut r = fixture();
        r.dispatch("a", &json!({"worker":"one","task":"work"}))
            .unwrap();
        r.tasks[0].state = TaskState::Running;
        r.recover();
        assert!(r.paused);
        assert_eq!(r.tasks[0].state, TaskState::Interrupted);
    }
}
