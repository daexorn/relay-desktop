use super::*;
use crate::model::{AgentNode, Appearance};

impl RelayApp {
    pub(super) fn seed_workspace_demo(&mut self, ctx: &egui::Context, args: &[String]) {
        if let Some(i) = args.iter().position(|a| a == "--theme") {
            if let Some(name) = args.get(i + 1) {
                self.settings.appearance.theme = name.clone();
                self.settings.light = name == "paper";
            }
        }
        self.settings_open = args.iter().any(|a| a == "--demo-settings");
        self.cluster_open = args.iter().any(|a| a == "--demo-cluster");
        if args.iter().any(|a| a == "--demo-attachments") {
            let image = egui::ColorImage::new([120, 72], egui::Color32::from_rgb(45, 65, 95));
            let texture =
                ctx.load_texture("attachment-fixture", image, egui::TextureOptions::LINEAR);
            let files = (0..2)
                .map(|i| Attachment {
                    name: format!("Из буфера — длинное имя файла {i}.png"),
                    texture: Some(texture.clone()),
                    bytes: 32000,
                    source: Value::Null,
                })
                .collect::<Vec<_>>();
            if let Some(k) = self.current_key() {
                let c = self.chats.get_mut(&k).unwrap();
                c.messages=vec![Message{role:"user".into(),text:"Проверь, пожалуйста, эти два изображения. Имена файлов должны читаться, а карточки — оставаться компактными.".into(),..Default::default()}];
                c.sent_images.insert(0, files.clone());
                c.images = files;
                c.draft = "Добавь ещё один вариант оформления".into();
                c.busy = true;
                c.queue.push_back(QueuedPrompt {
                    text: "Сохрани текущие настройки и проверь Linux-сборку".into(),
                    files: vec![],
                });
            }
        }
        if args.iter().any(|a| a == "--demo-send") {
            if let Some(k) = self.current_key() {
                let c = self.chats.get_mut(&k).unwrap();
                c.messages=vec![
                Message{role:"user".into(),text:"привет".into(),..Default::default()},
                Message{role:"assistant".into(),text:"Привет! Чем могу помочь?".into(),..Default::default()},
                Message{role:"user".into(),text:"привет".into(),..Default::default()},
                Message{role:"assistant".into(),text:"Сообщения выровнены по правому краю.".into(),..Default::default()},
                Message{role:"user".into(),text:"А это длинное сообщение, чтобы проверить выравнивание при переносе текста на несколько строк.".into(),..Default::default()}];
            }
        }
        if self.cluster_open {
            self.cluster_target = "demo".into();
            for (host, sid, title, busy) in [
                (
                    "demo",
                    "cluster-a",
                    "Обновить интерфейс и собрать приложение",
                    true,
                ),
                (
                    "stage",
                    "cluster-b",
                    "Проверить совместимость формата сессий",
                    false,
                ),
            ] {
                let mut c = Chat::new(sid.into(), "/opt/project".into(), title.into());
                c.busy = busy;
                c.loaded = true;
                c.messages.push(Message {
                    role: "assistant".into(),
                    text: if busy {
                        "Изучаю существующие экраны и настройки."
                    } else {
                        "Проверки завершены. Старые профили читаются корректно."
                    }
                    .into(),
                    ..Default::default()
                });
                self.chats.insert(key(host, sid), c);
                self.cluster_jobs
                    .push((host.into(), sid.into(), title.into()));
            }
        }
    }
    pub(super) fn settings_panel(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let p = design::animated_palette(ctx, self.settings.light);
        let before = serde_json::to_string(&self.settings.appearance).unwrap_or_default();
        let mut open = true;
        egui::Window::new("Настройки Relay").id(egui::Id::new("preferences"))
            .open(&mut open).default_width(610.).default_height(640.).default_pos(egui::pos2(((ctx.screen_rect().width()-650.)/2.).max(12.),((ctx.screen_rect().height()-700.)/2.).max(12.))).resizable(true).collapsible(false).show(ctx,|ui| {
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height()-150.).max(240.)).show(ui,|ui| {
                label(ui,"Твоя рабочая среда",22.,p.text);
                label(ui,"Оформление и поведение приложения",12.,p.muted);
                ui.add_space(18.);
                label(ui,"Цветовая тема",13.,p.text);
                ui.horizontal_wrapped(|ui| {
                    for (id,name) in design::THEMES {
                        let selected=self.settings.theme()==id;
                        let colors=design::theme_palette(id);
                        let (r,response)=ui.allocate_exact_size(Vec2::new(104.,75.),egui::Sense::click());
                        ui.painter().rect_filled(r,10.,colors.base);
                        ui.painter().rect_filled(egui::Rect::from_min_size(r.min+Vec2::new(7.,7.),Vec2::new(18.,34.)),5.,colors.rail);
                        ui.painter().rect_filled(egui::Rect::from_min_size(r.min+Vec2::new(31.,7.),Vec2::new(65.,18.)),4.,colors.surface);
                        ui.painter().line_segment([r.min+Vec2::new(32.,32.),r.min+Vec2::new(82.,32.)],Stroke::new(2_f32,colors.muted));
                        ui.painter().text(r.center_bottom()-Vec2::new(0.,15.),egui::Align2::CENTER_CENTER,name,FontId::proportional(12.),colors.text);
                        ui.painter().rect_stroke(r,10.,Stroke::new(if selected{2_f32}else{1_f32},if selected{ACCENT}else{p.line}),egui::StrokeKind::Inside);
                        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked(){self.settings.appearance.theme=id.into();self.settings.light=id=="paper";}
                    }
                });
                ui.add_space(16.);
                let a=&mut self.settings.appearance;
                ui.spacing_mut().slider_width=170.;
                ui.add(egui::Slider::new(&mut a.scale,0.8..=1.35).trailing_fill(true).text("Масштаб интерфейса").suffix("×").fixed_decimals(2));
                ui.add(egui::Slider::new(&mut a.chat_width,560. ..=1160.).trailing_fill(true).text("Ширина разговора").suffix(" px").step_by(20.));
                ui.checkbox(&mut a.compact,"Компактные отступы");
                ui.checkbox(&mut a.animations,"Плавные переходы панелей и тем");
                ui.add_space(16.);ui.separator();ui.add_space(8.);
                label(ui,"Общение и команды",15.,p.text);
                ui.checkbox(&mut a.send_on_enter,"Enter отправляет, Shift+Enter добавляет строку");
                if !a.send_on_enter {label(ui,"Отправка по Ctrl+Enter",12.,p.muted);}
                ui.checkbox(&mut a.wrap_output,"Переносить длинные строки в выводе команд");
                ui.checkbox(&mut a.open_errors,"Раскрывать действия с ошибками");
                label(ui,"Во время ответа новые сообщения добавляются в очередь. Их можно изменить, удалить или поставить на паузу.",12.,p.muted);
                ui.add_space(16.);ui.separator();ui.add_space(8.);
                ui.horizontal(|ui| {
                    label(ui,format!("Relay {}",env!("CARGO_PKG_VERSION")),12.,p.muted);
                    if design::button(ui,"Сбросить оформление",None,Kind::Secondary,p,Vec2::ZERO).clicked(){self.settings.appearance=Appearance::default();self.settings.light=false;}
                });
            });
        });
        self.settings_open = open;
        if serde_json::to_string(&self.settings.appearance).unwrap_or_default() != before {
            self.save();
        }
    }

    pub(super) fn drain_queues(&mut self) {
        if self.demo {
            return;
        }
        let ready: Vec<_> = self
            .chats
            .iter()
            .filter(|(_, c)| c.queued_ready())
            .filter_map(|(k, _)| {
                self.settings
                    .servers
                    .iter()
                    .find(|s| k.starts_with(&format!("{}/", s.id)) && self.connected(&s.id))
                    .cloned()
                    .map(|s| (k.clone(), s))
            })
            .collect();
        for (k, server) in ready {
            let c = self.chats.get_mut(&k).unwrap();
            let Some(job) = c.queue.pop_front() else {
                continue;
            };
            let draft = std::mem::replace(&mut c.draft, job.text);
            let files = std::mem::replace(&mut c.images, job.files);
            self.send_to_chat(server, k.clone());
            let c = self.chats.get_mut(&k).unwrap();
            // Keep both the unsent queued message and any newer draft if the transport failed.
            if !c.draft.is_empty() || !c.images.is_empty() {
                c.queue.push_front(QueuedPrompt {
                    text: std::mem::take(&mut c.draft),
                    files: std::mem::take(&mut c.images),
                });
                c.queue_paused = true;
            }
            c.draft = draft;
            c.images = files;
        }
    }

    pub(super) fn cluster_panel(&mut self, ctx: &egui::Context) {
        let p = design::animated_palette(ctx, self.settings.light);
        for s in &self.settings.servers {
            if !self.settings.cluster.iter().any(|n| n.server == s.id) {
                self.settings.cluster.push(AgentNode {
                    server: s.id.clone(),
                    role: "worker".into(),
                    enabled: true,
                });
            }
        }
        let mut connect = None;
        let mut start = false;
        let mut open_chat = None;
        let before = serde_json::to_string(&self.settings.cluster).unwrap_or_default();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.base).inner_margin(26)).show(ctx,|ui| {
            ui.horizontal(|ui| {label(ui,"Кластер",27.,p.text);label(ui,"Несколько хостов · независимые сессии",12.,p.muted);});
            let online=self.settings.cluster.iter().filter(|n|n.enabled && self.connected(&n.server)).count();
            let working=self.chats.values().filter(|c|c.busy).count();
            ui.horizontal(|ui| {design::chip(ui,&format!("В сети: {online}"),Icon::Server,p,online>0);design::chip(ui,&format!("В работе: {working}"),Icon::Cluster,p,working>0);});
            ui.add_space(18.);
            egui::ScrollArea::vertical().id_salt("cluster-scroll").show(ui,|ui| {
                label(ui,"Хосты и обязанности",16.,p.text);
                for server in self.settings.servers.clone() {
                    let connected=self.connected(&server.id);
                    let connecting=self.remote.get(&server.id).is_some_and(|r|r.connecting);
                    let active=self.chats.iter().filter(|(k,c)|k.starts_with(&format!("{}/",server.id))&&c.busy).count();
                    let node=self.settings.cluster.iter_mut().find(|n|n.server==server.id).unwrap();
                    egui::Frame::new().fill(p.surface).stroke(Stroke::new(1_f32,p.line)).corner_radius(12.).inner_margin(14).show(ui,|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut node.enabled,"");
                            label(ui,&server.name,15.,p.text);label(ui,&server.host,12.,p.muted);
                            egui::ComboBox::from_id_salt(("agent-role",&server.id)).icon(design::combo_icon)
                                .selected_text(role_name(&node.role)).show_ui(ui,|ui| {
                                for (id,name) in [("worker","Разработка"),("planner","Планирование"),("reviewer","Проверка")] {ui.selectable_value(&mut node.role,id.into(),name);}
                            });
                            label(ui,if connected{if active>0{"Работает"}else{"Готов"}}else if connecting{"Подключение…"}else{"Не подключён"},12.,if connected{GREEN}else{p.muted});
                            if !connected && !connecting && design::button(ui,"Подключить",None,Kind::Secondary,p,Vec2::ZERO).clicked(){connect=Some(server.clone());}
                        });
                        label(ui,format!("{} · {} · {active} активных",server.cwd,if server.model=="default"{"модель сервера"}else{&server.model}),11.,p.muted);
                    });ui.add_space(6.);
                }
                if self.settings.servers.is_empty(){label(ui,"Добавь сервер кнопкой + слева.",13.,p.muted);}
                ui.add_space(16.);label(ui,"Новая задача",16.,p.text);
                egui::Frame::new().fill(p.surface).corner_radius(14.).inner_margin(14).show(ui,|ui| {
                    egui::ComboBox::from_id_salt("cluster-target").icon(design::combo_icon)
                        .selected_text(self.settings.servers.iter().find(|s|s.id==self.cluster_target).map(|s|s.name.as_str()).unwrap_or("Выбрать хост"))
                        .show_ui(ui,|ui| {for s in &self.settings.servers {if self.settings.cluster.iter().any(|n|n.server==s.id&&n.enabled){ui.selectable_value(&mut self.cluster_target,s.id.clone(),&s.name);}}});
                    ui.add(egui::TextEdit::multiline(&mut self.cluster_prompt).frame(false).desired_width(f32::INFINITY).desired_rows(3).hint_text("Задача для агента на выбранном хосте…"));
                    let ready=!self.cluster_prompt.trim().is_empty() && self.connected(&self.cluster_target) && self.settings.cluster.iter().any(|n|n.server==self.cluster_target&&n.enabled);
                    ui.add_enabled_ui(ready,|ui| {if design::button(ui,"Запустить задачу",Some(Icon::Send),Kind::Primary,p,Vec2::ZERO).clicked(){start=true;}});
                    label(ui,"Каждая задача открывает отдельную сессию Claude Code. Можно запускать задачи параллельно на разных хостах.",11.,p.muted);
                });
                ui.add_space(20.);label(ui,"Задачи кластера",16.,p.text);
                for (server_id,sid,title) in self.cluster_jobs.iter().rev() {
                    let Some(c)=self.chats.get(&key(server_id,sid)) else{continue;};
                    let server=self.settings.servers.iter().find(|s|&s.id==server_id);
                    egui::Frame::new().fill(p.surface).corner_radius(12.).inner_margin(14).show(ui,|ui| {
                        ui.horizontal_wrapped(|ui| {
                            label(ui,if !c.error.is_empty(){"Ошибка"}else if !c.permissions.is_empty(){"Ждёт ответа"}else if c.busy{"В работе"}else{"Завершена"},12.,if c.busy{ACCENT}else{p.muted});
                            label(ui,server.map(|s|s.name.as_str()).unwrap_or("Хост"),12.,p.muted);
                            if design::button(ui,"Открыть сессию",Some(Icon::Arrow),Kind::Ghost,p,Vec2::ZERO).clicked(){open_chat=Some((server_id.clone(),sid.clone()));}
                        });
                        ui.add(egui::Label::new(RichText::new(model::shorten(title,180)).size(14.)).wrap());
                        if let Some(m)=c.messages.iter().rev().find(|m|m.role=="assistant"&&!m.text.is_empty()) {label(ui,model::shorten(&m.text,240),12.,p.muted);}
                    });ui.add_space(8.);
                }
            });
        });
        if before != serde_json::to_string(&self.settings.cluster).unwrap_or_default() {
            self.save();
        }
        if let Some(s) = connect {
            self.connect(s);
        }
        if start {
            if let Some(server) = self
                .settings
                .servers
                .iter()
                .find(|s| s.id == self.cluster_target)
                .cloned()
            {
                let sid = uuid::Uuid::new_v4().to_string();
                let text = std::mem::take(&mut self.cluster_prompt);
                let role = self
                    .settings
                    .cluster
                    .iter()
                    .find(|n| n.server == server.id)
                    .map(|n| n.role.as_str())
                    .unwrap_or("worker");
                let mut chat =
                    Chat::new(sid.clone(), server.cwd.clone(), model::shorten(&text, 60));
                chat.model = server.model.clone();
                chat.mode = if role == "planner" {
                    "plan".into()
                } else {
                    server.mode.clone()
                };
                chat.effort = server.effort.clone();
                chat.loaded = true;
                chat.agent_role = role.to_owned();
                chat.draft = text.clone();
                let k = key(&server.id, &sid);
                self.cluster_jobs
                    .push((server.id.clone(), sid.clone(), text));
                self.chats.insert(k.clone(), chat);
                self.send_to_chat(server, k);
            }
        }
        if let Some((server, sid)) = open_chat {
            self.settings.selected = Some(server.clone());
            self.selected_chat.insert(server, sid);
            self.cluster_open = false;
            self.save();
        }
    }
}
fn role_name(role: &str) -> &str {
    match role {
        "planner" => "Планирование",
        "reviewer" => "Проверка",
        _ => "Разработка",
    }
}
