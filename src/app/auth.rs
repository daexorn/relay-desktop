use super::*;
use crate::auth_browser;

pub(super) struct Login {
    pub(super) server: String,
    pub(super) id: String,
    state: String,
    message: String,
    code: String,
    texture: Option<egui::TextureHandle>,
    size: [u32; 2],
    frame: u64,
    origin: String,
    error: String,
    info: Value,
    preparing: bool,
    button: Option<&'static str>,
    pointer: [f32; 2],
    standalone: bool,
    address: String,
    address_focused: bool,
    tabs: Vec<Value>,
    native: bool,
    remote_modifiers: u8,
    visible: bool,
    pub(super) expanded: bool,
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
        self.open_server_browser(server, false);
    }

    pub(super) fn start_browser(&mut self, server: &Server) {
        if let Some(login) = self
            .login
            .as_mut()
            .filter(|l| l.server == server.id && l.active())
        {
            self.cluster_open = false;
            self.board_open = false;
            login.visible = true;
            if login.frame > 0 {
                self.controller.send(
                    &login.server,
                    json!({"op":"auth_browser_ack","id":login.id,"frame":login.frame}),
                );
            }
            return;
        }
        self.open_server_browser(server, true);
    }

    fn open_server_browser(&mut self, server: &Server, standalone: bool) {
        self.cluster_open = false;
        self.board_open = false;
        if let Some(login) = self
            .login
            .as_mut()
            .filter(|l| l.server == server.id && l.standalone == standalone && l.active())
        {
            login.visible = true;
            if login.frame > 0 {
                self.controller.send(
                    &login.server,
                    json!({"op":"auth_browser_ack","id":login.id,"frame":login.frame}),
                );
            }
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
                "Открываем браузер на сервере…"
            } else {
                "Переподключись к серверу, чтобы загрузить трансляцию браузера."
            }
            .into(),
            code: String::new(),
            texture: None,
            size: [1200, 800],
            frame: 0,
            origin: String::new(),
            error: String::new(),
            info: Value::Null,
            preparing: false,
            button: None,
            pointer: [0., 0.],
            standalone,
            address: "https://claude.ai/".into(),
            address_focused: false,
            tabs: Vec::new(),
            native: false,
            remote_modifiers: 0,
            visible: true,
            expanded: self.controller.bus.ctx.screen_rect().width() < 1100.,
        });
        if available {
            self.controller
                .send(&server.id, json!({"op":"browser_info","id":id}));
            self.controller.send(
                &server.id,
                json!({"op":if standalone {"browser_open"} else {"auth_start"},"id":id}),
            );
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
        match value["type"].as_str().unwrap_or("") {
            "auth_frame" => {
                if !login.active() {
                    return;
                }
                match auth_browser::Frame::decode(value) {
                    Ok(frame) if frame.sequence > login.frame => {
                        login.size = [frame.width, frame.height];
                        login.frame = frame.sequence;
                        if let Some(texture) = login.texture.as_mut() {
                            texture.set(frame.image, egui::TextureOptions::LINEAR);
                        } else {
                            login.texture = Some(self.controller.bus.ctx.load_texture(
                                format!("server-browser-{}", login.id),
                                frame.image,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                    }
                    Ok(_) => {}
                    Err(_) => login.error = "Не удалось прочитать кадр серверного браузера.".into(),
                }
                if login.visible {
                    self.controller.send(
                        server,
                        json!({"op":"auth_browser_ack","id":login.id,"frame":value["frame"]}),
                    );
                }
            }
            "auth_browser" => {
                login.native = value["native"] == true;
                login.origin = value["origin"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(180)
                    .collect();
                if value["connected"] == false {
                    login.error = "Браузер сервера закрылся. Открой его ещё раз.".into();
                    login.state = "failed".into();
                }
            }
            "browser_navigation" if login.standalone && login.active() => {
                if !login.address_focused {
                    let address = value["url"].as_str().unwrap_or("");
                    login.address = if address == "about:blank" {
                        String::new()
                    } else {
                        address.chars().take(8192).collect()
                    };
                }
                login.tabs = value["tabs"]
                    .as_array()
                    .map(|tabs| tabs.iter().take(16).cloned().collect())
                    .unwrap_or_default();
            }
            "auth_clipboard" => {
                if login.active() && login.visible {
                    if let Some(text) = value["text"].as_str().filter(|v| v.len() <= 65536) {
                        self.controller.bus.ctx.copy_text(text.to_owned());
                    }
                }
            }
            "browser_info" => login.info = value["info"].clone(),
            "browser_setup" => {
                login.preparing = value["running"] == true;
                login.info = value["info"].clone();
                login.message = value["message"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(500)
                    .collect();
                if !login.preparing {
                    login.state = "failed".into();
                }
            }
            "auth" => {
                let state = value["state"].as_str().unwrap_or("failed");
                if login.state == "success" || login.state == "cancelled" {
                    return;
                }
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
                login.state = if state == "input_error" {
                    "browser"
                } else {
                    state
                }
                .into();
                login.message = value["message"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(500)
                    .collect();
                if !login.active() {
                    login.code.zeroize();
                    login.texture = None;
                }
                if state == "success" {
                    self.controller.send(server, json!({"op":"usage"}));
                }
            }
            _ => {}
        }
    }

    pub(super) fn login_disconnected(&mut self, server: &str) {
        if let Some(login) = self
            .login
            .as_mut()
            .filter(|l| l.server == server && l.active())
        {
            login.texture = None;
            login.code.zeroize();
            login.state = "failed".into();
            login.message =
                "Связь с сервером прервалась. Переподключись и открой браузер ещё раз.".into();
        }
    }

    pub(super) fn browser_visible(&self) -> bool {
        self.login.as_ref().is_some_and(|l| l.visible)
    }
    pub(super) fn browser_expanded(&self) -> bool {
        self.login.as_ref().is_some_and(|l| l.visible && l.expanded)
    }
    pub(super) fn hide_browser(&mut self) {
        if let Some(login) = self.login.as_mut() {
            login.visible = false;
            release_browser_input(&self.controller, login);
        }
    }

    pub(super) fn browser_panel(&mut self, ctx: &egui::Context) {
        let Some(mut login) = self.login.take() else {
            return;
        };
        let p = design::animated_palette(ctx, self.settings.light);
        let server = self
            .settings
            .servers
            .iter()
            .find(|s| s.id == login.server)
            .cloned();
        let name = server.as_ref().map(|s| s.name.as_str()).unwrap_or("Сервер");
        let mut close = false;
        let mut hide = false;
        let mut retry = false;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.base)).show(ctx,|ui|{
            egui::TopBottomPanel::top("browser-header").show_separator_line(false)
                .frame(egui::Frame::new().fill(p.side).inner_margin(egui::Margin::symmetric(20,14))).show_inside(ui,|ui|{
                let narrow=ui.available_width()<440.;
                ui.horizontal(|ui|{
                    let title_width=(ui.available_width()-50.-if narrow{112.}else{178.}).max(70.);
                    let (r,_)=ui.allocate_exact_size(Vec2::splat(34.),egui::Sense::hover());
                    ui.painter().rect_filled(r,9.,design::mix(p.surface,accent(),0.13));
                    design::icon(ui.painter(),r.shrink(7.),Icon::Browser,accent());
                    ui.allocate_ui_with_layout(Vec2::new(title_width,40.),Layout::top_down(Align::Min),|ui|{
                        ui.set_width(title_width);
                        label(ui,if login.standalone{"Браузер"}else{"Вход в Claude"},17.,p.text);
                        label(ui,model::shorten(name,if narrow{12}else{26}),11.5,p.muted);
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center),|ui|{
                        if design::button(ui,"",Some(Icon::Close),Kind::Ghost,p,Vec2::splat(32.)).on_hover_text("Закрыть браузер").clicked(){close=true;}
                        if design::button(ui,"",Some(if login.expanded{Icon::Collapse}else{Icon::Expand}),Kind::Ghost,p,Vec2::splat(32.)).on_hover_text(if login.expanded{"Вернуть боковую панель"}else{"Развернуть рабочую область"}).clicked(){login.expanded=!login.expanded;}
                        if design::button(ui,if narrow{""}else{"Сессии"},Some(Icon::Chat),Kind::Secondary,p,Vec2::new(if narrow{32.}else{98.},32.)).on_hover_text("Вернуться в чат и оставить браузер открытым").clicked(){hide=true;}
                    });
                });
                if login.texture.is_some() && login.active(){
                    ui.add_space(10.);
                    ui.horizontal(|ui|{
                        for (which,kind,hint) in [(Icon::Back,"back","Назад"),(Icon::Forward,"forward","Вперёд"),(Icon::Refresh,"reload","Обновить страницу"),(Icon::Plus,"new_tab","Новая вкладка"),(Icon::Copy,"copy","Копировать выделение")]{
                            if design::button(ui,"",Some(which),Kind::Ghost,p,Vec2::splat(30.)).on_hover_text(hint).clicked(){self.controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":{"kind":kind}}));}
                        }
                        if ui.available_width()>94.{ui.with_layout(Layout::right_to_left(Align::Center),|ui|{design::badge(ui,"На сервере",GREEN,p);});}
                    });
                }
            });
            egui::TopBottomPanel::bottom("browser-footer").show_separator_line(false)
                .frame(egui::Frame::new().fill(p.base).inner_margin(egui::Margin::symmetric(20,12))).show_inside(ui,|ui|{
                if !login.error.is_empty(){label(ui,&login.error,12.,design::RED);}
                if login.info["ready"]==false && login.info["can_install"]==true && !login.preparing {
                    if design::button(ui,"Подготовить браузер",Some(Icon::Browser),Kind::Primary,p,Vec2::ZERO).clicked(){login.preparing=true;self.controller.send(&login.server,json!({"op":"browser_setup","id":login.id}));}
                }
                if login.active() && !login.standalone {
                    egui::CollapsingHeader::new("Ввести код подтверждения").show(ui,|ui|{
                        ui.horizontal(|ui|{
                            let width=(ui.available_width()-132.).max(80.);
                            let response=ui.allocate_ui_with_layout(Vec2::new(width,36.),Layout::top_down(Align::Min),|ui|design::field(ui,&mut login.code,"Код из браузера",true)).inner;
                            let submit=design::button(ui,"Подтвердить",Some(Icon::Check),Kind::Primary,p,Vec2::new(120.,36.)).clicked() || response.lost_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter));
                            if submit && !login.code.trim().is_empty(){
                                self.controller.send(&login.server,json!({"op":"auth_input","id":login.id,"code":login.code.trim()}));login.code.zeroize();login.code.clear();
                            }
                        });
                    });
                }
                if login.active(){label(ui,if login.standalone{"Адресная строка и вкладки работают в окне браузера"}else{&login.message},11.5,p.muted);}
            });
            ui.add_space(14.);
            if login.texture.is_some() && login.active(){
                self.browser_canvas(ui,&mut login,ctx,p);
            }else{
                let available=ui.available_size();
                ui.allocate_ui_with_layout(available,Layout::top_down(Align::Center),|ui|{
                    ui.add_space((available.y*0.26).max(16.));
                    let success=login.state=="success";
                    let (r,_)=ui.allocate_exact_size(Vec2::splat(54.),egui::Sense::hover());
                    ui.painter().rect_filled(r,16.,p.surface);
                    design::icon(ui.painter(),r.shrink(13.),if success{Icon::Check}else{Icon::Browser},if success{GREEN}else{accent()});
                    ui.add_space(16.);
                    label(ui,if success{"Вход выполнен"}else if login.active() || login.preparing{"Открываем браузер"}else{"Браузер не подключён"},22.,p.text);
                    ui.add_space(8.);ui.label(RichText::new(&login.message).size(13.).color(p.muted));ui.add_space(18.);
                    if login.active() || login.preparing {ui.spinner();}
                    else if design::button(ui,if success{"Продолжить работу"}else{"Попробовать ещё раз"},Some(if success{Icon::Chat}else{Icon::Refresh}),Kind::Primary,p,Vec2::new(200.,38.)).clicked(){if success{close=true;}else{retry=true;}}
                });
            }
        });
        if close || retry {
            if login.active() {
                self.controller
                    .send(&login.server, json!({"op":"auth_cancel","id":login.id}));
            }
            if retry {
                if let Some(server) = server {
                    self.open_server_browser(&server, login.standalone);
                }
            }
        } else {
            self.login = Some(login);
            if hide {
                self.hide_browser();
            }
        }
    }

    fn browser_canvas(
        &self,
        ui: &mut egui::Ui,
        login: &mut Login,
        _ctx: &egui::Context,
        p: Palette,
    ) {
        let Some(texture) = login.texture.as_ref() else {
            return;
        };
        let available = ui.available_rect_before_wrap().shrink2(Vec2::new(16., 8.));
        let ratio = login.size[0] as f32 / login.size[1] as f32;
        let width = available.width().min(available.height() * ratio).max(1.);
        let size = Vec2::new(width, width / ratio);
        let slot = egui::Rect::from_center_size(available.center(), size);
        ui.painter().rect_filled(slot.expand(1.), 10., p.line);
        let response = ui.put(
            slot,
            egui::Image::new(texture)
                .fit_to_exact_size(size)
                .corner_radius(9.)
                .sense(egui::Sense::click_and_drag()),
        );
        if self.editor.is_some()
            || self.api_editor.is_some()
            || self.settings_open
            || self.folder_open
            || self.cluster_edit.is_some()
            || self.delete_chat.is_some()
            || self.close_confirm
            || !self.desktop_requests.is_empty()
        {
            response.surrender_focus();
            if login.remote_modifiers!=0 || login.button.is_some(){release_browser_input(&self.controller, login);}
            ui.advance_cursor_after_rect(available);
            return;
        }
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        let events = ui.input(|i| i.events.clone());
        let has_paste = events.iter().any(|e| matches!(e, egui::Event::Paste(_)));
        let mut pointer = None;
        for event in events {
            let mut input = None;
            match event {
                egui::Event::PointerMoved(pos) if response.hovered() || response.dragged() => {
                    if let Some((x, y)) =
                        auth_browser::point(response.rect, pos, login.size[0], login.size[1])
                    {
                        login.pointer = [x, y];
                        let button = if ui.input(|i| i.pointer.primary_down()) {
                            "left"
                        } else {
                            "none"
                        };
                        pointer = Some(json!({"kind":"move","x":x,"y":y,"button":button}));
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } if response.hovered() || login.button.is_some() => {
                    let pos = if !pressed && login.button.is_some() {
                        response.rect.clamp(pos)
                    } else {
                        pos
                    };
                    if let Some((x, y)) =
                        auth_browser::point(response.rect, pos, login.size[0], login.size[1])
                    {
                        login.pointer = [x, y];
                        let name = match button {
                            egui::PointerButton::Primary => "left",
                            egui::PointerButton::Secondary => "right",
                            egui::PointerButton::Middle => "middle",
                            _ => "none",
                        };
                        if pressed {
                            response.request_focus();
                            login.button = Some(name);
                        } else {
                            login.button = None;
                        }
                        input = Some(
                            json!({"kind":if pressed{"down"}else{"up"},"x":x,"y":y,"button":name}),
                        );
                    }
                }
                egui::Event::MouseWheel { delta, unit, .. } if response.hovered() => {
                    if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                        if let Some((x, y)) =
                            auth_browser::point(response.rect, pos, login.size[0], login.size[1])
                        {
                            let factor = match unit {
                                egui::MouseWheelUnit::Point => 1.,
                                egui::MouseWheelUnit::Line => 40.,
                                egui::MouseWheelUnit::Page => login.size[1] as f32,
                            };
                            input = Some(
                                json!({"kind":"wheel","x":x,"y":y,"deltaX":-delta.x*factor,"deltaY":-delta.y*factor}),
                            );
                        }
                    }
                }
                egui::Event::Copy if response.has_focus() => input = Some(json!({"kind":"copy"})),
                egui::Event::Text(text) | egui::Event::Paste(text) if response.has_focus() => {
                    if !text.is_empty() {
                        input = Some(json!({"kind":"text","text":text}));
                    }
                }
                egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } if response.has_focus() => {
                    if !(has_paste && key == egui::Key::V && modifiers.command) {
                        if let Some(key) = auth_browser::key(key, modifiers.command) {
                            input = Some(
                                json!({"kind":"key","key":key,"pressed":pressed,"modifiers":auth_browser::modifiers(modifiers)}),
                            );
                        }
                    }
                }
                _ => {}
            }
            if let Some(input) = input {
                self.controller.send(
                    &login.server,
                    json!({"op":"auth_browser_input","id":login.id,"input":input}),
                );
            }
        }
        if let Some(input) = pointer {
            self.controller.send(
                &login.server,
                json!({"op":"auth_browser_input","id":login.id,"input":input}),
            );
        }
        if login.native {
            let modifiers = if response.has_focus() {
                auth_browser::modifiers(ui.input(|i| i.modifiers))
            } else {
                0
            };
            if modifiers != login.remote_modifiers {
                login.remote_modifiers = modifiers;
                self.controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":{"kind":"modifiers","modifiers":modifiers}}));
            }
        }

        ui.advance_cursor_after_rect(available);
    }
}

fn release_browser_input(controller: &Controller, login: &mut Login) {
    if login.native && login.active() {
        controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":{"kind":"modifiers","modifiers":0}}));
        if let Some(button) = login.button.take() {
            controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":{"kind":"up","button":button,"x":login.pointer[0],"y":login.pointer[1]}}));
        }
        login.remote_modifiers = 0;
    }
}
