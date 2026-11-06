use super::*;

pub(super) struct DeleteChat {
    pub server: String,
    pub session: Session,
    pub pending: bool,
    pub error: String,
}

impl RelayApp {
    pub(super) fn deletion_blocked(&self, server: &str, sid: &str) -> Option<&'static str> {
        if !self.connected(server) {
            return Some("Подключись к серверу, чтобы удалить сессию.");
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
        // Retain completed task results and prompts, but use a fresh conversation
        // if the user later continues a cluster whose old history was deleted.
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
            .frame(egui::Frame::new().fill(p.surface).corner_radius(14.).inner_margin(22.))
            .show(ctx, |ui| {
                ui.set_width(400.);
                label(ui, "Удалить сессию?", 19., p.text);
                ui.add_space(10.);
                label(ui, model::shorten(&dialog.session.title, 90), 14., p.text);
                ui.add_space(6.);
                label(ui, "История этой сессии будет удалена из Relay и Claude на сервере. Файлы проекта сохранятся.", 13., p.muted);
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
}
