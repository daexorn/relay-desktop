use super::*;
use crate::model::Appearance;

impl RelayApp {
    pub(super) fn seed_workspace_demo(&mut self, ctx: &egui::Context, args: &[String]) {
        if let Some(i) = args.iter().position(|a| a == "--theme") {
            if let Some(name) = args.get(i + 1) {
                self.settings.appearance.theme = name.clone();
                self.settings.light = design::is_light(name);
            }
        }
        self.settings_open = args.iter().any(|a| a == "--demo-settings");
        self.cluster_open = args.iter().any(|a| a == "--demo-cluster");
        if args.iter().any(|a| a == "--demo-delete") {
            if let Some(session) = self
                .remote
                .get("demo")
                .and_then(|r| r.sessions.first())
                .cloned()
            {
                self.request_delete("demo", session);
            }
        }
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
        if args.iter().any(|a| a == "--demo-upload") {
            if let Some(k) = self.current_key() {
                let chat = self.chats.get_mut(&k).unwrap();
                chat.busy = true;
                chat.status = "Отправка файлов".into();
                chat.upload_progress = Some((15 * 1024 * 1024, 64 * 1024 * 1024));
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
                        ui.painter().rect_filled(egui::Rect::from_min_size(r.min+Vec2::new(31.,38.),Vec2::new(30.,6.)),3.,design::theme_accent(id));
                        ui.painter().text(r.center_bottom()-Vec2::new(0.,15.),egui::Align2::CENTER_CENTER,name,FontId::proportional(12.),colors.text);
                        ui.painter().rect_stroke(r,10.,Stroke::new(if selected{2_f32}else{1_f32},if selected{accent()}else{p.line}),egui::StrokeKind::Inside);
                        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked(){self.settings.appearance.theme=id.into();self.settings.light=design::is_light(id);}
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
}
