use super::*;
use zeroize::Zeroize;

pub(super) struct DeleteChat {
    pub server: String,
    pub session: Session,
    pub pending: bool,
    pub error: String,
}

pub(super) struct LockDialog {
    pub server: String,
    pub sid: String,
    pub title: String,
    pub already_locked: bool,
    pub remove_mode: bool,
    pub current_password: String,
    pub new_password: String,
    pub confirm_password: String,
    pub pending: bool,
    pub error: String,
}

pub(super) struct FolderDialog {
    pub server: String,
    pub name: String,
    pub rename_folder_id: Option<String>,
    /// `(sid, title, current_folder_id)` when moving or creating a folder for a specific session.
    pub move_session: Option<(String, String, String)>,
    pub focused_once: bool,
    pub error: String,
}

impl Drop for LockDialog {
    fn drop(&mut self) {
        self.current_password.zeroize();
        self.new_password.zeroize();
        self.confirm_password.zeroize();
    }
}

impl RelayApp {
    pub(super) fn backend_installed(&self, server: &str, agent: &str) -> bool {
        if self.demo || agent == "api" {
            return true;
        }
        let Some(remote) = self.remote.get(server) else {
            return true;
        };
        if !remote.connected {
            return true;
        }
        if let Some(flag) = remote.backends[agent]["installed"].as_bool() {
            return flag;
        }
        agent != "codex" && agent != "agy"
    }

    pub(super) fn backend_installing(&self, server: &str, agent: &str) -> Option<&str> {
        self.remote
            .get(server)
            .and_then(|r| r.installing_agents.get(agent).map(String::as_str))
    }

    pub(super) fn install_server_agent(&mut self, server: &str, agent: &str, action: &str) {
        if self.demo {
            self.notice = "В режиме демонстрации установка на сервер отключена.".into();
            return;
        }
        if !self.connected(server) {
            self.notice = "Сначала подключись к серверу.".into();
            return;
        }
        if self.controller.send(
            server,
            json!({
                "op": "agent_install",
                "agent": agent,
                "action": action,
            }),
        ) {
            let r = self.remote.entry(server.into()).or_default();
            if agent == "all" {
                for id in ["claude", "codex", "agy"] {
                    if r.backends[id]["installed"] != true {
                        r.installing_agents
                            .insert(id.into(), "Запускаем установку на сервере…".into());
                    }
                }
            } else {
                r.installing_agents
                    .insert(agent.into(), "Запускаем установку на сервере…".into());
            }
        }
    }

    pub(super) fn open_lock_dialog(&mut self, server: &str, sid: &str) {
        if !self.connected(server) && !self.demo {
            self.notice = "Подключись к серверу, чтобы управлять защитой сессии.".into();
            return;
        }
        let (title, already_locked) = self
            .remote
            .get(server)
            .and_then(|r| r.sessions.iter().find(|s| s.id == sid))
            .map(|s| (s.title.clone(), s.locked))
            .or_else(|| {
                self.chats
                    .get(&key(server, sid))
                    .map(|c| (c.title.clone(), c.locked))
            })
            .unwrap_or_else(|| ("Сессия".into(), false));
        self.lock_dialog = Some(LockDialog {
            server: server.into(),
            sid: sid.into(),
            title,
            already_locked,
            remove_mode: false,
            current_password: String::new(),
            new_password: String::new(),
            confirm_password: String::new(),
            pending: false,
            error: String::new(),
        });
    }

    pub(super) fn locked_sidebar_title(&self, server: &str, sid: &str) -> String {
        let modified = self.remote.get(server).and_then(|r| r.sessions.iter().find(|s| s.id == sid))
            .map_or(0., |s| s.modified);
        self.settings.label_locked_session(&key(server, sid), sid, modified)
    }

    pub(super) fn remember_lock_title(&mut self, server: &str, sid: &str) {
        let k = key(server, sid);
        let title = self.remote.get(server).and_then(|r| r.sessions.iter().find(|s| s.id == sid && !s.is_locked_gate()))
            .map(|s| s.title.as_str()).or_else(|| self.chats.get(&k).filter(|c| !c.locked || c.unlocked).map(|c| c.title.as_str()));
        if title.is_some_and(|title| self.settings.remember_locked_title(&k, sid, title)) {
            self.save();
        }
    }

    pub(super) fn relock_session(&mut self, server: &str, sid: &str) {
        self.remember_lock_title(server, sid);
        let title = self.locked_sidebar_title(server, sid);
        if let Some(c) = self.chats.get_mut(&key(server, sid)) {
            c.title.clone_from(&title);
            c.messages.clear();
            c.sent_images.clear();
            c.permissions.clear();
            c.loaded = false;
            c.loading = false;
            c.locked = true;
            c.unlocked = false;
            c.unlock_input.zeroize();
            c.unlock_error.clear();
        }
        if let Some(r) = self.remote.get_mut(server) {
            if let Some(s) = r.sessions.iter_mut().find(|s| s.id == sid) {
                s.locked = true;
                s.unlocked = false;
                s.title = title;
                s.auto_title.clear();
            }
        }
        if !self.demo {
            self.controller
                .send(server, json!({"op": "session_relock", "sid": sid}));
        }
        self.notice = "Сессия заблокирована в этом окне.".into();
    }

    pub(super) fn submit_unlock(&mut self, server: &str, sid: &str) {
        let Some(c) = self.chats.get_mut(&key(server, sid)) else {
            return;
        };
        let password = c.unlock_input.trim().to_owned();
        if password.is_empty() {
            c.unlock_error = "Введи пароль сессии.".into();
            return;
        }
        c.unlock_input.zeroize();
        if self.demo {
            c.unlocked = true;
            c.unlock_error.clear();
            return;
        }
        if self.controller.send(
            server,
            json!({
                "op": "session_unlock",
                "sid": sid,
                "password": password,
            }),
        ) {
            c.unlocking = true;
            c.unlock_error.clear();
        } else {
            c.unlock_error = "Нет связи с сервером.".into();
        }
        let mut pw = password;
        pw.zeroize();
    }

    pub(super) fn deletion_blocked(&self, server: &str, sid: &str) -> Option<&'static str> {
        if !self.connected(server) {
            return Some("Подключись к серверу, чтобы удалить сессию.");
        }
        if self
            .remote
            .get(server)
            .and_then(|r| r.sessions.iter().find(|s| s.id == sid))
            .is_some_and(Session::is_locked_gate)
            || self
                .chats
                .get(&key(server, sid))
                .is_some_and(|c| c.locked && !c.unlocked)
        {
            return Some("Сначала разблокируй защищённую паролем сессию.");
        }
        if self.chats.get(&key(server, sid)).is_some_and(|c| c.busy) {
            return Some("Сначала останови ответ в этой сессии.");
        }
        if let Some(id) = self.cluster_for_chat(server, sid) {
            let run = self.cluster_store.runs.iter().find(|r| r.id == id).unwrap();
            let busy = self
                .chats
                .get(&key(&run.config.leader.server, &run.leader_sid))
                .is_some_and(|c| c.busy)
                || run.tasks.iter().any(|t| {
                    !t.state.terminal()
                        || self
                            .chats
                            .get(&key(&t.agent.server, &t.sid))
                            .is_some_and(|c| c.busy)
                })
                || self.cluster_calls.iter().any(|call| call.run == id);
            if busy {
                return Some("Сначала останови команду в разделе «Кластер».");
            }
        }
        None
    }

    pub(super) fn request_delete(&mut self, server: &str, session: Session) {
        if let Some(reason) = self.deletion_blocked(server, &session.id) {
            self.notice = reason.into();
            return;
        }
        self.delete_chat = Some(DeleteChat {
            server: server.into(),
            session,
            pending: false,
            error: String::new(),
        });
    }

    pub(super) fn deletion_event(&mut self, server: &str, value: &Value) -> bool {
        if value["type"] == "error" && value["op"] == "delete_session" {
            if let Some(dialog) = self
                .delete_chat
                .as_mut()
                .filter(|d| d.server == server && value["sid"] == d.session.id)
            {
                dialog.pending = false;
                dialog.error = value["message"]
                    .as_str()
                    .unwrap_or("Не удалось удалить сессию.")
                    .into();
            }
            return true;
        }
        if value["type"] != "session_deleted" {
            return false;
        }
        let Some(sid) = value["sid"]
            .as_str()
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        else {
            return true;
        };
        self.deleted_sessions.insert(key(server, sid));
        self.settings.locked_session_names.remove(&key(server, sid));
        if let Some(remote) = self.remote.get_mut(server) {
            remote.sessions.retain(|s| s.id != sid);
        }
        self.chats.remove(&key(server, sid));
        let selected = self.selected_chat.get(server).is_some_and(|id| id == sid);
        if selected {
            self.selected_chat.remove(server);
        }
        for profile in &mut self.settings.servers {
            if profile.id == server && profile.last_session.as_deref() == Some(sid) {
                profile.last_session = None;
            }
        }
        let mut cluster_changed = false;
        for run in &mut self.cluster_store.runs {
            let mut changed = false;
            if run.config.leader.server == server && run.leader_sid == sid {
                run.leader_sid = uuid::Uuid::new_v4().to_string();
                changed = true;
            }
            let fresh = uuid::Uuid::new_v4().to_string();
            for task in &mut run.tasks {
                if task.agent.server == server && task.sid == sid {
                    task.sid = fresh.clone();
                    changed = true;
                }
            }
            if changed {
                run.paused = true;
                run.log(
                    "Пользователь удалил историю сессии. Для продолжения создана новая сессия.",
                );
                cluster_changed = true;
            }
        }
        if cluster_changed {
            if let Err(error) = self.cluster_store.save() {
                self.notice = error;
            }
        }
        if self
            .delete_chat
            .as_ref()
            .is_some_and(|d| d.server == server && d.session.id == sid)
        {
            self.delete_chat = None;
        }
        if selected && self.settings.selected.as_deref() == Some(server) {
            self.new_chat();
        }
        self.save();
        true
    }

    pub(super) fn deletion_dialog(&mut self, ctx: &egui::Context, p: Palette, t: f32) {
        let Some(mut dialog) = self.delete_chat.take() else {
            return;
        };
        let mut cancel = false;
        let mut confirm = false;
        let reason = self.deletion_blocked(&dialog.server, &dialog.session.id);
        let response = egui::Modal::new(egui::Id::new("delete-session"))
            .area(egui::Modal::default_area(egui::Id::new("delete-session"))
                .anchor(egui::Align2::CENTER_CENTER, Vec2::new(0., (1. - t) * 9.)))
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.line))
                    .corner_radius(14.)
                    .inner_margin(22.),
            )
            .show(ctx, |ui| {
                ui.set_width(400.);
                label(ui, "Удалить сессию?", 19., p.text);
                ui.add_space(10.);
                label(ui, model::shorten(&dialog.session.title, 90), 14., p.text);
                ui.add_space(6.);
                label(ui, "История этой сессии будет удалена из Relay и CLI-агентов на сервере. Файлы проекта сохранятся.", 13., p.muted);
                if self.chats.get(&key(&dialog.server, &dialog.session.id)).is_some_and(|c| !c.draft.is_empty() || !c.images.is_empty() || !c.queue.is_empty()) {
                    label(ui, "Черновик и очередь этой сессии тоже будут удалены.", 12., p.muted);
                }
                if let Some(reason) = reason { label(ui, reason, 12., design::RED); }
                if !dialog.error.is_empty() { label(ui, &dialog.error, 12., design::RED); }
                ui.add_space(18.);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!dialog.pending, |ui| {
                        cancel = design::button(ui, "Отмена", None, Kind::Secondary, p, Vec2::new(118., 34.)).clicked();
                    });
                    ui.add_enabled_ui(!dialog.pending && reason.is_none(), |ui| {
                        confirm = design::button(ui, if dialog.pending {"Удаляем…"} else {"Удалить сессию"}, Some(Icon::Trash), Kind::Primary, p, Vec2::new(196., 34.)).clicked();
                    });
                });
            });
        if response.should_close() && !dialog.pending {
            cancel = true;
        }
        if confirm {
            if self.demo {
                dialog.error = "Это предпросмотр. Реальная история не меняется.".into();
            } else if self.controller.send(
                &dialog.server,
                json!({"op":"delete_session","sid":dialog.session.id}),
            ) {
                dialog.pending = true;
                dialog.error.clear();
                if let Some(id) = self.cluster_for_chat(&dialog.server, &dialog.session.id) {
                    if let Some(run) = self.cluster_store.runs.iter_mut().find(|r| r.id == id) {
                        run.paused = true;
                    }
                    if let Err(error) = self.cluster_store.save() {
                        self.notice = error;
                    }
                }
            } else {
                dialog.error = "Подключись к серверу и повтори удаление.".into();
            }
        }
        if !cancel {
            self.delete_chat = Some(dialog);
        }
    }

    pub(super) fn lock_dialog(&mut self, ctx: &egui::Context, p: Palette, t: f32) {
        let Some(mut dialog) = self.lock_dialog.take() else {
            return;
        };
        let mut cancel = false;
        let mut submit = false;
        let mut relock_now = false;
        let response = egui::Modal::new(egui::Id::new("session-lock-modal"))
            .area(
                egui::Modal::default_area(egui::Id::new("session-lock-modal"))
                    .anchor(egui::Align2::CENTER_CENTER, Vec2::new(0., (1. - t) * 9.)),
            )
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.line))
                    .corner_radius(14.)
                    .inner_margin(22.),
            )
            .show(ctx, |ui| {
                ui.set_width(420.);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(20.), egui::Sense::hover());
                    design::icon(ui.painter(), r, Icon::Lock, accent());
                    label(
                        ui,
                        if dialog.already_locked {
                            "Защита приватной сессии"
                        } else {
                            "Защитить сессию паролем"
                        },
                        18.,
                        p.text,
                    );
                });
                ui.add_space(6.);
                label(ui, model::shorten(&dialog.title, 80), 13., p.muted);
                ui.add_space(6.);
                label(
                    ui,
                    "На сервере хранится только криптографический ключ PBKDF2-SHA256. Другие подключения увидят замок без текста сообщений.",
                    12.,
                    p.muted,
                );
                if dialog.already_locked {
                    ui.add_space(12.);
                    let half_w = ((ui.available_width() - 8.) * 0.5).max(120.);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.;
                        if design::button(
                            ui,
                            "Сменить пароль",
                            Some(Icon::Key),
                            if !dialog.remove_mode { Kind::Primary } else { Kind::Secondary },
                            p,
                            Vec2::new(half_w, 32.),
                        )
                        .clicked()
                        {
                            dialog.remove_mode = false;
                            dialog.error.clear();
                        }
                        if design::button(
                            ui,
                            "Снять пароль",
                            Some(Icon::Unlock),
                            if dialog.remove_mode { Kind::Primary } else { Kind::Secondary },
                            p,
                            Vec2::new(half_w, 32.),
                        )
                        .clicked()
                        {
                            dialog.remove_mode = true;
                            dialog.error.clear();
                        }
                    });
                    ui.add_space(6.);
                    if design::button(
                        ui,
                        "Заблокировать сессию сейчас",
                        Some(Icon::Lock),
                        Kind::Secondary,
                        p,
                        Vec2::new(ui.available_width(), 32.),
                    )
                    .clicked()
                    {
                        relock_now = true;
                    }
                    ui.add_space(12.);
                    label(ui, "Текущий пароль", 12., p.muted);
                    let r = design::field(
                        ui,
                        &mut dialog.current_password,
                        "Введи текущий пароль",
                        true,
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && dialog.remove_mode {
                        submit = true;
                    }
                }
                if !dialog.remove_mode {
                    ui.add_space(10.);
                    label(
                        ui,
                        if dialog.already_locked {
                            "Новый пароль"
                        } else {
                            "Пароль сессии"
                        },
                        12.,
                        p.muted,
                    );
                    design::field(
                        ui,
                        &mut dialog.new_password,
                        "Минимум 1 символ",
                        true,
                    );
                    ui.add_space(8.);
                    label(ui, "Повтори пароль", 12., p.muted);
                    let r = design::field(
                        ui,
                        &mut dialog.confirm_password,
                        "Повтори пароль без опечаток",
                        true,
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                }
                if !dialog.error.is_empty() {
                    ui.add_space(8.);
                    label(ui, &dialog.error, 12., design::RED);
                }
                ui.add_space(18.);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!dialog.pending, |ui| {
                        cancel = design::button(
                            ui,
                            "Отмена",
                            None,
                            Kind::Secondary,
                            p,
                            Vec2::new(118., 34.),
                        )
                        .clicked();
                        let cta = if dialog.pending {
                            "Сохраняем…"
                        } else if dialog.remove_mode {
                            "Снять защиту"
                        } else if dialog.already_locked {
                            "Обновить пароль"
                        } else {
                            "Установить пароль"
                        };
                        if design::button(
                            ui,
                            cta,
                            Some(if dialog.remove_mode {
                                Icon::Unlock
                            } else {
                                Icon::Lock
                            }),
                            Kind::Primary,
                            p,
                            Vec2::new(206., 34.),
                        )
                        .clicked()
                        {
                            submit = true;
                        }
                    });
                });
            });
        if response.should_close() && !dialog.pending {
            cancel = true;
        }
        if relock_now {
            self.relock_session(&dialog.server, &dialog.sid);
            return;
        }
        if submit && !dialog.pending {
            if dialog.already_locked && dialog.current_password.trim().is_empty() {
                dialog.error = "Введи текущий пароль сессии.".into();
            } else if !dialog.remove_mode && dialog.new_password.trim().is_empty() {
                dialog.error = "Пароль не может быть пустым.".into();
            } else if !dialog.remove_mode && dialog.new_password != dialog.confirm_password {
                dialog.error = "Пароли не совпадают.".into();
            } else if self.demo {
                dialog.error = "В режиме демонстрации защита на сервере не сохраняется.".into();
            } else if self.controller.send(
                &dialog.server,
                json!({
                    "op": "session_lock",
                    "sid": dialog.sid,
                    "password": if dialog.remove_mode { Value::Null } else { json!(dialog.new_password.trim()) },
                    "current_password": if dialog.already_locked { json!(dialog.current_password.trim()) } else { Value::Null },
                    "remove": dialog.remove_mode,
                    "actor": self.settings.actor(),
                }),
            ) {
                dialog.pending = true;
                dialog.error.clear();
            } else {
                dialog.error = "Подключись к серверу и попробуй снова.".into();
            }
        }
        if !cancel {
            self.lock_dialog = Some(dialog);
        }
    }

    pub(super) fn agents_dialog(&mut self, ctx: &egui::Context, p: Palette, t: f32) {
        let Some(server_id) = self.agents_open.clone() else {
            return;
        };
        let Some(server) = self.settings.servers.iter().find(|s| s.id == server_id).cloned() else {
            self.agents_open = None;
            return;
        };
        let mut close = false;
        let mut trigger_install: Option<(&'static str, &'static str)> = None;
        let mut trigger_login: Option<&'static str> = None;
        let mut refresh_status = false;

        let response = egui::Modal::new(egui::Id::new("agents-installer-modal"))
            .area(
                egui::Modal::default_area(egui::Id::new("agents-installer-modal"))
                    .anchor(egui::Align2::CENTER_CENTER, Vec2::new(0., (1. - t) * 9.)),
            )
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.line))
                    .corner_radius(14.)
                    .inner_margin(22.),
            )
            .show(ctx, |ui| {
                ui.set_width(490.);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(20.), egui::Sense::hover());
                    design::icon(ui.painter(), r, Icon::Download, accent());
                    label(ui, "CLI-агенты на сервере", 18.5, p.text);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        label(ui, &server.name, 12.5, p.muted);
                    });
                });
                ui.add_space(6.);
                label(
                    ui,
                    "Установи или обнови официальные CLI в ~/.local/bin на выбранном сервере в 1 клик и авторизуй их через серверный браузер.",
                    12.5,
                    p.muted,
                );
                ui.add_space(14.);

                let connected = self.connected(&server.id);
                let specs = [
                    (
                        "claude",
                        "Claude Code",
                        "Anthropic CLI · потоковые ответы, инструменты, команды и MCP",
                    ),
                    (
                        "codex",
                        "OpenAI Codex CLI",
                        "codex app-server · автономный кодинг, патчи и живое управление",
                    ),
                    (
                        "agy",
                        "Google Antigravity (agy)",
                        "agy acp · Gemini и агентная среда Google на твоём сервере",
                    ),
                ];

                for (id, title, subtitle) in specs {
                    let installed = self.backend_installed(&server.id, id);
                    let installing = self.backend_installing(&server.id, id).map(str::to_owned);
                    let version = self
                        .remote
                        .get(&server.id)
                        .and_then(|r| {
                            r.backends[id]["version"]
                                .as_str()
                                .filter(|v| !v.is_empty())
                                .map(str::to_owned)
                                .or_else(|| {
                                    if id == "claude" && installed && !r.version.is_empty() {
                                        Some(r.version.clone())
                                    } else {
                                        None
                                    }
                                })
                        })
                        .unwrap_or_default();
                    let last_log = self
                        .remote
                        .get(&server.id)
                        .and_then(|r| r.agent_logs.get(id).cloned())
                        .unwrap_or_default();

                    egui::Frame::new()
                        .fill(design::mix(p.base, p.surface, 0.45))
                        .stroke(Stroke::new(1.0_f32, p.line))
                        .corner_radius(11.)
                        .inner_margin(egui::Margin::symmetric(14, 11))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let (dot_r, _) =
                                    ui.allocate_exact_size(Vec2::splat(8.), egui::Sense::hover());
                                ui.painter().circle_filled(
                                    dot_r.center(),
                                    3.6,
                                    if installing.is_some() {
                                        accent()
                                    } else if installed {
                                        GREEN
                                    } else {
                                        p.muted
                                    },
                                );
                                label(ui, title, 14., p.text);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let status_text = if installing.is_some() {
                                        "устанавливается…".to_owned()
                                    } else if installed {
                                        if version.is_empty() {
                                            "установлен".to_owned()
                                        } else {
                                            model::shorten(&version, 34)
                                        }
                                    } else {
                                        "не установлен".to_owned()
                                    };
                                    label(
                                        ui,
                                        status_text,
                                        11.5,
                                        if installed { GREEN } else { p.muted },
                                    );
                                });
                            });
                            ui.add_space(3.);
                            label(ui, subtitle, 11.5, p.muted);
                            if let Some(msg) = &installing {
                                ui.add_space(6.);
                                ui.horizontal(|ui| {
                                    design::dots(ui, accent());
                                    label(ui, msg, 11.5, accent());
                                });
                            } else if !last_log.is_empty() && !installed {
                                ui.add_space(4.);
                                label(ui, model::shorten(&last_log, 120), 11., p.muted);
                            }
                            ui.add_space(8.);
                            ui.horizontal(|ui| {
                                ui.add_enabled_ui(connected && installing.is_none(), |ui| {
                                    if installed {
                                        if design::button(
                                            ui,
                                            "Обновить",
                                            Some(Icon::Refresh),
                                            Kind::Secondary,
                                            p,
                                            Vec2::new(112., 30.),
                                        )
                                        .clicked()
                                        {
                                            trigger_install = Some((id, "update"));
                                        }
                                        if design::button(
                                            ui,
                                            "Войти",
                                            Some(Icon::Key),
                                            Kind::Secondary,
                                            p,
                                            Vec2::new(98., 30.),
                                        )
                                        .clicked()
                                        {
                                            trigger_login = Some(id);
                                        }
                                    } else if design::button(
                                        ui,
                                        "Установить в 1 клик",
                                        Some(Icon::Download),
                                        Kind::Primary,
                                        p,
                                        Vec2::new(178., 30.),
                                    )
                                    .clicked()
                                    {
                                        trigger_install = Some((id, "install"));
                                    }
                                });
                            });
                        });
                    ui.add_space(8.);
                }

                ui.add_space(8.);
                ui.horizontal(|ui| {
                    let any_missing = ["claude", "codex", "agy"]
                        .iter()
                        .any(|id| !self.backend_installed(&server.id, id));
                    ui.add_enabled_ui(connected && any_missing, |ui| {
                        if design::button(
                            ui,
                            "Установить все недостающие",
                            Some(Icon::Download),
                            Kind::Primary,
                            p,
                            Vec2::new(226., 34.),
                        )
                        .clicked()
                        {
                            trigger_install = Some(("all", "install"));
                        }
                    });
                    if connected
                        && design::button(
                            ui,
                            "Проверить",
                            Some(Icon::Refresh),
                            Kind::Secondary,
                            p,
                            Vec2::new(114., 34.),
                        )
                        .clicked()
                    {
                        refresh_status = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if design::button(
                            ui,
                            "Готово",
                            None,
                            Kind::Secondary,
                            p,
                            Vec2::new(104., 34.),
                        )
                        .clicked()
                        {
                            close = true;
                        }
                    });
                });
            });

        if response.should_close() {
            close = true;
        }
        if let Some((agent, action)) = trigger_install {
            self.install_server_agent(&server.id, agent, action);
        }
        if refresh_status {
            self.controller
                .send(&server.id, json!({"op": "backends_status"}));
        }
        if let Some(prov) = trigger_login {
            self.agents_open = None;
            self.start_provider_login(&server, prov);
            return;
        }
        if close {
            self.agents_open = None;
        }
    }

    pub(super) fn radar_dialog(&mut self, ctx: &egui::Context, p: Palette, modal_t: f32) {
        let Some(server_id) = self.radar_open.clone() else {
            return;
        };
        let Some(server) = self
            .settings
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .cloned()
        else {
            self.radar_open = None;
            return;
        };
        let light = self.settings.light;
        let connected = self.connected(&server.id);
        let sessions: Vec<Session> = self
            .remote
            .get(&server.id)
            .map(|r| r.sessions.clone())
            .unwrap_or_default();
        let mut sorted = sessions.clone();
        sorted.sort_by(|a, b| {
            let a_busy = a.running
                || self.chats.get(&key(&server.id, &a.id)).is_some_and(|c| c.busy)
                || self
                    .remote
                    .get(&server.id)
                    .is_some_and(|r| r.running.contains(&a.id));
            let b_busy = b.running
                || self.chats.get(&key(&server.id, &b.id)).is_some_and(|c| c.busy)
                || self
                    .remote
                    .get(&server.id)
                    .is_some_and(|r| r.running.contains(&b.id));
            b_busy
                .cmp(&a_busy)
                .then_with(|| b.modified.total_cmp(&a.modified))
        });

        let active_count = sorted
            .iter()
            .filter(|s| {
                s.running
                    || self.chats.get(&key(&server.id, &s.id)).is_some_and(|c| c.busy)
                    || self
                        .remote
                        .get(&server.id)
                        .is_some_and(|r| r.running.contains(&s.id))
            })
            .count();

        let mut close = false;
        let mut jump_to: Option<Session> = None;
        let modal_w = (ctx.screen_rect().width() - 48.).clamp(340., 680.);
        let response = egui::Modal::new(egui::Id::new("agent-radar-dialog"))
            .area(
                egui::Modal::default_area(egui::Id::new("agent-radar-dialog")).anchor(
                    egui::Align2::CENTER_CENTER,
                    Vec2::new(0., (1. - modal_t) * 9.),
                ),
            )
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .corner_radius(16.)
                    .inner_margin(22.)
                    .stroke(Stroke::new(1.0_f32, design::mix(p.line, accent(), 0.32))),
            )
            .backdrop_color(Color32::from_black_alpha((128. * modal_t) as u8))
            .show(ctx, |ui| {
                ui.set_width(modal_w);
                ui.set_opacity(modal_t);
                ui.horizontal(|ui| {
                    let (orb, _) = ui.allocate_exact_size(Vec2::splat(28.), egui::Sense::hover());
                    design::orbital_core(ui, orb, accent(), active_count > 0);
                    ui.vertical(|ui| {
                        label(ui, "Радар агентов · Живая телеметрия", 20., p.text);
                        label(
                            ui,
                            format!(
                                "{} · активных потоков: {} · всего сессий: {}",
                                server.name,
                                active_count,
                                sorted.len()
                            ),
                            12.,
                            p.muted,
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if design::button(ui, "", Some(Icon::Close), Kind::Ghost, p, Vec2::splat(28.))
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(12.);

                let shown_nodes: Vec<_> = sorted.iter().take(5).collect();
                let canvas_h = (shown_nodes.len().max(2) as f32 * 64. + 24.).clamp(160., 348.);
                let (canvas, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), canvas_h), egui::Sense::hover());
                ui.painter()
                    .rect_filled(canvas, 13., design::mix(p.side, p.base, 0.45));
                ui.painter().rect_stroke(
                    canvas,
                    13.,
                    Stroke::new(1.0_f32, p.line),
                    egui::StrokeKind::Inside,
                );
                let mut gx = canvas.left() + 16.;
                while gx < canvas.right() - 10. {
                    let mut gy = canvas.top() + 16.;
                    while gy < canvas.bottom() - 10. {
                        ui.painter()
                            .circle_filled(egui::pos2(gx, gy), 0.9, p.line.gamma_multiply(0.45));
                        gy += 22.;
                    }
                    gx += 22.;
                }

                let hub_w = (canvas.width() * 0.28).clamp(112., 168.);
                let hub_rect = egui::Rect::from_center_size(
                    egui::pos2(canvas.left() + hub_w * 0.5 + 18., canvas.center().y),
                    Vec2::new(hub_w, 76.),
                );
                ui.painter()
                    .circle_filled(hub_rect.center(), hub_w * 0.42, accent().gamma_multiply(0.08));
                ui.painter().rect_filled(hub_rect, 11., p.surface);
                ui.painter().rect_stroke(
                    hub_rect,
                    11.,
                    Stroke::new(1.3_f32, design::mix(p.line, accent(), 0.55)),
                    egui::StrokeKind::Inside,
                );
                let hc = ui.painter().with_clip_rect(hub_rect.shrink(8.));
                hc.text(
                    hub_rect.left_top() + Vec2::new(11., 11.),
                    egui::Align2::LEFT_TOP,
                    "УЗЕЛ СЕРВЕРА",
                    FontId::proportional(9.0),
                    accent(),
                );
                hc.text(
                    hub_rect.left_top() + Vec2::new(11., 27.),
                    egui::Align2::LEFT_TOP,
                    model::shorten(&server.name, 16),
                    FontId::proportional(13.5),
                    p.text,
                );
                hc.text(
                    hub_rect.left_bottom() + Vec2::new(11., -10.),
                    egui::Align2::LEFT_BOTTOM,
                    if connected { "● на связи" } else { "○ нет связи" },
                    FontId::proportional(10.5),
                    if connected { GREEN } else { p.muted },
                );

                if shown_nodes.is_empty() {
                    ui.painter().text(
                        egui::pos2(
                            (hub_rect.right() + canvas.right()) * 0.5,
                            canvas.center().y,
                        ),
                        egui::Align2::CENTER_CENTER,
                        "Нет открытых сессий на сервере",
                        FontId::proportional(12.5),
                        p.muted,
                    );
                } else {
                    let node_w = (canvas.right() - hub_rect.right() - 56.).clamp(140., 360.);
                    let step_y = 62.;
                    let total_h = (shown_nodes.len() - 1) as f32 * step_y;
                    let y0 = canvas.center().y - total_h * 0.5;
                    let now = ui.input(|i| i.time) as f32;
                    let mut need_repaint = false;

                    for (idx, sess) in shown_nodes.iter().enumerate() {
                        let sk = key(&server.id, &sess.id);
                        let busy = sess.running
                            || self.chats.get(&sk).is_some_and(|c| c.busy)
                            || self
                                .remote
                                .get(&server.id)
                                .is_some_and(|r| r.running.contains(&sess.id));
                        let sb = self.backend_for_chat(&server, &sk);
                        let bcol = design::backend_color(&sb, light);
                        let n_rect = egui::Rect::from_center_size(
                            egui::pos2(canvas.right() - node_w * 0.5 - 16., y0 + idx as f32 * step_y),
                            Vec2::new(node_w, 52.),
                        );
                        let p0 = hub_rect.right_center();
                        let p3 = n_rect.left_center();
                        let dx = (p3.x - p0.x) * 0.5;
                        let p1 = p0 + Vec2::new(dx, 0.);
                        let p2 = p3 - Vec2::new(dx, 0.);
                        ui.painter().add(egui::epaint::CubicBezierShape::from_points_stroke(
                            [p0, p1, p2, p3],
                            false,
                            Color32::TRANSPARENT,
                            Stroke::new(
                                if busy { 1.8_f32 } else { 1.1_f32 },
                                if busy { bcol } else { p.line },
                            ),
                        ));
                        if busy {
                            need_repaint = true;
                            for p_idx in 0..2 {
                                let t = (now * 0.55 + p_idx as f32 * 0.5 + idx as f32 * 0.2) % 1.0;
                                let q = 1.0 - t;
                                let pt = (p0.to_vec2() * (q * q * q)
                                    + p1.to_vec2() * (3.0 * q * q * t)
                                    + p2.to_vec2() * (3.0 * q * t * t)
                                    + p3.to_vec2() * (t * t * t))
                                .to_pos2();
                                ui.painter().circle_filled(pt, 4.5, bcol.gamma_multiply(0.24));
                                ui.painter().circle_filled(pt, 2.4, bcol);
                            }
                        }

                        let n_resp = ui.interact(
                            n_rect,
                            ui.id().with(("radar-node", &sess.id)),
                            egui::Sense::click(),
                        );
                        let hov = n_resp.hovered();
                        ui.painter().rect_filled(
                            n_rect,
                            10.,
                            design::mix(
                                p.surface,
                                bcol,
                                if busy { 0.14 } else if hov { 0.09 } else { 0.03 },
                            ),
                        );
                        ui.painter().rect_stroke(
                            n_rect,
                            10.,
                            Stroke::new(
                                if busy || hov { 1.3_f32 } else { 1.0_f32 },
                                design::mix(p.line, bcol, if busy || hov { 0.58 } else { 0.22 }),
                            ),
                            egui::StrokeKind::Inside,
                        );
                        let icon_r = egui::Rect::from_center_size(
                            egui::pos2(n_rect.left() + 18., n_rect.center().y),
                            Vec2::splat(18.),
                        );
                        if busy {
                            design::orbital_core(ui, icon_r, bcol, true);
                        } else if sess.is_locked_gate() {
                            design::icon(ui.painter(), icon_r.shrink(2.), Icon::Lock, accent());
                        } else {
                            design::icon(
                                ui.painter(),
                                icon_r.shrink(2.),
                                design::backend_icon(&sb),
                                bcol,
                            );
                        }
                        let tel = self
                            .chats
                            .get(&sk)
                            .map(|c| {
                                crate::model::analyze_turn(
                                    &c.messages,
                                    c.user_at,
                                    busy,
                                    &c.status,
                                )
                            })
                            .unwrap_or_default();
                        let sub_line = if sess.is_locked_gate() {
                            "Защищено паролем".to_owned()
                        } else if busy && !tel.active_target.is_empty() {
                            tel.active_target.clone()
                        } else if busy && !tel.active_headline.is_empty() {
                            tel.active_headline.clone()
                        } else {
                            format!(
                                "{} · {}",
                                crate::model::backend_short_label(&sb),
                                folder_name(&sess.cwd)
                            )
                        };
                        let nc = ui.painter().with_clip_rect(n_rect.shrink2(Vec2::new(8., 4.)));
                        nc.text(
                            egui::pos2(n_rect.left() + 34., n_rect.top() + 16.),
                            egui::Align2::LEFT_CENTER,
                            model::shorten(&sess.title, 30),
                            FontId::proportional(12.5),
                            p.text,
                        );
                        nc.text(
                            egui::pos2(n_rect.left() + 34., n_rect.bottom() - 14.),
                            egui::Align2::LEFT_CENTER,
                            model::shorten(&sub_line, 36),
                            FontId::monospace(10.5),
                            if busy { bcol } else { p.muted },
                        );
                        if tel.total_actions > 0 && n_rect.width() >= 220. {
                            let pb = egui::Rect::from_min_size(
                                egui::pos2(n_rect.right() - 56., n_rect.top() + 13.),
                                Vec2::new(44., 5.),
                            );
                            design::phase_bar(ui.painter(), pb, &tel, light, p);
                        }
                        if n_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Перейти к этой сессии")
                            .clicked()
                        {
                            jump_to = Some((*sess).clone());
                        }
                    }
                    if need_repaint {
                        ctx.request_repaint_after(Duration::from_millis(28));
                    }
                }

                ui.add_space(12.);
                ui.horizontal(|ui| {
                    if design::button(
                        ui,
                        "Команда агентов",
                        Some(Icon::Cluster),
                        Kind::Secondary,
                        p,
                        Vec2::new(164., 32.),
                    )
                    .clicked()
                    {
                        self.cluster_open = true;
                        close = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if design::button(ui, "Закрыть", None, Kind::Secondary, p, Vec2::new(98., 32.))
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
            });

        if response.should_close() {
            close = true;
        }
        if let Some(sess) = jump_to {
            self.radar_open = None;
            self.select_chat(sess);
        } else if close {
            self.radar_open = None;
        }
    }

    pub(super) fn folder_modal(&mut self, ctx: &egui::Context, p: Palette, t: f32) {
        let Some(mut dialog) = self.folder_dialog.take() else {
            return;
        };
        let folders = self
            .remote
            .get(&dialog.server)
            .map(|r| r.folders.clone())
            .unwrap_or_default();
        let mut cancel = false;
        let mut submit_name = false;
        let mut pick_existing: Option<String> = None;

        let response = egui::Modal::new(egui::Id::new("session-folder-modal"))
            .area(
                egui::Modal::default_area(egui::Id::new("session-folder-modal"))
                    .anchor(egui::Align2::CENTER_CENTER, Vec2::new(0., (1. - t) * 9.)),
            )
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.line))
                    .corner_radius(14.)
                    .inner_margin(22.),
            )
            .show(ctx, |ui| {
                ui.set_width(430.);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(20.), egui::Sense::hover());
                    design::icon(ui.painter(), r, Icon::Folder, accent());
                    label(
                        ui,
                        if dialog.rename_folder_id.is_some() {
                            "Переименовать папку"
                        } else if dialog.move_session.is_some() {
                            "Переместить сессию в папку"
                        } else {
                            "Новая папка для сессий"
                        },
                        18.,
                        p.text,
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if design::button(ui, "", Some(Icon::Close), Kind::Ghost, p, Vec2::splat(28.))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
                ui.add_space(6.);
                if let Some((_, ref s_title, _)) = dialog.move_session {
                    label(
                        ui,
                        format!("Сессия: «{}»", model::shorten(s_title, 54)),
                        12.5,
                        p.muted,
                    );
                } else {
                    label(
                        ui,
                        "Папки общие для всех участников, подключённых к этому серверу.",
                        12.5,
                        p.muted,
                    );
                }

                if let Some((_, _, ref cur_folder)) = dialog.move_session {
                    ui.add_space(12.);
                    label(ui, "Выбери существующую папку", 11.5, p.muted);
                    ui.add_space(4.);
                    if folders.is_empty() && cur_folder.is_empty() {
                        label(
                            ui,
                            "На сервере пока нет папок — создай первую ниже.",
                            12.,
                            p.muted,
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("folder-modal-list")
                            .max_height(190.)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 5.;
                                if !cur_folder.is_empty()
                                    && design::service_action(
                                        ui,
                                        "Убрать из папки (в общий список)",
                                        Icon::Close,
                                        false,
                                        p,
                                    )
                                    .clicked()
                                {
                                    pick_existing = Some(String::new());
                                }
                                for f in &folders {
                                    let active = f.id == *cur_folder;
                                    if design::service_action(
                                        ui,
                                        &f.name,
                                        if active { Icon::Check } else { Icon::Folder },
                                        active,
                                        p,
                                    )
                                    .clicked()
                                    {
                                        pick_existing = Some(f.id.clone());
                                    }
                                }
                            });
                    }
                    ui.add_space(14.);
                    label(ui, "Или создай новую папку и перемести туда", 11.5, p.muted);
                } else {
                    ui.add_space(14.);
                    label(ui, "Название папки", 12., p.muted);
                }

                ui.add_space(4.);
                let field_resp = design::field(
                    ui,
                    &mut dialog.name,
                    "Например, Продакшен или Фронтенд",
                    false,
                );
                if !dialog.focused_once && dialog.move_session.is_none() {
                    field_resp.request_focus();
                    dialog.focused_once = true;
                }
                if field_resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit_name = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
                if !dialog.error.is_empty() {
                    ui.add_space(6.);
                    label(ui, &dialog.error, 12., design::RED);
                }
                ui.add_space(18.);
                ui.horizontal(|ui| {
                    if design::button(ui, "Отмена", None, Kind::Secondary, p, Vec2::new(112., 34.))
                        .clicked()
                    {
                        cancel = true;
                    }
                    let cta = if dialog.rename_folder_id.is_some() {
                        "Сохранить"
                    } else if dialog.move_session.is_some() {
                        "Создать и переместить"
                    } else {
                        "Создать папку"
                    };
                    if design::button(
                        ui,
                        cta,
                        Some(Icon::Folder),
                        Kind::Primary,
                        p,
                        Vec2::new(ui.available_width(), 34.),
                    )
                    .clicked()
                    {
                        submit_name = true;
                    }
                });
            });

        if response.should_close() {
            cancel = true;
        }
        let actor = self.settings.actor();
        if let Some(target_folder) = pick_existing {
            if let Some((sid, _, _)) = dialog.move_session {
                self.controller.send(
                    &dialog.server,
                    json!({"op":"session_meta","sid":sid,"folder":target_folder,"actor":actor}),
                );
            }
            return;
        }
        if submit_name {
            let trimmed = dialog.name.trim().to_owned();
            if trimmed.is_empty() {
                dialog.error = "Введи непустое название папки.".into();
                self.folder_dialog = Some(dialog);
                return;
            }
            if let Some(fid) = dialog.rename_folder_id {
                self.controller.send(
                    &dialog.server,
                    json!({"op":"session_folder","id":fid,"name":trimmed}),
                );
            } else {
                let moving_sid = dialog.move_session.map(|(sid, _, _)| sid);
                self.controller.send(
                    &dialog.server,
                    json!({"op":"session_folder","name":trimmed,"sid":moving_sid,"actor":actor}),
                );
            }
            return;
        }
        if !cancel {
            self.folder_dialog = Some(dialog);
        }
    }
}
