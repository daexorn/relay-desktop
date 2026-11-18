use super::*;
use crate::cluster::{Agent, Config, Run, TaskState};
use crate::templates::{self, Context, Snippet, Variable};

pub(super) struct Editor {
    config: Config,
    tab: usize,
    picked: usize,
    error: String,
    snippets: Vec<Snippet>,
    search: String,
    preview: bool,
}
impl RelayApp {
    pub(super) fn seed_orchestration_preview(&mut self, ctx: &egui::Context, args: &[String]) {
        let preview = args
            .iter()
            .any(|a| a.starts_with("--demo-cluster-") || a == "--demo-leader");
        if preview {
            self.seed_cluster_demo();
            self.settings.prompt_snippets=vec![Snippet{key:"контекст_проекта".into(),value:"Проект Relay. Исходники: [данные_папки_(Production)]. Сначала изучи существующие правила проекта.".into()}];
            let cfg = &mut self.cluster_store.runs.last_mut().unwrap().config;
            cfg.shared_prompt = "Команда: [имя_кластера].\n[контекст_проекта]".into();
            cfg.leader.prompt="Ты руководитель команды. Распредели задачи и проверь результаты.\nОсновной сервер: [данные_айпи_(Production)]\nРабочая папка: [рабочая_папка]\nВ конце перечисли реальные проверки.".into();
            let cfg = cfg.clone();
            self.settings.orchestrators = vec![cfg.clone()];
            if args.iter().any(|a| {
                a == "--demo-cluster-editor"
                    || a == "--demo-cluster-prompts"
                    || a == "--demo-cluster-keys"
            }) {
                self.cluster_open = true;
                self.edit_cluster_config(cfg);
                let e = self.cluster_edit.as_mut().unwrap();
                if args.iter().any(|a| a == "--demo-cluster-prompts") {
                    e.tab = 1;
                    e.picked = 1;
                    e.preview = true;
                }
                if args.iter().any(|a| a == "--demo-cluster-keys") {
                    e.tab = 3;
                }
            }
            if args.iter().any(|a| a == "--demo-leader") {
                let r = self.cluster_store.runs.last().unwrap();
                let sid = r.leader_sid.clone();
                let server = r.config.leader.server.clone();
                let mut c = Chat::new(
                    sid.clone(),
                    "/opt/atlas".into(),
                    model::shorten(&r.title, 70),
                );
                c.loaded = true;
                let result = r.task_json(&r.tasks[1].id, true).unwrap();
                c.messages=vec![Message{role:"user".into(),text:r.title.clone(),..Default::default()},Message{role:"assistant".into(),text:"Передал работу над интерфейсом исполнителю. Проверка совместимости уже завершена; жду второй результат и затем сверю сборку.".into(),..Default::default()},Message{role:"user".into(),text:format!("<relay-cluster-results>\n[{}]\n</relay-cluster-results>",result),..Default::default()}];
                self.chats.insert(key(&server, &sid), c);
                self.settings.selected = Some(server.clone());
                self.selected_chat.insert(server.clone(), sid.clone());
                if let Some(remote) = self.remote.get_mut(&server) {
                    remote.sessions.insert(
                        0,
                        Session {
                            id: sid,
                            title: "Команда Relay".into(),
                            cwd: "/opt/atlas".into(),
                            modified: crate::cluster::now(),
                            ..Default::default()
                        },
                    );
                }
                self.cluster_open = false;
            }
        }
        if args.iter().any(|a| a == "--demo-command-output") {
            if let Some(k) = self.current_key() {
                let c = self.chats.get_mut(&k).unwrap();
                c.messages=vec![Message{role:"assistant".into(),text:"Проверяю сборку и тесты.".into(),..Default::default()},
                    Message{role:"tool".into(),tool:"Bash".into(),tool_id:"build".into(),input:json!({"command":"cargo build --release"}),..Default::default()},
                    Message{role:"tool_result".into(),tool_id:"build".into(),text:"Compiling relay-desktop v0.7.0\nFinished release profile in 42.3s".into(),..Default::default()},
                    Message{role:"tool".into(),tool:"mcp__relay_desktop__run_local_command".into(),tool_id:"test".into(),input:json!({"command":"cargo test --locked"}),..Default::default()},
                    Message{role:"tool_result".into(),tool_id:"test".into(),text:json!({"ok":true,"exit_code":0,"stdout":"running tests\nAll tests passed\nNo failures or ignored checks.","stderr":""}).to_string(),..Default::default()},
                    Message{role:"assistant".into(),text:"Сборка и проверки завершены. Результаты приведены в карточках команд.".into(),..Default::default()}];
                let id = egui::Id::new(("activity-group", &k, 1usize));
                for id in [id, id.with(0usize), id.with(1usize)] {
                    let mut state =
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ctx, id, true,
                        );
                    state.set_open(true);
                    state.store(ctx);
                }
            }
        }
    }
    fn default_cluster_config(&self) -> Config {
        let servers = &self.settings.servers;
        let mut c = Config::default();
        if let Some(s) = self.server().or_else(|| servers.first().cloned()) {
            c.leader.server = s.id.clone();
            c.leader.model = "opus".into();
            c.workers = (0..2)
                .map(|i| Agent {
                    name: format!("Исполнитель {}", i + 1),
                    server: s.id.clone(),
                    model: "sonnet".into(),
                    ..Default::default()
                })
                .collect();
        }
        c
    }
    fn edit_cluster_config(&mut self, config: Config) {
        self.cluster_edit = Some(Editor {
            config,
            tab: 0,
            picked: 0,
            error: String::new(),
            snippets: self.settings.prompt_snippets.clone(),
            search: String::new(),
            preview: false,
        });
    }
    pub(super) fn cluster_panel(&mut self, ctx: &egui::Context) {
        if self.demo && self.cluster_store.runs.is_empty() {
            self.seed_cluster_demo();
        }
        let p = design::animated_palette(ctx, self.settings.light);
        let selected = self.cluster_store.selected.clone();
        let valid = self
            .cluster_snapshot
            .as_ref()
            .is_some_and(|(revision, id, _)| {
                *revision == self.cluster_store.revision && Some(id) == selected.as_ref()
            });
        if !valid {
            self.cluster_snapshot = selected.as_ref().and_then(|id| {
                self.cluster_store
                    .runs
                    .iter()
                    .find(|r| &r.id == id)
                    .map(|r| {
                        (
                            self.cluster_store.revision,
                            id.clone(),
                            std::sync::Arc::new(r.clone()),
                        )
                    })
            });
        }
        let run = self
            .cluster_snapshot
            .as_ref()
            .map(|(_, _, r)| std::sync::Arc::clone(r));
        if self.cluster_target.is_empty() {
            self.cluster_target = self
                .settings
                .orchestrators
                .first()
                .map(|c| c.id.clone())
                .unwrap_or_default();
        }
        let fallback = if run.is_none() {
            self.settings
                .orchestrators
                .iter()
                .find(|c| c.id == self.cluster_target)
                .cloned()
        } else {
            None
        };
        let config = run.as_ref().map(|r| &r.config).or(fallback.as_ref());
        let mut edit = false;
        let mut new_config = false;
        let mut new_session = false;
        let mut connect = false;
        let mut start = false;
        let mut open = None;
        let mut pause = false;
        let mut stop = false;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.base).inner_margin(24)).show(ctx,|ui| {
            design::ambient_horizon(ui.painter(), ui.max_rect().expand(24.), accent(), self.settings.light);
            ui.horizontal_wrapped(|ui| {
                let (orb, _) = ui.allocate_exact_size(Vec2::splat(26.), egui::Sense::hover());
                let any_active = run.as_ref().is_some_and(|r| {
                    !r.paused && (r.tasks.iter().any(|t| t.state.active()) || self.chats.get(&key(&r.config.leader.server, &r.leader_sid)).is_some_and(|c| c.busy))
                });
                design::orbital_core(ui, orb, accent(), any_active);
                label(ui,"Команда агентов",23.,p.text);
                if self.demo{design::mini_pill(ui, Some(Icon::Spark), "Предпросмотр", accent(), p);}
                if design::button(ui,"Новая команда",Some(Icon::Plus),Kind::Secondary,p,Vec2::ZERO).clicked(){new_config=true;}
                if config.is_some()&&design::button(ui,"Настроить",Some(Icon::Settings),Kind::Ghost,p,Vec2::ZERO).clicked(){edit=true;}
            });
            label(ui,"Живая оркестрация: руководитель распределяет задачи по серверам и движкам, исполнители возвращают результаты",12.,p.muted);
            if !self.notice.is_empty(){egui::Frame::new().fill(p.surface).corner_radius(9.).inner_margin(10).show(ui,|ui|{ui.horizontal_wrapped(|ui|{label(ui,&self.notice,12.,p.text);if ui.small_button("Закрыть").clicked(){self.notice.clear();}});});}
            ui.add_space(12.);
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("cluster-config").icon(design::combo_icon)
                    .selected_text(config.as_ref().map(|c|c.name.as_str()).unwrap_or("Выбрать команду"))
                    .show_ui(ui,|ui|{for c in &self.settings.orchestrators {if ui.selectable_value(&mut self.cluster_target,c.id.clone(),&c.name).clicked(){self.cluster_store.selected=None;}}});
                egui::ComboBox::from_id_salt("cluster-run").icon(design::combo_icon)
                    .selected_text(run.as_ref().map(|r|model::shorten(&r.title,36)).unwrap_or("Новая сессия".into()))
                    .show_ui(ui,|ui| {for r in self.cluster_store.runs.iter().rev(){ui.selectable_value(&mut self.cluster_store.selected,Some(r.id.clone()),format!("{} · {}",r.config.name,model::shorten(&r.title,52)));}});
                if design::button(ui,"Новая сессия",Some(Icon::Plus),Kind::Ghost,p,Vec2::ZERO).clicked(){new_session=true;}
            });
            ui.add_space(12.);
            egui::ScrollArea::vertical().id_salt("orchestrator-dashboard").show(ui,|ui| {
                if let Some(c)=config {
                    let light = self.settings.light;
                    let enabled_workers = c.workers.iter().filter(|w| w.enabled).count();
                    let active_tasks = run.as_ref().map(|r| r.tasks.iter().filter(|t| t.state.active()).count()).unwrap_or(0);
                    let done_tasks = run.as_ref().map(|r| r.tasks.iter().filter(|t| t.state == TaskState::Done).count()).unwrap_or(0);
                    let total_tasks = run.as_ref().map(|r| r.tasks.len()).unwrap_or(0);

                    // Mission Telemetry Ribbon
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(8., 6.);
                        design::mini_pill(
                            ui,
                            Some(Icon::Cluster),
                            &format!("Исполнителей: {enabled_workers}"),
                            accent(),
                            p,
                        );
                        design::mini_pill(
                            ui,
                            Some(Icon::Pulse),
                            &format!("Активно: {active_tasks} / {}", c.max_parallel),
                            if active_tasks > 0 { accent() } else { p.muted },
                            p,
                        );
                        design::mini_pill(
                            ui,
                            Some(Icon::Check),
                            &format!("Завершено: {done_tasks} из {total_tasks}"),
                            if done_tasks > 0 { GREEN } else { p.muted },
                            p,
                        );
                        design::mini_pill(
                            ui,
                            Some(Icon::Layers),
                            &format!("Режим: {}", mode_label(&c.mode)),
                            p.muted,
                            p,
                        );
                    });
                    ui.add_space(10.);

                    let worker_nodes: Vec<_> = c
                        .workers
                        .iter()
                        .filter(|w| w.enabled)
                        .map(|a| build_node_telemetry(self, run.as_deref(), a, false, light))
                        .collect();
                    let leader_node = build_node_telemetry(self, run.as_deref(), &c.leader, true, light);
                    if let Some(clicked) = draw_graph(ui, &leader_node, &worker_nodes, light, p) {
                        if clicked=="leader" {if let Some(r)=&run{open=Some((r.config.leader.server.clone(),r.leader_sid.clone()));}}
                        else if let Some(r)=&run{if let Some(t)=r.tasks.iter().rev().find(|t|t.agent.id==clicked){open=Some((t.agent.server.clone(),t.sid.clone()));}}
                    }
                    ui.add_space(10.);
                    ui.horizontal_wrapped(|ui| {
                        if design::button(ui,"Подключить хосты",Some(Icon::Server),Kind::Secondary,p,Vec2::ZERO).clicked(){connect=true;}
                        if let Some(r)=&run {
                            if design::button(ui,"К руководителю",Some(Icon::Cluster),Kind::Primary,p,Vec2::ZERO).clicked(){open=Some((r.config.leader.server.clone(),r.leader_sid.clone()));}
                            if design::button(ui,if r.paused{"Продолжить очередь"}else{"Пауза очереди"},None,Kind::Secondary,p,Vec2::ZERO).clicked(){pause=true;}
                            if design::button(ui,"Остановить команду",Some(Icon::Stop),Kind::Ghost,p,Vec2::ZERO).clicked(){stop=true;}
                        }
                    });
                    if let Some(r)=&run{if r.paused{label(ui,"Очередь на паузе. Сохранённые результаты доступны руководителю.",12.,p.muted);}}
                    ui.add_space(16.);
                    egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0_f32,p.line)).corner_radius(14.).inner_margin(14).show(ui,|ui| {
                        ui.set_width(ui.available_width());
                        label(ui,if run.is_some(){"Сообщение руководителю"}else{"Цель для всей команды"},14.,p.text);
                        ui.add(egui::TextEdit::multiline(&mut self.cluster_prompt).id(egui::Id::new("cluster-goal-input")).desired_width(f32::INFINITY).desired_rows(3).frame(false)
                            .hint_text("Опиши результат, ограничения и условия готовности…"));
                        let vars=templates::variables(&self.settings.servers,&self.settings.prompt_snippets,&Context::default()).into_iter().filter(|v|v.token!="[задача]").collect::<Vec<_>>();
                        variable_picker(ui,&mut self.cluster_prompt,egui::Id::new("cluster-goal-input"),&vars,p);
                        ui.add_enabled_ui(!self.cluster_prompt.trim().is_empty(),|ui| {
                            if design::button(ui,if run.is_some(){"Отправить руководителю"}else{"Запустить сессию кластера"},Some(Icon::Send),Kind::Primary,p,Vec2::ZERO).clicked(){start=true;}
                        });
                        label(ui,format!("До {} задач одновременно · до {} назначений · {} исполнителей",c.max_parallel,c.max_tasks,enabled_workers),11.,p.muted);
                    });
                    if let Some(r)=&run {
                        ui.add_space(20.);label(ui,"Задачи и результаты",16.,p.text);
                        if r.tasks.is_empty(){label(ui,"Задачи появятся, когда руководитель распределит работу.",12.,p.muted);}
                        for t in r.tasks.iter().rev() {
                            let eff_b = if t.agent.backend.is_empty() {
                                self.settings.servers.iter().find(|s| s.id == t.agent.server).map(|s| s.effective_backend()).unwrap_or("claude")
                            } else {
                                t.agent.backend.as_str()
                            };
                            let b_col = design::backend_color(eff_b, light);
                            let st_col = state_color(t.state, p);
                            egui::Frame::new()
                                .fill(design::mix(p.surface, st_col, if t.state.active() { 0.06 } else { 0.02 }))
                                .stroke(Stroke::new(1.0_f32, design::mix(p.line, st_col, if t.state.active() { 0.45 } else { 0.15 })))
                                .corner_radius(12.)
                                .inner_margin(13)
                                .show(ui,|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal_wrapped(|ui| {
                                        design::mini_pill(ui, Some(if t.state == TaskState::Done { Icon::Check } else { Icon::Pulse }), t.state.label(), st_col, p);
                                        design::mini_pill(ui, Some(design::backend_icon(eff_b)), crate::model::backend_short_label(eff_b), b_col, p);
                                        label(ui,&t.agent.name,12.5,p.text);
                                        if t.delivered {
                                            design::mini_pill(ui, Some(Icon::Check), "Передано руководителю", GREEN, p);
                                        }
                                        if design::button(ui,"Открыть сессию",Some(Icon::Arrow),Kind::Ghost,p,Vec2::ZERO).clicked(){open=Some((t.agent.server.clone(),t.sid.clone()));}
                                    });
                                    ui.add_space(4.);
                                    ui.add(egui::Label::new(RichText::new(&t.title).size(14.).color(p.text)).wrap());
                                    if !t.error.is_empty(){label(ui,model::shorten(&t.error,240),12.,Color32::from_rgb(228,126,136));}
                                    if !t.result.is_empty(){label(ui,model::shorten(&t.result,350),12.,p.muted);}
                                });
                            ui.add_space(7.);
                        }
                        ui.add_space(18.);label(ui,"Журнал оркестрации",15.,p.text);
                        for entry in r.journal.iter().rev().take(12) {
                            ui.horizontal(|ui| {
                                let (dr, _) = ui.allocate_exact_size(Vec2::splat(10.), egui::Sense::hover());
                                ui.painter().circle_filled(dr.center(), 2.5, accent().gamma_multiply(0.7));
                                label(ui,&entry.message,12.,p.muted);
                            });
                        }
                    }
                }else{
                    ui.add_space(40.);label(ui,"Создай команду и выбери её руководителя",21.,p.text);
                    label(ui,"У каждого исполнителя может быть свой хост, папка, движок (Claude, Codex, AGY, API) и промпт.",13.,p.muted);
                }
            });
        });
        if new_config {
            self.edit_cluster_config(self.default_cluster_config());
        }
        if edit {
            if let Some(c) = config {
                self.edit_cluster_config(Config::clone(c));
            }
        }
        if new_session {
            if let Some(c) = config {
                self.cluster_target = c.id.clone();
            }
            self.cluster_store.selected = None;
            self.cluster_store.dirty = true;
        }
        if connect {
            if let Some(c) = config {
                let ids: std::collections::HashSet<_> = std::iter::once(&c.leader)
                    .chain(c.workers.iter().filter(|a| a.enabled))
                    .map(|a| a.server.clone())
                    .collect();
                for id in ids {
                    if !self.connected(&id) && !self.remote.get(&id).is_some_and(|r| r.connecting) {
                        if let Some(s) = self.settings.servers.iter().find(|s| s.id == id).cloned()
                        {
                            self.connect(s);
                        }
                    }
                }
            }
        }
        if let Some((server, sid)) = open {
            self.open_cluster_chat(&server, &sid);
        }
        if pause {
            if let Some(id) = &selected {
                if let Some(r) = self.cluster_store.runs.iter_mut().find(|r| &r.id == id) {
                    r.paused = !r.paused;
                    if !r.paused {
                        if let Some(c) = self
                            .chats
                            .get_mut(&key(&r.config.leader.server, &r.leader_sid))
                        {
                            c.queue_paused = false;
                        }
                    }
                    r.log(if r.paused {
                        "Очередь приостановлена"
                    } else {
                        "Очередь продолжена"
                    });
                    self.cluster_store.dirty = true;
                }
            }
        }
        if stop {
            if let Some(id) = &selected {
                self.stop_cluster(id);
            }
        }
        if start {
            if let Some(r) = run.as_ref() {
                let server_id = r.config.leader.server.clone();
                let sid = r.leader_sid.clone();
                if let Some(server) = self
                    .settings
                    .servers
                    .iter()
                    .find(|s| s.id == server_id)
                    .cloned()
                {
                    let k = key(&server_id, &sid);
                    if let Some(c) = self.chats.get_mut(&k) {
                        if c.draft.is_empty() {
                            c.draft = std::mem::take(&mut self.cluster_prompt);
                            if c.busy {
                                c.enqueue_draft();
                            } else {
                                self.send_to_chat(server, k);
                            }
                        } else {
                            self.notice =
                                "У руководителя есть черновик. Открой сессию, чтобы продолжить."
                                    .into();
                        }
                    }
                }
            } else if let Some(c) = config {
                if let Err(e) = self.start_cluster(Config::clone(c), self.cluster_prompt.clone()) {
                    self.notice = e;
                }
            }
        }
    }
    pub(super) fn cluster_settings(&mut self, ctx: &egui::Context) {
        let Some(mut e) = self.cluster_edit.take() else {
            return;
        };
        let p = design::animated_palette(ctx, self.settings.light);
        let mut open = true;
        let mut save = false;
        egui::Window::new("Настройка агентского кластера").id(egui::Id::new("cluster-editor")).open(&mut open)
            .default_width(760.).default_height(680.).default_pos(egui::pos2(((ctx.screen_rect().width()-800.)/2.).max(8.),35.))
            .resizable(true).collapsible(false).show(ctx,|ui| {
                ui.horizontal_wrapped(|ui|{for (i,name) in ["Состав","Промпты","Лимиты","Вставки"].iter().enumerate(){if ui.selectable_label(e.tab==i,*name).clicked(){e.tab=i;}}});
                ui.add_space(12.);
                egui::ScrollArea::vertical().id_salt("cluster-editor-scroll").max_height((ctx.screen_rect().height()-230.).clamp(220.,650.)).show(ui,|ui| {
                    ui.set_min_width(ui.available_width().min(700.));
                    match e.tab {
                        0=>{
                            label(ui,"Название команды",12.,p.muted);ui.add(egui::TextEdit::singleline(&mut e.config.name).desired_width(f32::INFINITY));
                            ui.add_space(10.);agent_editor(ui,&mut e.config.leader,&self.settings.servers,true,p);
                            let mut remove=None;
                            for (i,a) in e.config.workers.iter_mut().enumerate(){ui.add_space(10.);ui.push_id(i,|ui|{agent_editor(ui,a,&self.settings.servers,false,p);if design::button(ui,"Убрать исполнителя",Some(Icon::Close),Kind::Ghost,p,Vec2::ZERO).clicked(){remove=Some(i);}});}
                            if let Some(i)=remove{e.config.workers.remove(i);}
                            ui.add_space(10.);
                            if e.config.workers.len()<12&&design::button(ui,"Добавить исполнителя",Some(Icon::Plus),Kind::Secondary,p,Vec2::ZERO).clicked(){e.config.workers.push(Agent{name:format!("Исполнитель {}",e.config.workers.len()+1),server:e.config.leader.server.clone(),model:"default".into(),..Default::default()});}
                        }
                        1=>{
                            let names:Vec<_>=std::iter::once("Общие инструкции команды".into()).chain(std::iter::once(format!("Руководитель · {}",e.config.leader.name))).chain(e.config.workers.iter().map(|a|format!("Исполнитель · {}",a.name))).collect();
                            e.picked=e.picked.min(names.len()-1);
                            egui::ComboBox::from_id_salt("prompt-agent").icon(design::combo_icon).selected_text(&names[e.picked]).show_ui(ui,|ui|{for (i,name) in names.iter().enumerate(){ui.selectable_value(&mut e.picked,i,name);}});
                            let agent=if e.picked<=1{e.config.leader.clone()}else{e.config.workers[e.picked-2].clone()};
                            let server=self.settings.servers.iter().find(|s|s.id==agent.server);
                            let cwd=server.map(|s|super::cluster_runtime::agent_cwd(&agent,s)).unwrap_or_default();
                            let context=Context{cluster:&e.config.name,agent:&agent.name,task:"Пример задачи для предпросмотра",host:server.map(|s|s.host.as_str()).unwrap_or(""),cwd:&cwd};
                            let vars=templates::variables(&self.settings.servers,&e.snippets,&context);
                            let field=egui::Id::new(("custom-cluster-prompt",&e.config.id,e.picked));
                            let text=if e.picked==0{&mut e.config.shared_prompt}else if e.picked==1{&mut e.config.leader.prompt}else{&mut e.config.workers[e.picked-2].prompt};
                            label(ui,"Эти инструкции добавляются к системному промпту выбранного агента.",12.,p.muted);
                            ui.add(egui::TextEdit::multiline(text).id(field).desired_rows(12).desired_width(f32::INFINITY).hint_text("Обязанности, ограничения, формат результата и правила проверки…"));
                            variable_picker(ui,text,field,&vars,p);
                            ui.checkbox(&mut e.preview,"Предпросмотр подстановок");
                            if e.preview{match templates::expand(text,&vars){Ok(value)=>{egui::Frame::new().fill(p.base).corner_radius(9.).inner_margin(12).show(ui,|ui|{ui.add(egui::Label::new(RichText::new(value).monospace().size(12.)).wrap().selectable(true));});},Err(error)=>{label(ui,error,12.,Color32::from_rgb(222,143,151));}}}
                        }
                        2=>{
                            label(ui,"Как работает команда",18.,p.text);
                            ui.add(egui::Slider::new(&mut e.config.max_parallel,1..=8).text("Задач одновременно"));
                            ui.add(egui::Slider::new(&mut e.config.max_tasks,1..=128).text("Назначений за сессию"));
                            egui::ComboBox::from_id_salt("cluster-mode").icon(design::combo_icon).selected_text(mode_label(&e.config.mode)).show_ui(ui,|ui|{for id in ["auto","manual","acceptEdits","plan"]{ui.selectable_value(&mut e.config.mode,id.into(),mode_label(id));}});
                            ui.checkbox(&mut e.config.auto_results,"Возвращать результаты руководителю автоматически, если он завершил ответ раньше исполнителей");
                            label(ui,"Один исполнитель получает по одной задаче. Зависимости и общий предел параллелизма соблюдаются Relay.",12.,p.muted);
                        }
                        _=>{
                            label(ui,"Данные серверов берутся из сохранённых подключений Relay слева.",12.,p.muted);
                            ui.add(egui::TextEdit::singleline(&mut e.search).hint_text("Найти сервер или ключ").desired_width(f32::INFINITY));
                            let vars=templates::variables(&self.settings.servers,&e.snippets,&Context::default());
                            for v in vars.iter().filter(|v|!v.label.ends_with(" · ID")&&(e.search.is_empty()||format!("{} {}",v.token,v.label).to_lowercase().contains(&e.search.to_lowercase()))) {
                                ui.horizontal_wrapped(|ui|{if ui.small_button("Копировать").clicked(){ctx.copy_text(v.token.clone());}ui.label(RichText::new(&v.token).monospace().size(12.));label(ui,&v.label,11.,p.muted);});
                            }
                            ui.add_space(16.);label(ui,"Мои фрагменты промпта",16.,p.text);
                            let mut remove=None;
                            for (i,s) in e.snippets.iter_mut().enumerate(){ui.push_id(i,|ui|{
                                ui.horizontal(|ui|{label(ui,"Ключ",12.,p.muted);ui.add(egui::TextEdit::singleline(&mut s.key).desired_width(200.));if ui.small_button("Удалить").clicked(){remove=Some(i);}});
                                ui.add(egui::TextEdit::multiline(&mut s.value).desired_rows(3).desired_width(f32::INFINITY).hint_text("Текст, который будет вставлен вместо ключа. Можно использовать данные серверов."));ui.add_space(10.);
                            });}
                            if let Some(i)=remove{e.snippets.remove(i);}
                            if design::button(ui,"Добавить фрагмент",Some(Icon::Plus),Kind::Secondary,p,Vec2::ZERO).clicked(){e.snippets.push(Snippet{key:format!("мой_контекст_{}",e.snippets.len()+1),value:String::new()});}
                        }
                    }
                });
                ui.add_space(12.);
                if !e.error.is_empty(){label(ui,&e.error,12.,Color32::from_rgb(222,143,151));}
                ui.horizontal(|ui|{
                    if design::button(ui,"Сохранить настройки",Some(Icon::Check),Kind::Primary,p,Vec2::ZERO).clicked(){save=true;}
                    label(ui,"Промпты сохраняются вместе с командой",11.,p.muted);
                });
            });
        if save {
            let validation = e.config.validate().and_then(|_| {
                let mut keys = std::collections::HashSet::new();
                for s in &e.snippets {
                    if s.key.is_empty()
                        || s.key.len() > 128
                        || !s
                            .key
                            .chars()
                            .all(|c| c.is_alphanumeric() || "_.-".contains(c))
                        || !keys.insert(s.key.clone())
                    {
                        return Err("У вставок должны быть разные имена из букв, цифр и _.−".into());
                    }
                }
                let vars =
                    templates::variables(&self.settings.servers, &e.snippets, &Context::default());
                for s in &e.snippets {
                    templates::expand(&format!("[{}]", s.key), &vars)?;
                }
                Ok(())
            });
            match validation {
                Ok(()) => {
                    if let Some(c) = self
                        .settings
                        .orchestrators
                        .iter_mut()
                        .find(|c| c.id == e.config.id)
                    {
                        *c = e.config.clone();
                    } else {
                        self.settings.orchestrators.push(e.config.clone());
                    }
                    self.settings.prompt_snippets = e.snippets.clone();
                    self.cluster_target = e.config.id.clone();
                    if let Some(id) = self.cluster_store.selected.clone() {
                        if let Some(r) = self
                            .cluster_store
                            .runs
                            .iter_mut()
                            .find(|r| r.id == id && r.config.id == e.config.id)
                        {
                            let busy = self
                                .chats
                                .get(&key(&r.config.leader.server, &r.leader_sid))
                                .is_some_and(|c| c.busy)
                                || r.tasks.iter().any(|t| t.state.active());
                            if r.config.leader.server == e.config.leader.server
                                && r.config.leader.cwd == e.config.leader.cwd
                            {
                                r.pending_config = Some(e.config.clone());
                                r.log(if busy {
                                    "Новые промпты применятся после текущей работы"
                                } else {
                                    "Промпты обновлены для следующего сообщения"
                                });
                                self.cluster_store.dirty = true;
                            } else {
                                self.notice="Настройки сохранены для новой сессии команды. Текущая работа продолжается со своим составом.".into();
                            }
                        }
                    }
                    if self
                        .cluster_store
                        .selected
                        .as_ref()
                        .and_then(|id| self.cluster_store.runs.iter().find(|r| &r.id == id))
                        .is_some_and(|r| r.config.id != e.config.id)
                    {
                        self.cluster_store.selected = None;
                    }
                    self.save();
                    open = false;
                }
                Err(error) => e.error = error,
            }
        }
        if open {
            self.cluster_edit = Some(e);
        }
    }
    fn seed_cluster_demo(&mut self) {
        let mut c = self.default_cluster_config();
        c.name = "Команда Relay".into();
        c.leader.name = "Архитектор".into();
        c.leader.backend = "claude".into();
        c.workers = vec![
            Agent {
                id: "ui-worker".into(),
                name: "Интерфейс ПК".into(),
                server: "demo".into(),
                backend: "codex".into(),
                model: "gpt-5.4".into(),
                ..Default::default()
            },
            Agent {
                id: "review-worker".into(),
                name: "Проверка ядра".into(),
                server: "stage".into(),
                backend: "claude".into(),
                model: "sonnet".into(),
                ..Default::default()
            },
            Agent {
                id: "mobile-worker".into(),
                name: "Android и TUI".into(),
                server: "demo".into(),
                backend: "agy".into(),
                model: "gemini-3.1-pro-preview".into(),
                ..Default::default()
            },
        ];
        let mut r = Run::new(
            c.clone(),
            "Собрать визуализацию конвейера агентов и проверить совместимость".into(),
        );
        r.dispatch(
            "one",
            &json!({"worker":"ui-worker","title":"Топология и конвейер шагов","task":"Отрисовать узлы агентов и потоки"}),
        )
        .unwrap();
        r.dispatch(
            "two",
            &json!({"worker":"review-worker","title":"Проверить совместимость","task":"Проверить старые настройки и тесты"}),
        )
        .unwrap();
        r.dispatch(
            "three",
            &json!({"worker":"mobile-worker","title":"Радар агентов на Android","task":"Обновить экраны и шторку радара"}),
        )
        .unwrap();
        r.tasks[0].state = TaskState::Running;
        r.tasks[1].state = TaskState::Done;
        r.tasks[1].result = "Старые профили загружаются. 142 проверки пройдены.".into();
        r.tasks[1].delivered = true;
        r.tasks[2].state = TaskState::Running;

        // Populate live tool telemetry on demo workers so topology cards show real actions
        let mut w1_chat = Chat::new(r.tasks[0].sid.clone(), "/opt/relay".into(), "Интерфейс ПК".into());
        w1_chat.busy = true;
        w1_chat.loaded = true;
        w1_chat.messages = vec![
            Message {
                role: "tool".into(),
                tool: "Edit".into(),
                tool_id: "w1-t1".into(),
                input: json!({"file_path": "/opt/relay/src/timeline.rs", "description": "Конвейер шагов"}),
                ..Default::default()
            },
            Message {
                role: "tool_result".into(),
                tool_id: "w1-t1".into(),
                text: "ok".into(),
                ..Default::default()
            },
            Message {
                role: "tool".into(),
                tool: "Bash".into(),
                tool_id: "w1-t2".into(),
                input: json!({"command": "cargo test --locked", "description": "Проверка Rust-тестов"}),
                ..Default::default()
            },
        ];
        self.chats.insert(key("demo", &r.tasks[0].sid), w1_chat);

        let mut w3_chat = Chat::new(r.tasks[2].sid.clone(), "/opt/relay/android".into(), "Android и TUI".into());
        w3_chat.busy = true;
        w3_chat.loaded = true;
        w3_chat.messages = vec![Message {
            role: "tool".into(),
            tool: "Edit".into(),
            tool_id: "w3-t1".into(),
            input: json!({"file_path": "/opt/relay/android/app/src/main/java/dev/relay/mobile/Ui.kt", "description": "Шторка радара агентов"}),
            ..Default::default()
        }];
        self.chats.insert(key("demo", &r.tasks[2].sid), w3_chat);

        self.settings.orchestrators = vec![c];
        self.cluster_store.selected = Some(r.id.clone());
        self.cluster_store.runs.push(r);
    }
}
fn mode_label(mode: &str) -> &str {
    match mode {
        "auto" => "Авто",
        "acceptEdits" => "Правки файлов",
        "plan" => "Планирование",
        _ => "С подтверждением",
    }
}
fn agent_editor(ui: &mut egui::Ui, a: &mut Agent, servers: &[Server], leader: bool, p: Palette) {
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(12.)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            label(
                ui,
                if leader {
                    "Руководитель"
                } else {
                    "Исполнитель"
                },
                14.,
                if leader { accent() } else { p.text },
            );
            ui.horizontal_wrapped(|ui| {
                if !leader {
                    ui.checkbox(&mut a.enabled, "Участвует");
                }
                ui.add(
                    egui::TextEdit::singleline(&mut a.name)
                        .desired_width(180.)
                        .hint_text("Имя агента"),
                );
                egui::ComboBox::from_id_salt(("agent-server", &a.id))
                    .icon(design::combo_icon)
                    .selected_text(
                        servers
                            .iter()
                            .find(|s| s.id == a.server)
                            .map(|s| s.name.as_str())
                            .unwrap_or("Выбрать хост"),
                    )
                    .show_ui(ui, |ui| {
                        for s in servers {
                            ui.selectable_value(&mut a.server, s.id.clone(), &s.name);
                        }
                    });
            });
            ui.add(
                egui::TextEdit::singleline(&mut a.cwd)
                    .desired_width(f32::INFINITY)
                    .hint_text("Рабочая папка · пусто — папка профиля"),
            );
            ui.horizontal_wrapped(|ui| {
                let backend_text = match a.backend.as_str() {
                    "claude" => "Агент: Claude Code",
                    "codex" => "Агент: Codex",
                    "agy" => "Агент: AGY",
                    "api" => "Агент: API",
                    _ => "Агент: по профилю хоста",
                };
                let prev_backend = a.backend.clone();
                egui::ComboBox::from_id_salt(("agent-backend", &a.id))
                    .icon(design::combo_icon)
                    .selected_text(backend_text)
                    .show_ui(ui, |ui| {
                        for (id, name) in [
                            ("", "По профилю хоста"),
                            ("claude", "Claude Code"),
                            ("codex", "Codex"),
                            ("agy", "Antigravity (agy)"),
                            ("api", "Пользовательский API"),
                        ] {
                            ui.selectable_value(&mut a.backend, id.into(), name);
                        }
                    });
                if a.backend != prev_backend {
                    a.model = "default".into();
                }
                let effective_backend = if a.backend.is_empty() {
                    servers
                        .iter()
                        .find(|s| s.id == a.server)
                        .map(|s| s.effective_backend())
                        .unwrap_or("claude")
                } else {
                    a.backend.as_str()
                };
                egui::ComboBox::from_id_salt(("agent-model", &a.id))
                    .icon(design::combo_icon)
                    .selected_text(format!("Модель: {}", a.model))
                    .show_ui(ui, |ui| {
                        match effective_backend {
                            "codex" => {
                                ui.selectable_value(&mut a.model, "inherit".into(), "Модель профиля");
                                for m in crate::model::default_backend_models("codex") {
                                    ui.selectable_value(&mut a.model, m.value, m.display_name);
                                }
                            }
                            "agy" => {
                                ui.selectable_value(&mut a.model, "inherit".into(), "Модель профиля");
                                for m in crate::model::default_backend_models("agy") {
                                    ui.selectable_value(&mut a.model, m.value, m.display_name);
                                }
                            }
                            "api" => {
                                ui.selectable_value(&mut a.model, "default".into(), "Модель API профиля");
                                if let Some(srv) = servers.iter().find(|s| s.id == a.server) {
                                    for id in &srv.api.models {
                                        ui.selectable_value(&mut a.model, id.clone(), id);
                                    }
                                }
                            }
                            _ => {
                                for (id, name) in [
                                    ("default", "Модель Claude по умолчанию"),
                                    ("inherit", "Модель профиля"),
                                    ("opus", "Opus"),
                                    ("sonnet", "Sonnet"),
                                    ("haiku", "Haiku"),
                                ] {
                                    ui.selectable_value(&mut a.model, id.into(), name);
                                }
                            }
                        }
                        ui.separator();
                        ui.add(
                            egui::TextEdit::singleline(&mut a.model)
                                .desired_width(180.)
                                .hint_text("Своя модель…"),
                        );
                    });
                egui::ComboBox::from_id_salt(("agent-effort", &a.id))
                    .icon(design::combo_icon)
                    .selected_text(format!("Глубина: {}", a.effort))
                    .show_ui(ui, |ui| {
                        for id in ["default", "low", "medium", "high", "xhigh", "max"] {
                            ui.selectable_value(&mut a.effort, id.into(), id);
                        }
                    });
            });
        });
}
fn variable_picker(
    ui: &mut egui::Ui,
    text: &mut String,
    field: egui::Id,
    vars: &[Variable],
    p: Palette,
) {
    egui::ComboBox::from_id_salt(field.with("variables"))
        .icon(design::combo_icon)
        .selected_text(RichText::new("Вставить ключ").size(12.).color(p.muted))
        .show_ui(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(250.)
                .show(ui, |ui| {
                    for v in vars.iter().filter(|v| !v.label.ends_with(" · ID")) {
                        if ui
                            .selectable_label(false, format!("{}  {}", v.token, v.label))
                            .on_hover_text(&v.value)
                            .clicked()
                        {
                            let pos = egui::TextEdit::load_state(ui.ctx(), field)
                                .and_then(|s| s.cursor.char_range())
                                .map(|r| r.primary.index)
                                .unwrap_or_else(|| text.chars().count());
                            let at = text
                                .char_indices()
                                .nth(pos)
                                .map(|(i, _)| i)
                                .unwrap_or(text.len());
                            text.insert_str(at, &v.token);
                            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field) {
                                state
                                    .cursor
                                    .set_char_range(Some(egui::text::CCursorRange::one(
                                        egui::text::CCursor::new(pos + v.token.chars().count()),
                                    )));
                                state.store(ui.ctx(), field);
                            }
                            ui.memory_mut(|m| m.request_focus(field));
                        }
                    }
                });
        });
}
struct NodeTelemetry {
    id: String,
    name: String,
    host: String,
    backend: String,
    status: String,
    detail: String,
    tel: crate::model::TurnTelemetry,
    active: bool,
    done: bool,
    delivered: bool,
    failed: bool,
    assigned_count: usize,
    done_count: usize,
}

fn build_node_telemetry(
    app: &RelayApp,
    run: Option<&Run>,
    agent: &Agent,
    leader: bool,
    _light: bool,
) -> NodeTelemetry {
    let srv = app.settings.servers.iter().find(|s| s.id == agent.server);
    let host = srv
        .map(|s| s.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "хост".into());
    let backend = if crate::model::valid_backend(&agent.backend) {
        agent.backend.clone()
    } else {
        srv.map(|s| s.effective_backend().to_owned())
            .unwrap_or_else(|| "claude".into())
    };

    if leader {
        let (status, active, sid_opt) = if let Some(r) = run {
            let k = key(&r.config.leader.server, &r.leader_sid);
            let st = if r.paused {
                ("На паузе".into(), false)
            } else {
                match app.chats.get(&k) {
                    Some(chat) if !chat.permissions.is_empty() => {
                        ("Ждёт подтверждения".into(), true)
                    }
                    Some(chat) if chat.busy => ("Координирует команду".into(), true),
                    _ if r.tasks.iter().any(|t| !t.state.terminal()) => {
                        ("Принимает отчёты".into(), true)
                    }
                    _ => ("Ожидает цель".into(), false),
                }
            };
            (st.0, st.1, Some(k))
        } else {
            ("Готов к запуску".into(), false, None)
        };
        let tel = sid_opt
            .as_ref()
            .and_then(|k| app.chats.get(k))
            .map(|c| crate::model::analyze_turn(&c.messages, c.user_at, c.busy, &c.status))
            .unwrap_or_default();
        let detail = if !tel.active_target.is_empty() {
            tel.active_target.clone()
        } else if let Some(r) = run {
            model::shorten(&r.title, 42)
        } else {
            "Оркестрация и проверка результатов".into()
        };
        let assigned_count = run.map(|r| r.tasks.len()).unwrap_or(0);
        let done_count = run
            .map(|r| r.tasks.iter().filter(|t| t.state == TaskState::Done).count())
            .unwrap_or(0);
        return NodeTelemetry {
            id: "leader".into(),
            name: agent.name.clone(),
            host,
            backend,
            status,
            detail,
            tel,
            active,
            done: assigned_count > 0 && done_count == assigned_count && !active,
            delivered: false,
            failed: false,
            assigned_count,
            done_count,
        };
    }

    let worker_tasks: Vec<_> = run
        .map(|r| r.tasks.iter().filter(|t| t.agent.id == agent.id).collect())
        .unwrap_or_default();
    let assigned_count = worker_tasks.len();
    let done_count = worker_tasks
        .iter()
        .filter(|t| t.state == TaskState::Done)
        .count();
    let latest = worker_tasks.last().copied();
    let status = if let Some(t) = latest {
        t.state.label().to_owned()
    } else if app.connected(&agent.server) {
        "Готов".into()
    } else {
        "Нет связи".into()
    };
    let active = latest.is_some_and(|t| matches!(t.state, TaskState::Running | TaskState::Waiting));
    let done = latest.is_some_and(|t| t.state == TaskState::Done);
    let delivered = latest.is_some_and(|t| t.delivered);
    let failed =
        latest.is_some_and(|t| matches!(t.state, TaskState::Failed | TaskState::Interrupted));

    let tel = latest
        .and_then(|t| app.chats.get(&key(&t.agent.server, &t.sid)))
        .map(|c| crate::model::analyze_turn(&c.messages, c.user_at, c.busy, &c.status))
        .unwrap_or_default();
    let detail = if active && !tel.active_target.is_empty() {
        tel.active_target.clone()
    } else if active && !tel.active_headline.is_empty() {
        tel.active_headline.clone()
    } else if let Some(t) = latest {
        model::shorten(&t.title, 42)
    } else {
        "Свободный слот исполнителя".into()
    };

    NodeTelemetry {
        id: agent.id.clone(),
        name: agent.name.clone(),
        host,
        backend,
        status,
        detail,
        tel,
        active,
        done,
        delivered,
        failed,
        assigned_count,
        done_count,
    }
}

fn state_color(state: TaskState, p: Palette) -> Color32 {
    match state {
        TaskState::Done => GREEN,
        TaskState::Running | TaskState::Waiting => accent(),
        TaskState::Failed | TaskState::Interrupted => Color32::from_rgb(228, 126, 136),
        _ => p.muted,
    }
}

fn bezier_point(p0: egui::Pos2, p1: egui::Pos2, p2: egui::Pos2, p3: egui::Pos2, t: f32) -> egui::Pos2 {
    let q = 1.0 - t;
    (p0.to_vec2() * (q * q * q)
        + p1.to_vec2() * (3.0 * q * q * t)
        + p2.to_vec2() * (3.0 * q * t * t)
        + p3.to_vec2() * (t * t * t))
    .to_pos2()
}

fn draw_graph(
    ui: &mut egui::Ui,
    leader: &NodeTelemetry,
    workers: &[NodeTelemetry],
    light: bool,
    p: Palette,
) -> Option<String> {
    let width = ui.available_width();
    let narrow = width < 640.;
    let node_h = 104.;
    let row_step = 116.;
    let height = if narrow {
        166. + workers.len() as f32 * row_step
    } else {
        (workers.len() as f32 * row_step + 34.).max(264.)
    };
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    ui.painter()
        .rect_filled(area, 16., design::mix(p.side, p.base, 0.38));
    ui.painter().rect_stroke(
        area,
        16.,
        Stroke::new(1.0_f32, design::mix(p.line, accent(), 0.22)),
        egui::StrokeKind::Inside,
    );
    design::specular_rim(ui.painter(), area, 16., light);

    // Subtle next-gen coordinate dot-matrix canvas
    let grid_step = 24.0_f32;
    let dot_col = p.line.gamma_multiply(if light { 0.55 } else { 0.42 });
    let mut gx = area.left() + 18.0;
    while gx < area.right() - 12.0 {
        let mut gy = area.top() + 18.0;
        while gy < area.bottom() - 12.0 {
            ui.painter().circle_filled(egui::pos2(gx, gy), 0.95, dot_col);
            gy += grid_step;
        }
        gx += grid_step;
    }

    let w = if narrow {
        (width - 40.).min(320.)
    } else {
        (width * 0.38).clamp(186., 296.)
    };
    let leader_rect = egui::Rect::from_center_size(
        egui::pos2(
            if narrow {
                area.center().x
            } else {
                area.left() + w / 2. + 24.
            },
            if narrow {
                area.top() + 68.
            } else {
                area.center().y
            },
        ),
        Vec2::new(w, node_h + 6.),
    );

    // Ambient radial aura behind leader node
    let lead_col = design::backend_color(&leader.backend, light);
    ui.painter().circle_filled(
        leader_rect.center(),
        w * 0.44,
        lead_col.gamma_multiply(if leader.active { 0.08 } else { 0.04 }),
    );

    let animated = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("relay-animations")))
        .unwrap_or(true);
    let now = ui.input(|i| i.time) as f32;
    let mut any_anim = false;
    let mut clicked = None;

    let total_workers_h = if workers.is_empty() {
        0.
    } else {
        (workers.len() - 1) as f32 * row_step
    };
    let workers_start_y = if narrow {
        area.top() + 190.
    } else {
        area.center().y - total_workers_h * 0.5
    };

    for (i, w_node) in workers.iter().enumerate() {
        let node = egui::Rect::from_center_size(
            egui::pos2(
                if narrow {
                    area.center().x
                } else {
                    area.right() - w / 2. - 24.
                },
                workers_start_y + i as f32 * row_step,
            ),
            Vec2::new(w, node_h),
        );
        let w_col = design::backend_color(&w_node.backend, light);
        if w_node.active {
            ui.painter()
                .circle_filled(node.center(), w * 0.38, w_col.gamma_multiply(0.075));
        }

        let start = if narrow {
            leader_rect.center_bottom()
        } else {
            leader_rect.right_center()
        };
        let end = if narrow {
            node.center_top()
        } else {
            node.left_center()
        };
        let d = if narrow {
            Vec2::new(0., (end.y - start.y) * 0.52)
        } else {
            Vec2::new((end.x - start.x) * 0.52, 0.)
        };
        let p1 = start + d;
        let p2 = end - d;
        let line_col = if w_node.failed {
            Color32::from_rgb(228, 126, 136)
        } else if w_node.active {
            w_col
        } else if w_node.done {
            GREEN.gamma_multiply(0.75)
        } else {
            p.line
        };

        // Soft outer conduit glow + crisp inner conduit
        if w_node.active || w_node.done {
            ui.painter()
                .add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [start, p1, p2, end],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(5.5_f32, line_col.gamma_multiply(0.14)),
                ));
        }
        ui.painter()
            .add(egui::epaint::CubicBezierShape::from_points_stroke(
                [start, p1, p2, end],
                false,
                Color32::TRANSPARENT,
                Stroke::new(
                    if w_node.active { 2.0_f32 } else { 1.25_f32 },
                    line_col,
                ),
            ));
        // Endpoint anchor sockets
        ui.painter().circle_filled(start, 3.0, lead_col);
        ui.painter().circle_filled(end, 3.0, line_col);

        // Multi-particle forward stream while active; return telemetry pulse when delivered
        if w_node.active {
            any_anim = true;
            for k in 0..3 {
                let t = if animated {
                    (now * 0.48 + k as f32 * 0.33 + i as f32 * 0.17) % 1.0
                } else {
                    0.25 + k as f32 * 0.28
                };
                let pt = bezier_point(start, p1, p2, end, t);
                ui.painter()
                    .circle_filled(pt, 5.2, w_col.gamma_multiply(0.22));
                ui.painter().circle_filled(pt, 2.7, w_col);
            }
        } else if w_node.delivered {
            let t = if animated {
                any_anim = true;
                1.0 - ((now * 0.30 + i as f32 * 0.25) % 1.0)
            } else {
                0.5
            };
            let pt = bezier_point(start, p1, p2, end, t);
            ui.painter()
                .circle_filled(pt, 4.6, GREEN.gamma_multiply(0.22));
            ui.painter().circle_filled(pt, 2.3, GREEN);
        }

        if graph_node(ui, node, w_node, false, light, p).clicked() {
            clicked = Some(w_node.id.clone());
        }
    }

    if graph_node(ui, leader_rect, leader, true, light, p).clicked() {
        clicked = Some("leader".into());
    }
    if any_anim && animated {
        ui.ctx()
            .request_repaint_after(Duration::from_millis(28));
    }
    clicked
}

fn graph_node(
    ui: &mut egui::Ui,
    r: egui::Rect,
    n: &NodeTelemetry,
    leader: bool,
    light: bool,
    p: Palette,
) -> egui::Response {
    let response = ui.interact(r, ui.id().with(("agent-graph", &n.id)), egui::Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        design::duration(ui.ctx(), 0.18),
    );
    let b_col = design::backend_color(&n.backend, light);
    let status_col = if n.failed {
        Color32::from_rgb(228, 126, 136)
    } else if n.active {
        b_col
    } else if n.done {
        GREEN
    } else {
        p.muted
    };

    ui.painter().rect_filled(
        r,
        13.,
        design::mix(
            p.surface,
            design::mix(p.line, b_col, 0.25),
            if n.active { 0.22 } else { 0.06 } + hover * 0.28,
        ),
    );
    ui.painter().rect_stroke(
        r,
        13.,
        Stroke::new(
            if leader || n.active { 1.45_f32 } else { 1.0_f32 },
            if n.active || leader {
                design::mix(p.line, b_col, 0.62)
            } else {
                design::mix(p.line, b_col, 0.22 + hover * 0.35)
            },
        ),
        egui::StrokeKind::Inside,
    );
    // Signature top-left accent bar
    ui.painter().line_segment(
        [
            egui::pos2(r.left() + 14., r.top() + 1.0),
            egui::pos2(r.right() - 14., r.top() + 1.0),
        ],
        Stroke::new(1.6_f32, b_col.gamma_multiply(if n.active || leader { 0.65 } else { 0.28 })),
    );

    // Orbital / status emblem in top-right of card
    let orb_rect = egui::Rect::from_center_size(
        egui::pos2(r.right() - 22., r.top() + 22.),
        Vec2::splat(20.),
    );
    if n.active {
        design::orbital_core(ui, orb_rect, b_col, true);
    } else if n.done {
        ui.painter()
            .circle_filled(orb_rect.center(), 9.5, GREEN.gamma_multiply(0.16));
        design::icon(ui.painter(), orb_rect.shrink(4.), Icon::Check, GREEN);
    } else {
        ui.painter()
            .circle_filled(orb_rect.center(), 9.5, b_col.gamma_multiply(0.13));
        design::icon(
            ui.painter(),
            orb_rect.shrink(4.),
            design::backend_icon(&n.backend),
            b_col,
        );
    }

    let painter = ui.painter().with_clip_rect(r.shrink2(Vec2::new(12., 8.)));
    let role_tag = format!(
        "{} · {} · @{}",
        if leader { "ОРКЕСТРАТОР" } else { "ИСПОЛНИТЕЛЬ" },
        crate::model::backend_short_label(&n.backend).to_uppercase(),
        model::shorten(&n.host, 12),
    );
    painter.text(
        r.left_top() + Vec2::new(13., 11.),
        egui::Align2::LEFT_TOP,
        role_tag,
        FontId::proportional(9.5),
        b_col,
    );
    painter.text(
        r.left_top() + Vec2::new(13., 28.),
        egui::Align2::LEFT_TOP,
        model::shorten(&n.name, 24),
        FontId::proportional(14.5),
        p.text,
    );
    painter.text(
        r.left_top() + Vec2::new(13., 49.),
        egui::Align2::LEFT_TOP,
        model::shorten(&n.detail, 34),
        FontId::monospace(10.8),
        design::mix(p.text, p.muted, 0.35),
    );

    // Bottom telemetry row: status + mini phase bar or task counter
    painter.circle_filled(r.left_bottom() + Vec2::new(17., -15.), 3.0, status_col);
    painter.text(
        r.left_bottom() + Vec2::new(24., -9.),
        egui::Align2::LEFT_BOTTOM,
        &n.status,
        FontId::proportional(10.8),
        status_col,
    );
    if n.tel.total_actions > 0 && r.width() >= 210. {
        let pb = egui::Rect::from_min_size(
            egui::pos2(r.right() - 62., r.bottom() - 18.),
            Vec2::new(48., 5.),
        );
        design::phase_bar(ui.painter(), pb, &n.tel, light, p);
    } else if n.assigned_count > 0 && r.width() >= 210. {
        painter.text(
            r.right_bottom() + Vec2::new(-13., -9.),
            egui::Align2::RIGHT_BOTTOM,
            format!("{}/{}", n.done_count, n.assigned_count),
            FontId::proportional(10.5),
            p.muted,
        );
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
