use super::*;
use crate::cluster::{self, Agent, Config, Run, TaskState};
use crate::templates::{self, Context};
use std::collections::HashSet;

pub(super) struct Pending {
    pub server: String,
    pub request: String,
    pub sid: String,
    pub run: String,
    pub job: String,
    pub deadline: Instant,
}
impl RelayApp {
    pub(super) fn load_cluster_sessions(&mut self) {
        match cluster::Store::load() {
            Ok(store) => self.cluster_store = store,
            Err(e) => {
                self.notice = e;
                self.cluster_store.save_disabled = true;
                return;
            }
        }
        let sessions: Vec<_> = self
            .cluster_store
            .runs
            .iter()
            .flat_map(|r| {
                std::iter::once((
                    r.config.leader.clone(),
                    r.leader_sid.clone(),
                    model::shorten(&r.title, 70),
                    r.config.mode.clone(),
                ))
                .chain(r.tasks.iter().map(|t| {
                    (
                        t.agent.clone(),
                        t.sid.clone(),
                        format!("{} · {}", t.agent.name, t.title),
                        r.config.mode.clone(),
                    )
                }))
            })
            .collect();
        for (agent, sid, title, mode) in sessions {
            if let Some(server) = self.settings.servers.iter().find(|s| s.id == agent.server) {
                let mut c = Chat::new(sid.clone(), agent_cwd(&agent, server), title);
                c.mode = mode;
                c.model = agent_model(&agent, server);
                c.effort = agent_effort(&agent, server);
                self.chats.insert(key(&server.id, &sid), c);
            }
        }
    }
    pub(super) fn cluster_label(&self, server: &str, sid: &str) -> Option<String> {
        for r in &self.cluster_store.runs {
            if r.config.leader.server == server && r.leader_sid == sid {
                return Some(format!("Кластер · {}", r.config.name));
            }
            if let Some(t) = r
                .tasks
                .iter()
                .find(|t| t.agent.server == server && t.sid == sid)
            {
                return Some(format!("Исполнитель · {}", t.agent.name));
            }
        }
        None
    }
    pub(super) fn cluster_for_chat(&self, server: &str, sid: &str) -> Option<String> {
        self.cluster_store
            .runs
            .iter()
            .find(|r| {
                (r.config.leader.server == server && r.leader_sid == sid)
                    || r.tasks
                        .iter()
                        .any(|t| t.agent.server == server && t.sid == sid)
            })
            .map(|r| r.id.clone())
    }
    pub(super) fn prepare_cluster_chat(&mut self, server: &Server, k: &str) -> Result<(), String> {
        let Some(chat) = self.chats.get(k) else {
            return Ok(());
        };
        let Some(run_id) = self.cluster_for_chat(&server.id, &chat.sid) else {
            return Ok(());
        };
        let run = self
            .cluster_store
            .runs
            .iter()
            .find(|r| r.id == run_id)
            .unwrap();
        let leader = run.leader_sid == chat.sid && run.config.leader.server == server.id;
        let (agent, task) = if leader {
            (&run.config.leader, run.title.as_str())
        } else {
            let task = run
                .tasks
                .iter()
                .find(|t| t.sid == chat.sid && t.agent.server == server.id && t.state.active())
                .or_else(|| {
                    run.tasks
                        .iter()
                        .rev()
                        .find(|t| t.sid == chat.sid && t.agent.server == server.id)
                })
                .ok_or("Задача кластера не найдена.")?;
            (&task.agent, task.instructions.as_str())
        };
        let cwd = agent_cwd(agent, server);
        let context = Context {
            cluster: &run.config.name,
            agent: &agent.name,
            task,
            host: &server.host,
            cwd: &cwd,
        };
        let vars = templates::variables(
            &self.settings.servers,
            &self.settings.prompt_snippets,
            &context,
        );
        let custom = templates::expand(
            &format!("{}\n\n{}", run.config.shared_prompt, agent.prompt),
            &vars,
        )?;
        if custom.len() > 96000 {
            return Err(
                "Промпт после подстановок превышает 96 КБ. Сократи инструкции или вставки.".into(),
            );
        }
        let draft = if chat.draft.starts_with("<relay-cluster-results>") {
            chat.draft.clone()
        } else {
            templates::expand(&chat.draft, &vars)?
        };
        let c = self.chats.get_mut(k).unwrap();
        c.custom_prompt = custom;
        c.model = agent_model(agent, server);
        c.effort = agent_effort(agent, server);
        c.mode = run.config.mode.clone();
        c.cluster_context =
            json!({"run":run.id,"name":run.config.name,"role":if leader{"leader"}else{"worker"}});
        c.draft = draft;
        c.cwd = cwd;
        c.agent_role = String::new();
        Ok(())
    }
    pub(super) fn start_cluster(&mut self, config: Config, text: String) -> Result<(), String> {
        if self.demo {
            return Err("Это предпросмотр. Создай команду в обычном окне Relay.".into());
        }

        config.validate()?;
        for a in std::iter::once(&config.leader).chain(config.workers.iter().filter(|w| w.enabled))
        {
            if !self.settings.servers.iter().any(|s| s.id == a.server) {
                return Err(format!("Не найден профиль хоста: {}", a.name));
            }
        }
        let server = self
            .settings
            .servers
            .iter()
            .find(|s| s.id == config.leader.server)
            .cloned()
            .unwrap();
        if !self.connected(&server.id) {
            self.connect(server);
            return Err(
                "Подключи хост руководителя и повтори запуск. Текст задачи сохранён.".into(),
            );
        }
        let mut run = Run::new(config, text.clone());
        run.log("Начата отдельная сессия руководителя");
        let sid = run.leader_sid.clone();
        let mut c = Chat::new(
            sid.clone(),
            agent_cwd(&run.config.leader, &server),
            model::shorten(&text, 70),
        );
        c.mode = run.config.mode.clone();
        c.model = agent_model(&run.config.leader, &server);
        c.effort = agent_effort(&run.config.leader, &server);
        c.loaded = true;
        c.draft = text;
        let k = key(&server.id, &sid);
        self.chats.insert(k.clone(), c);
        self.cluster_store.selected = Some(run.id.clone());
        self.cluster_store.runs.push(run);
        self.cluster_store.dirty = true;
        if let Err(e) = self.cluster_store.save() {
            self.cluster_store.runs.last_mut().unwrap().paused = true;
            return Err(e);
        }
        self.settings.selected = Some(server.id.clone());
        self.selected_chat.insert(server.id.clone(), sid);
        self.send_to_chat(server, k);
        self.cluster_open = false;
        self.cluster_prompt.clear();
        Ok(())
    }
    pub(super) fn open_cluster_chat(&mut self, server_id: &str, sid: &str) {
        let Some(server) = self
            .settings
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .cloned()
        else {
            return;
        };
        self.settings.selected = Some(server.id.clone());
        self.selected_chat.insert(server.id.clone(), sid.into());
        if self.connected(server_id) {
            if let Some(c) = self.chats.get_mut(&key(server_id, sid)) {
                if !c.loaded && !c.busy {
                    c.loading = true;
                    self.controller
                        .send(server_id, json!({"op":"history","sid":sid}));
                }
            }
        } else {
            self.connect(server);
        }
        self.cluster_open = false;
        self.save();
    }
    fn reply_cluster(&self, server: &str, request: &str, sid: &str, run: &str, result: Value) {
        self.controller.send(
            server,
            json!({"op":"cluster_reply","id":request,"sid":sid,"run":run,"result":result}),
        );
    }
    pub(super) fn cluster_request(&mut self, server: String, value: Value) {
        let request = value["id"].as_str().unwrap_or("");
        let sid = value["sid"].as_str().unwrap_or("");
        let run_id = value["run"].as_str().unwrap_or("");
        let valid = self
            .cluster_store
            .runs
            .iter()
            .any(|r| r.id == run_id && r.leader_sid == sid && r.config.leader.server == server);
        let answer = if !valid {
            Err("Этот инструмент доступен только руководителю своей сессии кластера.".into())
        } else {
            self.cluster_operation(
                run_id,
                request,
                value["tool"].as_str().unwrap_or(""),
                &value["input"],
            )
        };
        match answer {
            Ok(result) if value["tool"] == "wait_for_task" && result["finished"] != true => {
                if self.cluster_calls.len() >= 64 {
                    self.reply_cluster(
                        &server,
                        request,
                        sid,
                        run_id,
                        json!({"ok":false,"message":"Слишком много ожидающих запросов."}),
                    );
                    return;
                }
                if !self
                    .cluster_calls
                    .iter()
                    .any(|p| p.server == server && p.request == request)
                {
                    self.cluster_calls.push(Pending {
                        server,
                        request: request.into(),
                        sid: sid.into(),
                        run: run_id.into(),
                        job: value["input"]["job"].as_str().unwrap_or("").into(),
                        deadline: Instant::now()
                            + Duration::from_secs(
                                value["input"]["timeout_seconds"]
                                    .as_u64()
                                    .unwrap_or(20)
                                    .clamp(1, 20),
                            ),
                    });
                }
            }
            Ok(result) => self.reply_cluster(&server, request, sid, run_id, result),
            Err(e) => self.reply_cluster(
                &server,
                request,
                sid,
                run_id,
                json!({"ok":false,"message":e}),
            ),
        }
    }
    fn cluster_operation(
        &mut self,
        run_id: &str,
        request: &str,
        tool: &str,
        input: &Value,
    ) -> Result<Value, String> {
        let index = self
            .cluster_store
            .runs
            .iter()
            .position(|r| r.id == run_id)
            .ok_or("Сессия кластера не найдена.")?;
        if tool == "list_workers" {
            let r = &self.cluster_store.runs[index];
            let workers:Vec<_>=r.config.workers.iter().filter(|a|a.enabled).map(|a|{
                let server=self.settings.servers.iter().find(|s|s.id==a.server);
                json!({"worker":a.id,"name":a.name,"host":server.map(|s|s.host.as_str()),"cwd":server.map(|s|agent_cwd(a,s)),
                    "connected":self.connected(&a.server),"busy":r.tasks.iter().any(|t|t.agent.id==a.id&&t.state.active()),"instructions":a.prompt})
            }).collect();
            return Ok(
                json!({"ok":true,"workers":workers,"max_parallel":r.config.max_parallel,"max_tasks":r.config.max_tasks}),
            );
        }
        if tool == "dispatch_task" {
            let value = self.cluster_store.runs[index].dispatch(request, input)?;
            self.cluster_store.dirty = true;
            if let Err(e) = self.cluster_store.save() {
                self.cluster_store.runs[index].paused = true;
                return Err(e);
            }
            return Ok(value);
        }
        if tool == "cluster_status" {
            let r = &self.cluster_store.runs[index];
            let tasks: Vec<_> = r
                .tasks
                .iter()
                .filter_map(|t| r.task_json(&t.id, false))
                .collect();
            return Ok(
                json!({"ok":true,"name":r.config.name,"paused":r.paused,"tasks":tasks,"max_parallel":r.config.max_parallel,"remaining_tasks":r.config.max_tasks.saturating_sub(r.tasks.len())}),
            );
        }
        let job = input["job"].as_str().ok_or("Укажи job из dispatch_task.")?;
        if tool == "cancel_task" {
            self.cancel_cluster_task(index, job)?;
        }
        if !["task_status", "wait_for_task", "cancel_task"].contains(&tool) {
            return Err("Неизвестный инструмент кластера.".into());
        }
        let r = &mut self.cluster_store.runs[index];
        let result = r
            .task_json(job, true)
            .ok_or("Задача не найдена в этой сессии.")?;
        if result["finished"] == true {
            if let Some(t) = r.tasks.iter_mut().find(|t| t.id == job) {
                if !t.delivered {
                    t.delivered = true;
                    self.cluster_store.dirty = true;
                }
            }
        }
        Ok(result)
    }
    fn cancel_cluster_task(&mut self, index: usize, job: &str) -> Result<(), String> {
        let r = &mut self.cluster_store.runs[index];
        let t = r
            .tasks
            .iter_mut()
            .find(|t| t.id == job)
            .ok_or("Задача не найдена.")?;
        if !t.state.terminal() {
            if t.state.active() {
                self.controller
                    .send(&t.agent.server, json!({"op":"stop","sid":t.sid}));
                if let Some(c) = self.chats.get_mut(&key(&t.agent.server, &t.sid)) {
                    c.queue_paused = true;
                }
                t.state = TaskState::Cancelling;
            } else {
                t.state = TaskState::Cancelled;
                t.finished = Some(cluster::now());
            }
            t.error = "Задача отменена руководителем или пользователем.".into();
            r.log("Запрошена отмена задачи");
            self.cluster_store.dirty = true;
        }
        Ok(())
    }
    pub(super) fn stop_cluster(&mut self, run_id: &str) {
        let Some(index) = self.cluster_store.runs.iter().position(|r| r.id == run_id) else {
            return;
        };
        let r = &mut self.cluster_store.runs[index];
        r.paused = true;
        self.controller.send(
            &r.config.leader.server,
            json!({"op":"stop","sid":r.leader_sid}),
        );
        if let Some(c) = self
            .chats
            .get_mut(&key(&r.config.leader.server, &r.leader_sid))
        {
            c.queue_paused = true;
        }
        let ids: Vec<_> = r
            .tasks
            .iter()
            .filter(|t| !t.state.terminal())
            .map(|t| t.id.clone())
            .collect();
        for id in ids {
            let _ = self.cancel_cluster_task(index, &id);
        }
        self.cluster_store.runs[index].log("Пользователь остановил кластер");
        self.cluster_store.dirty = true;
    }
    pub(super) fn cluster_tick(&mut self) {
        if self.demo {
            return;
        }
        let mut completed = vec![];
        for r in &mut self.cluster_store.runs {
            if !r.paused
                && !self
                    .remote
                    .get(&r.config.leader.server)
                    .is_some_and(|s| s.connected)
            {
                r.paused = true;
                r.log("Связь с руководителем прервалась. Очередь на паузе.");
                self.cluster_store.dirty = true;
            }

            for t in &mut r.tasks {
                if !t.state.active() {
                    continue;
                }
                if let Some(c) = self.chats.get(&key(&t.agent.server, &t.sid)) {
                    if c.busy {
                        if t.state == TaskState::Cancelling {
                            continue;
                        }
                        let state = if c.permissions.is_empty() {
                            TaskState::Running
                        } else {
                            TaskState::Waiting
                        };
                        if t.state != state {
                            t.state = state;
                            self.cluster_store.dirty = true;
                        }
                    } else {
                        let result = c
                            .messages
                            .iter()
                            .skip(c.user_at.saturating_add(1))
                            .rev()
                            .find(|m| m.role == "assistant" && !m.text.is_empty())
                            .map(|m| m.text.clone())
                            .unwrap_or_default();
                        completed.push((r.id.clone(), t.id.clone(), result, c.error.clone()));
                    }
                }
            }
        }
        for (run, job, result, error) in completed {
            if let Some(r) = self.cluster_store.runs.iter_mut().find(|r| r.id == run) {
                r.complete(&job, &result, &error);
                self.cluster_store.dirty = true;
            }
        }
        for r in &mut self.cluster_store.runs {
            if !r.tasks.iter().any(|t| t.state.active())
                && !self
                    .chats
                    .get(&key(&r.config.leader.server, &r.leader_sid))
                    .is_some_and(|c| c.busy)
            {
                if r.pending_config.is_some() {
                    r.apply_pending();
                    if let (Some(c), Some(s)) = (
                        self.chats
                            .get_mut(&key(&r.config.leader.server, &r.leader_sid)),
                        self.settings
                            .servers
                            .iter()
                            .find(|s| s.id == r.config.leader.server),
                    ) {
                        c.model = agent_model(&r.config.leader, s);
                        c.effort = agent_effort(&r.config.leader, s);
                        c.mode = r.config.mode.clone();
                    }
                    self.cluster_store.dirty = true;
                }
            }
        }
        let pending = std::mem::take(&mut self.cluster_calls);
        for p in pending {
            let result = self
                .cluster_store
                .runs
                .iter()
                .find(|r| r.id == p.run)
                .and_then(|r| r.task_json(&p.job, true));
            if result.as_ref().is_none_or(|v| v["finished"] == true) || Instant::now() >= p.deadline
            {
                let value = result
                    .unwrap_or_else(|| json!({"ok":false,"message":"Сессия или задача удалена."}));
                if value["finished"] == true {
                    if let Some(r) = self.cluster_store.runs.iter_mut().find(|r| r.id == p.run) {
                        if let Some(t) = r.tasks.iter_mut().find(|t| t.id == p.job) {
                            t.delivered = true;
                            self.cluster_store.dirty = true;
                        }
                    }
                }
                self.reply_cluster(&p.server, &p.request, &p.sid, &p.run, value);
            } else {
                self.cluster_calls.push(p);
            }
        }
        let online: HashSet<_> = self
            .remote
            .iter()
            .filter(|(_, r)| r.connected)
            .map(|(s, _)| s.clone())
            .collect();
        let mut launch = vec![];
        for r in &mut self.cluster_store.runs {
            let before = r.tasks.iter().filter(|t| t.state.terminal()).count();
            for id in r.runnable(&online) {
                if let Some(t) = r.tasks.iter_mut().find(|t| t.id == id) {
                    t.state = TaskState::Running;
                    launch.push((r.id.clone(), t.clone(), r.config.mode.clone()));
                    self.cluster_store.dirty = true;
                }
            }
            if before != r.tasks.iter().filter(|t| t.state.terminal()).count() {
                self.cluster_store.dirty = true;
            }
        }
        for (run_id, t, mode) in launch {
            let Some(server) = self
                .settings
                .servers
                .iter()
                .find(|s| s.id == t.agent.server)
                .cloned()
            else {
                continue;
            };
            let k = key(&server.id, &t.sid);
            let mut c = self.chats.remove(&k).unwrap_or_else(|| {
                Chat::new(
                    t.sid.clone(),
                    agent_cwd(&t.agent, &server),
                    format!("{} · {}", t.agent.name, t.title),
                )
            });
            c.title = format!("{} · {}", t.agent.name, t.title);
            c.mode = mode;
            c.model = agent_model(&t.agent, &server);
            c.effort = agent_effort(&t.agent, &server);
            c.loaded = true;
            c.draft = t.instructions.clone();
            self.chats.insert(k.clone(), c);
            self.send_to_chat(server, k.clone());
            let c = &self.chats[&k];
            if !c.busy {
                if let Some(r) = self.cluster_store.runs.iter_mut().find(|r| r.id == run_id) {
                    r.complete(
                        &t.id,
                        "",
                        if c.error.is_empty() {
                            "Не удалось запустить исполнителя."
                        } else {
                            &c.error
                        },
                    );
                }
            }
        }
        let mut deliveries = vec![];
        for r in &self.cluster_store.runs {
            if r.paused || !r.config.auto_results || !self.connected(&r.config.leader.server) {
                continue;
            }
            let Some(c) = self.chats.get(&key(&r.config.leader.server, &r.leader_sid)) else {
                continue;
            };
            if c.busy || c.loading || !c.queue.is_empty() || c.queue_paused || !c.error.is_empty() {
                continue;
            }
            let ids: Vec<_> = r
                .tasks
                .iter()
                .filter(|t| t.state.terminal() && !t.delivered)
                .map(|t| t.id.clone())
                .collect();
            if !ids.is_empty() {
                let results: Vec<_> = ids.iter().filter_map(|id| r.task_json(id, true)).collect();
                deliveries.push((
                    r.id.clone(),
                    r.config.leader.server.clone(),
                    r.leader_sid.clone(),
                    ids,
                    results,
                ));
            }
        }
        for (run_id, server_id, sid, ids, results) in deliveries {
            let Some(server) = self
                .settings
                .servers
                .iter()
                .find(|s| s.id == server_id)
                .cloned()
            else {
                continue;
            };
            let k = key(&server_id, &sid);
            let c = self.chats.get_mut(&k).unwrap();
            let draft=std::mem::replace(&mut c.draft,format!("<relay-cluster-results>\n{}\n</relay-cluster-results>\nReview these actual worker results. Request corrections if needed, then report the verified outcome to the user.",serde_json::to_string_pretty(&results).unwrap_or_default()));
            let files = std::mem::take(&mut c.images);
            self.send_to_chat(server, k.clone());
            let c = self.chats.get_mut(&k).unwrap();
            let sent = c.busy;
            c.draft = draft;
            c.images = files;
            if let Some(r) = self.cluster_store.runs.iter_mut().find(|r| r.id == run_id) {
                if sent {
                    for t in &mut r.tasks {
                        if ids.contains(&t.id) {
                            t.delivered = true;
                        }
                    }
                    r.log("Результаты исполнителей автоматически переданы руководителю");
                } else {
                    r.paused = true;
                    r.log("Не удалось передать результаты. Кластер на паузе.");
                }
            }
            self.cluster_store.dirty = true;
        }
        if let Err(e) = self.cluster_store.save() {
            self.notice = e;
        }
        if !self.cluster_calls.is_empty() {
            self.controller
                .bus
                .ctx
                .request_repaint_after(Duration::from_millis(150));
        }
    }
}
pub(super) fn agent_cwd(a: &Agent, s: &Server) -> String {
    if a.cwd.trim().is_empty() {
        s.cwd.clone()
    } else {
        a.cwd.clone()
    }
}
fn agent_model(a: &Agent, s: &Server) -> String {
    if a.model == "inherit" {
        s.model.clone()
    } else {
        a.model.clone()
    }
}
fn agent_effort(a: &Agent, s: &Server) -> String {
    if a.effort == "inherit" {
        s.effort.clone()
    } else {
        a.effort.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        RelayApp,
        Server,
        Server,
        Config,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
    ) {
        let (mut controller, events) = Controller::new(egui::Context::default());
        let a = Server {
            id: cluster::id(),
            name: "Lead host".into(),
            host: "127.0.0.1".into(),
            cwd: "/project".into(),
            tests: "off".into(),
            ..Default::default()
        };
        let b = Server {
            id: cluster::id(),
            name: "Build host".into(),
            host: "127.0.0.2".into(),
            cwd: "/worker".into(),
            tests: "off".into(),
            ..Default::default()
        };
        let rx_a = controller.mock_host(&a.id);
        let rx_b = controller.mock_host(&b.id);
        let config = Config {
            leader: Agent {
                server: a.id.clone(),
                prompt: "Lead on [данные_айпи_(Lead host)]".into(),
                ..Default::default()
            },
            workers: vec![Agent {
                id: "worker".into(),
                server: b.id.clone(),
                prompt: "Only the assigned task in [рабочая_папка]".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut app = RelayApp {
            test_no_save: true,
            pending: Default::default(),
            pending_saved: Default::default(),
            pending_checked: None,
            settings: Settings {
                servers: vec![a.clone(), b.clone()],
                selected: Some(a.id.clone()),
                ..Default::default()
            },
            controller,
            events,
            remote: HashMap::from([
                (
                    a.id.clone(),
                    Remote {
                        connected: true,
                        ..Default::default()
                    },
                ),
                (
                    b.id.clone(),
                    Remote {
                        connected: true,
                        ..Default::default()
                    },
                ),
            ]),
            chats: HashMap::new(),
            selected_chat: HashMap::new(),
            delete_chat: None,
            deleted_sessions: Default::default(),
            renaming: None,
            folder_renaming: None,
            folder_creating: None,
            search: String::new(),
            editor: None,
            trust: None,
            folder_open: false,
            folder_path: String::new(),
            folder_parent: String::new(),
            folders: vec![],
            folder_hidden: false,
            folder_request: 0,
            folder_confirmed: String::new(),
            answers: HashMap::new(),
            notice: String::new(),
            markdown: CommonMarkCache::default(),
            demo: false,
            screenshot: None,
            frames: 0,
            capture_at: 0.,
            capture_requested: false,
            close_confirm: false,
            desktop_requests: vec![],
            game_tests: vec![],
            board_open: false,
            settings_open: false,
            cluster_open: false,
            cluster_prompt: String::new(),
            cluster_target: String::new(),
            cluster_jobs: vec![],
            cluster_store: cluster::Store::default(),
            cluster_calls: vec![],
            cluster_edit: None,
            cluster_snapshot: None,
            board_view: Default::default(),
            boards: HashMap::new(),
            board_polled: None,
            downloads: Default::default(),
        };
        app.cluster_store.memory_only = true;
        (app, a, b, config, rx_a, rx_b)
    }
    fn take(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Value>, op: &str) -> Value {
        while let Ok(v) = rx.try_recv() {
            if v["op"] == op {
                return v;
            }
        }
        panic!("No {op} request")
    }
    fn send_result(app: &mut RelayApp, server: &str, sid: &str, text: &str) {
        app.data(server.into(),json!({"type":"claude","sid":sid,"data":{"type":"assistant","message":{"content":[{"type":"text","text":text}]}}}));
        app.data(server.into(),json!({"type":"claude","sid":sid,"data":{"type":"result","result":text,"is_error":false}}));
    }
    #[test]
    fn leader_dispatches_to_another_host_and_receives_its_actual_result() {
        let (mut app, a, b, config, mut rx_a, mut rx_b) = fixture();
        app.start_cluster(config, "Build and verify the change".into())
            .unwrap();
        let lead = take(&mut rx_a, "prompt");
        assert_eq!(lead["cluster_context"]["role"], "leader");
        assert!(lead["custom_prompt"]
            .as_str()
            .unwrap()
            .contains("127.0.0.1"));
        let run = app.cluster_store.runs[0].id.clone();
        let sid = app.cluster_store.runs[0].leader_sid.clone();
        app.data(a.id.clone(),json!({"type":"cluster_request","id":cluster::id(),"sid":sid,"run":run,"tool":"dispatch_task","input":{"worker":"worker","task":"Implement the change and report checks"}}));
        let assigned = take(&mut rx_a, "cluster_reply");
        let job = assigned["result"]["job"].as_str().unwrap().to_owned();
        app.cluster_tick();
        let work = take(&mut rx_b, "prompt");
        assert_eq!(work["cluster_context"]["role"], "worker");
        assert_eq!(work["cwd"], "/worker");
        assert_eq!(work["desktop_enabled"], false);
        app.data(a.id.clone(),json!({"type":"cluster_request","id":cluster::id(),"sid":sid,"run":run,"tool":"wait_for_task","input":{"job":job}}));
        assert_eq!(app.cluster_calls.len(), 1);
        send_result(
            &mut app,
            &b.id,
            work["sid"].as_str().unwrap(),
            "Changed src/file.rs; 3 checks passed",
        );
        app.cluster_tick();
        let result = take(&mut rx_a, "cluster_reply");
        assert_eq!(result["result"]["state"], "done");
        assert_eq!(
            result["result"]["result"],
            "Changed src/file.rs; 3 checks passed"
        );
        assert!(app.cluster_store.runs[0].tasks[0].delivered);
    }
    #[test]
    fn completed_work_wakes_an_idle_leader_without_replacing_the_users_draft() {
        let (mut app, a, b, config, mut rx_a, mut rx_b) = fixture();
        app.start_cluster(config, "Goal".into()).unwrap();
        let lead = take(&mut rx_a, "prompt");
        let run = app.cluster_store.runs[0].id.clone();
        let sid = app.cluster_store.runs[0].leader_sid.clone();
        app.data(a.id.clone(),json!({"type":"cluster_request","id":cluster::id(),"sid":sid,"run":run,"tool":"dispatch_task","input":{"worker":"worker","task":"Do the work"}}));
        take(&mut rx_a, "cluster_reply");
        app.cluster_tick();
        let work = take(&mut rx_b, "prompt");
        send_result(
            &mut app,
            &a.id,
            lead["sid"].as_str().unwrap(),
            "Waiting for the worker",
        );
        app.chats.get_mut(&key(&a.id, &sid)).unwrap().draft = "My unsent clarification".into();
        send_result(
            &mut app,
            &b.id,
            work["sid"].as_str().unwrap(),
            "Real worker result",
        );
        app.cluster_tick();
        let followup = take(&mut rx_a, "prompt");
        assert!(followup["text"]
            .as_str()
            .unwrap()
            .starts_with("<relay-cluster-results>"));
        assert!(followup["text"]
            .as_str()
            .unwrap()
            .contains("Real worker result"));
        assert_eq!(
            app.chats[&key(&a.id, &sid)].draft,
            "My unsent clarification"
        );
        assert!(app.cluster_store.runs[0].tasks[0].delivered);
    }
    #[test]
    fn another_host_cannot_impersonate_the_leader() {
        let (mut app, _a, b, config, _rx_a, mut rx_b) = fixture();
        app.start_cluster(config, "Goal".into()).unwrap();
        let r = &app.cluster_store.runs[0];
        app.data(b.id.clone(),json!({"type":"cluster_request","id":cluster::id(),"sid":r.leader_sid,"run":r.id,"tool":"dispatch_task","input":{"worker":"worker","task":"Wrong source"}}));
        assert_eq!(take(&mut rx_b, "cluster_reply")["result"]["ok"], false);
        assert!(app.cluster_store.runs[0].tasks.is_empty());
    }
    #[test]
    fn followups_reuse_the_worker_context_and_keep_the_correct_task_prompt() {
        let (mut app, a, b, mut config, mut rx_a, mut rx_b) = fixture();
        config.workers[0].prompt = "Current assignment: [задача]".into();
        app.start_cluster(config, "Goal".into()).unwrap();
        take(&mut rx_a, "prompt");
        let run = app.cluster_store.runs[0].id.clone();
        let sid = app.cluster_store.runs[0].leader_sid.clone();
        for text in ["FIRST_TASK", "SECOND_TASK"] {
            app.data(a.id.clone(),json!({"type":"cluster_request","id":cluster::id(),"sid":sid,"run":run,"tool":"dispatch_task","input":{"worker":"worker","task":text}}));
            take(&mut rx_a, "cluster_reply");
        }
        app.cluster_tick();
        let first = take(&mut rx_b, "prompt");
        assert!(first["custom_prompt"]
            .as_str()
            .unwrap()
            .contains("FIRST_TASK"));
        assert!(!first["custom_prompt"]
            .as_str()
            .unwrap()
            .contains("SECOND_TASK"));
        send_result(
            &mut app,
            &b.id,
            first["sid"].as_str().unwrap(),
            "First result remains in context",
        );
        app.cluster_tick();
        let second = take(&mut rx_b, "prompt");
        assert_eq!(first["sid"], second["sid"]);
        assert!(second["custom_prompt"]
            .as_str()
            .unwrap()
            .contains("SECOND_TASK"));
        assert!(app.chats[&key(&b.id, first["sid"].as_str().unwrap())]
            .messages
            .iter()
            .any(|m| m.text == "First result remains in context"));
        assert_eq!(
            app.cluster_store.runs[0].tasks[0].result,
            "First result remains in context"
        );
    }
    #[test]
    fn cancelling_a_queued_followup_does_not_interrupt_the_current_task() {
        let (mut app, a, _b, config, mut rx_a, mut rx_b) = fixture();
        app.start_cluster(config, "Goal".into()).unwrap();
        take(&mut rx_a, "prompt");
        let run = app.cluster_store.runs[0].id.clone();
        let sid = app.cluster_store.runs[0].leader_sid.clone();
        let mut jobs = vec![];
        for text in ["first", "follow-up"] {
            app.data(a.id.clone(),json!({"type":"cluster_request","id":cluster::id(),"sid":sid,"run":run,"tool":"dispatch_task","input":{"worker":"worker","task":text}}));
            jobs.push(
                take(&mut rx_a, "cluster_reply")["result"]["job"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
        assert_eq!(
            app.cluster_store.runs[0].tasks[0].sid,
            app.cluster_store.runs[0].tasks[1].sid
        );
        app.cluster_tick();
        take(&mut rx_b, "prompt");
        app.data(a.id.clone(),json!({"type":"cluster_request","id":cluster::id(),"sid":sid,"run":run,"tool":"cancel_task","input":{"job":jobs[1]}}));
        assert_eq!(app.cluster_store.runs[0].tasks[0].state, TaskState::Running);
        assert_eq!(
            app.cluster_store.runs[0].tasks[1].state,
            TaskState::Cancelled
        );
        assert!(
            rx_b.try_recv().is_err(),
            "Cancelling a queued task sent a stop to its shared worker session"
        );
    }
    #[test]
    fn deletion_waits_for_ack_and_never_affects_another_host() {
        let (mut app, a, b, _, mut rx_a, _) = fixture();
        let sid = cluster::id();
        let session = Session {
            id: sid.clone(),
            title: "Delete this".into(),
            cwd: a.cwd.clone(),
            modified: 1.,
            ..Default::default()
        };
        for host in [&a, &b] {
            app.remote
                .get_mut(&host.id)
                .unwrap()
                .sessions
                .push(session.clone());
            app.chats.insert(
                key(&host.id, &sid),
                Chat::new(sid.clone(), host.cwd.clone(), "Keep until ack".into()),
            );
            app.selected_chat.insert(host.id.clone(), sid.clone());
            app.settings
                .servers
                .iter_mut()
                .find(|s| s.id == host.id)
                .unwrap()
                .last_session = Some(sid.clone());
        }
        app.request_delete(&a.id, session.clone());
        assert!(app.delete_chat.is_some());
        assert!(app.chats.contains_key(&key(&a.id, &sid)));
        assert!(
            rx_a.try_recv().is_err(),
            "Opening the dialog sent a deletion request"
        );
        app.data(
            a.id.clone(),
            json!({"type":"error","op":"delete_session","sid":sid,"message":"fixture failure"}),
        );
        assert!(app.chats.contains_key(&key(&a.id, &sid)));
        assert_eq!(app.delete_chat.as_ref().unwrap().error, "fixture failure");
        app.data(a.id.clone(), json!({"type":"session_deleted","sid":sid}));
        assert!(!app.chats.contains_key(&key(&a.id, &sid)));
        assert!(app.chats.contains_key(&key(&b.id, &sid)));
        assert_ne!(app.selected_chat.get(&a.id), Some(&sid));
        assert_eq!(app.selected_chat.get(&b.id), Some(&sid));
        assert!(app.delete_chat.is_none());
        app.data(a.id.clone(), json!({"type":"sessions","sessions":[{"id":sid,"title":"stale response","cwd":"/project","modified":1.}]}));
        assert!(app.remote[&a.id].sessions.is_empty());
        app.data(
            a.id.clone(),
            json!({"type":"history","sid":sid,"messages":[{"role":"user","text":"stale history"}]}),
        );
        assert!(!app.chats.contains_key(&key(&a.id, &sid)));
        assert_eq!(app.remote[&b.id].sessions[0].id, sid);
        app.notice = "Current notification".into();
        app.data(
            a.id.clone(),
            json!({"type":"error","op":"history","sid":sid,"message":"Late prefetch failure"}),
        );
        assert_eq!(app.notice, "Current notification");
    }
    #[test]
    fn busy_session_deletion_is_blocked_and_idle_cluster_gets_a_fresh_session() {
        let (mut app, a, _, config, mut rx_a, _) = fixture();
        app.start_cluster(config, "Goal".into()).unwrap();
        let prompt = take(&mut rx_a, "prompt");
        let sid = prompt["sid"].as_str().unwrap().to_owned();
        let session = Session {
            id: sid.clone(),
            title: "Leader".into(),
            cwd: a.cwd.clone(),
            modified: 1.,
            ..Default::default()
        };
        app.request_delete(&a.id, session.clone());
        assert!(app.delete_chat.is_none());
        send_result(&mut app, &a.id, &sid, "Done");
        app.request_delete(&a.id, session);
        assert!(app.delete_chat.is_some());
        app.data(a.id, json!({"type":"session_deleted","sid":sid}));
        assert_ne!(app.cluster_store.runs[0].leader_sid, sid);
        assert!(app.cluster_store.runs[0].paused);
        assert_eq!(app.cluster_store.runs[0].title, "Goal");
    }
}
