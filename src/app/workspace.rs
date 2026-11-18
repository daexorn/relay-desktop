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
        if args.iter().any(|a| a == "--demo-empty") {
            if let Some(k) = self.current_key() {
                let chat = self.chats.get_mut(&k).unwrap();
                chat.messages.clear();
                chat.busy = false;
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
        let mut close = false;
        let width = (ctx.screen_rect().width() - 64.).clamp(320., 660.);
        egui::Window::new("Настройки Relay")
            .id(egui::Id::new("preferences"))
            .open(&mut open)
            .title_bar(false)
            .default_width(width)
            .max_width(width)
            .default_height((ctx.screen_rect().height() - 80.).min(800.))
            .min_height(420.)
            .max_height(ctx.screen_rect().height() - 32.)
            .resizable(true)
            .collapsible(false)
            .frame(
                egui::Frame::new()
                    .fill(p.side)
                    .corner_radius(18.)
                    .stroke(Stroke::new(1.0_f32, p.line))
                    .inner_margin(22),
            )
            .default_pos(egui::pos2(
                ((ctx.screen_rect().width() - width) / 2.).max(16.),
                32.,
            ))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        label(ui, "Настройки", 23., p.text);
                        label(ui, "Рабочее пространство под тебя", 12., p.muted);
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if design::button(
                            ui,
                            "",
                            Some(Icon::Close),
                            Kind::Ghost,
                            p,
                            Vec2::splat(30.),
                        )
                        .on_hover_text("Закрыть настройки")
                        .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(18.);
                egui::ScrollArea::vertical()
                    .max_height((ctx.screen_rect().height() - 200.).max(220.))
                    .show(ui, |ui| {
                        label(ui, "Оформление · 24 темы", 14., p.text);
                        ui.add_space(8.);
                        let columns = if ui.available_width() < 480. { 3. } else { 4. };
                        let tile_width =
                            ((ui.available_width() - 8. * (columns - 1.)) / columns).max(70.);
                        for (group_light, heading) in [(false, "Тёмные палитры"), (true, "Светлые палитры")] {
                            label(ui, heading, 11.5, p.muted);
                            ui.add_space(6.);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::splat(8.);
                                for (id, name) in design::THEMES {
                                    if design::is_light(id) != group_light {
                                        continue;
                                    }
                                    let colors = design::theme_palette(id);
                                    let selected = self.settings.theme() == id;
                                    let (r, response) = ui.allocate_exact_size(
                                        Vec2::new(tile_width, 82.),
                                        egui::Sense::click(),
                                    );
                                    let hovered = response.hovered();
                                    ui.painter().rect_filled(r, 10., colors.base);
                                    let inner = r.shrink2(Vec2::new(9., 10.));
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_size(inner.min, Vec2::new(6., 34.)),
                                        3.,
                                        colors.rail,
                                    );
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_size(
                                            inner.min + Vec2::new(8., 0.),
                                            Vec2::new(13., 34.),
                                        ),
                                        4.,
                                        colors.side,
                                    );
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_size(
                                            inner.min + Vec2::new(25., 0.),
                                            Vec2::new((inner.width() - 25.).max(18.), 15.),
                                        ),
                                        4.,
                                        colors.surface,
                                    );
                                    ui.painter().line_segment(
                                        [
                                            inner.min + Vec2::new(26., 24.),
                                            inner.min + Vec2::new((inner.width() - 12.).max(30.), 24.),
                                        ],
                                        Stroke::new(2.0_f32, colors.line),
                                    );
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_size(
                                            inner.min + Vec2::new(25., 31.),
                                            Vec2::new(22., 5.),
                                        ),
                                        2.5,
                                        design::theme_accent(id),
                                    );
                                    ui.painter().text(
                                        egui::pos2(r.left() + 10., r.bottom() - 14.),
                                        egui::Align2::LEFT_CENTER,
                                        name,
                                        FontId::proportional(11.5),
                                        colors.text,
                                    );
                                    if selected {
                                        design::icon(
                                            ui.painter(),
                                            egui::Rect::from_center_size(
                                                r.right_bottom() - Vec2::splat(14.),
                                                Vec2::splat(13.),
                                            ),
                                            Icon::Check,
                                            design::theme_accent(id),
                                        );
                                    }
                                    ui.painter().rect_stroke(
                                        r,
                                        10.,
                                        Stroke::new(
                                            if selected { 1.7_f32 } else { 1.0_f32 },
                                            if selected {
                                                accent()
                                            } else if hovered {
                                                design::mix(p.line, accent(), 0.5)
                                            } else {
                                                p.line
                                            },
                                        ),
                                        egui::StrokeKind::Inside,
                                    );
                                    if response
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        self.settings.appearance.theme = id.into();
                                        self.settings.light = design::is_light(id);
                                    }
                                }
                            });
                            ui.add_space(12.);
                        }
                        ui.add_space(18.);
                        design::card(p).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().slider_width = 160.;
                            let a = &mut self.settings.appearance;
                            label(ui, "Размер и отступы", 14., p.text);
                            ui.add_space(8.);
                            ui.add(
                                egui::Slider::new(&mut a.scale, 0.8..=1.35)
                                    .trailing_fill(true)
                                    .text("Масштаб")
                                    .suffix("×")
                                    .fixed_decimals(2),
                            );
                            ui.add(
                                egui::Slider::new(&mut a.chat_width, 560. ..=1160.)
                                    .trailing_fill(true)
                                    .text("Ширина чата")
                                    .suffix(" px")
                                    .step_by(20.),
                            );
                            design::preference(
                                ui,
                                &mut a.compact,
                                "Компактные отступы",
                                "Больше содержимого на одном экране",
                                p,
                            );
                            design::preference(
                                ui,
                                &mut a.animations,
                                "Плавные переходы",
                                "Мягкое появление панелей и сообщений",
                                p,
                            );
                        });
                        ui.add_space(12.);
                        design::card(p).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let a = &mut self.settings.appearance;
                            label(ui, "Чат и команды", 14., p.text);
                            ui.add_space(4.);
                            design::preference(
                                ui,
                                &mut a.send_on_enter,
                                "Отправлять по Enter",
                                "Shift+Enter добавляет строку; иначе Ctrl+Enter",
                                p,
                            );
                            design::preference(
                                ui,
                                &mut a.wrap_output,
                                "Переносить вывод команд",
                                "Длинные строки остаются в границах панели",
                                p,
                            );
                            design::preference(
                                ui,
                                &mut a.open_errors,
                                "Раскрывать ошибки",
                                "Показывать подробности неудачных действий",
                                p,
                            );
                        });
                        ui.add_space(16.);
                        ui.horizontal(|ui| {
                            label(
                                ui,
                                format!("Relay {}", env!("CARGO_PKG_VERSION")),
                                11.5,
                                p.muted,
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if design::button(
                                    ui,
                                    "Сбросить оформление",
                                    Some(Icon::Refresh),
                                    Kind::Ghost,
                                    p,
                                    Vec2::ZERO,
                                )
                                .clicked()
                                {
                                    self.settings.appearance = Appearance::default();
                                    self.settings.light = false;
                                }
                            });
                        });
                    });
            });
        self.settings_open = open && !close;
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
