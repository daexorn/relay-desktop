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
    standalone: bool,
    address: String,
    address_focused: bool,
    tabs: Vec<Value>,
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
        self.open_server_browser(server, true);
    }

    fn open_server_browser(&mut self, server: &Server, standalone: bool) {
        if self
            .login
            .as_ref()
            .is_some_and(|l| l.server == server.id && l.standalone == standalone && l.active())
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
            standalone,
            address: "https://claude.ai/".into(),
            address_focused: false,
            tabs: Vec::new(),
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
                self.controller.send(
                    server,
                    json!({"op":"auth_browser_ack","id":login.id,"frame":value["frame"]}),
                );
            }
            "auth_browser" => {
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
                if login.active() {
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

    pub(super) fn login_dialog(&mut self, ctx: &egui::Context) {
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
        let title = server.as_ref().map_or("Сервер", |s| s.name.as_str());
        let mut open = true;
        let mut retry = false;
        let mut close = false;
        let browser_width=(ctx.screen_rect().width()-32.).max(320.);
        egui::Window::new(format!("Браузер сервера · {title}"))
            .id(egui::Id::new("claude-login")).open(&mut open).collapsible(false).resizable(true)
            .default_pos(egui::pos2(16.,16.)).default_width(browser_width.min(1100.)).min_width(320.).max_width(browser_width).show(ctx,|ui|{
                ui.horizontal_wrapped(|ui|{
                    label(ui,format!("● {title} · на сервере"),13.,GREEN);
                    if !login.origin.is_empty(){label(ui,&login.origin,12.,p.muted);}
                    if login.texture.is_some(){
                        for (text,kind) in [("Назад","back"),("Вперёд","forward"),("Обновить","reload"),("Копировать выделение","copy")]{
                            if ui.small_button(text).clicked(){self.controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":{"kind":kind}}));}
                        }
                    }
                });
                if login.standalone && login.active(){
                    if !login.tabs.is_empty(){
                        ui.horizontal_wrapped(|ui|{
                            for tab in &login.tabs {
                                let title=tab["title"].as_str().filter(|s|!s.is_empty()).unwrap_or("Новая вкладка");
                                let short:String=title.chars().take(20).collect();
                                if ui.selectable_label(tab["active"]==true,short).on_hover_text(title).clicked(){
                                    self.controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":{"kind":"switch_tab","target":tab["id"]}}));
                                }
                                if ui.small_button("×").on_hover_text("Закрыть вкладку").clicked(){
                                    self.controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":{"kind":"close_tab","target":tab["id"]}}));
                                }
                            }
                            if ui.small_button("+").on_hover_text("Новая вкладка").clicked(){
                                self.controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":{"kind":"new_tab"}}));
                            }
                        });
                    }
                    ui.horizontal(|ui|{
                        let address_id=egui::Id::new(("browser-address",&login.id));
                        if ui.input_mut(|i|i.consume_key(egui::Modifiers::COMMAND,egui::Key::L)){ui.memory_mut(|m|m.request_focus(address_id));}
                        let address=ui.add(egui::TextEdit::singleline(&mut login.address).id(address_id).hint_text("Адрес сайта").desired_width((ui.available_width()-80.).max(140.)));
                        login.address_focused=address.has_focus();
                        if ui.button("Перейти").clicked() || address.lost_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter)){
                            self.controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":{"kind":"navigate","url":login.address.trim()}}));
                        }
                    });
                }
                ui.add_space(6.);label(ui,&login.message,12.,p.muted);
                if !login.error.is_empty(){label(ui,&login.error,12.,p.text);}
                if let Some(texture)=login.texture.as_ref(){
                    let max_height=(ctx.screen_rect().height()-if login.standalone{280.}else{235.}).max(180.);
                    let width=ui.available_width().min(max_height*login.size[0] as f32/login.size[1] as f32);
                    let height=width*login.size[1] as f32/login.size[0] as f32;
                    let response=ui.add(egui::Image::new(texture).fit_to_exact_size(Vec2::new(width,height)).sense(egui::Sense::click_and_drag()));
                    if response.clicked() || response.drag_started(){response.request_focus();}
                    let events=ui.input(|i|i.events.clone());let has_paste=events.iter().any(|e|matches!(e,egui::Event::Paste(_)));
                    let mut pointer=None;
                    for event in events {
                        let mut input=None;
                        match event {
                            egui::Event::PointerMoved(pos) if response.hovered() || response.dragged()=>{
                                if let Some((x,y))=auth_browser::point(response.rect,pos,login.size[0],login.size[1]){
                                    let button=if ui.input(|i|i.pointer.primary_down()){"left"}else{"none"};pointer=Some(json!({"kind":"move","x":x,"y":y,"button":button}));
                                }
                            }
                            egui::Event::PointerButton{pos,button,pressed,..} if response.hovered() || login.button.is_some()=>{
                                let pos=if !pressed && login.button.is_some(){response.rect.clamp(pos)}else{pos};
                                if let Some((x,y))=auth_browser::point(response.rect,pos,login.size[0],login.size[1]){
                                    let name=match button{egui::PointerButton::Primary=>"left",egui::PointerButton::Secondary=>"right",egui::PointerButton::Middle=>"middle",_=>"none"};
                                    if pressed{response.request_focus();login.button=Some(name);}else{login.button=None;}
                                    input=Some(json!({"kind":if pressed{"down"}else{"up"},"x":x,"y":y,"button":name}));
                                }
                            }
                            egui::Event::MouseWheel{delta,unit,..} if response.hovered()=>{
                                if let Some(pos)=ui.input(|i|i.pointer.hover_pos()){
                                    if let Some((x,y))=auth_browser::point(response.rect,pos,login.size[0],login.size[1]){
                                        let factor=match unit{egui::MouseWheelUnit::Point=>1.,egui::MouseWheelUnit::Line=>40.,egui::MouseWheelUnit::Page=>login.size[1] as f32};
                                        input=Some(json!({"kind":"wheel","x":x,"y":y,"deltaX":-delta.x*factor,"deltaY":-delta.y*factor}));
                                    }
                                }
                            }
                            egui::Event::Copy if response.has_focus()=>input=Some(json!({"kind":"copy"})),
                            egui::Event::Text(text)|egui::Event::Paste(text) if response.has_focus()=>{if !text.is_empty(){input=Some(json!({"kind":"text","text":text}));}}
                            egui::Event::Key{key,pressed,modifiers,..} if response.has_focus()=>{
                                if !(has_paste && key==egui::Key::V && modifiers.command){
                                    if let Some(key)=auth_browser::key(key,modifiers.command){input=Some(json!({"kind":"key","key":key,"pressed":pressed,"modifiers":auth_browser::modifiers(modifiers)}));}
                                }
                            }
                            _=>{}
                        }
                        if let Some(input)=input{self.controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":input}));}
                    }
                    if let Some(input)=pointer{self.controller.send(&login.server,json!({"op":"auth_browser_input","id":login.id,"input":input}));}
                }else if login.active() || login.preparing {
                    ui.add_space(30.);ui.spinner();label(ui,"Ждём изображение с сервера…",13.,p.muted);ui.add_space(30.);
                }
                if login.info["ready"]==false && login.info["can_install"]==true && !login.preparing {
                    ui.add_space(10.);
                    if ui.button("Подготовить браузер на сервере").clicked(){
                        login.preparing=true;self.controller.send(&login.server,json!({"op":"browser_setup","id":login.id}));
                    }
                }
                if login.active() && !login.standalone{
                    egui::CollapsingHeader::new("Код подтверждения, если его показал Claude").show(ui,|ui|{
                        ui.horizontal(|ui|{
                            let response=ui.add(egui::TextEdit::singleline(&mut login.code).password(true).hint_text("Код с экрана сервера"));
                            let ready=!login.code.trim().is_empty();
                            if ready && (ui.button("Подтвердить").clicked() || response.lost_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter))){
                                self.controller.send(&login.server,json!({"op":"auth_input","id":login.id,"code":login.code.trim()}));login.code.zeroize();login.code.clear();
                            }
                        });
                    });
                }
                ui.add_space(8.);ui.horizontal(|ui|{
                    if !login.active() && !login.preparing && login.state!="success" && ui.button(if login.standalone{"Открыть браузер"}else{"Открыть вход"}).clicked(){retry=true;}
                    if ui.button(if login.state=="success"{"Продолжить"}else{"Закрыть"}).clicked(){close=true;}
                });
            });
        if !open || close || retry {
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
        }
    }
}
