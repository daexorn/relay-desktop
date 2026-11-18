use super::*;

pub(super) struct Editor {
    server: String,
    name: String,
    profile: crate::model::ApiProfile,
    key: String,
    error: String,
    status: String,
    request: String,
}
impl Drop for Editor {
    fn drop(&mut self) { self.key.zeroize(); }
}

impl RelayApp {
    pub(super) fn backend_for_chat(&self, server: &Server, chat: &str) -> String {
        let remote_backend = self
            .remote
            .get(&server.id)
            .and_then(|r| r.sessions.iter().find(|s| key(&server.id, &s.id) == chat))
            .map(|s| s.backend.as_str())
            .filter(|b| !b.is_empty());
        self.settings.session_backend(server, chat, remote_backend)
    }
    #[cfg(test)]
    pub(super) fn api_for_chat(&self, server: &Server, chat: &str) -> bool {
        self.backend_for_chat(server, chat) == "api"
    }
    pub(super) fn models_for_backend(
        &self,
        server: &Server,
        backend: &str,
    ) -> Vec<crate::model::ModelInfo> {
        match backend {
            "api" => self.api_model_catalog(server),
            "codex" => {
                let remote = self
                    .remote
                    .get(&server.id)
                    .map(|r| r.codex_models.clone())
                    .unwrap_or_default();
                if remote.is_empty() {
                    crate::model::default_backend_models("codex")
                } else {
                    remote
                }
            }
            "agy" => {
                let remote = self
                    .remote
                    .get(&server.id)
                    .map(|r| r.agy_models.clone())
                    .unwrap_or_default();
                if remote.is_empty() {
                    crate::model::default_backend_models("agy")
                } else {
                    remote
                }
            }
            _ => self
                .remote
                .get(&server.id)
                .map(|r| r.models.clone())
                .unwrap_or_default(),
        }
    }
    pub(super) fn open_api_settings(&mut self, server: &Server) {
        self.api_editor = Some(Editor {
            server: server.id.clone(), name: server.name.clone(), profile: server.api.clone(),
            key: storage::api_key(&server.id).unwrap_or_default(), error: String::new(), status: String::new(), request: String::new(),
        });
    }
    pub(super) fn api_models_reply(&mut self, server: &str, value: &Value) {
        let models: Vec<crate::model::ModelInfo> = serde_json::from_value(value["models"].clone()).unwrap_or_default();
        if let Some(editor) = &mut self.api_editor {
            if editor.server == server && value["request"].as_str() == Some(editor.request.as_str()) {
                editor.error = value["error"].as_str().unwrap_or("").into();
                editor.status = if editor.error.is_empty() { format!("Подключено · моделей: {}", models.len()) } else { String::new() };
                if editor.error.is_empty() { editor.profile.models = models.iter().map(|m| m.value.clone()).collect(); }
                editor.request.clear();
            }
        }
        if let Some(profile) = self.settings.servers.iter_mut().find(|s| s.id == server) {
            if value["base_url"].as_str().is_some_and(|url| url == profile.api.base_url.trim_end_matches('/')) {
                profile.api.models = models.iter().map(|m| m.value.clone()).collect();
                self.remote.entry(server.into()).or_default().api_models = models;
                self.save();
            }
        }
    }
    pub(super) fn api_model_catalog(&self, server: &Server) -> Vec<crate::model::ModelInfo> {
        let remote = self.remote.get(&server.id).map(|r| r.api_models.clone()).unwrap_or_default();
        if !remote.is_empty() { return remote; }
        serde_json::from_value(json!(server.api.models.iter().map(|id| json!({"value":id,"displayName":id,"description":"API"})).collect::<Vec<_>>())).unwrap_or_default()
    }
    pub(super) fn apply_api_request(&self, server: &Server, request: &mut Value) {
        if !server.api.enabled { return; }
        let mut profile = server.api.clone();
        if let Some(model) = request["model"].as_str().filter(|m| !m.is_empty() && *m != "default") {
            if ["opus", "sonnet", "haiku", "fable"].contains(&model) {
                let prefix = format!("claude-{model}-");
                if let Some(id) = profile.models.iter().filter(|m| m.starts_with(&prefix)).max() { profile.model = id.clone(); }
            } else { profile.model = model.to_owned(); }
        }
        request["model"] = json!(profile.model);
        request["backend"] = json!("api");
        request["api"] = profile.request(storage::api_key(&server.id).unwrap_or_default());
    }
    pub(super) fn api_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.api_editor.take() else { return; };
        let p = design::animated_palette(ctx, self.settings.light);
        let mut save = false;let mut cancel = false;let mut test = false;
        let response = egui::Modal::new(egui::Id::new("api-settings"))
            .frame(egui::Frame::new().fill(p.side).corner_radius(16.).inner_margin(24.).stroke(Stroke::new(1_f32, p.line)))
            .show(ctx, |ui| {
                ui.set_width((ctx.screen_rect().width() - 64.).clamp(260., 500.));
                ui.spacing_mut().interact_size.y = 32.;
                label(ui, format!("API · {}", editor.name), 20., p.text);
                label(ui, "Продолжай те же сессии через свой API", 12., p.muted);
                ui.add_space(14.);
                egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 240.).max(220.)).show(ui, |ui| {
                    ui.checkbox(&mut editor.profile.enabled, "API по умолчанию для сессий этого сервера");
                    ui.add_space(10.);
                    label(ui, "Адрес API", 12., p.muted);
                    design::field(ui,&mut editor.profile.base_url,"https://tooken.club/v1",false);
                    ui.add_space(8.);
                    label(ui, "API-ключ", 12., p.muted);
                    design::field(ui,&mut editor.key,"Ключ этого API",true);
                    ui.add_space(8.);
                    label(ui, "Модель", 12., p.muted);
                    if !editor.profile.models.is_empty() {
                        egui::ComboBox::from_id_salt("api-config-model").width(ui.available_width() - 8.)
                            .selected_text(if editor.profile.model.is_empty() { "Выбрать модель".to_owned() } else { super::model_label(&[], &editor.profile.model) })
                            .icon(design::combo_icon).show_ui(ui, |ui| {
                                for id in &editor.profile.models {
                                    if id.contains("image") { continue; }
                                    ui.selectable_value(&mut editor.profile.model, id.clone(), super::model_label(&[],id)).on_hover_text(id);
                                }
                            });
                    }
                    if editor.profile.models.is_empty() {
                        design::field(ui,&mut editor.profile.model,"Идентификатор модели",false);
                    } else {
                        egui::CollapsingHeader::new("Свой идентификатор модели").show(ui, |ui| {
                            design::field(ui,&mut editor.profile.model,"Модель",false);
                        });
                    }
                    ui.add_space(10.);
                    ui.horizontal_wrapped(|ui| {
                        label(ui, "Токенов на ответ", 12., p.muted);
                        ui.add(egui::DragValue::new(&mut editor.profile.max_tokens).range(256..=65536).speed(256.));
                        label(ui, "Шагов", 12., p.muted);
                        ui.add(egui::DragValue::new(&mut editor.profile.max_steps).range(1..=256));
                    });
                    ui.add_space(10.);
                    if design::button(ui, if editor.request.is_empty() { "Проверить и загрузить модели" } else { "Проверяю…" }, None, Kind::Secondary, p, Vec2::new(ui.available_width(), 32.)).clicked() && editor.request.is_empty() { test = true; }
                    if !editor.error.is_empty() { label(ui, &editor.error, 12., accent()); }
                    if !editor.status.is_empty() { label(ui, &editor.status, 12., GREEN); }
                });
                ui.add_space(16.);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    save = design::button(ui, "Сохранить", None, Kind::Primary, p, Vec2::new(124., 34.)).clicked();
                    cancel = design::button(ui, "Отмена", None, Kind::Ghost, p, Vec2::new(96., 34.)).clicked();
                });
            });
        if response.should_close() || cancel { return; }
        if test {
            if !self.connected(&editor.server) { editor.error = "Подключись к серверу, чтобы проверить API с него.".into(); }
            else {
                editor.request = uuid::Uuid::new_v4().to_string();editor.error.clear();editor.status.clear();
                if !self.controller.send(&editor.server, json!({"op":"api_models","request":editor.request,"api":editor.profile.request(editor.key.clone())})) { editor.request.clear();editor.error = "Соединение с сервером потеряно.".into(); }
            }
        }
        if save {
            if self.settings.servers.iter().any(|s| s.id == editor.server && s.api.enabled != editor.profile.enabled)
                && self.remote.get(&editor.server).is_some_and(|r| !r.running.is_empty()) {
                editor.error = "Дождись завершения работы перед сменой источника модели.".into();self.api_editor = Some(editor);return;
            }
            match editor.profile.validate().and_then(|_| storage::remember_api_key(&editor.server, &editor.key)) {
                Err(e) => editor.error = e.to_string(),
                Ok(()) => {
                    if let Some(server) = self.settings.servers.iter_mut().find(|s| s.id == editor.server) {
                        server.api = editor.profile.clone();
                        if server.api.enabled {
                            server.default_backend = "api".into();
                        } else if server.default_backend == "api" {
                            server.default_backend = "claude".into();
                        }
                    }
                    self.save();self.notice = "Настройки API сохранены".into();return;
                }
            }
        }
        self.api_editor = Some(editor);
    }
}

#[cfg(test)]
pub(super) fn source_picker(ui: &mut egui::Ui, api: &mut bool, configure: &mut bool, busy: bool, p: Palette) {
    let mut backend = if *api { "api".to_owned() } else { "claude".to_owned() };
    let mut login = None;
    let mut open_agents = false;
    backend_picker(ui, &mut backend, configure, &mut login, &mut open_agents, [true, true, true], busy, p);
    *api = backend == "api";
}

#[allow(clippy::too_many_arguments)]
pub(super) fn backend_picker(
    ui: &mut egui::Ui,
    backend: &mut String,
    configure: &mut bool,
    login_provider: &mut Option<String>,
    open_agents: &mut bool,
    installed: [bool; 3],
    busy: bool,
    p: Palette,
) {
    let previous_style = ui.style().clone();
    ui.spacing_mut().button_padding = Vec2::new(8., 5.);
    ui.spacing_mut().interact_size.y = 28.;
    let short = crate::model::backend_short_label(backend);
    design::wrap_control(ui, short);
    egui::ComboBox::from_id_salt("source-selector")
        .width(0.)
        .wrap_mode(egui::TextWrapMode::Truncate)
        .selected_text(
            RichText::new(short)
                .size(12.5)
                .color(if backend == "claude" { p.muted } else { accent() }),
        )
        .icon(design::combo_icon)
        .show_ui(ui, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                for ((id, title), ok) in [
                    (("claude", "Claude Code"), installed[0]),
                    (("codex", "Codex"), installed[1]),
                    (("agy", "Antigravity (agy)"), installed[2]),
                ] {
                    let label = if ok {
                        title.to_owned()
                    } else {
                        format!("{title} · не установлен")
                    };
                    ui.selectable_value(backend, id.into(), label);
                }
                ui.selectable_value(backend, "api".into(), "Пользовательский API");
            });
            ui.separator();
            if ui.button("Установить / обновить CLI-агентов…").clicked() {
                *open_agents = true;
                ui.close_menu();
            }
            if ui.button("Настроить API…").clicked() {
                *configure = true;
                ui.close_menu();
            }
            for (prov, title) in [
                ("claude", "Войти в Claude…"),
                ("codex", "Войти в Codex…"),
                ("agy", "Войти в AGY…"),
            ] {
                if ui.button(title).clicked() {
                    *login_provider = Some(prov.into());
                    ui.close_menu();
                }
            }
        });
    ui.set_style(previous_style);
}
