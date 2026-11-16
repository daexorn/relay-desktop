use super::*;
use crate::auth_browser::Browser;

pub(super) struct Login {
    pub(super) server: String,
    pub(super) id: String,
    state: String,
    message: String,
    url: String,
    code: String,
    browser: Option<Browser>,
    browser_error: String,
}
impl Login {
    pub(super) fn active(&self) -> bool {
        matches!(self.state.as_str(), "starting" | "browser" | "verifying")
    }
}
impl Drop for Login {
    fn drop(&mut self) {
        self.code.zeroize();
    }
}

impl RelayApp {
    pub(super) fn start_login(&mut self, server: &Server) {
        if self
            .login
            .as_ref()
            .is_some_and(|l| l.server == server.id && l.active())
        {
            return;
        }
        if let Some(old) = self.login.take() {
            if old.active() {
                self.controller
                    .send(&old.server, json!({"op":"auth_cancel","id":old.id}));
            }
        }
        let available = self
            .remote
            .get(&server.id)
            .is_some_and(|r| r.connected && r.auth_available);
        let id = uuid::Uuid::new_v4().to_string();
        self.login = Some(Login {
            server: server.id.clone(),
            id: id.clone(),
            state: if available { "starting" } else { "failed" }.into(),
            message: if available {
                "Готовим вход на сервере…"
            } else {
                "Подключись к серверу заново, чтобы включить вход из Relay."
            }
            .into(),
            url: String::new(),
            code: String::new(),
            browser: None,
            browser_error: String::new(),
        });
        if available {
            self.controller
                .send(&server.id, json!({"op":"auth_start","id":id}));
        }
    }

    pub(super) fn login_event(&mut self, server: &str, value: &Value) {
        let Some(login) = self
            .login
            .as_mut()
            .filter(|l| l.server == server && value["id"] == l.id)
        else {
            return;
        };
        let state = value["state"].as_str().unwrap_or("failed");
        if !matches!(
            state,
            "starting"
                | "browser"
                | "verifying"
                | "success"
                | "failed"
                | "cancelled"
                | "input_error"
        ) {
            return;
        }
        login.state = state.into();
        login.message = value["message"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(500)
            .collect();
        if state == "input_error" {
            login.state = "browser".into();
            return;
        }
        if state == "browser" {
            let url = value["url"].as_str().unwrap_or("");
            if !crate::auth_browser::valid_login_url(url) {
                login.state = "failed".into();
                login.message = "Сервер вернул неизвестную страницу входа.".into();
                return;
            }
            login.url = url.into();
            if !self.demo && !cfg!(test) {
                let title = self
                    .settings
                    .servers
                    .iter()
                    .find(|s| s.id == server)
                    .map_or("Сервер", |s| s.name.as_str());
                match Browser::open(server, title, url) {
                    Ok(browser) => login.browser = Some(browser),
                    Err(error) => login.browser_error = error.to_string(),
                }
            }
        }
        if !login.active() {
            login.browser = None;
            login.code.zeroize();
        }
        if state == "success" {
            self.controller.send(server, json!({"op":"usage"}));
        }
    }

    pub(super) fn login_disconnected(&mut self, server: &str) {
        if let Some(login) = self
            .login
            .as_mut()
            .filter(|l| l.server == server && l.active())
        {
            login.browser = None;
            login.code.zeroize();
            login.state = "failed".into();
            login.message = "Подключение прервалось. Подключись к серверу и повтори вход.".into();
        }
    }

    pub(super) fn login_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut login) = self.login.take() else {
            return;
        };
        let p = design::animated_palette(ctx, self.settings.light);
        let title = self
            .settings
            .servers
            .iter()
            .find(|s| s.id == login.server)
            .map_or("Сервер", |s| s.name.as_str());
        let mut open = true;
        let mut retry = false;
        let mut close = false;
        if login.browser.as_mut().is_some_and(|b| !b.running()) {
            login.browser = None;
        }
        egui::Window::new("Вход в Claude")
            .id(egui::Id::new("claude-login"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.)
            .show(ctx, |ui| {
                label(ui, title, 20., p.text);
                ui.add_space(8.);
                if login.active() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        label(ui, &login.message, 13., p.muted);
                    });
                } else {
                    label(
                        ui,
                        &login.message,
                        13.,
                        if login.state == "success" {
                            GREEN
                        } else {
                            p.text
                        },
                    );
                }
                if login.active() && !login.url.is_empty() {
                    ui.add_space(14.);
                    ui.horizontal_wrapped(|ui| {
                        if design::button(
                            ui,
                            "Открыть окно входа",
                            None,
                            Kind::Primary,
                            p,
                            Vec2::new(0., 32.),
                        )
                        .clicked()
                        {
                            login.browser = None;
                            match Browser::open(&login.server, title, &login.url) {
                                Ok(browser) => {
                                    login.browser = Some(browser);
                                    login.browser_error.clear();
                                }
                                Err(error) => login.browser_error = error.to_string(),
                            }
                        }
                        if ui.button("В обычном браузере").clicked() {
                            ctx.open_url(egui::OpenUrl::new_tab(&login.url));
                        }
                    });
                    if !login.browser_error.is_empty() {
                        label(ui, &login.browser_error, 12., p.muted);
                    }
                    ui.add_space(14.);
                    label(
                        ui,
                        "Если Claude показал код — вставь его сюда",
                        12.,
                        p.muted,
                    );
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut login.code)
                            .password(true)
                            .hint_text("Код входа")
                            .desired_width(f32::INFINITY),
                    );
                    let enter =
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let ready = !login.code.trim().is_empty() && login.state != "verifying";
                    let send = ui
                        .add_enabled(ready, egui::Button::new("Подтвердить вход"))
                        .clicked()
                        || ready && enter;
                    if send {
                        self.controller.send(
                            &login.server,
                            json!({"op":"auth_input","id":login.id,"code":login.code.trim()}),
                        );
                        login.code.zeroize();
                        login.code.clear();
                        login.state = "verifying".into();
                        login.message = "Claude Code проверяет код…".into();
                    }
                }
                ui.add_space(16.);
                label(
                    ui,
                    "Вход относится к выбранному серверу. История сессий сохраняется.",
                    12.,
                    p.muted,
                );
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    if !login.active()
                        && login.state != "success"
                        && ui.button("Повторить").clicked()
                    {
                        retry = true;
                    }
                    if ui
                        .button(if login.state == "success" {
                            "Продолжить"
                        } else {
                            "Закрыть"
                        })
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
        if !open || close || retry {
            if login.active() {
                self.controller
                    .send(&login.server, json!({"op":"auth_cancel","id":login.id}));
            }
            if retry {
                if let Some(server) = self
                    .settings
                    .servers
                    .iter()
                    .find(|s| s.id == login.server)
                    .cloned()
                {
                    self.start_login(&server);
                }
            }
        } else {
            self.login = Some(login);
        }
    }
}
