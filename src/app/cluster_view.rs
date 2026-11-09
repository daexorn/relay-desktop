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
            ui.horizontal_wrapped(|ui| {
                label(ui,"Агентский кластер",26.,p.text);
                if self.demo{label(ui,"Предпросмотр",11.,p.muted);}
                if design::button(ui,"Новая команда",Some(Icon::Plus),Kind::Secondary,p,Vec2::ZERO).clicked(){new_config=true;}
                if config.is_some()&&design::button(ui,"Настроить",Some(Icon::Settings),Kind::Ghost,p,Vec2::ZERO).clicked(){edit=true;}
            });
            label(ui,"Один руководитель распределяет работу, исполнители возвращают ему результаты",12.,p.muted);
            if !self.notice.is_empty(){egui::Frame::new().fill(p.surface).corner_radius(9.).inner_margin(10).show(ui,|ui|{ui.horizontal_wrapped(|ui|{label(ui,&self.notice,12.,p.text);if ui.small_button("Закрыть").clicked(){self.notice.clear();}});});}
            ui.add_space(14.);
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("cluster-config").icon(design::combo_icon)
                    .selected_text(config.as_ref().map(|c|c.name.as_str()).unwrap_or("Выбрать команду"))
                    .show_ui(ui,|ui|{for c in &self.settings.orchestrators {if ui.selectable_value(&mut self.cluster_target,c.id.clone(),&c.name).clicked(){self.cluster_store.selected=None;}}});
                egui::ComboBox::from_id_salt("cluster-run").icon(design::combo_icon)
                    .selected_text(run.as_ref().map(|r|model::shorten(&r.title,36)).unwrap_or("Новая сессия".into()))
                    .show_ui(ui,|ui| {for r in self.cluster_store.runs.iter().rev(){ui.selectable_value(&mut self.cluster_store.selected,Some(r.id.clone()),format!("{} · {}",r.config.name,model::shorten(&r.title,52)));}});
                if design::button(ui,"Новая сессия",Some(Icon::Plus),Kind::Ghost,p,Vec2::ZERO).clicked(){new_session=true;}
            });
            ui.add_space(14.);
            egui::ScrollArea::vertical().id_salt("orchestrator-dashboard").show(ui,|ui| {
                if let Some(c)=config {
                    let statuses:Vec<_>=c.workers.iter().filter(|w|w.enabled).map(|a|node_status(self,run.as_deref(),a)).collect();
                    let leader_status:String=run.as_ref().map(|r|if r.paused{"Пауза".into()}else{match self.chats.get(&key(&r.config.leader.server,&r.leader_sid)){Some(chat) if !chat.permissions.is_empty()=>"Ждёт ответа пользователя".into(),Some(chat) if chat.busy=>"Работает".into(),_ if r.tasks.iter().any(|t|!t.state.terminal())=>"Ждёт исполнителей".into(),_=>"Ожидает указаний".into()}}).unwrap_or_else(||"Отдельная сессия Claude".into());
                    if let Some(clicked)=draw_graph(ui,c,run.as_deref(),&statuses,&leader_status,p){
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
                        label(ui,format!("До {} задач одновременно · до {} назначений · {} исполнителей",c.max_parallel,c.max_tasks,c.workers.iter().filter(|a|a.enabled).count()),11.,p.muted);
                    });
                    if let Some(r)=&run {
                        ui.add_space(20.);label(ui,"Задачи и результаты",16.,p.text);
                        if r.tasks.is_empty(){label(ui,"Задачи появятся, когда руководитель распределит работу.",12.,p.muted);}
                        for t in r.tasks.iter().rev() {
                            egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0_f32,p.line)).corner_radius(12.).inner_margin(12).show(ui,|ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
                                    label(ui,t.state.label(),11.5,state_color(t.state,p));label(ui,&t.agent.name,12.,p.muted);
                                    if design::button(ui,"Открыть",Some(Icon::Arrow),Kind::Ghost,p,Vec2::ZERO).clicked(){open=Some((t.agent.server.clone(),t.sid.clone()));}
                                });
                                ui.add(egui::Label::new(RichText::new(&t.title).size(14.)).wrap());
                                if !t.error.is_empty(){label(ui,model::shorten(&t.error,240),12.,Color32::from_rgb(222,143,151));}
                                if !t.result.is_empty(){label(ui,model::shorten(&t.result,350),12.,p.muted);}
                                if t.delivered{label(ui,"Результат передан руководителю",10.5,GREEN);}
                            });ui.add_space(7.);
                        }
                        ui.add_space(18.);label(ui,"Журнал команды",15.,p.text);
                        for entry in r.journal.iter().rev().take(12) {label(ui,&entry.message,12.,p.muted);}
                    }
                }else{
                    ui.add_space(40.);label(ui,"Создай команду и выбери её руководителя",21.,p.text);
                    label(ui,"У каждого исполнителя может быть свой хост, папка, модель и промпт.",13.,p.muted);
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
                            if e.config.workers.len()<12&&design::button(ui,"Добавить исполнителя",Some(Icon::Plus),Kind::Secondary,p,Vec2::ZERO).clicked(){e.config.workers.push(Agent{name:format!("Исполнитель {}",e.config.workers.len()+1),server:e.config.leader.server.clone(),model:"sonnet".into(),..Default::default()});}
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
        c.workers = vec![
            Agent {
                id: "ui-worker".into(),
                name: "Интерфейс".into(),
                server: "demo".into(),
                ..Default::default()
            },
            Agent {
                id: "review-worker".into(),
                name: "Проверка".into(),
                server: "stage".into(),
                ..Default::default()
            },
        ];
        let mut r = Run::new(
            c.clone(),
            "Обновить приложение и проверить результат".into(),
        );
        r.dispatch(
            "one",
            &json!({"worker":"ui-worker","title":"Новый интерфейс","task":"Обновить интерфейс"}),
        )
        .unwrap();
        r.dispatch("two",&json!({"worker":"review-worker","title":"Проверить совместимость","task":"Проверить старые настройки"})).unwrap();
        r.tasks[0].state = TaskState::Running;
        r.tasks[1].state = TaskState::Done;
        r.tasks[1].result = "Старые профили загружаются. Проверки пройдены.".into();
        r.tasks[1].delivered = true;
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
                egui::ComboBox::from_id_salt(("agent-model", &a.id))
                    .icon(design::combo_icon)
                    .selected_text(format!("Модель: {}", a.model))
                    .show_ui(ui, |ui| {
                        for (id, name) in [
                            ("default", "Модель Claude по умолчанию"),
                            ("inherit", "Модель профиля"),
                            ("opus", "Opus"),
                            ("sonnet", "Sonnet"),
                            ("haiku", "Haiku"),
                        ] {
                            ui.selectable_value(&mut a.model, id.into(), name);
                        }
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
fn node_status(app: &RelayApp, run: Option<&Run>, agent: &Agent) -> String {
    if let Some(t) = run.and_then(|r| r.tasks.iter().rev().find(|t| t.agent.id == agent.id)) {
        return t.state.label().into();
    }
    if app.connected(&agent.server) {
        "Готов".into()
    } else {
        "Нет подключения".into()
    }
}
fn state_color(state: TaskState, p: Palette) -> Color32 {
    match state {
        TaskState::Done => GREEN,
        TaskState::Running | TaskState::Waiting => accent(),
        TaskState::Failed | TaskState::Interrupted => Color32::from_rgb(222, 143, 151),
        _ => p.muted,
    }
}
fn draw_graph(
    ui: &mut egui::Ui,
    c: &Config,
    _run: Option<&Run>,
    statuses: &[String],
    leader_status: &str,
    p: Palette,
) -> Option<String> {
    let workers: Vec<_> = c.workers.iter().filter(|w| w.enabled).collect();
    let width = ui.available_width();
    let narrow = width < 580.;
    let height = if narrow {
        140. + workers.len() as f32 * 82.
    } else {
        (workers.len() as f32 * 85. + 20.).max(230.)
    };
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    ui.painter().rect_filled(area, 14., p.side);
    ui.painter().rect_stroke(
        area,
        14.,
        Stroke::new(1.0_f32, p.line),
        egui::StrokeKind::Inside,
    );
    let w = if narrow {
        (width - 44.).min(280.)
    } else {
        (width * 0.36).clamp(160., 255.)
    };
    let leader = egui::Rect::from_center_size(
        egui::pos2(
            if narrow {
                area.center().x
            } else {
                area.left() + w / 2. + 22.
            },
            if narrow {
                area.top() + 58.
            } else {
                area.center().y
            },
        ),
        Vec2::new(w, 86.),
    );
    let mut clicked = None;
    for (i, a) in workers.iter().enumerate() {
        let node = egui::Rect::from_center_size(
            egui::pos2(
                if narrow {
                    area.center().x
                } else {
                    area.right() - w / 2. - 22.
                },
                if narrow {
                    area.top() + 158. + i as f32 * 82.
                } else {
                    area.top() + 55. + i as f32 * 85.
                },
            ),
            Vec2::new(w, 74.),
        );
        let start = if narrow {
            leader.center_bottom()
        } else {
            leader.right_center()
        };
        let end = if narrow {
            node.center_top()
        } else {
            node.left_center()
        };
        let d = if narrow {
            Vec2::new(0., (end.y - start.y) * 0.5)
        } else {
            Vec2::new((end.x - start.x) * 0.5, 0.)
        };
        let active = statuses.get(i).is_some_and(|s| s == "В работе");
        ui.painter()
            .add(egui::epaint::CubicBezierShape::from_points_stroke(
                [start, start + d, end - d, end],
                false,
                Color32::TRANSPARENT,
                Stroke::new(
                    if active { 2.0_f32 } else { 1.0_f32 },
                    if active { accent() } else { p.line },
                ),
            ));
        if active {
            let t = (ui.input(|i| i.time) as f32 * 0.45) % 1.;
            let q = 1. - t;
            let pos = start.to_vec2() * (q * q * q)
                + (start + d).to_vec2() * (3. * q * q * t)
                + (end - d).to_vec2() * (3. * q * t * t)
                + end.to_vec2() * (t * t * t);
            ui.painter().circle_filled(pos.to_pos2(), 3., accent());
        }

        if graph_node(
            ui,
            node,
            &a.name,
            statuses.get(i).map(String::as_str).unwrap_or("Готов"),
            p,
            false,
            a.id.as_str(),
        )
        .clicked()
        {
            clicked = Some(a.id.clone());
        }
    }
    if graph_node(ui, leader, &c.leader.name, leader_status, p, true, "leader").clicked() {
        clicked = Some("leader".into());
    }
    clicked
}
fn graph_node(
    ui: &mut egui::Ui,
    r: egui::Rect,
    name: &str,
    status: &str,
    p: Palette,
    leader: bool,
    id: &str,
) -> egui::Response {
    let response = ui.interact(r, ui.id().with(("agent-graph", id)), egui::Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        design::duration(ui.ctx(), 0.18),
    );
    ui.painter()
        .rect_filled(r, 12., design::mix(p.surface, p.line, hover * 0.5));
    ui.painter().rect_stroke(
        r,
        12.,
        Stroke::new(
            if leader { 1.5_f32 } else { 1.0_f32 },
            if leader { accent() } else { p.line },
        ),
        egui::StrokeKind::Inside,
    );
    let painter = ui.painter().with_clip_rect(r.shrink(10.));
    painter.text(
        r.left_top() + Vec2::new(12., 12.),
        egui::Align2::LEFT_TOP,
        if leader {
            "РУКОВОДИТЕЛЬ"
        } else {
            "ИСПОЛНИТЕЛЬ"
        },
        FontId::proportional(9.5),
        if leader { accent() } else { p.muted },
    );
    painter.text(
        r.left_top() + Vec2::new(12., 29.),
        egui::Align2::LEFT_TOP,
        name,
        FontId::proportional(14.),
        p.text,
    );
    painter.text(
        r.left_bottom() + Vec2::new(12., -11.),
        egui::Align2::LEFT_BOTTOM,
        status,
        FontId::proportional(10.5),
        p.muted,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
