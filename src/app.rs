mod cluster_runtime;
mod cluster_view;
mod project_icon;
mod session_actions;
mod api_settings;
mod workspace;
mod auth;

use crate::{
    connection::{Controller, Event},
    images::{self, Attachment},
    limits::{self, Usage},
    model::{self, Folder, Message, Permission, Server, Session, Settings},
    storage,
};
use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, Stroke, Vec2};
use egui_commonmark::CommonMarkCache;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::mpsc,
    time::{Duration, Instant},
};
use zeroize::Zeroize;

use crate::ui::{self as design, Icon, Kind, Palette, accent, GREEN};
#[derive(Default)]
struct Remote {
    auth_available: bool,
    connected: bool,
    connecting: bool,
    version: String,
    sessions: Vec<Session>,
    folders: Vec<crate::model::SessionFolder>,
    home: String,
    usage: Usage,
    usage_requested: Option<Instant>,
    /// Account limits of every installed agent for the permanent sidebar block.
    usage_by: HashMap<String, Usage>,
    /// When limits of each agent were last requested (every 5 minutes).
    usage_polled: HashMap<String, Instant>,
    sessions_requested: Option<Instant>,
    sessions_loaded: bool,
    sessions_retries: u8,
    sessions_error: String,
    /// Names, pins and folders change on other computers too; the list is polled.
    sessions_polled: Option<Instant>,
    /// 4 and later: Claude runs in a host process on the server and outlives the app.
    protocol: u64,
    /// Sessions Claude is answering in right now, on any computer.
    running: std::collections::HashSet<String>,
    models: Vec<crate::model::ModelInfo>,
    codex_models: Vec<crate::model::ModelInfo>,
    agy_models: Vec<crate::model::ModelInfo>,
    api_models: Vec<crate::model::ModelInfo>,
    api_tokens: (u64, u64),
    commands: Vec<crate::model::CommandInfo>,
    codex_commands: Vec<crate::model::CommandInfo>,
    agy_commands: Vec<crate::model::CommandInfo>,
    backends: Value,
    installing_agents: HashMap<String, String>,
    agent_logs: HashMap<String, String>,
    /// `claude update` is running on the server.
    updating: bool,
}
#[derive(Default)]
struct TurnSummary {
    backend: String,
    diff: Option<(u64, u64)>,
    context: Option<(u64, u64)>,
}

struct Chat {
    sid: String,
    cwd: String,
    title: String,
    messages: Vec<Message>,
    draft: String,
    images: Vec<Attachment>,
    busy: bool,
    upload_progress: Option<(u64, u64)>,
    loading: bool,
    loaded: bool,
    locked: bool,
    unlocked: bool,
    unlock_input: String,
    unlock_error: String,
    unlocking: bool,
    status: String,
    error: String,
    model: String,
    mode: String,
    effort: String,
    agent_role: String,
    custom_prompt: String,
    cluster_context: Value,
    permissions: Vec<Permission>,
    blocks: HashMap<usize, usize>,
    user_at: usize,
    sent_images: HashMap<usize, Vec<Attachment>>,
    queue: std::collections::VecDeque<QueuedPrompt>,
    queue_paused: bool,
    /// Server `modified` time of the session the last time this chat was in sync with it.
    synced_modified: f64,
    turn_backend: String,
    turn_model: String,
    turn_mode: String,
    turn_effort: String,
    turn_actor: String,
    turn_agent_name: String,
    turn_summary: TurnSummary,
}
struct QueuedPrompt {
    text: String,
    files: Vec<Attachment>,
}
impl Chat {
    fn restore_failed_upload(&mut self) {
        if self.upload_progress.take().is_none() {
            return;
        }
        if self.user_at + 1 == self.messages.len() && self.messages[self.user_at].role == "user" {
            let text = self.messages.pop().unwrap().text;
            let files = self.sent_images.remove(&self.user_at).unwrap_or_default();
            if self.draft.is_empty() && self.images.is_empty() {
                self.draft = text;
                self.images = files;
            } else {
                self.queue.push_front(QueuedPrompt { text, files });
            }
        }
        self.busy = false;
        self.queue_paused = true;
        self.status.clear();
    }
    fn enqueue_draft(&mut self) -> bool {
        if self.draft.trim().is_empty() && self.images.is_empty() {
            return false;
        }
        if self.queue.len() >= 12 {
            self.error = "В очереди уже 12 сообщений.".into();
            return false;
        }
        self.queue.push_back(QueuedPrompt {
            text: std::mem::take(&mut self.draft),
            files: std::mem::take(&mut self.images),
        });
        true
    }
    fn queued_ready(&self) -> bool {
        !self.busy && !self.loading && !self.queue_paused && !self.queue.is_empty()
    }
    fn new(sid: String, cwd: String, title: String) -> Self {
        Self {
            sid,
            cwd,
            title,
            messages: vec![],
            draft: String::new(),
            images: vec![],
            busy: false,
            upload_progress: None,
            loading: false,
            loaded: false,
            locked: false,
            unlocked: true,
            unlock_input: String::new(),
            unlock_error: String::new(),
            unlocking: false,
            status: String::new(),
            error: String::new(),
            model: "default".into(),
            mode: "auto".into(),
            effort: "default".into(),
            agent_role: String::new(),
            custom_prompt: String::new(),
            cluster_context: Value::Null,
            permissions: vec![],
            blocks: HashMap::new(),
            user_at: 0,
            sent_images: HashMap::new(),
            queue: Default::default(),
            queue_paused: false,
            synced_modified: 0.,
            turn_backend: String::new(),
            turn_model: String::new(),
            turn_mode: String::new(),
            turn_effort: String::new(),
            turn_actor: String::new(),
            turn_agent_name: String::new(),
            turn_summary: TurnSummary::default(),
        }
    }
}
struct Editor {
    server: Server,
    password: String,
    is_new: bool,
    error: String,
}
struct Trust {
    server: String,
    fingerprint: String,
    answer: tokio::sync::oneshot::Sender<bool>,
}
struct FolderPassword {
    server: String,
    folder: String,
    name: String,
    action: &'static str,
    password: String,
    confirm: String,
}

impl Drop for FolderPassword {
    fn drop(&mut self) {
        self.password.zeroize();
        self.confirm.zeroize();
    }
}

pub struct RelayApp {
    folder_password: Option<FolderPassword>,
    phone_pairing: Option<(String, zeroize::Zeroizing<String>)>,
    /// Drafts and queued messages from the last run, not yet put back into a chat.
    pending: crate::model::PendingStore,
    pending_saved: crate::model::PendingStore,
    pending_checked: Option<Instant>,
    #[cfg(test)]
    test_no_save: bool,
    settings: Settings,
    controller: Controller,
    events: mpsc::Receiver<Event>,
    remote: HashMap<String, Remote>,
    chats: HashMap<String, Chat>,
    selected_chat: HashMap<String, String>,
    delete_chat: Option<session_actions::DeleteChat>,
    lock_dialog: Option<session_actions::LockDialog>,
    agents_open: Option<String>,
    radar_open: Option<String>,
    deleted_sessions: std::collections::HashSet<String>,
    /// Servers whose connection dropped: next automatic attempt and how many were made.
    reconnect: HashMap<String, (Instant, u32)>,
    session_unlock_attempts: std::collections::HashSet<(String,String)>,
    login_passwords: HashMap<String, zeroize::Zeroizing<String>>,
    verified_session_keys: HashMap<String, zeroize::Zeroizing<String>>,
    pending_session_keys: HashMap<String, zeroize::Zeroizing<String>>,
    /// Session being renamed in the sidebar, with the text typed so far.
    renaming: Option<(String, String)>,
    folder_dialog: Option<session_actions::FolderDialog>,
    icon_dialog: Option<project_icon::IconDialog>,
    search: String,
    editor: Option<Editor>,
    api_editor: Option<api_settings::Editor>,
    login: Option<auth::Login>,
    trust: Option<Trust>,
    folder_open: bool,
    folder_path: String,
    folder_parent: String,
    folders: Vec<Folder>,
    folder_hidden: bool,
    folder_request: u64,
    folder_confirmed: String,
    answers: HashMap<String, String>,
    notice: String,
    markdown: CommonMarkCache,
    demo: bool,
    screenshot: Option<String>,
    frames: u32,
    capture_at: f64,
    capture_requested: bool,
    close_confirm: bool,
    desktop_requests: Vec<(String, Value)>,
    /// Latest status of each in-game test, by request id.
    game_tests: Vec<(String, Value)>,
    board_open: bool,
    settings_open: bool,
    cluster_open: bool,
    cluster_prompt: String,
    cluster_target: String,
    cluster_jobs: Vec<(String, String, String)>,
    cluster_store: crate::cluster::Store,
    cluster_calls: Vec<cluster_runtime::Pending>,
    cluster_edit: Option<cluster_view::Editor>,
    chat_orchestra_dialog: Option<cluster_view::ChatOrchestraDialog>,
    cluster_snapshot: Option<(u64, String, std::sync::Arc<crate::cluster::Run>)>,
    board_view: crate::board::BoardView,
    boards: HashMap<String, Value>,
    board_polled: Option<Instant>,
    downloads: crate::artifacts::Downloads,
}
fn key(server: &str, sid: &str) -> String {
    format!("{server}/{sid}")
}
fn label(ui: &mut egui::Ui, s: impl Into<String>, size: f32, color: Color32) -> egui::Response {
    ui.label(RichText::new(s).size(size).color(color))
}
impl RelayApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (settings, error) = match storage::load() {
            Ok(s) => (s, String::new()),
            Err(e) => (Settings::default(), e.to_string()),
        };
        let (controller, events) = Controller::new(cc.egui_ctx.clone());
        let args: Vec<_> = std::env::args().collect();
        let demo = args.iter().any(|a| a == "--demo");
        let screenshot = args
            .iter()
            .position(|a| a == "--screenshot")
            .and_then(|i| args.get(i + 1))
            .cloned();
        let mut app = Self {
            #[cfg(test)]
            test_no_save: false,
            settings,
            controller,
            events,
            remote: HashMap::new(),
            chats: HashMap::new(),
            selected_chat: HashMap::new(),
            delete_chat: None,
            folder_password: None,
            phone_pairing: None,
            lock_dialog: None,
            agents_open: None,
            radar_open: None,
            deleted_sessions: Default::default(),
            reconnect: Default::default(),
            session_unlock_attempts: Default::default(),
            login_passwords: Default::default(),
            verified_session_keys: Default::default(),
            pending_session_keys: Default::default(),
            renaming: None,
            folder_dialog: None,
            icon_dialog: None,
            search: String::new(),
            editor: None,
            api_editor: None,
            login: None,
            trust: None,
            folder_open: false,
            folder_path: String::new(),
            folder_parent: String::new(),
            folders: vec![],
            folder_hidden: false,
            folder_request: 0,
            folder_confirmed: String::new(),
            answers: HashMap::new(),
            notice: error,
            markdown: CommonMarkCache::default(),
            demo,
            screenshot,
            frames: 0,
            capture_at: 0.0,
            capture_requested: false,
            close_confirm: false,
            pending: if demo { Default::default() } else { storage::load_private_json("pending-messages.json") },
            pending_saved: Default::default(),
            pending_checked: None,
            desktop_requests: vec![],
            game_tests: vec![],
            board_open: false,
            settings_open: false,
            cluster_open: false,
            cluster_prompt: String::new(),
            cluster_target: String::new(),
            cluster_jobs: vec![],
            cluster_store: Default::default(),
            cluster_calls: vec![],
            cluster_edit: None,
            chat_orchestra_dialog: None,
            cluster_snapshot: None,
            board_view: Default::default(),
            boards: HashMap::new(),
            board_polled: None,
            downloads: Default::default(),
        };
        if !demo {
            app.load_cluster_sessions();
        }
        if demo {
            app.settings = Settings::default();
            app.notice.clear();
            app.seed_demo();
            if let Some(at)=args.iter().position(|a|a=="--demo-browser-frame") {
                if let Some(path)=args.get(at+1){
                    if let Ok(bytes)=std::fs::read(path){
                        if bytes.len()<=crate::auth_browser::MAX_FRAME {
                            use base64::Engine;
                            let server=app.settings.servers[0].clone();app.start_browser(&server);
                            let id=app.login.as_ref().unwrap().id.clone();
                            app.login_event(&server.id,&json!({"type":"auth","id":id,"state":"browser","message":"Браузер работает на сервере. Управляй изображением ниже."}));
                            app.login_event(&server.id,&json!({"type":"browser_navigation","id":id,"url":"https://example.com/","tabs":[{"id":"preview","title":"Проверка браузера сервера","active":true}]}));
                            let (width,height)=image::load_from_memory(&bytes).map(|image|(image.width(),image.height())).unwrap_or((1200,800));
                            app.login_event(&server.id,&json!({"type":"auth_browser","id":id,"connected":true,"native":true}));
                            app.login_event(&server.id,&json!({"type":"auth_frame","id":id,"frame":1,"width":width,"height":height,"data":base64::engine::general_purpose::STANDARD.encode(bytes)}));
                            if args.iter().any(|a|a=="--demo-browser-expanded"){app.login.as_mut().unwrap().expanded=true;}
                            if args.iter().any(|a|a=="--demo-browser-docked"){app.login.as_mut().unwrap().expanded=false;}
                        }
                    }
                }
            }
            if args.iter().any(|a| a == "--demo-api" || a == "--demo-api-settings") {
                let profile = &mut app.settings.servers[0];
                profile.api.enabled = true;profile.api.base_url = "https://tooken.club/v1".into();
                profile.api.model = "claude-sonnet-5-5".into();
                profile.api.models = vec!["claude-sonnet-5-5".into(), "claude-opus-5-5".into(), "gpt-6.1-sol".into()];
                if args.iter().any(|a| a == "--demo-api-settings") {
                    let server = profile.clone();app.open_api_settings(&server);
                }
            }
            if args.iter().any(|a| a == "--demo-connect") {
                app.editor = Some(Editor {
                    server: Server {
                        name: "Рабочий сервер".into(),
                        host: "192.168.1.100".into(),
                        cwd: "/opt/projects".into(),
                        ..Default::default()
                    },
                    password: String::new(),
                    is_new: true,
                    error: String::new(),
                });
            }
            if args.iter().any(|a| a == "--demo-empty") {
                app.new_chat();
            }
            if args
                .iter()
                .any(|a| a == "--demo-activity" || a == "--demo-activity-open")
            {
                app.settings.servers[0].local_control = true;
                app.settings.servers[0].local_workspace = Some("/home/player".into());
                if let Some(k) = app.current_key() {
                    let chat = app.chats.get_mut(&k).unwrap();
                    chat.messages = vec![Message {
                        role: "assistant".into(),
                        text: "Проверяю каждый слайд визуально.".into(),
                        ..Default::default()
                    }];
                    for slide in 1..=12 {
                        chat.messages.push(Message{role:"tool".into(),tool:"Read".into(),tool_id:format!("slide-{slide}"),input:json!({"file_path":format!("/tmp/presentation/slides/s-{slide:02}.jpg")}),..Default::default()});
                        chat.messages.push(Message {
                            role: "tool_result".into(),
                            tool_id: format!("slide-{slide}"),
                            text: "Изображение открыто.".into(),
                            ..Default::default()
                        });
                    }
                    chat.messages.push(Message{role:"assistant".into(),text:"Проверил 12 слайдов. Текст читается, таблицы помещаются, номера на месте.".into(),..Default::default()});
                    if args.iter().any(|a| a == "--demo-activity-open") {
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            &cc.egui_ctx,
                            egui::Id::new(("activity-group", k.as_str(), 1usize)),
                            true,
                        )
                        .store(&cc.egui_ctx);
                    }
                }
            }
            if args.iter().any(|a| a == "--demo-commands") {
                if let Some(k) = app.current_key() {
                    let chat = app.chats.get_mut(&k).unwrap();
                    let tool = |id: &str, name: &str, input: Value| Message { role: "tool".into(), tool: name.into(), tool_id: id.into(), input, ..Default::default() };
                    let done = |id: &str, text: &str, failed: bool| Message { role: "tool_result".into(), tool_id: id.into(), text: text.into(), failed, ..Default::default() };
                    chat.messages = vec![
                        Message { role: "user".into(), text: "Собери мод и проверь тесты".into(), ..Default::default() },
                        Message { role: "assistant".into(), text: "Сначала найду, где считается урон, потом соберу.".into(), ..Default::default() },
                        tool("c1", "Grep", json!({"pattern":"fn damage","path":"src/"})),
                        done("c1", "src/combat.rs:12:pub fn damage(base: f32, crit: f32) -> f32 {\nsrc/combat.rs:40:    damage -= armor;", false),
                        tool("c_sub", "invoke_subagent", json!({"Role":"Combat Verifier","Prompt":"Проверь краевые случаи формулы брони и урона в тестах"})),
                        done("c_sub", "Проверено 6 сценариев: отрицательный урон при высокой броне устранён.", false),
                        tool("c2", "Edit", json!({"file_path":"/opt/sbrp-client/src/combat.rs","old_string":"    damage -= armor;","new_string":"    damage = (damage - armor).max(0.0);"})),
                        done("c2", "The file /opt/sbrp-client/src/combat.rs has been updated.", false),
                        tool("c5", "Bash", json!({"command":"cd /root/relay-desktop && sed -n \"$(grep -n 'fn send_prompt' src/app.rs | cut -d: -f1),+30p\" src/app.rs; grep -n \"send_prompt\\|fn submit\\|send = true\" src/app.rs src/app/*.rs | head -40 && echo done-with-a-very-long-command-line-that-never-ends"})),
                        done("c5", "src/app.rs:2737:                                send = true;\n 472309  464349 Tue Nov 10 09:40:52 2026 /usr/local/bin/claude -p --output-format stream-json --input-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio --permission-mode auto --session-id=0e33b7a5", false),
                        tool("c3", "Bash", json!({"command":"./gradlew build","description":"Собрать мод"})),
                        done("c3", "> Task :compileJava\n> Task :processResources\n> Task :jar\n\nBUILD SUCCESSFUL in 41s\n7 actionable tasks: 7 executed", false),
                        tool("c4", "Bash", json!({"command":"./gradlew test","description":"Прогнать тесты"})),
                        done("c4", "> Task :test FAILED\n\nCombatTest > armorNeverHeals() FAILED\n    expected: <0.0> but was: <-3.0>\n\n3 tests completed, 1 failed", true),
                        Message { role: "assistant".into(), text: "Один тест упал: броня делала урон отрицательным. Исправил. Вот очень длинная строка текста, которая должна переноситься по ширине колонки чата, а не уходить за край окна, сколько бы слов в ней ни было, и ещё немного слов для проверки переноса.\n\n```\nssh -L 8090:127.0.0.1:8090 root@2.27.202.166\n```\n\nЧто ещё важно:\n\n1. Нет подкачки вообще (ни swap, ни zram). Игре выдано 4 ГБ, остальное съедают Firefox, Telegram и Discord, поэтому выполните сами:\n   ```\n   sudo xbps-install -S zramen\n   sudo ln -s /etc/sv/zramen /var/service/\n   ```\n2. Трава в настройках мода тяжеловата для встроенной видеокарты: радиус 96 и детализация 16.\n\n```\n472309  464349 Tue Nov 10 09:40:52 2026 /usr/local/bin/claude -p --output-format stream-json --input-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio\n```\n\n| Колонка | Очень длинное значение |\n|---|---|\n| a | /root/.cache/relay-desktop/host/f43d31ba77ddc39e.py--relay-host/root/.cache/relay-desktop/host/f43d31ba77ddc39e.sock |".into(), ..Default::default() },
                    ];
                    egui::collapsing_header::CollapsingState::load_with_default_open(&cc.egui_ctx, egui::Id::new(("activity-group", k.as_str(), 2usize)), true).store(&cc.egui_ctx);
                }
            }
            if args.iter().any(|a| a == "--demo-artifact") {
                if let Some(k) = app.current_key() {
                    app.chats.get_mut(&k).unwrap().messages=vec![Message{role:"assistant".into(),text:"Собрал мод. Готовый файл:\n\n[sbrp-client-0.7.300.jar](/opt/project/build/libs/sbrp-client-0.7.300.jar)\n\nСкачай его кнопкой ниже.".into(),..Default::default()}];
                }
            }
            if args.iter().any(|a| a == "--demo-desktop") {
                app.settings.servers[0].local_workspace = Some("/home/player/Minecraft".into());
                app.settings.servers[0].local_control = true;
                if let Some(k) = app.current_key() {
                    app.chats.get_mut(&k).unwrap().mode = "manual".into();
                }
                app.desktop_requests.push(("demo".into(),json!({"id":"demo-action","tool":"run_local_command","input":{"command":"ls -lh mods/ && tail -n 20 logs/latest.log","timeout_seconds":30}})));
            }
            if let Some(path) = args
                .iter()
                .position(|a| a == "--demo-board")
                .and_then(|i| args.get(i + 1))
            {
                if let Ok(state) =
                    std::fs::read_to_string(path).map(|t| serde_json::from_str::<Value>(&t))
                {
                    if let Ok(state) = state {
                        app.boards.insert("demo".into(), state);
                        app.board_open = true;
                        app.board_view.selected = args
                            .iter()
                            .position(|a| a == "--demo-select")
                            .and_then(|i| args.get(i + 1))
                            .cloned();
                        app.board_view.demo(&args);
                    }
                }
            }
            if args.iter().any(|a| a == "--demo-gametest") {
                app.game_tests.push(("demo".into(),json!({"id":"t1","stage":"done","purpose":"Наковальня: мини-игра ковки открывается и принимает удары","result":{"ok":true,"seconds":38,"local_folder":"/tmp"}})));
                if args.iter().any(|a| a == "--demo-gametest-ask") {
                    app.desktop_requests.push(("demo".into(),json!({"id":"t3","tool":"minecraft_test","input":{"purpose":"Наковальня: мини-игра ковки открывается и принимает удары","minecraft_version":"1.21.8","timeout_seconds":300,"mods":[{"name":"sbrp-client-0.7.309.jar"},{"name":"fabric-api-0.136.1+1.21.8.jar"}],"scenario":"test_world\nwait_world\ncommand /give @s sbrp:hammer\nkey key.use\nscreenshot anvil\nquit"}})));
                }
                app.game_tests.push(("demo".into(),json!({"id":"t2","stage":"running","message":"шаг 4/9","purpose":"Новый инвентарь: палитры переключаются"})));
            }
            if args.iter().any(|a| a == "--demo-tables") {
                if let Some(k) = app.current_key() {
                    app.chats.get_mut(&k).unwrap().messages=vec![Message{role:"assistant".into(),text:"Шесть рисков, все взяты из проекта:\n\n| № | Риск | Ранг | Стратегия |\n|---|---|---|---|\n| 1 | Распознавание не справляется с нарезками из коротких видео | 3×3 = 9 | Снижение влияния: добавить обработку аудио |\n| 2 | Потеря файлов при передаче готовой сборки | 2×3 = 6 | Проверять размер и контрольную сумму |\n| 3 | Автоматические проверки занимают много времени | 3×2 = 6 | Разделить запуск и проверку результата |\n\nТекст после таблицы остаётся читаемым.".into(),..Default::default()}];
                }
            }
            if args.iter().any(|a| a == "--demo-math") {
                if let Some(k) = app.current_key() {
                    app.chats.get_mut(&k).unwrap().messages=vec![Message{role:"assistant".into(),text:"### Формулы прямо в чате\n\nПусть $f(x) = x^x$ при $x > 0$. Тогда $f'(x) = x^x(\\ln x + 1)$, а минимум достигается в точке $x = \\frac{1}{e}$, где значение равно $e^{-1/e} \\approx 0{,}692$. Следующие строки этого абзаца идут с запасом по высоте, чтобы дробь не задевала соседние строки текста.\n\n$$\\lim_{x \\to 0} \\frac{\\sin x - x}{x^3} = -\\frac{1}{6}$$\n\nЗапись через скобки тоже работает: \\(\\int_0^1 x e^x \\, dx = 1\\), а блок выглядит так:\n\n\\[\n\\sum_{n=1}^{\\infty} \\frac{n}{2^n} = 2, \\qquad\ng(x) = \\begin{cases} x^2, & x \\ge 0 \\\\ -x, & x < 0 \\end{cases}\n\\]\n\nКириллица внутри формулы: $\\text{при } x > 0 \\text{ верно } y = \\text{яблоко, щука, шея}$. Короткие $a$, $b_i$ и $c^2$ не меняют высоту строки.\n\n$$\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix} \\begin{pmatrix} x \\\\ y \\end{pmatrix} = \\begin{pmatrix} ax + by \\\\ cx + dy \\end{pmatrix}, \\quad \\forall \\varepsilon > 0 \\; \\exists \\delta > 0$$".into(),..Default::default()}];
                }
            }
            if args.iter().any(|a| a == "--demo-math-2") {
                if let Some(k) = app.current_key() {
                    app.chats.get_mut(&k).unwrap().messages=vec![Message{role:"assistant".into(),text:"1. Ряд $\\sum_{n=1}^{\\infty} \\frac{1}{n^2} = \\frac{\\pi^2}{6}$ сходится, и длинный текст пункта переносится на следующие строки без наложений на формулу.\n2. Матрица $\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}$ обратима при $ad - bc \\ne 0$.\n- Маркер и $\\sqrt{x}$ на одной линии.\n\n| Что | Формула | Значение |\n|---|---|---|\n| Предел | $\\lim_{x\\to0}\\frac{\\sin x}{x}$ | $1$ |\n| Интеграл | $\\int_0^1 x e^x\\,dx$ и текст рядом | $1$ |\n| Модуль | $|x| \\le 1$ | да |\n\nЦена $5 или $10, путь $HOME/$USER и `$x^2$` в коде остаются как есть. Неверная формула $\\nosuchcommand{x}$ показывается исходником.\n\n```python\ncost = \"$5\"  # $x^2$ здесь просто текст\n```\n\n$$x_1 + x_2 + x_3 + x_4 + x_5 + x_6 + x_7 + x_8 + x_9 + x_{10} + x_{11} + x_{12} + x_{13} + x_{14} + x_{15} + x_{16} + x_{17} + x_{18} + x_{19} + x_{20} + x_{21} + x_{22} + x_{23} + x_{24} + x_{25} + x_{26} + x_{27} + x_{28} + x_{29} + x_{30}$$".into(),..Default::default()}];
                }
            }
            if args.iter().any(|a| a == "--demo-lists") {
                if let Some(k) = app.current_key() {
                    let c = app.chats.get_mut(&k).unwrap();
                    c.messages=vec![Message{role:"user".into(),text:"Как прикрепить файл к сессии?".into(),..Default::default()},Message{role:"assistant".into(),text:"Прикрепить файл можно прямо в приложении.\n\n1. Нажми **Прикрепить файлы** под полем задачи.\n2. Выбери документ или изображение на компьютере. Несколько файлов можно выбрать вместе.\n3. Отправь сообщение — вложения останутся в этой сессии.\n\nТакже доступны:\n\n- Перетаскивание файлов в окно.\n- Вставка изображения через **Ctrl+V**.\n\nСохранённую сессию можно продолжить в другой день. Контекст и прикреплённые файлы останутся доступны.\n\n- Длинный путь: `C:\\Users\\Player\\AppData\\Roaming\\Launcher\\profiles\\248a2ae9\\mods\\project-client-0.7.252.jar`. После пути текст должен переноситься внутри сообщения.\n- Ссылка: https://example.com/releases/very-long-project-name-with-a-long-path/downloads/project-client-0.7.252.jar — и пояснение после неё.\n\nОбычный абзац с `settings.json` и длинным именем `professions.json.bak-1018-smithpath-release-original-configuration` тоже должен помещаться в ширину окна.".into(),..Default::default()}];
                }
            }
            if args.iter().any(|a| a == "--light") {
                app.settings.light = true;
            }
        }
        if !demo {
            if app.server().is_none() && app.settings.servers.len() == 1 {
                app.settings.selected = Some(app.settings.servers[0].id.clone());
            }
            if let Some(server) = app.server() {
                if let Some(sid) = server.last_session.clone() {
                    let mut c =
                        Chat::new(sid.clone(), server.cwd.clone(), "Последний разговор".into());
                    c.mode = server.mode.clone();
                    c.model = server.model.clone();
                    c.effort = server.effort.clone();
                    app.chats.insert(key(&server.id, &sid), c);
                    app.selected_chat.insert(server.id.clone(), sid);
                }
                if server.remember {
                    app.connect(server);
                }
            }
        }
        cc.egui_ctx.set_fonts(design::fonts());
        if demo {
            app.seed_workspace_demo(&cc.egui_ctx, &args);
            app.seed_orchestration_preview(&cc.egui_ctx, &args);
        }
        app.theme(&cc.egui_ctx);
        app
    }
    fn theme(&self, ctx: &egui::Context) {
        let a = &self.settings.appearance;
        ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::new("relay-theme"),
                self.settings.theme().to_owned(),
            );
            d.insert_temp(egui::Id::new("relay-animations"), a.animations);
            d.insert_temp(egui::Id::new("relay-wrap-output"), a.wrap_output);
            d.insert_temp(egui::Id::new("relay-open-errors"), a.open_errors);
        });
        let scale = a.scale.clamp(0.8, 1.35);
        if (ctx.zoom_factor() - scale).abs() > 0.001 {
            ctx.set_zoom_factor(scale);
        }
        let p = design::animated_palette(ctx, self.settings.light);
        let mut v = if design::is_light(self.settings.theme()) {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        v.panel_fill = p.base;
        v.window_corner_radius = egui::CornerRadius::same(16);
        v.menu_corner_radius = egui::CornerRadius::same(10);
        v.window_shadow = egui::epaint::Shadow {
            offset: [0, 12],
            blur: 48,
            spread: 0,
            color: Color32::from_black_alpha(75),
        };
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.noninteractive,
        ] {
            w.corner_radius = egui::CornerRadius::same(8);
        }
        v.window_stroke = Stroke::new(1.0_f32, p.line);
        v.window_fill = p.side;
        v.faint_bg_color = p.surface;
        v.extreme_bg_color = p.side;
        v.override_text_color = Some(p.text);
        v.selection.bg_fill = accent().gamma_multiply(0.3);
        v.hyperlink_color = accent();
        v.selection.stroke = Stroke::new(1.0_f32, p.text);
        v.widgets.inactive.bg_fill = p.surface;
        v.widgets.inactive.weak_bg_fill = p.surface;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.line);
        v.widgets.hovered.bg_fill = p.line;
        v.widgets.hovered.weak_bg_fill = p.line;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, design::mix(p.line, accent(), 0.5));
        v.widgets.active.bg_fill = accent().gamma_multiply(0.35);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.line);
        // Bold Markdown uses the "active" text colour: keep it the theme's own text colour.
        v.widgets.active.fg_stroke.color = p.text;
        ctx.set_visuals(v);
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(8.0, if a.compact { 6.0 } else { 10.0 });
        style.spacing.button_padding = Vec2::new(12.0, 7.0);
        style.spacing.interact_size.y = 32.;
        style.spacing.window_margin = egui::Margin::same(18);
        style.spacing.menu_margin = egui::Margin::same(6);
        style.animation_time = if a.animations { 0.28 } else { 0. };
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
        ctx.set_style(style);
    }
    fn save(&mut self) {
        #[cfg(test)]
        if self.test_no_save {
            return;
        }
        if !self.demo {
            if let Err(e) = storage::save(&self.settings) {
                self.notice = e.to_string();
            }
        }
    }
    fn server(&self) -> Option<Server> {
        self.settings
            .selected
            .as_ref()
            .and_then(|id| self.settings.servers.iter().find(|s| &s.id == id))
            .cloned()
    }
    fn current_key(&self) -> Option<String> {
        let s = self.settings.selected.as_ref()?;
        Some(key(s, self.selected_chat.get(s)?))
    }
    fn connected(&self, id: &str) -> bool {
        self.demo || self.remote.get(id).is_some_and(|r| r.connected)
    }
    fn new_chat(&mut self) {
        self.hide_browser();
        if let Some(s) = self.server() {
            let sid = uuid::Uuid::new_v4().to_string();
            let b = s.effective_backend().to_owned();
            let initial_model = s.model_for(&b).to_owned();
            let mut chat = Chat::new(sid.clone(), s.cwd, "Новая сессия".into());
            chat.mode = s.mode;
            chat.model = initial_model;
            chat.effort = s.effort;
            chat.loaded = true;
            self.controller.bus.ctx.animate_bool_with_time(
                egui::Id::new(("chat-enter", key(&s.id, &sid))),
                false,
                0.,
            );
            self.chats.insert(key(&s.id, &sid), chat);
            self.selected_chat.insert(s.id, sid);
        }
    }
    fn select_chat(&mut self, session: Session) {
        self.hide_browser();
        if let Some(server) = self.server() {
            self.controller.bus.ctx.animate_bool_with_time(
                egui::Id::new(("chat-enter", key(&server.id, &session.id))),
                false,
                0.,
            );
        }
        if let Some(s) = self.server() {
            let locked_gate = session.is_locked_gate();
            let chat = self.chat_entry(&s, &session);
            chat.locked = session.locked;
            chat.unlocked = session.unlocked;
            if locked_gate {
                chat.loading = false;
                chat.loaded = false;
                chat.messages.clear();
            } else if !chat.loaded {
                chat.loading = true;
                self.controller
                    .send(&s.id, json!({"op":"history","sid":session.id}));
            } else if chat.loaded && !chat.busy {
                // Quietly pick up anything written from a terminal since the last load.
                self.controller
                    .send(&s.id, json!({"op":"history","sid":session.id}));
            }
            if locked_gate { self.restore_session_access(&s.id, &session.id); }
            self.selected_chat.insert(s.id.clone(), session.id.clone());
            if let Some(p) = self.settings.servers.iter_mut().find(|p| p.id == s.id) {
                p.cwd = session.cwd;
                p.last_session = Some(session.id);
            }
            self.save();
        }
    }
    fn open_folders(&mut self) {
        let Some(server) = self.server() else {
            return;
        };
        if self.connected(&server.id) || self.demo {
            self.folder_path = server.cwd.clone();
            self.folder_open = true;
            self.request_folders();
        } else {
            self.connect(server);
        }
    }
    fn chat_entry(&mut self, s: &Server, session: &Session) -> &mut Chat {
        let ck = key(&s.id, &session.id);
        let eff_backend = self.settings.session_backend(
            s,
            &ck,
            Some(session.backend.as_str()).filter(|b| !b.is_empty()),
        );
        self.chats
            .entry(ck.clone())
            .or_insert_with(|| {
                let mut chat = Chat::new(
                    session.id.clone(),
                    session.cwd.clone(),
                    session.title.clone(),
                );
                chat.mode = s.mode.clone();
                chat.model = s.model_for(&eff_backend).to_owned();
                chat.effort = s.effort.clone();
                chat
            });
        // What was typed or queued here before Relay was closed comes back.
        if let Some(saved) = self.pending.chats.remove(&key(&s.id, &session.id)) {
            let chat = self.chats.get_mut(&key(&s.id, &session.id)).unwrap();
            if chat.draft.is_empty() {
                chat.draft = saved.draft;
            }
            for text in saved.queue {
                if !chat.queue.iter().any(|q| q.text == text) {
                    chat.queue.push_back(QueuedPrompt { text, files: vec![] });
                }
            }
        }
        self.chats.get_mut(&key(&s.id, &session.id)).unwrap()
    }

    /// Writes drafts and the unsent queue to disk when they change (at most once a second).
    fn persist_pending(&mut self, force: bool) {
        if self.demo {
            return;
        }
        if !force && self.pending_checked.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return;
        }
        self.pending_checked = Some(Instant::now());
        let mut store = self.pending.clone();
        for (k, c) in &self.chats {
            let queue: Vec<String> = c.queue.iter().map(|q| q.text.clone()).filter(|t| !t.trim().is_empty()).collect();
            if c.draft.trim().is_empty() && queue.is_empty() {
                store.chats.remove(k);
            } else {
                store.chats.insert(k.clone(), crate::model::PendingChat { draft: c.draft.clone(), queue });
            }
        }
        if store != self.pending_saved {
            if storage::save_private_json("pending-messages.json", &store).is_ok() {
                self.pending_saved = store;
            }
        }
    }
    fn prefetch(&mut self, server: &str, session: &Session) {
        if self.demo
            || session.is_locked_gate()
            || !self.connected(server)
            || self.chats.contains_key(&key(server, &session.id))
        {
            return;
        }
        let Some(s) = self.server().filter(|s| s.id == server) else {
            return;
        };
        self.chat_entry(&s, session).loading = true;
        self.controller
            .send(server, json!({"op":"history","sid":session.id}));
    }
    fn refresh(&mut self, server: &str) {
        if let Some(remote) = self.remote.get_mut(server) {
            remote.sessions_requested = Some(Instant::now());
            remote.sessions_loaded = false;
            remote.sessions_retries = 0;
            remote.sessions_error.clear();
        }
        self.controller.send(server, json!({"op":"sessions"}));
        if let Some(sid) = self.selected_chat.get(server) {
            if let Some(c) = self
                .chats
                .get(&key(server, sid))
                .filter(|c| c.loaded && !c.busy)
            {
                self.controller
                    .send(server, json!({"op":"history","sid":c.sid}));
            }
        }
    }
    fn connect(&mut self, server: Server) {
        if self.demo {
            return;
        }
        self.notice.clear();
        self.remote.entry(server.id.clone()).or_default().connecting = true;
        if let Some(password) = self.login_passwords.get(&server.id) {
            self.establish(server, password.to_string());
            return;
        }
        let bus = self.controller.bus.clone();
        self.controller.runtime.spawn_blocking(move || {
            let password = if server.remember { storage::password(&server.id) } else { None };
            bus.send(Event::Password { server, password });
        });
    }
    fn establish(&mut self, server: Server, password: String) {
        self.remote.entry(server.id.clone()).or_default().connecting = true;
        self.login_passwords.insert(server.id.clone(), zeroize::Zeroizing::new(password.clone()));
        self.controller.connect(server, password);
    }
    fn request_folders(&mut self) {
        if let Some(s) = self.server() {
            self.folder_request += 1;
            self.controller.send(&s.id,json!({"op":"folders","path":self.folder_path,"hidden":self.folder_hidden,"request":self.folder_request}));
        }
    }
    fn load_images(&self, server: String, sid: String, paths: Vec<std::path::PathBuf>) {
        let bus = self.controller.bus.clone();
        self.controller.runtime.spawn_blocking(move || {
            let mut images = vec![];
            let mut errors = vec![];
            for path in paths.into_iter().take(8) {
                match images::from_file(&bus.ctx, &path) {
                    Ok(a) => images.push(a),
                    Err(e) => errors.push(e.to_string()),
                }
            }
            bus.send(Event::Images {
                server,
                sid,
                images,
                errors,
            });
        });
    }
    fn pick_images(&mut self) {
        if self.current_key().is_none() {
            self.new_chat();
        }
        let Some(server) = self.server() else {
            return;
        };
        let Some(sid) = self.selected_chat.get(&server.id).cloned() else {
            return;
        };
        let bus = self.controller.bus.clone();
        self.controller.runtime.spawn(async move {
            let paths = rfd::AsyncFileDialog::new()
                .set_title("Прикрепить файлы")
                .pick_files()
                .await;
            if let Some(files) = paths {
                bus.send(Event::Files {
                    server: server.id,
                    sid,
                    paths: files.into_iter().map(|f| f.path().to_owned()).collect(),
                });
            }
        });
    }
    fn paste_image(&mut self) {
        if self.current_key().is_none() {
            self.new_chat();
        }
        let Some(server) = self.server() else {
            return;
        };
        let Some(sid) = self.selected_chat.get(&server.id).cloned() else {
            return;
        };
        let bus = self.controller.bus.clone();
        self.controller
            .runtime
            .spawn_blocking(move || match images::from_clipboard(&bus.ctx) {
                Ok(Some(a)) => {
                    bus.send(Event::Images {
                        server: server.id,
                        sid,
                        images: vec![a],
                        errors: vec![],
                    });
                }
                Ok(None) => {}
                Err(error) => bus.send(Event::Images {
                    server: server.id,
                    sid,
                    images: vec![],
                    errors: vec![format!("Не удалось вставить изображение: {error}")],
                }),
            });
    }
    fn poll(&mut self) {
        for _ in 0..512 {
            let Ok(event) = self.events.try_recv() else {
                break;
            };
            let event = match event {
                Event::Epoch {
                    server,
                    generation,
                    event,
                } => {
                    if !self.controller.is_current(&server, generation) {
                        continue;
                    }
                    *event
                }
                event => event,
            };
            match event {
                Event::Epoch { .. } => {}
                Event::Trust {
                    server,
                    fingerprint,
                    answer,
                } => {
                    self.trust = Some(Trust {
                        server,
                        fingerprint,
                        answer,
                    })
                }
                Event::Password { server, password } => {
                    if let Some(password) = password {
                        self.establish(server, password);
                    } else if self.reconnect.remove(&server.id).is_some() {
                        self.remote.entry(server.id.clone()).or_default().connecting = false;
                        self.notice = "Связь прервалась, а пароль сервера не сохранён. Нажми «Отправить», чтобы подключиться.".into();
                    } else {
                        self.remote.entry(server.id.clone()).or_default().connecting = false;
                        self.editor = Some(Editor {
                            server,
                            password: String::new(),
                            is_new: false,
                            error: String::new(),
                        });
                    }
                }
                Event::Error { server, message } => {
                    if message.contains("Сервер не принял логин или пароль.") {
                        self.login_passwords.remove(&server);
                        storage::forget(&server);
                    }
                    self.notice = message;
                    self.remote.entry(server).or_default().connecting = false;
                }
                Event::Disconnected { server } => {
                    self.login_disconnected(&server);
                    if let Some(dialog) = self.delete_chat.as_mut().filter(|d| d.server == server) {
                        dialog.pending = false;
                        dialog.error =
                            "Подключение прервалось. Подключись и повтори удаление.".into();
                    }
                    self.downloads.disconnected(&server);
                    let r = self.remote.entry(server.clone()).or_default();
                    self.desktop_requests.retain(|(s, _)| s != &server);
                    r.connected = false;
                    r.connecting = false;
                    let known = self.settings.servers.iter().any(|s| s.id == server);
                    for (k, c) in &mut self.chats {
                        if k.starts_with(&format!("{server}/")) {
                            if c.busy {
                                c.status = "Связь прервалась · переподключаемся".into();
                            }
                            // An upload cut midway goes back to the front of the queue and is resent.
                            let failed_upload = c.upload_progress.is_some();
                            c.restore_failed_upload();
                            if failed_upload && (!c.draft.trim().is_empty() || !c.images.is_empty()) {
                                c.enqueue_draft();
                            }
                            c.busy = false;
                            c.queue_paused = false;
                            c.loading = false;
                            c.permissions.clear();
                        }
                    }
                    if known && !self.demo {
                        let attempt = self.reconnect.get(&server).map_or(0, |(_, n)| *n);
                        let delay = [1u64, 2, 3, 5, 8, 13, 20, 30][(attempt as usize).min(7)];
                        self.reconnect.insert(server.clone(), (Instant::now() + Duration::from_secs(delay), attempt + 1));
                    }
                }
                Event::Files { server, sid, paths } => self.load_images(server, sid, paths),
                Event::Images {
                    server,
                    sid,
                    images,
                    errors,
                } => {
                    if let Some(c) = self.chats.get_mut(&key(&server, &sid)) {
                        for a in images {
                            let inline = if a.source["type"] == "file" {
                                0
                            } else {
                                a.bytes
                            };
                            let inline_total = c
                                .images
                                .iter()
                                .filter(|f| f.source["type"] != "file")
                                .map(|f| f.bytes)
                                .sum::<usize>();
                            if c.images.len() < images::MAX_ATTACHMENTS
                                && c.images.iter().map(|f| f.bytes).sum::<usize>() + a.bytes
                                    <= images::MAX_ATTACHMENT_BYTES
                                && inline_total + inline <= images::MAX_INLINE_BYTES
                            {
                                c.images.push(a)
                            } else {
                                c.error = if inline_total + inline > images::MAX_INLINE_BYTES {
                                    "Изображения в сообщении — до 16 МБ. Остальные файлы — до 512 МБ."
                                } else { "До восьми вложений общим размером 512 МБ." }.into();
                            }
                        }
                        if !errors.is_empty() {
                            c.error = errors.join("\n");
                        }
                    }
                }
                Event::Data { server, value } => self.data(server, value),
            }
        }
    }
    fn data(&mut self, server: String, value: Value) {
        if self.deletion_event(&server, &value) {
            return;
        }
        let kind = value["type"].as_str().unwrap_or("");
        // A hover-prefetch can fail after deletion was already acknowledged.
        // Its late error belongs to the retired conversation, not the new one.
        if kind == "error"
            && value["sid"]
                .as_str()
                .is_some_and(|sid| self.deleted_sessions.contains(&key(&server, sid)))
        {
            return;
        }
        if matches!(kind, "upload_progress" | "upload_complete") {
            if let Some(c) = value["sid"]
                .as_str()
                .and_then(|sid| self.chats.get_mut(&key(&server, sid)))
                .filter(|c| c.upload_progress.is_some())
            {
                if kind == "upload_complete" {
                    c.upload_progress = None;
                    c.status = "Думает над ответом".into();
                } else if let (Some(sent), Some(total)) =
                    (value["sent"].as_u64(), value["total"].as_u64())
                {
                    c.upload_progress = Some((sent, total));
                    c.status = "Отправка файлов".into();
                }
            }
            return;
        }
        if kind == "error" && value["op"] == "sessions" {
            let remote = self.remote.entry(server).or_default();
            remote.sessions_error = "Не удалось загрузить сессии. Повторяем…".into();
            remote.sessions_requested = Some(Instant::now());
            return;
        }
        match kind {
            "cluster_request" => {
                self.cluster_request(server, value);
                return;
            }
            "cluster_cancelled" => {
                self.cluster_calls
                    .retain(|p| p.server != server || p.request != value["id"]);
                return;
            }
            "artifact_status" => self.downloads.event(&server, &value),
            "desktop_request" => {
                if self.desktop_requests.len() < 8
                    && !self
                        .desktop_requests
                        .iter()
                        .any(|(s, v)| s == &server && v["id"] == value["id"])
                {
                    self.desktop_requests.push((server, value));
                }
            }
            "desktop_cancel" => {
                self.desktop_requests
                    .retain(|(s, v)| s != &server || v["id"] != value["id"]);
            }
            "board" => {
                if value["state"]["unchanged"] != true && value["state"].is_object() {
                    self.boards.insert(server, value["state"].clone());
                }
            }
            "board_error" => {
                self.notice = format!("Доска: {}", value["message"].as_str().unwrap_or(""));
            }
            "game_test_status" => {
                let id = value["id"].clone();
                self.game_tests.retain(|(_, v)| v["id"] != id);
                self.game_tests.push((server, value));
                if self.game_tests.len() > 4 {
                    self.game_tests.remove(0);
                }
            }
            "desktop_status" => {
                self.notice = value["message"].as_str().unwrap_or("").into();
            }
            "hello" => {
                self.session_unlock_attempts.retain(|(host,_)| host != &server);
                self.pending_session_keys.retain(|k,_| !k.starts_with(&format!("{server}/")));
                self.reconnect.remove(&server);
                let r = self.remote.entry(server.clone()).or_default();
                r.connected = true;
                r.connecting = false;
                r.version = value["version"].as_str().unwrap_or("Claude Code").into();
                r.protocol = value["protocol"].as_u64().unwrap_or(0);
                r.auth_available = value["auth_available"] == true && value["remote_browser"] == true;
                r.backends = value["backends"].clone();
                r.home = value["home"].as_str().unwrap_or("~").into();
                r.usage_requested = Some(Instant::now());
                r.sessions_requested = Some(Instant::now());
                r.sessions_loaded = false;
                r.sessions_retries = 0;
                if let Some(s) = self.settings.servers.iter_mut().find(|s| s.id == server) {
                    if s.cwd == "~" {
                        s.cwd = r.home.clone();
                    }
                }
                self.controller.send(&server, json!({"op":"sessions"}));
                let active_backend = self
                    .settings
                    .servers
                    .iter()
                    .find(|s| s.id == server)
                    .map(|s| {
                        self.selected_chat
                            .get(&server)
                            .map(|sid| self.backend_for_chat(s, &key(&server, sid)))
                            .unwrap_or_else(|| s.effective_backend().to_owned())
                    })
                    .unwrap_or_else(|| "claude".into());
                if active_backend == "claude" {
                    self.controller.send(&server, json!({"op":"usage"}));
                } else if matches!(active_backend.as_str(), "codex" | "agy") {
                    self.controller
                        .send(&server, json!({"op":"catalog","backend":active_backend}));
                }
                let workspace = self
                    .settings
                    .servers
                    .iter()
                    .find(|s| s.id == server)
                    .filter(|s| s.local_control)
                    .and_then(|s| s.local_workspace.clone());
                self.controller
                    .send(&server, json!({"op":"_desktop_workspace","path":workspace}));
                let tests = self
                    .settings
                    .servers
                    .iter()
                    .find(|s| s.id == server)
                    .map_or("ask".to_owned(), |s| s.tests.clone());
                self.controller
                    .send(&server, json!({"op":"_tests_policy","mode":tests}));
                // Reset any in-flight history loaders from a dropped connection so sessions never stay stuck.
                let prefix = format!("{server}/");
                for (k, c) in &mut self.chats {
                    if k.starts_with(&prefix) {
                        c.loading = false;
                    }
                }
                if let Some(sid) = self.selected_chat.get(&server) {
                    if let Some(c) = self.chats.get_mut(&key(&server, sid)) {
                        if !(c.locked && !c.unlocked) {
                            if !c.loaded {
                                c.loading = true;
                                self.controller
                                    .send(&server, json!({"op":"history","sid":sid}));
                            } else if !c.busy {
                                // Quietly sync any messages written while disconnected.
                                self.controller
                                    .send(&server, json!({"op":"history","sid":sid}));
                            }
                        }
                    }
                }
                self.save();
            }
            "catalog" => {
                let r = self.remote.entry(server.clone()).or_default();
                let models = serde_json::from_value(value["models"].clone()).unwrap_or_default();
                let commands = serde_json::from_value(value["commands"].clone()).unwrap_or_default();
                match value["backend"].as_str().unwrap_or("claude") {
                    "codex" => {
                        r.codex_models = models;
                        r.codex_commands = commands;
                    }
                    "agy" => {
                        r.agy_models = models;
                        r.agy_commands = commands;
                    }
                    _ => {
                        r.models = models;
                        r.commands = commands;
                    }
                }
            }
            "api_models" => self.api_models_reply(&server, &value),
            "auth"|"auth_frame"|"auth_browser"|"auth_clipboard"|"browser_info"|"browser_setup"|"browser_navigation" => self.login_event(&server, &value),
            "model_changed" | "mode_changed" | "effort_changed" | "notice" => {
                let sid = value["sid"].as_str().unwrap_or("");
                let chat_key = key(&server, sid);
                let applies = value["backend"].as_str().is_none_or(|backend| {
                    self.settings.servers.iter().find(|profile| profile.id == server)
                        .is_some_and(|profile| self.backend_for_chat(profile, &chat_key) == backend)
                });
                if let Some(c) = self.chats.get_mut(&chat_key).filter(|_| applies) {
                    match kind {
                        "model_changed" => c.model = value["model"].as_str().unwrap_or("default").into(),
                        "mode_changed" => c.mode = value["mode"].as_str().unwrap_or("manual").into(),
                        "effort_changed" => c.effort = model::selected_effort(value["effort"].as_str().unwrap_or("")).into(),
                        _ => {}
                    }
                }
                if kind == "notice" {
                    self.notice = value["message"].as_str().unwrap_or("").into();
                }
            }
            "backends_status" => {
                let r = self.remote.entry(server.clone()).or_default();
                r.backends = value["backends"].clone();
                if let Some(ver) = r.backends["claude"]["version"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                {
                    r.version = ver.to_owned();
                }
            }
            "agent_install" => {
                let agent = value["agent"].as_str().unwrap_or("").to_owned();
                let running = value["running"] == true;
                let msg = value["message"].as_str().unwrap_or("").to_owned();
                let r = self.remote.entry(server.clone()).or_default();
                if running {
                    if !agent.is_empty() {
                        r.installing_agents.insert(
                            agent,
                            if msg.is_empty() {
                                "Устанавливаем на сервере…".into()
                            } else {
                                msg
                            },
                        );
                    }
                } else {
                    r.installing_agents.remove(&agent);
                    if let Some(out) = value["output"].as_str().filter(|s| !s.is_empty()) {
                        r.agent_logs.insert(agent.clone(), out.to_owned());
                    }
                    if r.backends.is_object() && !agent.is_empty() {
                        if let Some(inst) = value["installed"].as_bool() {
                            r.backends[&agent]["installed"] = json!(inst);
                        }
                        if let Some(ver) = value["version"].as_str() {
                            r.backends[&agent]["version"] = json!(ver);
                            if agent == "claude" && !ver.is_empty() {
                                r.version = ver.to_owned();
                            }
                        }
                    }
                    if !msg.is_empty() {
                        self.notice = msg;
                    }
                    if value["ok"] == true {
                        if matches!(agent.as_str(), "codex" | "agy") {
                            self.controller
                                .send(&server, json!({"op": "catalog", "backend": agent}));
                        } else if agent == "claude" {
                            self.controller.send(&server, json!({"op": "usage"}));
                        }
                    }
                }
            }
            "claude_update" => {
                let r = self.remote.entry(server.clone()).or_default();
                r.updating = value["running"] == true;
                if let Some(v) = value["version"].as_str() {
                    r.version = v.into();
                }
                if !r.updating {
                    self.notice = if value["changed"] == true {
                        format!("Claude Code на сервере обновлён: {} → {}", value["before"].as_str().unwrap_or(""), r.version)
                    } else if value["ok"] == true {
                        format!("Claude Code уже последней версии: {}", r.version)
                    } else {
                        let output = value["output"].as_str().unwrap_or("");
                        format!("Не удалось обновить Claude Code: {}", &output[output.len().saturating_sub(400)..])
                    };
                }
            }
            "turn_summary" => {
                let sid = value["sid"].as_str().unwrap_or("");
                let backend = value["backend"].as_str().unwrap_or("");
                if let Some(c) = self.chats.get_mut(&key(&server, sid)).filter(|c| !c.locked || c.unlocked) {
                    if c.turn_summary.backend != backend {
                        c.turn_summary = TurnSummary { backend: backend.into(), ..Default::default() };
                    }
                    if value["reset_diff"] == true {
                        c.turn_summary.diff = None;
                    }
                    if let (Some(added), Some(removed)) = (value["added"].as_u64(), value["removed"].as_u64()) {
                        c.turn_summary.diff = Some((added, removed));
                    }
                    if let (Some(used), Some(limit)) = (value["used"].as_u64(), value["limit"].as_u64()) {
                        c.turn_summary.context = (limit > 0).then_some((used, limit));
                    }
                }
            }
            "run_state" => {
                let sid = value["sid"].as_str().unwrap_or("").to_owned();
                let busy = value["busy"] == true;
                if busy {
                    if let Some(backend @ ("claude" | "codex" | "agy" | "api")) = value["backend"].as_str() {
                        self.settings.set_session_backend(key(&server, &sid), backend);
                    }
                }
                let r = self.remote.entry(server.clone()).or_default();
                if busy {
                    r.running.insert(sid.clone());
                } else {
                    r.running.remove(&sid);
                }
                if let Some(c) = self.chats.get_mut(&key(&server, &sid)) {
                    let background: Vec<&str> = value["background"].as_array().map(|a| a.iter().filter_map(|t| t.as_str()).collect()).unwrap_or_default();
                    if busy && !c.busy && c.loaded {
                        c.busy = true;
                        if c.status.is_empty() {
                            c.status = "Работает".into();
                        }
                    }
                    if busy && !background.is_empty() && (c.status.is_empty() || c.status == "Работает" || c.status.starts_with("В фоне")) {
                        c.status = format!("В фоне: {}", background.join(", "));
                    } else if !busy && c.busy {
                        c.busy = false;
                        c.status.clear();
                        // The turn ended without this window seeing every event (other computer, reconnect):
                        // reload so no messages go missing.
                        if c.loaded {
                            self.controller.send(&server, json!({"op":"history","sid":sid}));
                        }
                    }
                }
            }
            "usage" => {
                if let Ok(usage) = serde_json::from_value::<Usage>(value.clone()) {
                    let backend = value["backend"].as_str().unwrap_or("claude").to_owned();
                    let remote = self.remote.entry(server.clone()).or_default();
                    if backend == "claude" {
                        remote.usage = usage.clone();
                    }
                    remote.usage_by.insert(backend, usage);
                }
            }
            "sessions" => {
                let Some(rows) = value["sessions"].as_array() else {
                    return;
                };
                let mut sessions: Vec<Session> = rows
                    .iter()
                    .filter_map(|row| serde_json::from_value::<Session>(row.clone()).ok())
                    .filter(|s| !s.id.is_empty())
                    .filter(|s| !self.deleted_sessions.contains(&key(&server, &s.id)))
                    .collect();
                let closed: std::collections::HashSet<_> = value["folders"].as_array().into_iter().flatten()
                    .filter(|f| f["closed"] == true).filter_map(|f| f["id"].as_str()).collect();
                sessions.retain(|s| {
                    if !closed.contains(s.folder.as_str()) {
                        return true;
                    }
                    self.chats.remove(&key(&server, &s.id));
                    if self.selected_chat.get(&server) == Some(&s.id) {
                        self.selected_chat.remove(&server);
                    }
                    false
                });
                if self.settings.restore_locked_titles(&server, &mut sessions) {
                    self.save();
                }
                let remote = self.remote.entry(server.clone()).or_default();
                remote.sessions = sessions.clone();
                if let Some(folders) = value["folders"].as_array() {
                    remote.folders = folders
                        .iter()
                        .filter_map(|f| serde_json::from_value(f.clone()).ok())
                        .collect();
                }
                remote.sessions_loaded = true;
                remote.sessions_requested = None;
                remote.sessions_error.clear();
                if remote.protocol >= 4 {
                    remote.running = sessions.iter().filter(|s| s.running).map(|s| s.id.clone()).collect();
                }
                let selected_sid = self.selected_chat.get(&server).cloned();
                for s in &sessions {
                    if let Some(c) = self.chats.get_mut(&key(&server, &s.id)) {
                        // Messages written from another device or a terminal: pull them into the open chat.
                        if s.modified > c.synced_modified + 0.25 {
                            if c.synced_modified > 0.
                                && c.loaded
                                && !c.busy
                                && !c.loading
                                && selected_sid.as_deref() == Some(s.id.as_str())
                                && !s.is_locked_gate()
                            {
                                self.controller.send(&server, json!({"op":"history","sid":s.id}));
                            }
                            c.synced_modified = s.modified;
                        }
                        c.title = s.title.clone();
                        c.cwd = s.cwd.clone();
                        c.locked = s.locked;
                        c.unlocked = s.unlocked;
                        if s.is_locked_gate() {
                            c.messages.clear();
                            c.sent_images.clear();
                            c.permissions.clear();
                            c.loaded = false;
                            c.loading = false;
                        }
                    }
                }
                if let Some(s) = sessions.iter().find(|s| s.is_locked_gate() && Some(&s.id) == selected_sid.as_ref()) {
                    self.restore_session_access(&server, &s.id);
                }
                if !self.selected_chat.contains_key(&server)
                    && self.settings.selected.as_deref() == Some(&server)
                {
                    let last = self.server().and_then(|s| s.last_session);
                    if let Some(session) = sessions
                        .iter()
                        .find(|s| Some(&s.id) == last.as_ref())
                        .cloned()
                    {
                        self.select_chat(session);
                    } else {
                        self.new_chat();
                    }
                }
            }
            "folders" => {
                if self.settings.selected.as_deref() != Some(&server)
                    || value["request"].as_u64() != Some(self.folder_request)
                {
                    return;
                }
                self.folder_confirmed = value["path"].as_str().unwrap_or("~").into();
                self.folder_path = value["path"].as_str().unwrap_or("~").into();
                self.folder_parent = value["parent"].as_str().unwrap_or("/").into();
                self.folders = serde_json::from_value(value["folders"].clone()).unwrap_or_default();
            }
            "session_locked" => {
                let sid = value["sid"].as_str().unwrap_or("");
                if let Some(c) = self.chats.get_mut(&key(&server, sid)) {
                    c.loading = false;
                    c.loaded = false;
                    c.locked = true;
                    c.unlocked = false;
                    c.messages.clear();
                    c.sent_images.clear();
                    c.permissions.clear();
                }
            }
            "session_unlock_result" => {
                let sid = value["sid"].as_str().unwrap_or("");
                let pending = self.pending_session_keys.remove(&key(&server,sid));
                if let Some(slot) = self.unlock_slot(&server,sid) {
                    if value["ok"] == true {
                        if let Some(pw) = pending {
                            let _ = storage::remember(&slot, &pw);
                            self.verified_session_keys.insert(slot, pw);
                        }
                    } else {
                        self.verified_session_keys.remove(&slot);
                        storage::forget(&slot);
                    }
                }
                if let Some(c) = self.chats.get_mut(&key(&server, sid)) {
                    c.unlocking = false;
                    if value["ok"] == true {
                        c.unlocked = true;
                        c.unlock_error.clear();
                        c.loading = true;
                        self.controller
                            .send(&server, json!({"op": "history", "sid": sid}));
                    } else {
                        c.unlock_error = value["message"]
                            .as_str()
                            .unwrap_or("Неверный пароль сессии.")
                            .into();
                    }
                }
            }
            "session_lock_changed" => {
                let sid = value["sid"].as_str().unwrap_or("");
                let locked = value["locked"] == true;
                let unlocked = value["unlocked"].as_bool().unwrap_or(!locked);
                if locked && !unlocked {
                    self.remember_lock_title(&server, sid);
                }
                let sidebar_title = self.locked_sidebar_title(&server, sid);
                if let Some(r) = self.remote.get_mut(&server) {
                    if let Some(s) = r.sessions.iter_mut().find(|s| s.id == sid) {
                        s.locked = locked;
                        s.unlocked = unlocked;
                        if locked && !unlocked {
                            s.title.clone_from(&sidebar_title);
                            s.auto_title.clear();
                        }
                    }
                }
                if let Some(c) = self.chats.get_mut(&key(&server, sid)) {
                    c.locked = locked;
                    c.unlocked = unlocked;
                    if locked && !unlocked {
                        c.title = sidebar_title;
                        c.messages.clear();
                        c.sent_images.clear();
                        c.permissions.clear();
                        c.loaded = false;
                        c.loading = false;
                    }
                }
                if self
                    .lock_dialog
                    .as_ref()
                    .is_some_and(|d| d.server == server && d.sid == sid)
                {
                    self.lock_dialog = None;
                    self.notice = if locked {
                        "Сессия защищена паролем.".into()
                    } else {
                        "Защита паролем снята.".into()
                    };
                }
            }
            "history" => {
                let sid = value["sid"].as_str().unwrap_or("");
                if let Some(c) = self.chats.get_mut(&key(&server, sid)) {
                    c.loading = false;
                    if !c.busy || !c.loaded {
                        let mut messages: Vec<Message> = value["messages"]
                            .as_array()
                            .map(|rows| {
                                rows.iter()
                                    .filter_map(|r| serde_json::from_value(r.clone()).ok())
                                    .collect()
                            })
                            .unwrap_or_default();
                        // Pictures from history become previews in their message bubble.
                        let ctx = self.controller.bus.ctx.clone();
                        for (index, message) in messages.iter_mut().enumerate() {
                            if !message.images.is_empty() {
                                let previews = message
                                    .images
                                    .drain(..)
                                    .map(|image| crate::images::from_history(&ctx, &image))
                                    .collect();
                                c.sent_images.insert(index, previews);
                            }
                        }
                        c.messages = messages;
                        c.loading = false;
                        c.loaded = true;
                        // Opened while Claude is still answering, maybe started from another computer.
                        if self.remote.get(&server).is_some_and(|r| r.running.contains(sid)) {
                            c.busy = true;
                            c.user_at = c.messages.iter().rposition(|m| m.role == "user").unwrap_or(0);
                            c.status = "Работает".into();
                        }
                    }
                }
            }
            "claude" | "permission" | "permission_cancelled" | "run_closed" | "error" => {
                let sid = value["sid"].as_str().unwrap_or("");
                if let Some(c) = self.chats.get_mut(&key(&server, sid)) {
                    match kind {
                        "claude" => {
                            reduce(c, &value["data"]);
                            if value["data"]["type"] == "result" {
                                if value["data"]["relay_backend"] == "api" {
                                    let remote = self.remote.entry(server.clone()).or_default();
                                    remote.api_tokens.0 += value["data"]["usage"]["input_tokens"].as_u64().unwrap_or(0);
                                    remote.api_tokens.1 += value["data"]["usage"]["output_tokens"].as_u64().unwrap_or(0);
                                }
                                self.controller.send(&server, json!({"op":"sessions"}));
                            }
                        }
                        "permission" => c.permissions.push(Permission {
                            id: value["id"].as_str().unwrap_or("").into(),
                            tool: value["tool"].as_str().unwrap_or("Действие").into(),
                            input: value["input"].clone(),
                        }),
                        "permission_cancelled" => c
                            .permissions
                            .retain(|p| Some(p.id.as_str()) != value["id"].as_str()),
                        "run_closed" => {
                            if c.busy {
                                c.error = if value["detail"].as_str().unwrap_or("").is_empty() {
                                    "Claude остановлен. История сохранена, можно продолжить разговор.".into()
                                } else {
                                    value["detail"].as_str().unwrap_or("").into()
                                };
                            }
                            c.busy = false;
                            c.queue_paused = true;
                            c.permissions.clear();
                            if c.loaded {
                                self.controller.send(&server, json!({"op":"history","sid":sid}));
                            }
                        }
                        "error" => {
                            if value["op"] == "session_lock" {
                                if let Some(d) = self
                                    .lock_dialog
                                    .as_mut()
                                    .filter(|d| d.server == server && d.sid == sid)
                                {
                                    d.pending = false;
                                    d.error = value["message"]
                                        .as_str()
                                        .unwrap_or("Не удалось изменить защиту сессии.")
                                        .into();
                                    return;
                                }
                            }
                            if value["upload_failed"] == true {
                                c.restore_failed_upload();
                            }
                            c.error = value["message"]
                                .as_str()
                                .unwrap_or("Не удалось выполнить действие.")
                                .into();
                            if value["op"] == "prompt" {
                                c.busy = false;
                                c.queue_paused = true;
                            }
                            if value["op"] == "history" {
                                c.loading = false;
                            }
                        }
                        _ => {}
                    }
                } else if kind == "error" {
                    if value["op"] == "session_lock" {
                        if let Some(d) = self
                            .lock_dialog
                            .as_mut()
                            .filter(|d| d.server == server)
                        {
                            d.pending = false;
                            d.error = value["message"]
                                .as_str()
                                .unwrap_or("Не удалось изменить защиту сессии.")
                                .into();
                            return;
                        }
                    }
                    self.notice = value["message"].as_str().unwrap_or("").into();
                }
            }
            "fatal" => {
                self.notice = value["message"]
                    .as_str()
                    .unwrap_or("Не удалось запустить помощник.")
                    .into();
                self.remote.entry(server).or_default().connecting = false;
            }
            _ => {}
        }
    }
    fn send_prompt(&mut self) {
        let Some(mut server) = self.server() else {
            return;
        };
        let Some(k) = self.current_key() else {
            return;
        };
        if self.chats.get(&k).is_some_and(|c| c.locked && !c.unlocked) {
            return;
        }
        let backend = self.backend_for_chat(&server, &k);
        server.api.enabled = backend == "api";
        if let Some(run_id) = self.cluster_for_chat(&server.id, &self.chats[&k].sid) {
            if let Some(r) = self.cluster_store.runs.iter().find(|r| r.id == run_id) {
                if r.leader_sid != self.chats[&k].sid {
                    self.notice = "Отправь уточнение руководителю команды.".into();
                    return;
                }
            }
        }
        if self.local_command(&server, &k) {
            return;
        }
        if !self.demo && !self.connected(&server.id) {
            // Offline: keep the message and send it by itself as soon as the connection is back.
            let c = self.chats.get_mut(&k).unwrap();
            if c.enqueue_draft() {
                c.queue_paused = false;
                c.error.clear();
                c.status = "Ждёт подключения · отправится автоматически".into();
            }
            let connecting = self.remote.get(&server.id).is_some_and(|r| r.connecting);
            if !connecting {
                self.reconnect.remove(&server.id);
                self.connect(server);
            }
            return;
        }
        let live = self.remote.get(&server.id).is_some_and(|r| r.protocol >= 4);
        let can_steer = live && matches!(backend.as_str(), "claude" | "codex");
        let c = self.chats.get_mut(&k).unwrap();
        if (c.busy && !can_steer) || c.loading {
            if c.enqueue_draft() && c.loading {
                c.queue_paused = false;
            }
            return;
        }
        self.send_to_chat(server, k);
    }
    /// Reconnects servers whose connection dropped, with growing pauses between attempts.
    fn auto_reconnect(&mut self, ctx: &egui::Context) {
        if self.demo || self.reconnect.is_empty() {
            return;
        }
        let now = Instant::now();
        let mut due = vec![];
        let mut next: Option<Duration> = None;
        for (id, (at, _)) in &self.reconnect {
            let r = self.remote.get(id);
            if r.is_some_and(|r| r.connected || r.connecting) {
                continue;
            }
            if *at <= now {
                due.push(id.clone());
            } else {
                let wait = *at - now;
                next = Some(next.map_or(wait, |n| n.min(wait)));
            }
        }
        self.reconnect.retain(|id, _| self.settings.servers.iter().any(|s| &s.id == id));
        for id in due {
            if let Some(server) = self.settings.servers.iter().find(|s| s.id == id).cloned() {
                if let Some(entry) = self.reconnect.get_mut(&id) {
                    entry.0 = now + Duration::from_secs(60);
                }
                let notice = std::mem::take(&mut self.notice);
                self.connect(server);
                if !notice.is_empty() && !notice.starts_with("Связь") {
                    self.notice = notice;
                }
            }
        }
        if let Some(wait) = next {
            ctx.request_repaint_after(wait);
        }
    }
    /// Commands that act at once, also in the middle of an answer: `/model`, `/mode`, `/backend`, `/login`, `/stop`.
    fn local_command(&mut self, server: &Server, k: &str) -> bool {
        let backend_owned = self.backend_for_chat(server, k);
        let backend = backend_owned.as_str();
        let draft_trimmed = self.chats.get(k).map(|c| c.draft.trim().to_owned()).unwrap_or_default();
        if draft_trimmed == "/radar" || draft_trimmed == "/agents" {
            self.radar_open = Some(server.id.clone());
            if let Some(c) = self.chats.get_mut(k) {
                c.draft.clear();
            }
            return true;
        }
        if matches!(draft_trimmed.as_str(), "/clear" | "/new") {
            if let Some(c) = self.chats.get_mut(k) {
                c.draft.clear();
            }
            self.new_chat();
            self.notice = "Открыта новая чистая сессия.".into();
            return true;
        }
        if draft_trimmed == "/folder" || draft_trimmed.starts_with("/folder ") {
            let arg = draft_trimmed.strip_prefix("/folder").unwrap_or("").trim().to_owned();
            let move_session = self.chats.get(k).map(|c| {
                let cur_f = self
                    .remote
                    .get(&server.id)
                    .and_then(|r| r.sessions.iter().find(|s| s.id == c.sid))
                    .map(|s| s.folder.clone())
                    .unwrap_or_default();
                (c.sid.clone(), c.title.clone(), cur_f)
            });
            self.folder_dialog = Some(session_actions::FolderDialog {
                server: server.id.clone(),
                name: arg,
                rename_folder_id: None,
                move_session,
                focused_once: false,
                error: String::new(),
            });
            if let Some(c) = self.chats.get_mut(k) {
                c.draft.clear();
            }
            return true;
        }
        if draft_trimmed == "/theme" || draft_trimmed.starts_with("/theme ") {
            let arg = draft_trimmed.strip_prefix("/theme").unwrap_or("").trim().to_lowercase();
            if arg.is_empty() {
                self.settings_open = true;
            } else if let Some(&(id, title, is_light, _)) = crate::model::THEME_SPECS
                .iter()
                .find(|&&(id, lbl, _, _)| id == arg || lbl.to_lowercase() == arg)
            {
                self.settings.appearance.theme = id.into();
                self.settings.light = is_light;
                self.notice = format!("Тема: {title}");
                self.save();
            } else {
                self.settings_open = true;
            }
            if let Some(c) = self.chats.get_mut(k) {
                c.draft.clear();
            }
            return true;
        }
        if let Some(new_title) = draft_trimmed.strip_prefix("/rename ") {
            let trimmed_title = new_title.trim().to_owned();
            if !trimmed_title.is_empty() {
                if let Some(sid) = self.chats.get(k).map(|c| c.sid.clone()) {
                    self.controller.send(
                        &server.id,
                        json!({"op":"session_meta","sid":sid,"title":trimmed_title,"actor":self.settings.actor()}),
                    );
                }
                if let Some(c) = self.chats.get_mut(k) {
                    c.title = trimmed_title;
                    c.draft.clear();
                }
                return true;
            }
        }
        if draft_trimmed == "/backend" {
            let order = ["claude", "codex", "agy", "api"];
            let pos = order.iter().position(|&x| x == backend).unwrap_or(0);
            let next_b = order[(pos + 1) % order.len()];
            self.settings.set_session_backend(k.to_owned(), next_b);
            if let Some(c) = self.chats.get_mut(k) {
                c.draft.clear();
            }
            self.notice = format!("Агент: {}", crate::model::backend_label(next_b));
            self.save();
            return true;
        }
        if draft_trimmed == "/install" || draft_trimmed.starts_with("/install ") {
            let arg = draft_trimmed.strip_prefix("/install").unwrap_or("").trim();
            if arg.is_empty() {
                self.agents_open = Some(server.id.clone());
            } else if matches!(arg, "claude" | "codex" | "agy" | "all") {
                let act = if arg != "all" && self.backend_installed(&server.id, arg) {
                    "update"
                } else {
                    "install"
                };
                self.install_server_agent(&server.id, arg, act);
            } else {
                self.notice = "Доступные агенты: /install claude, /install codex, /install agy, /install all".into();
            }
            if let Some(c) = self.chats.get_mut(k) {
                c.draft.clear();
            }
            return true;
        }
        if draft_trimmed == "/lock" {
            if let Some(sid) = self.chats.get(k).map(|c| c.sid.clone()) {
                let is_unlocked_locked = self.chats.get(k).is_some_and(|c| c.locked && c.unlocked);
                if is_unlocked_locked {
                    self.relock_session(&server.id, &sid);
                } else {
                    self.open_lock_dialog(&server.id, &sid);
                }
            }
            if let Some(c) = self.chats.get_mut(k) {
                c.draft.clear();
            }
            return true;
        }
        if draft_trimmed == "/login" || draft_trimmed.starts_with("/login ") {
            let arg = draft_trimmed.strip_prefix("/login").unwrap_or("").trim();
            let prov = if arg.is_empty() {
                match backend {
                    "codex" | "agy" => backend,
                    _ => "claude",
                }
            } else {
                match crate::model::normalize_backend(arg) {
                    Some("codex") => "codex",
                    Some("agy") => "agy",
                    _ => "claude",
                }
            };
            self.start_provider_login(server, prov);
            if let Some(c) = self.chats.get_mut(k) {
                c.draft.clear();
            }
            return true;
        }
        if draft_trimmed.starts_with("/backend ") {
            let arg = draft_trimmed.strip_prefix("/backend").unwrap_or("").trim();
            if let Some(next_b) = crate::model::normalize_backend(arg) {
                if self.chats.get(k).is_some_and(|c| c.busy) {
                    if let Some(c) = self.chats.get_mut(k) {
                        c.error = "Дождись завершения ответа перед сменой агента.".into();
                        c.draft.clear();
                    }
                    return true;
                }
                self.settings.set_session_backend(k.to_owned(), next_b);
                if let Some(c) = self.chats.get_mut(k) {
                    if !c.cluster_context.is_object() {
                        c.model = server.model_for(next_b).to_owned();
                    }
                    c.draft.clear();
                }
                if matches!(next_b, "codex" | "agy") {
                    self.controller.send(&server.id, json!({"op":"catalog","backend":next_b}));
                }
                self.notice = format!("Агент: {}", crate::model::backend_label(next_b));
                self.save();
                return true;
            }
        }
        let models = self.models_for_backend(server, backend);
        let Some(c) = self.chats.get_mut(k) else {
            return false;
        };
        let text = c.draft.trim().to_owned();
        let (name, arg) = text.split_once(' ').map_or((text.as_str(), ""), |(a, b)| (a, b.trim()));
        let mut saved_profile_change = false;
        let done = match name {
            "/model" if !arg.is_empty() => match resolve_model(&models, arg).or_else(|| {
                (backend == "api" || (models.is_empty() && backend != "claude")).then(|| arg.to_owned())
            }).filter(|model| crate::model::explicit_model(model)) {
                Some(value) => {
                    if backend == "api" {
                        if let Some(profile) = self.settings.servers.iter_mut().find(|s| s.id == server.id) {
                            profile.api.model = value.clone();
                        }
                        saved_profile_change = true;
                    } else {
                        c.model = value.clone();
                        if let Some(profile) = self.settings.servers.iter_mut().find(|s| s.id == server.id) {
                            profile.set_model_for(backend, value.clone());
                        }
                        saved_profile_change = true;
                        self.controller.send(&server.id, json!({"op":"control","sid":c.sid,"backend":backend,"action":"model","value":value}));
                    }
                    self.notice = format!("Модель: {}", model_label(&models, &value));
                    true
                }
                None => {
                    c.error = format!("Нет такой модели: {arg}. Доступны: {}", models.iter().map(|m| m.value.as_str()).collect::<Vec<_>>().join(", "));
                    true
                }
            },
            "/mode" if !arg.is_empty() => {
                let mode = match arg.to_lowercase().as_str() {
                    "auto" | "авто" => "auto",
                    "plan" | "план" => "plan",
                    "edits" | "acceptedits" | "правки" => "acceptEdits",
                    "manual" | "ручной" => "manual",
                    _ => {
                        c.error = "Режим: auto, manual, edits или plan.".into();
                        return true;
                    }
                };
                c.mode = mode.into();
                self.controller.send(&server.id, json!({"op":"control","sid":c.sid,"backend":backend,"action":"mode","value":mode}));
                true
            }
            "/effort" if backend == "agy" => {
                self.notice = "В Antigravity глубина выбирается вместе с моделью.".into();
                true
            }
            "/effort" if !arg.is_empty() => {
                let eff = match arg.to_lowercase().as_str() {
                    "low" | "быстро" => "low",
                    "medium" | "средне" => "medium",
                    "high" | "глубоко" => "high",
                    "xhigh" => "xhigh",
                    "ultra" if backend == "codex" && models.iter().any(|m| m.value == c.model && m.ultra) => "ultra",
                    "max" | "макс" => "max",
                    _ => {
                        c.error = "Глубина: low, medium, high, xhigh или max.".into();
                        return true;
                    },
                };
                c.effort = eff.into();
                self.notice = format!("Глубина: {eff}");
                true
            }
            "/stop" => {
                self.controller.send(&server.id, json!({"op":"stop","sid":c.sid}));
                c.status = "Останавливаем…".into();
                c.queue_paused = true;
                true
            }
            "/usage" | "/limits" | "/quota" | "/reset-limits" | "/reset-usage" | "/cost" | "/tokens" | "/status" | "/info" | "/help" | "/commands"
                if c.busy && c.images.is_empty() =>
            {
                self.controller.send(
                    &server.id,
                    json!({"op":"prompt","backend":backend,"sid":c.sid,"cwd":c.cwd,"text":text}),
                );
                true
            }
            _ => false,
        };
        if done {
            c.draft.clear();
        }
        if done && saved_profile_change { self.save(); }
        done
    }
    fn send_to_chat(&mut self, mut server: Server, k: String) {
        if self
            .delete_chat
            .as_ref()
            .is_some_and(|d| d.pending && key(&d.server, &d.session.id) == k)
        {
            if let Some(c) = self.chats.get_mut(&k) {
                c.queue_paused = true;
            }
            return;
        }
        if let Err(error) = self.prepare_cluster_chat(&server, &k) {
            if let Some(c) = self.chats.get_mut(&k) {
                c.error = error;
                c.queue_paused = true;
            }
            return;
        }
        let backend = self.backend_for_chat(&server, &k);
        let is_quick_slash = self.chats.get(&k).is_some_and(|c| {
            c.images.is_empty()
                && matches!(
                    c.draft.split_whitespace().next().unwrap_or(""),
                    "/usage"
                        | "/limits"
                        | "/quota"
                        | "/reset-limits"
                        | "/reset-usage"
                        | "/cost"
                        | "/tokens"
                        | "/status"
                        | "/info"
                        | "/help"
                        | "/commands"
                )
        });
        server.api.enabled = backend == "api";
        if server.api.enabled && !is_quick_slash {
            let mut profile = server.api.clone();
            if let Err(error) = profile.validate() {
                if let Some(chat) = self.chats.get_mut(&k) { chat.error = error.to_string();chat.queue_paused = true; }
                self.open_api_settings(&server);
                return;
            }
        }
        if !self.connected(&server.id) {
            self.connect(server);
            return;
        }
        let worker = self
            .cluster_for_chat(&server.id, &self.chats[&k].sid)
            .and_then(|id| self.cluster_store.runs.iter().find(|r| r.id == id))
            .is_some_and(|r| r.leader_sid != self.chats[&k].sid);
        let live = self.remote.get(&server.id).is_some_and(|r| r.protocol >= 4);
        let can_steer = live && matches!(backend.as_str(), "claude" | "codex");
        let is_cluster = self.chats[&k].cluster_context.is_object();
        let mut api_request = json!({"model":if is_cluster { self.chats[&k].model.as_str() } else { "default" }});
        self.apply_api_request(&server, &mut api_request);
        let catalog = self.models_for_backend(&server, &backend);
        let c = self.chats.get_mut(&k).unwrap();
        if backend != "api" && !is_quick_slash {
            if is_cluster {
                let m = c.model.trim();
                if m.is_empty() || m.eq_ignore_ascii_case("inherit") {
                    c.model = "default".into();
                } else {
                    c.model = m.to_owned();
                }
            } else {
                c.model = model::selected_model(&c.model, &catalog, &backend);
                if c.model.is_empty() {
                    c.error = "Выбери модель перед отправкой.".into();
                    c.queue_paused = true;
                    return;
                }
            }
        }
        if is_cluster {
            if backend == "agy"
                || c.effort.trim().is_empty()
                || c.effort.eq_ignore_ascii_case("inherit")
            {
                c.effort = "default".into();
            }
        } else {
            c.effort = model::selected_effort(&c.effort).to_owned();
        }
        let steering = c.busy && can_steer;
        if (c.busy && !can_steer) || c.loading || (c.draft.trim().is_empty() && c.images.is_empty()) {
            return;
        }
        let text = c.draft.clone();
        let images: Vec<_> = c
            .images
            .iter()
            .filter(|a| a.source["type"] != "file")
            .map(|a| a.source.clone())
            .collect();
        let files: Vec<_> = c
            .images
            .iter()
            .filter(|a| a.source["type"] == "file")
            .map(|a| a.source.clone())
            .collect();
        self.controller.send(
            &server.id,
            json!({"op":"_desktop_workspace","path":if server.local_control {server.local_workspace.clone()}else{None}}),
        );
        self.controller.send(&server.id,json!({"op":"_desktop_policy","sid":c.sid,"auto":server.local_control && c.mode=="auto"}));
        let actor_str = self.settings.actor();
        let mut request = json!({"op":"prompt","backend":backend,"sid":c.sid,"cwd":c.cwd,"text":text,"images":images,"files":files,"model":c.model,"mode":c.mode,"effort":if backend == "agy" { "default" } else { &c.effort },"agent_role":c.agent_role,"custom_prompt":c.custom_prompt,"cluster_context":c.cluster_context,"actor":actor_str,"desktop_enabled":!worker && ((server.local_control && server.local_workspace.is_some()) || server.tests!="off"),"desktop_context":{"os":std::env::consts::OS,"workspace":server.local_workspace,"downloads":dirs::download_dir(),"auto":c.mode=="auto","local":server.local_control && server.local_workspace.is_some(),"tests":server.tests}});
        if server.api.enabled {
            request["backend"] = json!("api");
            request["model"] = api_request["model"].take();
            request["api"] = api_request["api"].take();
        }
        let sent_backend = request["backend"].as_str().unwrap_or(&backend).to_owned();
        let sent_model = request["model"].as_str().unwrap_or(&c.model).to_owned();
        let sent_mode = c.mode.clone();
        let sent_effort = if backend == "agy" { String::new() } else { c.effort.clone() };
        let sent_agent_name = c.cluster_context["name"].as_str().unwrap_or("").to_owned();
        if self.demo {
            c.error = "Это предпросмотр интерфейса. Добавь свой сервер для работы.".into();
            return;
        }
        let uploading = crate::uploads::needed(&request);
        if !self.controller.send(&server.id, request) {
            c.error = "Подключись к серверу перед отправкой.".into();
            return;
        }
        c.turn_backend = sent_backend.clone();
        c.turn_model = sent_model.clone();
        c.turn_mode = sent_mode.clone();
        c.turn_effort = sent_effort.clone();
        c.turn_actor = actor_str.clone();
        c.turn_agent_name = sent_agent_name.clone();
        c.user_at = c.messages.len();
        c.messages.push(Message {
            role: "user".into(),
            text,
            backend: sent_backend,
            model: sent_model,
            actor: actor_str,
            mode: sent_mode,
            effort: sent_effort,
            steer: steering,
            agent_name: sent_agent_name,
            ..Default::default()
        });
        if !c.images.is_empty() {
            c.sent_images
                .insert(c.user_at, std::mem::take(&mut c.images));
        }
        c.draft.clear();
        if !steering {
            // Mid-answer the stream keeps going into the blocks it already started.
            c.blocks.clear();
        }
        c.busy = true;
        c.upload_progress =
            uploading.then(|| (0, files.iter().filter_map(|f| f["bytes"].as_u64()).sum()));
        if c.cluster_context["role"] == "leader" {
            c.queue_paused = false;
        }
        c.error.clear();
        c.status = if uploading {
            "Отправка файлов"
        } else {
            "Думает над ответом"
        }
        .into();
        if c.title == "Новая сессия" {
            c.title = model::shorten(&c.messages[c.user_at].text, 60);
        }
        if self.selected_chat.get(&server.id) == Some(&c.sid) {
            if let Some(s) = self.settings.servers.iter_mut().find(|s| s.id == server.id) {
                s.last_session = Some(c.sid.clone());
            }
        }
        self.save();
    }
    fn rail(&mut self, ctx: &egui::Context) {
        let p = design::animated_palette(ctx, self.settings.light);
        let rail_panel = egui::SidePanel::left("rail")
            .exact_width(60.)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(p.rail)
                    .inner_margin(egui::Margin::symmetric(12, 14)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.;
                let mut marker = None;
                for server in self.settings.servers.clone() {
                    let selected = self.settings.selected.as_deref() == Some(&server.id);
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::splat(36.), egui::Sense::click());
                    let t = ctx.animate_bool_with_time(response.id.with("active"), selected, 0.2);
                    let hover = ctx.animate_bool_with_time(
                        response.id.with("hover"),
                        response.hovered(),
                        0.16,
                    );
                    ui.painter().rect_filled(
                        rect,
                        10.,
                        design::mix(
                            design::mix(p.rail, p.surface, hover),
                            accent().gamma_multiply(0.22),
                            t,
                        ),
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        server
                            .name
                            .chars()
                            .take(2)
                            .collect::<String>()
                            .to_uppercase(),
                        FontId::proportional(12.),
                        design::mix(p.muted, p.text, t.max(hover)),
                    );
                    if selected {
                        marker = Some((rect.left() - 9., rect.center().y));
                    }
                    let server_busy = self
                        .remote
                        .get(&server.id)
                        .is_some_and(|r| !r.running.is_empty())
                        || self
                            .chats
                            .iter()
                            .any(|(ck, cc)| ck.starts_with(&format!("{}/", server.id)) && cc.busy);
                    if server_busy {
                        ui.painter().rect_stroke(
                            rect.expand(2.0),
                            11.5,
                            Stroke::new(1.35_f32, accent().gamma_multiply(0.65)),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if self.connected(&server.id) {
                        let dot = rect.right_bottom() - Vec2::splat(3.);
                        ui.painter().circle_filled(dot, 4.5, p.rail);
                        ui.painter().circle_filled(dot, 3., if server_busy { accent() } else { GREEN });
                    }
                    if response
                        .on_hover_text(format!("{} · {}", server.name, server.host))
                        .clicked()
                    {
                        self.hide_browser();
                        self.settings.selected = Some(server.id.clone());
                        self.search.clear();
                        self.save();
                        if !self.connected(&server.id)
                            && !self.remote.get(&server.id).is_some_and(|r| r.connecting)
                        {
                            self.connect(server);
                        }
                    }
                }
                if let Some((x, y)) = marker {
                    let y = design::glide(ctx, egui::Id::new("server-selection-y"), y);
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(egui::pos2(x, y), Vec2::new(3., 18.)),
                        2.,
                        accent(),
                    );
                }
                if design::button(ui, "", Some(Icon::Plus), Kind::Ghost, p, Vec2::splat(36.))
                    .on_hover_text("Добавить сервер")
                    .clicked()
                {
                    self.editor = Some(Editor {
                        server: Server::default(),
                        password: String::new(),
                        is_new: true,
                        error: String::new(),
                    });
                }
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    if design::button(
                        ui,
                        "",
                        Some(Icon::Settings),
                        Kind::Ghost,
                        p,
                        Vec2::splat(36.),
                    )
                    .on_hover_text("Темы и настройки")
                    .clicked()
                    {
                        self.settings_open = true;
                    }
                    if design::button(
                        ui,
                        "",
                        Some(Icon::Cluster),
                        if self.cluster_open {
                            Kind::Secondary
                        } else {
                            Kind::Ghost
                        },
                        p,
                        Vec2::splat(36.),
                    )
                    .on_hover_text("Кластер агентов")
                    .clicked()
                    {
                        self.hide_browser();
                        self.cluster_open = !self.cluster_open;
                        self.board_open = false;
                    }
                    let board = design::button(
                        ui,
                        "",
                        Some(Icon::Board),
                        if self.board_open {
                            Kind::Secondary
                        } else {
                            Kind::Ghost
                        },
                        p,
                        Vec2::splat(36.),
                    )
                    .on_hover_text(if self.board_open {
                        "Вернуться к разговорам"
                    } else {
                        "Доска задач команды"
                    });
                    if board.clicked() {
                        self.hide_browser();
                        self.board_open = !self.board_open;
                        self.cluster_open = false;
                        self.board_polled = None;
                    }
                    if design::button(
                        ui,
                        "",
                        Some(if self.settings.light {
                            Icon::Moon
                        } else {
                            Icon::Sun
                        }),
                        Kind::Ghost,
                        p,
                        Vec2::splat(36.),
                    )
                    .on_hover_text("Сменить тему")
                    .clicked()
                    {
                        self.settings.light = !self.settings.light;
                        self.settings.appearance.theme = if self.settings.light {
                            "paper"
                        } else {
                            "graphite"
                        }
                        .into();
                        self.theme(ctx);
                        self.save();
                    }
                });
            });
        let rr = rail_panel.response.rect;
        ctx.layer_painter(rail_panel.response.layer_id).line_segment(
            [egui::pos2(rr.right() - 0.5, rr.top()), egui::pos2(rr.right() - 0.5, rr.bottom())],
            Stroke::new(1.0_f32, design::mix(p.rail, p.line, 0.55)),
        );
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        let p = design::animated_palette(ctx, self.settings.light);
        let side_panel = egui::SidePanel::left("sessions")
            .default_width(276.)
            .width_range(240.0..=360.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(p.side)
                    .inner_margin(egui::Margin::symmetric(14, 14)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 7.;
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), 30.),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 6.;
                        ui.label(RichText::new("Relay").size(18.5).color(p.text).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 5.;
                            if design::button(
                                ui,
                                "",
                                Some(Icon::Settings),
                                Kind::Secondary,
                                p,
                                Vec2::splat(28.),
                            )
                            .on_hover_text("Настройки подключения")
                            .clicked()
                            {
                                if let Some(server) = self.server() {
                                    self.editor = Some(Editor {
                                        server,
                                        password: String::new(),
                                        is_new: false,
                                        error: String::new(),
                                    });
                                }
                            }
                            if let Some(server) = self.server() {
                                if design::button(
                                    ui,
                                    "",
                                    Some(Icon::Bolt),
                                    if server.api.enabled {
                                        Kind::Primary
                                    } else {
                                        Kind::Secondary
                                    },
                                    p,
                                    Vec2::splat(28.),
                                )
                                .on_hover_text("Пользовательский API этого сервера")
                                .clicked()
                                {
                                    self.open_api_settings(&server);
                                }
                                if design::button(
                                    ui,
                                    "",
                                    Some(Icon::Download),
                                    if self.agents_open.as_deref() == Some(server.id.as_str()) {
                                        Kind::Primary
                                    } else {
                                        Kind::Secondary
                                    },
                                    p,
                                    Vec2::splat(28.),
                                )
                                .on_hover_text("CLI-агенты: установка и обновление Claude, Codex и AGY")
                                .clicked()
                                {
                                    self.agents_open = Some(server.id.clone());
                                }
                            }
                        });
                    },
                );
                ui.add_space(2.);
                if design::button(
                    ui,
                    "Новая сессия",
                    Some(Icon::Plus),
                    Kind::Primary,
                    p,
                    Vec2::new(ui.available_width(), 36.),
                )
                .on_hover_text("Ctrl+N")
                .clicked()
                {
                    self.new_chat();
                }
                egui::Frame::new()
                    .fill(p.base)
                    .stroke(Stroke::new(1.0_f32, p.line))
                    .corner_radius(9.)
                    .inner_margin(egui::Margin::symmetric(11, 7))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), 19.),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                let (r, _) =
                                    ui.allocate_exact_size(Vec2::splat(14.), egui::Sense::hover());
                                design::icon(ui.painter(), r, Icon::Search, p.muted);
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.search)
                                        .frame(false)
                                        .hint_text(RichText::new("Поиск сессий…").color(p.muted))
                                        .font(FontId::proportional(13.0))
                                        .desired_width(ui.available_width()),
                                );
                            },
                        );
                    });
                let Some(server) = self.server() else {
                    ui.add_space(10.);
                    label(ui, "Подключи свой первый сервер", 13., p.muted);
                    return;
                };
                let connected = self.connected(&server.id);
                let connecting = self.remote.get(&server.id).is_some_and(|r| r.connecting);
                egui::TopBottomPanel::bottom("connection-footer")
                    .frame(egui::Frame::NONE)
                    .show_separator_line(false)
                    .show_inside(ui, |ui| {
                        ui.add_space(8.);
                        egui::Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, design::mix(p.side,p.line,0.65)))
                            .corner_radius(12.)
                            .inner_margin(egui::Margin::symmetric(12, 10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().interact_size.y = 16.;
                                ui.spacing_mut().item_spacing.y = 3.;
                                ui.horizontal(|ui| {
                                    let (r, _) = ui
                                        .allocate_exact_size(Vec2::splat(8.), egui::Sense::hover());
                                    ui.painter().circle_filled(
                                        r.center(),
                                        3.5,
                                        if connected {
                                            GREEN
                                        } else if connecting {
                                            accent()
                                        } else {
                                            p.muted
                                        },
                                    );
                                    label(ui, &server.name, 13., p.text);
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        label(
                                            ui,
                                            if connected {
                                                "на связи"
                                            } else if connecting {
                                                "подключаемся…"
                                            } else {
                                                "нет связи"
                                            },
                                            11.5,
                                            p.muted,
                                        );
                                    });
                                })
                                .response
                                .on_hover_text(format!("{}@{}", server.user, server.host));
                                if connected {
                                    let active_backend = self
                                        .selected_chat
                                        .get(&server.id)
                                        .map(|sid| self.backend_for_chat(&server, &key(&server.id, sid)))
                                        .unwrap_or_else(|| server.effective_backend().to_owned());
                                    if let Some(remote) = self.remote.get(&server.id) {
                                        let installing_msg = remote.installing_agents.get(&active_backend).cloned();
                                        let cli_installed = self.backend_installed(&server.id, &active_backend);
                                        ui.add_space(6.);
                                        if let Some(progress) = installing_msg {
                                            ui.horizontal(|ui| {
                                                design::dots(ui, accent());
                                                label(ui, progress, 11.5, accent());
                                            });
                                        } else {
                                            match active_backend.as_str() {
                                                "api" => {
                                                    label(ui, "Пользовательский API", 12., p.muted);
                                                    if remote.api_tokens != (0, 0) {
                                                        label(ui, format!("Токены: {} вход · {} выход", remote.api_tokens.0, remote.api_tokens.1), 11., p.muted);
                                                    }
                                                }
                                                "codex" => {
                                                    let ver = remote.backends["codex"]["version"].as_str().unwrap_or("").to_owned();
                                                    if !cli_installed {
                                                        if ui.add(egui::Label::new(RichText::new("Codex не установлен · установить").size(11.5).color(accent())).sense(egui::Sense::click())).clicked() {
                                                            self.install_server_agent(&server.id, "codex", "install");
                                                        }
                                                    } else {
                                                        let text = if ver.is_empty() { "OpenAI Codex · обновить".to_owned() } else { format!("Codex · {ver} · обновить") };
                                                        if ui.add(egui::Label::new(RichText::new(text).size(11.).color(p.muted)).truncate().sense(egui::Sense::click()))
                                                            .on_hover_text("Нажми, чтобы обновить OpenAI Codex CLI на сервере")
                                                            .clicked()
                                                        {
                                                            self.install_server_agent(&server.id, "codex", "update");
                                                        }
                                                    }
                                                }
                                                "agy" => {
                                                    let ver = remote.backends["agy"]["version"].as_str().unwrap_or("").to_owned();
                                                    if !cli_installed {
                                                        if ui.add(egui::Label::new(RichText::new("AGY не установлен · установить").size(11.5).color(accent())).sense(egui::Sense::click())).clicked() {
                                                            self.install_server_agent(&server.id, "agy", "install");
                                                        }
                                                    } else {
                                                        let text = if ver.is_empty() { "Antigravity · обновить".to_owned() } else { format!("{ver} · обновить") };
                                                        if ui.add(egui::Label::new(RichText::new(text).size(11.).color(p.muted)).truncate().sense(egui::Sense::click()))
                                                            .on_hover_text("Нажми, чтобы обновить Antigravity CLI на сервере")
                                                            .clicked()
                                                        {
                                                            self.install_server_agent(&server.id, "agy", "update");
                                                        }
                                                    }
                                                }
                                                _ => {
                                                    if !cli_installed {
                                                        if ui.add(egui::Label::new(RichText::new("Claude Code не установлен · установить").size(11.5).color(accent())).sense(egui::Sense::click())).clicked() {
                                                            self.install_server_agent(&server.id, "claude", "install");
                                                        }
                                                    } else if remote.protocol >= 4 {
                                                        ui.add_space(4.);
                                                        let text = if remote.updating {
                                                            format!("{} · обновляется…", remote.version)
                                                        } else {
                                                            format!("{} · обновить", remote.version)
                                                        };
                                                        let response = ui
                                                            .add(egui::Label::new(RichText::new(text).size(11.).color(p.muted)).truncate().sense(egui::Sense::click()))
                                                            .on_hover_text("Claude Code на сервере обновляется сам раз в 6 часов, когда ничего не делает. Нажми, чтобы обновить сейчас.");
                                                        if response.clicked() && !remote.updating {
                                                            self.controller.send(&server.id, json!({"op":"claude_update"}));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if let Some(remote) = self.remote.get(&server.id) {
                                            // Постоянный блок лимитов: все установленные агенты сервера, обновляется раз в 5 минут.
                                            let agents: Vec<(&str, &str)> = [("claude", "Claude"), ("codex", "Codex"), ("agy", "Antigravity")]
                                                .into_iter()
                                                .filter(|(id, _)| self.backend_installed(&server.id, id))
                                                .collect();
                                            if !agents.is_empty() {
                                                ui.add_space(8.);
                                                egui::CollapsingHeader::new("Лимиты")
                                                    .id_salt(("quota-header", &server.id))
                                                    .default_open(false)
                                                    .show(ui, |ui| {
                                                        egui::ScrollArea::vertical()
                                                            .id_salt(("server-quotas", &server.id))
                                                            .max_height((ui.ctx().screen_rect().height() * 0.19).clamp(72., 132.))
                                                            .auto_shrink([false, true])
                                                            .show(ui, |ui| {
                                                                for (id, name) in agents {
                                                                    ui.add_space(4.);
                                                                    label(ui, name, 11.5, if id == active_backend { accent() } else { p.text });
                                                                    let usage = remote
                                                                        .usage_by
                                                                        .get(id)
                                                                        .cloned()
                                                                        .unwrap_or_else(|| if id == "claude" { remote.usage.clone() } else { Usage::default() });
                                                                    limits::show(ui, &usage, p, connected, name);
                                                                }
                                                            });
                                                    });
                                            }
                                    }
                                    ui.add_space(8.);
                                    let active_browser =
                                        self.login.as_ref().is_some_and(|l| l.server == server.id && l.active());
                                    let (primary_prov, primary_label, primary_hint) = match active_backend.as_str() {
                                        "codex" => ("codex", "Войти в Codex", "Войти в учётную запись OpenAI Codex на этом сервере"),
                                        "agy" => ("agy", "Войти в AGY", "Войти в учётную запись Google Antigravity на этом сервере"),
                                        _ => ("claude", "Войти в Claude", "Войти в учётную запись Claude на этом сервере"),
                                    };
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.;
                                        let half_w = ((ui.available_width() - 6.) * 0.5).max(60.);
                                        if design::button(
                                            ui,
                                            "Браузер",
                                            Some(Icon::Browser),
                                            if active_browser { Kind::Primary } else { Kind::Secondary },
                                            p,
                                            Vec2::new(half_w, 30.),
                                        )
                                        .on_hover_text("Открыть браузер на сервере")
                                        .clicked()
                                        {
                                            self.start_browser(&server);
                                        }
                                        if design::button(
                                            ui,
                                            "Агенты",
                                            Some(Icon::Download),
                                            if self.agents_open.as_deref() == Some(server.id.as_str()) {
                                                Kind::Primary
                                            } else {
                                                Kind::Secondary
                                            },
                                            p,
                                            Vec2::new(half_w, 30.),
                                        )
                                        .on_hover_text("Установка, обновление и вход для Claude, Codex и AGY")
                                        .clicked()
                                        {
                                            self.agents_open = Some(server.id.clone());
                                        }
                                    });
                                    ui.add_space(4.);
                                    if design::service_action(ui, "Связать телефон", Icon::Terminal, false, p).clicked() && !self.demo {
                                        match relay_desktop::phone_tests::start_from_ui(&server) {
                                            Ok(code) => self.phone_pairing = Some((server.id.clone(), zeroize::Zeroizing::new(code))),
                                            Err(error) => self.notice = error.to_string(),
                                        }
                                    }
                                    let login_btn = design::service_action(ui, primary_label, Icon::Key, false, p)
                                        .on_hover_text(format!("{primary_hint} (ПКМ — выбрать другого агента)"));
                                    if login_btn.clicked() {
                                        self.start_provider_login(&server, primary_prov);
                                    }
                                    login_btn.context_menu(|ui| {
                                        for (prov, title) in [
                                            ("claude", "Войти в Claude Code"),
                                            ("codex", "Войти в OpenAI Codex"),
                                            ("agy", "Войти в Google Antigravity"),
                                        ] {
                                            if ui.button(title).clicked() {
                                                self.start_provider_login(&server, prov);
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                }
                                if !connected && !connecting {
                                    ui.add_space(6.);
                                    if design::button(
                                        ui,
                                        "Подключиться",
                                        Some(Icon::Arrow),
                                        Kind::Primary,
                                        p,
                                        Vec2::new(ui.available_width(), 30.),
                                    )
                                    .clicked()
                                    {
                                        self.connect(server.clone());
                                    }
                                }
                            });
                    });
                let sessions: Vec<Session> = self
                    .remote
                    .get(&server.id)
                    .map(|r| r.sessions.clone())
                    .unwrap_or_default();
                let folders = self
                    .remote
                    .get(&server.id)
                    .map(|r| r.folders.clone())
                    .unwrap_or_default();
                let query = self.search.to_lowercase();
                let refreshing = self
                    .remote
                    .get(&server.id)
                    .is_some_and(|r| connected && !r.sessions_loaded);

                // Live Active Agents Radar Card when one or more sessions are working
                let mut radar_jump: Option<Session> = None;
                let active_sessions: Vec<Session> = sessions
                    .iter()
                    .filter(|s| {
                        let k = key(&server.id, &s.id);
                        s.running
                            || self.chats.get(&k).is_some_and(|c| c.busy)
                            || self
                                .remote
                                .get(&server.id)
                                .is_some_and(|r| r.running.contains(&s.id))
                    })
                    .cloned()
                    .collect();
                if !active_sessions.is_empty() {
                    let light = self.settings.light;
                    ui.add_space(4.);
                    egui::CollapsingHeader::new(format!("Радар агентов · {}", active_sessions.len()))
                        .id_salt(("radar-header", &server.id))
                        .default_open(false)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt(("radar-list", &server.id))
                                .max_height((ui.ctx().screen_rect().height() * 0.18).clamp(64., 128.))
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    for act in &active_sessions {
                                        let ak = key(&server.id, &act.id);
                                        let b = self.backend_for_chat(&server, &ak);
                                        let b_col = design::backend_color(&b, light);
                                        let step_hint = self
                                            .chats
                                            .get(&ak)
                                            .map(|c| {
                                                let t = crate::model::analyze_turn(
                                                    &c.messages,
                                                    c.user_at,
                                                    c.busy,
                                                    &c.status,
                                                );
                                                if !t.active_target.is_empty() {
                                                    t.active_target
                                                } else {
                                                    t.active_headline
                                                }
                                            })
                                            .filter(|s| !s.is_empty())
                                            .unwrap_or_else(|| "В работе…".into());
                                        let r_resp = egui::Frame::new()
                                            .fill(p.base.gamma_multiply(0.65))
                                            .corner_radius(7.)
                                            .inner_margin(egui::Margin::symmetric(7, 4))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.horizontal(|ui| {
                                                    let (dot_r, _) = ui.allocate_exact_size(
                                                        Vec2::splat(8.),
                                                        egui::Sense::hover(),
                                                    );
                                                    ui.painter().circle_filled(dot_r.center(), 3.2, b_col);
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(model::shorten(&act.title, 22))
                                                                .size(11.5)
                                                                .color(p.text),
                                                        )
                                                        .truncate(),
                                                    );
                                                });
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(model::shorten(&step_hint, 28))
                                                            .monospace()
                                                            .size(10.2)
                                                            .color(b_col),
                                                    )
                                                    .truncate(),
                                                );
                                            })
                                            .response;
                                        if ui
                                            .interact(
                                                r_resp.rect,
                                                ui.id().with(("radar-jump", &act.id)),
                                                egui::Sense::click(),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            radar_jump = Some(act.clone());
                                        }
                                    }
                                });
                        });
                }
                if let Some(target_sess) = radar_jump {
                    self.select_chat(target_sess);
                }

                ui.add_space(4.);
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), 26.),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add_space(4.);
                        label(ui, "СЕССИИ", 10.5, p.muted);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.;
                            if refreshing {
                                design::dots(ui, p.muted);
                            } else if design::button(ui, "", Some(Icon::Refresh), Kind::Ghost, p, Vec2::splat(24.))
                                .on_hover_text("Обновить список и открытый разговор")
                                .clicked()
                            {
                                self.refresh(&server.id);
                            }
                            if connected
                                && design::button(ui, "+ Папка", None, Kind::Ghost, p, Vec2::new(0., 24.))
                                    .on_hover_text("Открыть окно создания общей папки для сессий")
                                    .clicked()
                            {
                                self.folder_dialog = Some(session_actions::FolderDialog {
                                    server: server.id.clone(),
                                    name: String::new(),
                                    rename_folder_id: None,
                                    move_session: None,
                                    focused_once: false,
                                    error: String::new(),
                                });
                            }
                        });
                    },
                );
                let matches = |s: &Session| {
                    query.is_empty()
                        || s.title.to_lowercase().contains(&query)
                        || s.auto_title.to_lowercase().contains(&query)
                        || s.cwd.to_lowercase().contains(&query)
                };
                let known: std::collections::HashSet<&str> = folders.iter().map(|f| f.id.as_str()).collect();
                let mut pinned: Vec<&Session> = sessions.iter().filter(|s| s.pinned > 0. && matches(s)).collect();
                pinned.sort_by(|a, b| b.pinned.total_cmp(&a.pinned));
                let loose: Vec<&Session> = sessions
                    .iter()
                    .filter(|s| s.pinned == 0. && !known.contains(s.folder.as_str()) && matches(s))
                    .collect();
                let mut actions: Vec<(Session, RowAction)> = vec![];
                egui::ScrollArea::vertical()
                    .id_salt("session-list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.;
                        let mut shown = 0;
                        if !pinned.is_empty() {
                            section_heading(ui, Some("📌"), "Закреплённые", pinned.len(), None, p, None);
                            for session in &pinned {
                                if let Some(action) = self.session_row(ui, ctx, &server.id, session, &folders, connected, p) {
                                    actions.push(((*session).clone(), action));
                                }
                                shown += 1;
                            }
                        }
                        for folder in &folders {
                            let inside: Vec<&Session> = sessions
                                .iter()
                                .filter(|s| s.pinned == 0. && s.folder == folder.id && matches(s))
                                .collect();
                            if !folder.closed && !query.is_empty() && inside.is_empty() {
                                continue;
                            }
                            let collapsed = folder.closed || (self.settings.collapsed_folders.contains(&folder.id) && query.is_empty());
                            let texture = project_icon::folder_texture(ctx, &folder.icon);
                            let header = section_heading(ui, Some(if folder.closed { "🔒" } else if collapsed { "📁" } else { "📂" }), &folder.name, inside.len(), Some(collapsed), p, texture.as_ref());
                            let password_dialog = |action| FolderPassword {
                                server: server.id.clone(), folder: folder.id.clone(), name: folder.name.clone(),
                                action, password: String::new(), confirm: String::new(),
                            };
                            if folder.closed && header.clicked() {
                                self.folder_password = Some(password_dialog("open"));
                            } else if header.clicked() {
                                if collapsed {
                                    self.settings.collapsed_folders.retain(|f| f != &folder.id);
                                } else {
                                    self.settings.collapsed_folders.push(folder.id.clone());
                                }
                                self.save();
                            }
                            if !folder.closed && header.double_clicked() {
                                self.folder_dialog = Some(session_actions::FolderDialog {
                                    server: server.id.clone(),
                                    name: folder.name.clone(),
                                    rename_folder_id: Some(folder.id.clone()),
                                    move_session: None,
                                    focused_once: false,
                                    error: String::new(),
                                });
                            }
                            header.context_menu(|ui| {
                                if folder.closed {
                                    if ui.button("Открыть паролем…").clicked() {
                                        self.folder_password = Some(password_dialog("open"));
                                        ui.close_menu();
                                    }
                                    return;
                                }
                                if folder.protected && ui.button("Закрыть папку").clicked() {
                                    self.controller.send(&server.id, json!({"op":"folder_access","action":"close","id":folder.id}));
                                    ui.close_menu();
                                }
                                if ui.button(if folder.protected { "Снять пароль…" } else { "Задать пароль…" }).clicked() {
                                    self.folder_password = Some(password_dialog(if folder.protected { "remove" } else { "set" }));
                                    ui.close_menu();
                                }
                                if ui.button("Значок проекта…").clicked() {
                                    self.icon_dialog = Some(project_icon::IconDialog::new(server.id.clone(), folder));
                                    ui.close_menu();
                                }
                                if ui.button("Переименовать папку…").clicked() {
                                    self.folder_dialog = Some(session_actions::FolderDialog {
                                        server: server.id.clone(),
                                        name: folder.name.clone(),
                                        rename_folder_id: Some(folder.id.clone()),
                                        move_session: None,
                                        focused_once: false,
                                        error: String::new(),
                                    });
                                    ui.close_menu();
                                }
                                if ui
                                    .button(RichText::new("Удалить папку").color(design::RED))
                                    .on_hover_text("Сессии останутся, просто без папки")
                                    .clicked()
                                {
                                    self.controller.send(&server.id, json!({"op":"session_folder","id":folder.id,"delete":true}));
                                    ui.close_menu();
                                }
                            });
                            if collapsed {
                                continue;
                            }
                            if inside.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.add_space(30.);
                                    label(ui, "Пусто — перенеси сюда сессию через «⋯»", 11.5, p.muted.gamma_multiply(0.8));
                                });
                            }
                            for session in inside {
                                if let Some(action) = self.session_row(ui, ctx, &server.id, session, &folders, connected, p) {
                                    actions.push((session.clone(), action));
                                }
                                shown += 1;
                            }
                        }
                        let mut group = "";
                        for session in &loose {
                            let heading = day_group(session.modified);
                            if heading != group {
                                section_heading(ui, None, heading, 0, None, p, None);
                                group = heading;
                            }
                            if let Some(action) = self.session_row(ui, ctx, &server.id, session, &folders, connected, p) {
                                actions.push(((*session).clone(), action));
                            }
                            shown += 1;
                        }
                        if shown == 0 && folders.is_empty() {
                            ui.add_space(14.);
                            ui.horizontal_wrapped(|ui| {
                                ui.add_space(8.);
                                if refreshing {
                                    design::dots(ui, p.muted);
                                }
                                let error = self
                                    .remote
                                    .get(&server.id)
                                    .map(|r| r.sessions_error.clone())
                                    .unwrap_or_default();
                                label(
                                    ui,
                                    if !connected && !connecting {
                                        "Подключись, чтобы увидеть сессии"
                                    } else if refreshing || connecting {
                                        if error.is_empty() { "Загружаем сессии…" } else { &error }
                                    } else if query.is_empty() {
                                        "Пока нет ни одной сессии"
                                    } else {
                                        "Ничего не найдено"
                                    },
                                    12.5,
                                    p.muted,
                                );
                            });
                        }
                        ui.add_space(8.);
                    });
                for (session, action) in actions {
                    self.row_action(&server.id, session, action);
                }
            });
        let sr = side_panel.response.rect;
        ctx.layer_painter(side_panel.response.layer_id).line_segment(
            [egui::pos2(sr.right() - 0.5, sr.top()), egui::pos2(sr.right() - 0.5, sr.bottom())],
            Stroke::new(1.0_f32, design::mix(p.side, p.line, 0.58)),
        );
    }
    /// One session in the sidebar; returns what the user asked for.
    #[allow(clippy::too_many_arguments)]
    fn session_row(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        server: &str,
        session: &Session,
        folders: &[crate::model::SessionFolder],
        connected: bool,
        p: Palette,
    ) -> Option<RowAction> {
        let mut action = None;
        let k = key(server, &session.id);
        let selected = self.selected_chat.get(server) == Some(&session.id);
        let busy = self.chats.get(&k).is_some_and(|c| c.busy)
            || self.remote.get(server).is_some_and(|r| r.running.contains(&session.id));
        let title = self.cluster_label(server, &session.id).unwrap_or_else(|| session.title.clone());
        if let Some((sid, text)) = self.renaming.clone().filter(|(sid, _)| *sid == session.id) {
            let mut text = text;
            let hint = if session.is_locked_gate() { "Метка на этом устройстве · Enter" } else { "Название сессии · Enter" };
            match folder_name_field(ui, &mut text, hint, p) {
                Some(true) => {
                    action = Some(RowAction::Rename(text.trim().to_owned()));
                    self.renaming = None;
                }
                Some(false) => self.renaming = None,
                None => self.renaming = Some((sid, text)),
            }
            return action;
        }
        let background = ui.painter().add(egui::Shape::Noop);
        let row = egui::Frame::new()
            .inner_margin(egui::Margin { left: 15, right: 10, top: 8, bottom: 8 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width().max(120.));
                ui.spacing_mut().item_spacing.y = 2.;
                ui.allocate_ui_with_layout(
                    Vec2::new((ui.available_width() - 30.).max(60.), 0.),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 5.;
                        if session.locked {
                            let (lr, _) = ui.allocate_exact_size(Vec2::splat(13.), egui::Sense::hover());
                            design::icon(
                                ui.painter(),
                                lr,
                                if session.unlocked { Icon::Unlock } else { Icon::Lock },
                                if session.unlocked { GREEN } else { accent() },
                            );
                        }
                        design::session_title(
                            ui,
                            &title,
                            if selected { p.text } else { design::mix(p.text, p.muted, 0.22) },
                        );
                    },
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().interact_size.y = 14.;
                    ui.spacing_mut().item_spacing.x = 5.;
                    let place = folder_name(&session.cwd);
                    let row_backend = if crate::model::valid_backend(&session.backend) {
                        session.backend.as_str()
                    } else if let Some(b) = self.settings.session_backends.get(&k).filter(|b| crate::model::valid_backend(b)) {
                        b.as_str()
                    } else {
                        "claude"
                    };
                    let b_col = design::backend_color(row_backend, self.settings.light);
                    let b_short = match row_backend {
                        "codex" => "Codex",
                        "agy" => "AGY",
                        "api" => "API",
                        _ => "Claude",
                    };
                    let b_galley = ui.painter().layout_no_wrap(
                        b_short.into(),
                        FontId::proportional(9.5),
                        b_col,
                    );
                    let (br, _) = ui.allocate_exact_size(
                        Vec2::new(b_galley.size().x + 9., 15.),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(br, 4.5, design::mix(p.surface, b_col, 0.16));
                    ui.painter().galley(
                        egui::pos2(br.left() + 4.5, br.center().y - b_galley.size().y * 0.5),
                        b_galley,
                        b_col,
                    );

                    let meta = if busy {
                        format!("активен · {place}")
                    } else {
                        format!("{place} · {}", relative(session.modified))
                    };
                    let width = (ui.available_width() - 30. - if busy { 28. } else { 0. }).max(24.);
                    let mut job = egui::text::LayoutJob::simple(
                        meta,
                        FontId::proportional(11.2),
                        if busy { b_col } else { p.muted },
                        width,
                    );
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = true;
                    job.wrap.overflow_character = Some('…');
                    ui.label(job);
                    if busy {
                        design::waveform(ui, b_col, 11.);
                    }
                });
            })
            .response;
        let response = ui.interact(row.rect, ui.id().with(&session.id), egui::Sense::click());
        let t = ctx.animate_bool_with_time(response.id.with("active"), selected, 0.18);
        let hover = ctx.animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
        let mut shapes = vec![egui::Shape::rect_filled(row.rect, 10., design::mix(p.side, design::mix(p.surface, accent(), 0.11), t.max(hover * 0.45)))];
        if selected {
            shapes.push(egui::Shape::rect_stroke(
                row.rect,
                10.,
                Stroke::new(1.0_f32, design::mix(p.line, accent(), 0.30)),
                egui::StrokeKind::Inside,
            ));
            let pill_h = (row.rect.height() - 24.).clamp(12., 19.);
            shapes.push(egui::Shape::rect_filled(
                egui::Rect::from_center_size(
                    egui::pos2(row.rect.left() + 5.5, row.rect.center().y),
                    Vec2::new(3., pill_h),
                ),
                1.5,
                accent(),
            ));
        }
        ui.painter().set(background, egui::Shape::Vec(shapes));
        let dots_rect = egui::Rect::from_center_size(
            egui::pos2(row.rect.right() - 18., row.rect.center().y),
            Vec2::splat(24.),
        );
        let menu_id = response.id.with("menu");
        let open = ui.memory(|m| m.is_popup_open(menu_id));
        let dots = ui.interact(dots_rect, response.id.with("dots"), egui::Sense::click());
        if response.hovered() || dots.hovered() || open || selected {
            if dots.hovered() || open {
                ui.painter().rect_filled(dots_rect, 7., p.line);
            }
            let dot_col = if dots.hovered() || open { p.text } else { p.muted };
            for dx in [-4.5_f32, 0.0, 4.5] {
                ui.painter()
                    .circle_filled(dots_rect.center() + Vec2::new(dx, 0.), 1.35, dot_col);
            }
        }
        if dots.clicked() {
            ui.memory_mut(|m| m.toggle_popup(menu_id));
        }
        let reason = self.deletion_blocked(server, &session.id);
        egui::popup::popup_below_widget(ui, menu_id, &dots, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(210.);
            if let Some(a) = session_menu(ui, session, folders, connected, reason, p) {
                action = Some(a);
            }
        });
        response.context_menu(|ui| {
            ui.set_min_width(210.);
            if let Some(a) = session_menu(ui, session, folders, connected, reason, p) {
                action = Some(a);
                ui.close_menu();
            }
        });
        if response.double_clicked() && connected && !session.is_locked_gate() {
            self.renaming = Some((session.id.clone(), session.title.clone()));
        } else if response.hovered() {
            self.prefetch(server, session);
        }
        let hint = if session.is_locked_gate() {
            "Защищённая паролем сессия · нажми, чтобы разблокировать".to_owned()
        } else if session.auto_title.is_empty() {
            format!("{}\n{}", session.title, session.cwd)
        } else {
            format!("{}\nClaude назвал: {}\n{}", session.title, session.auto_title, session.cwd)
        };
        if response.on_hover_text(hint).clicked() && action.is_none() && !dots.clicked() {
            action = Some(RowAction::Open);
        }
        action
    }
    fn row_action(&mut self, server: &str, session: Session, action: RowAction) {
        let actor = self.settings.actor();
        match action {
            RowAction::Open => self.select_chat(session),
            RowAction::Delete => self.request_delete(server, session),
            RowAction::ManageLock => self.open_lock_dialog(server, &session.id),
            RowAction::Relock => self.relock_session(server, &session.id),
            RowAction::Pin(on) => {
                self.controller.send(server, json!({"op":"session_meta","sid":session.id,"pinned":on,"actor":actor}));
            }
            RowAction::StartRename => self.renaming = Some((session.id.clone(), session.title.clone())),
            RowAction::Rename(title) => {
                if session.is_locked_gate() {
                    let k = key(server, &session.id);
                    if title.is_empty() {
                        self.settings.locked_session_names.remove(&k);
                    } else {
                        self.settings.locked_session_names.insert(k.clone(), model::shorten(&title, 120));
                    }
                    let title = self.settings.label_locked_session(&k, &session.id, session.modified);
                    if let Some(row) = self.remote.get_mut(server).and_then(|r| r.sessions.iter_mut().find(|s| s.id == session.id)) {
                        row.title.clone_from(&title);
                    }
                    if let Some(chat) = self.chats.get_mut(&k) {
                        chat.title = title;
                    }
                    self.save();
                } else {
                    if session.locked {
                        let k = key(server, &session.id);
                        if title.is_empty() {
                            self.settings.locked_session_names.remove(&k);
                        } else {
                            self.settings.locked_session_names.insert(k, model::shorten(&title, 120));
                        }
                        self.save();
                    }
                    self.controller.send(server, json!({"op":"session_meta","sid":session.id,"title":title,"actor":actor}));
                }
            }
            RowAction::MoveToFolder => {
                self.folder_dialog = Some(session_actions::FolderDialog {
                    server: server.to_owned(),
                    name: String::new(),
                    rename_folder_id: None,
                    move_session: Some((
                        session.id.clone(),
                        session.title.clone(),
                        session.folder.clone(),
                    )),
                    focused_once: false,
                    error: String::new(),
                });
            }
        }
    }
    fn main_panel(&mut self, ctx: &egui::Context) {
        let compact_appearance=self.settings.appearance.compact;
        let chat_width = self.settings.appearance.chat_width.clamp(560., 1160.);
        let light = self.settings.light;
        let p = design::animated_palette(ctx, light);
        let active_backend_id = self
            .server()
            .zip(self.current_key())
            .map(|(srv, k)| self.backend_for_chat(&srv, &k))
            .unwrap_or_else(|| "claude".into());
        let active_engine_col = design::backend_color(&active_backend_id, light);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.base))
            .show(ctx, |ui| {
                design::ambient_horizon(ui.painter(), ui.max_rect(), active_engine_col, light);
                let hdr_panel = egui::TopBottomPanel::top("chat-header")
                    .show_separator_line(false)
                    .frame(
                        egui::Frame::new()
                            .fill(Color32::TRANSPARENT)
                            .inner_margin(egui::Margin::symmetric(18, 10)),
                    )
                    .show_inside(ui, |ui| {
                        let mut cluster_jump: Option<(String, String)> = None;
                        if let (Some(server), Some(k)) = (self.server(), self.current_key()) {
                            if let Some(c) = self.chats.get(&k) {
                                if let Some(run_id) = self.cluster_for_chat(&server.id, &c.sid) {
                                    let title =
                                        self.cluster_label(&server.id, &c.sid).unwrap_or_default();
                                    let run_nodes: Vec<(String, String, String, bool)> = self
                                        .cluster_store
                                        .runs
                                        .iter()
                                        .find(|r| r.id == run_id)
                                        .map(|r| {
                                            let mut list = vec![(
                                                format!("◈ {}", r.config.leader.name),
                                                r.config.leader.server.clone(),
                                                r.leader_sid.clone(),
                                                self.chats
                                                    .get(&key(&r.config.leader.server, &r.leader_sid))
                                                    .is_some_and(|ch| ch.busy),
                                            )];
                                            for t in &r.tasks {
                                                list.push((
                                                    format!("{} · {}", t.agent.name, t.state.label()),
                                                    t.agent.server.clone(),
                                                    t.sid.clone(),
                                                    t.state.active(),
                                                ));
                                            }
                                            list
                                        })
                                        .unwrap_or_default();
                                    ui.horizontal_wrapped(|ui| {
                                        design::chip(ui, &title, Icon::Cluster, p, true);
                                        for (lbl, srv_id, node_sid, node_busy) in run_nodes.into_iter().take(5) {
                                            let is_current = srv_id == server.id && node_sid == c.sid;
                                            let col = if node_busy {
                                                accent()
                                            } else if is_current {
                                                GREEN
                                            } else {
                                                p.muted
                                            };
                                            if design::mini_pill(
                                                ui,
                                                Some(if node_busy { Icon::Pulse } else { Icon::Orbit }),
                                                &model::shorten(&lbl, 24),
                                                col,
                                                p,
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                                && !is_current
                                            {
                                                cluster_jump = Some((srv_id, node_sid));
                                            }
                                        }
                                        if design::button(
                                            ui,
                                            "Топология",
                                            Some(Icon::Cluster),
                                            Kind::Ghost,
                                            p,
                                            Vec2::ZERO,
                                        )
                                        .clicked()
                                        {
                                            self.cluster_store.selected = Some(run_id.clone());
                                            self.cluster_open = true;
                                        }
                                    });
                                    ui.add_space(5.);
                                }
                            }
                        }
                        if let Some((jsrv, jsid)) = cluster_jump {
                            self.open_cluster_chat(&jsrv, &jsid);
                        }
                        let chat=self.current_key().and_then(|k|self.chats.get(&k));
                        let title=chat.map(|c|c.title.clone()).unwrap_or_else(||"Рабочее пространство".into());
                        let cwd=chat.map(|c|c.cwd.clone()).unwrap_or_default();
                        let fresh=chat.is_some_and(|c|c.messages.is_empty() && !c.loading && !c.busy);
                        let chat_busy=chat.is_some_and(|c|c.busy);
                        let sid_opt=chat.map(|c|c.sid.clone());
                        let chat_locked=chat.is_some_and(|c|c.locked);
                        let chat_unlocked=chat.is_none_or(|c|c.unlocked);
                        let active_badge=self.server().zip(self.current_key()).map(|(srv, k)| crate::model::backend_short_label(&self.backend_for_chat(&srv, &k)));
                        ui.horizontal(|ui|{
                            let compact_hdr = ui.available_width() < 540.;
                            let right_reserve = if compact_hdr {
                                if sid_opt.is_some() { 126. } else { 88. }
                            } else if sid_opt.is_some() {
                                216.
                            } else {
                                176.
                            };
                            let available=(ui.available_width()-right_reserve).max(90.);
                            ui.allocate_ui_with_layout(Vec2::new(available,44.),Layout::top_down(Align::Min),|ui|{
                                ui.set_width(available);
                                ui.horizontal(|ui|{
                                    ui.spacing_mut().item_spacing.x = 6.;
                                    if chat_busy {
                                        let (orb,_)=ui.allocate_exact_size(Vec2::new(18., 15.),egui::Sense::hover());
                                        design::engine_icon(ui, orb, &active_backend_id);
                                    } else if chat_locked {
                                        let (lr,_)=ui.allocate_exact_size(Vec2::splat(14.),egui::Sense::hover());
                                        design::icon(ui.painter(),lr,if chat_unlocked{Icon::Unlock}else{Icon::Lock},if chat_unlocked{GREEN}else{accent()});
                                    } else {
                                        let (mr,_)=ui.allocate_exact_size(Vec2::new(16., 14.),egui::Sense::hover());
                                        design::engine_icon(ui, mr, &active_backend_id);
                                    }
                                    ui.add(egui::Label::new(RichText::new(title).size(15.).color(p.text)).truncate());
                                });
                                ui.horizontal(|ui|{
                                    ui.spacing_mut().item_spacing.x = 6.;
                                    let (r,_)=ui.allocate_exact_size(Vec2::splat(12.),egui::Sense::hover());design::icon(ui.painter(),r,Icon::Folder,p.muted);
                                    let response=ui.add(egui::Label::new(RichText::new(if cwd.is_empty(){"Выбери подключение"}else{folder_name(&cwd)}).size(11.5).color(p.muted)).sense(egui::Sense::click())).on_hover_text(&cwd);
                                    if fresh && (!chat_locked || chat_unlocked) && response.clicked(){self.open_folders();}
                                    if let Some(b_name) = active_badge {
                                        design::mini_pill(
                                            ui,
                                            Some(design::backend_icon(&active_backend_id)),
                                            b_name,
                                            active_engine_col,
                                            p,
                                        );
                                    }
                                });
                            });
                            if let Some(server)=self.server(){
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing.x = 6.;
                                    let browser_label = if compact_hdr { "" } else { "Браузер" };
                                    let browser_size = if compact_hdr { Vec2::splat(32.) } else { Vec2::new(106., 32.) };
                                    if design::button(ui,browser_label,Some(Icon::Browser),Kind::Secondary,p,browser_size).on_hover_text("Браузер выбранного сервера").clicked(){self.start_browser(&server);}
                                    if let Some(sid)=sid_opt {
                                        let lock_icon = if chat_locked && chat_unlocked { Icon::Unlock } else { Icon::Lock };
                                        let lock_tip = if chat_locked && !chat_unlocked {
                                            "Сессия заблокирована паролем"
                                        } else if chat_locked {
                                            "Настроить защиту паролем или заблокировать (ПКМ — заблокировать сразу)"
                                        } else {
                                            "Защитить эту сессию паролем"
                                        };
                                        let lock_btn = design::button(ui,"",Some(lock_icon),Kind::Secondary,p,Vec2::splat(32.)).on_hover_text(lock_tip);
                                        if lock_btn.clicked() && (!chat_locked || chat_unlocked) {
                                            self.open_lock_dialog(&server.id, &sid);
                                        }
                                        if lock_btn.secondary_clicked() && chat_locked && chat_unlocked {
                                            self.relock_session(&server.id, &sid);
                                        }
                                    }
                                    if design::button(ui,"",Some(Icon::Radar),Kind::Secondary,p,Vec2::splat(32.))
                                        .on_hover_text("Радар агентов · живая топология сессий на сервере (/radar)")
                                        .clicked()
                                    {
                                        self.radar_open = Some(server.id.clone());
                                    }
                                });
                            }
                        });
                    });
                let hr = hdr_panel.response.rect;
                ui.painter().line_segment(
                    [egui::pos2(hr.left(), hr.bottom() - 0.5), egui::pos2(hr.right(), hr.bottom() - 0.5)],
                    Stroke::new(1.0_f32, design::mix(p.base, p.line, 0.58)),
                );
                if !self.notice.is_empty() {
                    egui::TopBottomPanel::top("notice")
                        .show_separator_line(false)
                        .frame(
                            egui::Frame::new()
                                .fill(accent().gamma_multiply(0.12))
                                .inner_margin(egui::Margin::symmetric(18, 6)),
                        )
                        .show_inside(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                label(ui, self.notice.clone(), 13., accent());
                                if design::button(
                                    ui,
                                    "",
                                    Some(Icon::Close),
                                    Kind::Ghost,
                                    p,
                                    Vec2::splat(28.),
                                )
                                .clicked()
                                {
                                    self.notice.clear();
                                }
                            });
                        });
                }
                if self.server().is_none() {
                    ui.vertical_centered(|ui| {
                        ui.add_space((ui.available_height() * 0.25).max(30.));
                        design::mark(ui, 58.);
                        ui.add_space(28.);
                        label(ui, "Все проекты. Одно пространство.", 29., p.text);
                        ui.add_space(12.);
                        label(
                            ui,
                            "Подключи сервер и продолжай с того места, где остановился.",
                            15.,
                            p.muted,
                        );
                        ui.add_space(30.);
                        if design::button(
                            ui,
                            "Добавить сервер",
                            Some(Icon::Plus),
                            Kind::Primary,
                            p,
                            Vec2::new(220., 44.),
                        )
                        .clicked()
                        {
                            self.editor = Some(Editor {
                                server: Server::default(),
                                password: String::new(),
                                is_new: true,
                                error: String::new(),
                            });
                        }
                    });
                    return;
                }
                if self.current_key().is_none() {
                    self.new_chat();
                }
                let Some(k) = self.current_key() else {
                    return;
                };
                if self.chats.get(&k).is_some_and(|c| c.locked && !c.unlocked) {
                    let server_id = self.server().map(|s| s.id).unwrap_or_default();
                    let sid = self.chats.get(&k).map(|c| c.sid.clone()).unwrap_or_default();
                    let mut unlock_now = false;
                    egui::ScrollArea::vertical()
                        .id_salt(("locked-gate", &k))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space((ui.available_height() * 0.2).clamp(24., 130.));
                            design::content(ui, 400., |ui| {
                                egui::Frame::new()
                                    .fill(p.surface)
                                    .stroke(Stroke::new(1.0_f32, p.line))
                                    .corner_radius(14.)
                                    .inner_margin(24.)
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.vertical_centered(|ui| {
                                            let (ir, _) = ui.allocate_exact_size(
                                                Vec2::splat(38.),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().circle_filled(
                                                ir.center(),
                                                19.,
                                                accent().gamma_multiply(0.14),
                                            );
                                            design::icon(
                                                ui.painter(),
                                                ir.shrink(9.),
                                                Icon::Lock,
                                                accent(),
                                            );
                                            ui.add_space(12.);
                                            label(ui, "Сессия защищена паролем", 19., p.text);
                                            ui.add_space(6.);
                                            label(
                                                ui,
                                                "История и отправка сообщений скрыты. Введи пароль этой сессии, чтобы продолжить работу.",
                                                12.5,
                                                p.muted,
                                            );
                                        });
                                        ui.add_space(16.);
                                        if let Some(c) = self.chats.get_mut(&k) {
                                            let resp = design::field(
                                                ui,
                                                &mut c.unlock_input,
                                                "Пароль сессии",
                                                true,
                                            );
                                            if resp.lost_focus()
                                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                                && !c.unlocking
                                            {
                                                unlock_now = true;
                                            }
                                            if !c.unlock_error.is_empty() {
                                                ui.add_space(6.);
                                                label(ui, &c.unlock_error, 12., design::RED);
                                            }
                                            ui.add_space(14.);
                                            ui.add_enabled_ui(!c.unlocking, |ui| {
                                                if design::button(
                                                    ui,
                                                    if c.unlocking {
                                                        "Проверяем пароль…"
                                                    } else {
                                                        "Разблокировать сессию"
                                                    },
                                                    Some(Icon::Unlock),
                                                    Kind::Primary,
                                                    p,
                                                    Vec2::new(ui.available_width(), 36.),
                                                )
                                                .clicked()
                                                {
                                                    unlock_now = true;
                                                }
                                            });
                                        }
                                    });
                            });
                        });
                    if unlock_now {
                        self.submit_unlock(&server_id, &sid);
                    }
                    return;
                }
                egui::TopBottomPanel::bottom("composer-panel")
                    .frame(egui::Frame::new().fill(p.base).inner_margin(egui::Margin {
                        left: 20,
                        right: 20,
                        top: 4,
                        bottom: 14,
                    }))
                    .show_separator_line(false)
                    .show_inside(ui, |ui| {
                        design::content(ui, chat_width, |ui| {
                            self.permission_card(ui, &k, p);
                            self.composer(ui, &k, p);
                        });
                    });
                let chat = self.chats.get(&k).unwrap();
                if chat.messages.is_empty() && !chat.loading {
                    let narrow = ui.available_width() < 560.;
                    let mut picked_starter: Option<String> = None;
                    let mut picked_backend: Option<String> = None;
                    egui::ScrollArea::vertical()
                        .id_salt("welcome")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space((ui.available_height() * 0.11).clamp(16., 84.));
                            let width = (ui.available_width() - 40.).min(560.);
                            design::content(ui, width, |ui| {
                                ui.horizontal(|ui| {
                                    let (orb, _) = ui.allocate_exact_size(
                                        Vec2::new(42., 34.),
                                        egui::Sense::hover(),
                                    );
                                    design::engine_icon(ui, orb, &active_backend_id);
                                    ui.add_space(6.);
                                    ui.vertical(|ui| {
                                        label(
                                            ui,
                                            "Что создадим?",
                                            if narrow { 23. } else { 27. },
                                            p.text,
                                        );
                                        label(
                                            ui,
                                            "Выбери движок агента или стартовый сценарий ниже",
                                            12.5,
                                            p.muted,
                                        );
                                    });
                                });
                                ui.add_space(16.);

                                // Clickable Engine Signature Strip with pixel-art mascots
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::new(7., 7.);
                                    for (bid, btitle) in [
                                        ("claude", "Claude Code"),
                                        ("codex", "OpenAI Codex"),
                                        ("agy", "Antigravity"),
                                        ("api", "Свой API"),
                                    ] {
                                        let selected_b = active_backend_id == bid;
                                        let bcol = design::backend_color(bid, light);
                                        let g = ui.painter().layout_no_wrap(
                                            btitle.into(),
                                            FontId::proportional(12.0),
                                            if selected_b { p.text } else { p.muted },
                                        );
                                        let (pr, presp) = ui.allocate_exact_size(
                                            Vec2::new(g.size().x + 38., 30.),
                                            egui::Sense::click(),
                                        );
                                        let h = ui.ctx().animate_bool_with_time(
                                            presp.id.with("hover"),
                                            presp.hovered(),
                                            0.16,
                                        );
                                        ui.painter().rect_filled(
                                            pr,
                                            9.,
                                            if selected_b {
                                                design::mix(p.surface, bcol, 0.18)
                                            } else {
                                                design::mix(p.side, p.surface, 0.5 + 0.5 * h)
                                            },
                                        );
                                        ui.painter().rect_stroke(
                                            pr,
                                            9.,
                                            Stroke::new(
                                                if selected_b { 1.35_f32 } else { 1.0_f32 },
                                                if selected_b {
                                                    bcol
                                                } else {
                                                    design::mix(p.line, bcol, h * 0.45)
                                                },
                                            ),
                                            egui::StrokeKind::Inside,
                                        );
                                        design::engine_icon(
                                            ui,
                                            egui::Rect::from_center_size(
                                                egui::pos2(pr.left() + 15., pr.center().y),
                                                Vec2::splat(18.),
                                            ),
                                            bid,
                                        );
                                        ui.painter().galley(
                                            egui::pos2(
                                                pr.left() + 28.,
                                                pr.center().y - g.size().y * 0.5,
                                            ),
                                            g,
                                            if selected_b { p.text } else { p.muted },
                                        );
                                        if presp
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            picked_backend = Some(bid.into());
                                        }
                                    }
                                });
                                ui.add_space(18.);

                                let missions = [
                                    (
                                        "Изучить проект",
                                        "Архитектура, ключевые модули и точки входа",
                                        Icon::Search,
                                        design::phase_color(crate::model::ToolPhase::Research, light),
                                    ),
                                    (
                                        "Посмотреть изменения",
                                        "Проверить git diff, тесты и узкие места",
                                        Icon::Terminal,
                                        design::phase_color(crate::model::ToolPhase::Terminal, light),
                                    ),
                                    (
                                        "Улучшить код и дизайн",
                                        "Внести правки и проверить сборку",
                                        Icon::Pencil,
                                        design::phase_color(crate::model::ToolPhase::Edit, light),
                                    ),
                                    (
                                        "Запустить субагентов",
                                        "Параллельные агенты для сложных задач",
                                        Icon::Orbit,
                                        design::phase_color(crate::model::ToolPhase::Subagent, light),
                                    ),
                                ];
                                if narrow {
                                    for (m_title, m_det, m_ic, _) in missions {
                                        if design::starter_card(ui, m_title, m_det, m_ic, p).clicked()
                                        {
                                            picked_starter = Some(m_title.into());
                                        }
                                        ui.add_space(8.);
                                    }
                                } else {
                                    let col_w = ((width - 10.) * 0.5).max(160.);
                                    for pair in missions.chunks(2) {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 10.;
                                            for &(m_title, m_det, m_ic, m_tint) in pair {
                                                if design::bento_card(
                                                    ui, col_w, m_title, m_det, m_ic, m_tint, p,
                                                )
                                                .clicked()
                                                {
                                                    picked_starter = Some(m_title.into());
                                                }
                                            }
                                        });
                                        ui.add_space(10.);
                                    }
                                }
                            });
                        });
                    if let Some(t) = picked_starter {
                        if let Some(c) = self.chats.get_mut(&k) {
                            c.draft = t;
                        }
                    }
                    if let Some(new_b) = picked_backend {
                        self.settings.set_session_backend(k.clone(), &new_b);
                        if let Some(srv) = self.server() {
                            if let Some(c) = self.chats.get_mut(&k) {
                                if !c.cluster_context.is_object() {
                                    c.model = srv.model_for(&new_b).to_owned();
                                }
                            }
                            if matches!(new_b.as_str(), "codex" | "agy") {
                                self.controller
                                    .send(&srv.id, json!({"op": "catalog", "backend": new_b}));
                            }
                        }
                        self.save();
                    }
                    return;
                }
                let chat = self.chats.get_mut(&k).unwrap();
                let fade = ctx.animate_bool_with_time_and_easing(
                    egui::Id::new(("chat-enter", &k)),
                    true,
                    0.30,
                    egui::emath::easing::cubic_in_out,
                );
                // Measured from the actual panel header & window right edge so wide code blocks never widen the column.
                let visible = hr
                    .width()
                    .min(ctx.screen_rect().right() - ui.max_rect().left())
                    .min(ui.available_width())
                    .max(160.);
                egui::ScrollArea::vertical()
                    .id_salt(("chat", &k))
                    .max_width(visible)
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(24.);
                        design::column(ui, visible, chat_width, |ui| {
                            let col_w = ui.max_rect().width();
                            ui.set_opacity(fade);
                            if chat.loading {
                                ui.horizontal(|ui| {
                                    design::dots(ui, accent());
                                    label(ui, "Открываем разговор", 13., p.muted);
                                });
                            }
                            let attributions = crate::model::compute_attributions(
                                &chat.messages,
                                &active_backend_id,
                                chat.busy,
                            );
                            let timeline = crate::timeline::items(&chat.messages);
                            let grouped_busy = chat.busy
                                && matches!(
                                    timeline.last(),
                                    Some(crate::timeline::Item::Actions(_))
                                );
                            for item in timeline {
                                let index = match item {
                                    crate::timeline::Item::Actions(range) => {
                                        let busy = chat.busy && range.end == chat.messages.len();
                                        let item_rect = egui::Rect::from_min_size(
                                            ui.cursor().min,
                                            Vec2::new(col_w, f32::INFINITY),
                                        );
                                        ui.scope_builder(
                                            egui::UiBuilder::new().max_rect(item_rect),
                                            |ui| {
                                                ui.set_width(col_w);
                                                ui.set_max_width(col_w);
                                                crate::timeline::show(
                                                    ui,
                                                    &k,
                                                    &chat.messages,
                                                    range,
                                                    busy,
                                                    p,
                                                    &mut self.markdown,
                                                );
                                            },
                                        );
                                        continue;
                                    }
                                    crate::timeline::Item::Message(index) => index,
                                };
                                let message = &chat.messages[index];
                                let attr = &attributions[index];
                                // Each message fades in once, when it first appears.
                                let appear = ui.ctx().animate_bool_with_time_and_easing(
                                    egui::Id::new(("message-in", &k, index)),
                                    true,
                                    design::duration(ui.ctx(), 0.28),
                                    egui::emath::easing::cubic_out,
                                );
                                let msg_rect = egui::Rect::from_min_size(
                                    ui.cursor().min,
                                    Vec2::new(col_w, f32::INFINITY),
                                );
                                ui.scope_builder(
                                    egui::UiBuilder::new().id_salt(index).max_rect(msg_rect),
                                    |ui| {
                                        ui.set_width(col_w);
                                        ui.set_max_width(col_w);
                                ui.set_opacity(appear);
                                match message.role.as_str() {
                                    "user" => {
                                        ui.add_space(6.);
                                        if message.text.starts_with("<relay-cluster-results>") {
                                            crate::timeline::cluster_update_view(
                                                ui,
                                                &message.text,
                                                p,
                                                &mut self.markdown,
                                            );
                                        } else {
                                            user_turn_caption(ui, attr, light, p);
                                            user_message_bubble(
                                                ui,
                                                &message.text,
                                                chat.sent_images
                                                    .get(&index)
                                                    .map(Vec::as_slice)
                                                    .unwrap_or(&[]),
                                                p,
                                            );
                                        }
                                        ui.add_space(if compact_appearance{8.}else{14.});
                                    }
                                    _ => {
                                        let top = ui.cursor().top();
                                        let msg_col = design::backend_color(&attr.backend, light);
                                        // Per-message attribution: Кто → Кому · Как отвечал
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing = Vec2::new(5.5, 2.);
                                            let (er, _) = ui.allocate_exact_size(
                                                Vec2::splat(18.),
                                                egui::Sense::hover(),
                                            );
                                            design::engine_icon(ui, er, &attr.backend);
                                            label(ui, &attr.who, 11.4, msg_col);
                                            if !attr.to_whom.is_empty() {
                                                label(
                                                    ui,
                                                    format!("→ {}", attr.to_whom),
                                                    10.9,
                                                    p.muted,
                                                );
                                            }
                                            if !attr.how.is_empty() {
                                                label(
                                                    ui,
                                                    format!("· {}", attr.how),
                                                    10.7,
                                                    p.muted.gamma_multiply(0.84),
                                                );
                                            }
                                        });
                                        ui.add_space(3.);
                                        let file_links = crate::artifacts::links(&message.text);
                                        for (destination, _) in &file_links {
                                            self.markdown.add_link_hook(destination.clone());
                                        }
                                        ui.scope(|ui| {
                                            ui.style_mut().wrap_mode =
                                                Some(egui::TextWrapMode::Wrap);
                                            ui.spacing_mut().item_spacing.y = 4.;
                                            ui.spacing_mut().interact_size.y = 20.;
                                            crate::math::show(
                                                ui,
                                                &mut self.markdown,
                                                &message.text,
                                            );
                                        });
                                        for (destination, artifact) in &file_links {
                                            let clicked =
                                                self.markdown.remove_link_hook(destination)
                                                    == Some(true);
                                            if clicked {
                                                if let Some(server) =
                                                    self.settings.selected.as_deref()
                                                {
                                                    self.downloads.start(
                                                        &self.controller,
                                                        server,
                                                        &chat.cwd,
                                                        &artifact.path,
                                                    );
                                                }
                                            }
                                        }
                                        if let Some(server) = self.settings.selected.as_deref() {
                                            let connected = self.demo
                                                || self
                                                    .remote
                                                    .get(server)
                                                    .is_some_and(|r| r.connected);
                                            self.downloads.show(
                                                ui,
                                                &self.controller,
                                                server,
                                                &chat.cwd,
                                                &message.text,
                                                connected,
                                                p,
                                            );
                                        }
                                        // Copy appears on hover; the row is always reserved so nothing jumps.
                                        let (row, _) = ui.allocate_exact_size(
                                            Vec2::new(ui.available_width(), 22.),
                                            egui::Sense::hover(),
                                        );
                                        let area = egui::Rect::from_min_max(
                                            egui::pos2(row.left(), top),
                                            row.max,
                                        );
                                        let last = index + 1 == chat.messages.len() && !chat.busy;
                                        if last || ui.rect_contains_pointer(area) {
                                            let mut child = ui.new_child(
                                                egui::UiBuilder::new()
                                                    .max_rect(row)
                                                    .layout(Layout::left_to_right(Align::Center)),
                                            );
                                            if design::button(
                                                &mut child,
                                                "",
                                                Some(Icon::Copy),
                                                Kind::Ghost,
                                                p,
                                                Vec2::splat(22.),
                                            )
                                            .on_hover_text("Скопировать ответ")
                                            .clicked()
                                            {
                                                child.ctx().copy_text(message.text.clone());
                                            }
                                        }
                                        ui.add_space(if compact_appearance{4.}else{8.});
                                    }
                                }
                                });
                            }
                            if chat.busy {
                                let tel = crate::model::analyze_turn(
                                    &chat.messages,
                                    chat.user_at,
                                    chat.busy,
                                    &chat.status,
                                );
                                if !grouped_busy || !tel.subagents.is_empty() {
                                    ui.add_space(6.);
                                    let turn_bid = if chat.turn_backend.is_empty() {
                                        active_backend_id.as_str()
                                    } else {
                                        chat.turn_backend.as_str()
                                    };
                                    let turn_engine_col = design::backend_color(turn_bid, light);
                                    let pod_tint = tel
                                        .active_phase
                                        .map(|ph| design::phase_color(ph, light))
                                        .unwrap_or(turn_engine_col);
                                    let pod_resp = egui::Frame::new()
                                        .fill(design::mix(
                                            p.side,
                                            design::mix(p.surface, pod_tint, 0.12),
                                            0.75,
                                        ))
                                        .stroke(Stroke::new(
                                            1.1_f32,
                                            design::mix(p.line, pod_tint, 0.45),
                                        ))
                                        .corner_radius(13.)
                                        .inner_margin(egui::Margin::symmetric(14, 11))
                                        .show(ui, |ui| {
                                            ui.set_width((ui.available_width() - 28.).max(80.));
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 8.;
                                                let (orb, _) = ui.allocate_exact_size(
                                                    Vec2::new(24., 20.),
                                                    egui::Sense::hover(),
                                                );
                                                design::engine_icon(ui, orb, turn_bid);
                                                ui.add_space(3.);
                                                ui.vertical(|ui| {
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.spacing_mut().item_spacing = Vec2::new(9., 4.);
                                                        label(
                                                            ui,
                                                            &tel.active_headline,
                                                            13.0,
                                                            p.text,
                                                        );
                                                        design::waveform(ui, pod_tint, 13.);
                                                    });
                                                    if !tel.active_target.is_empty() {
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(&tel.active_target)
                                                                    .monospace()
                                                                    .size(11.2)
                                                                    .color(pod_tint),
                                                            )
                                                            .truncate(),
                                                        );
                                                    }
                                                });
                                            });
                                            if tel.total_actions > 0 || !tel.subagents.is_empty() {
                                                ui.add_space(6.);
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.spacing_mut().item_spacing = Vec2::new(6., 4.);
                                                    if tel.total_actions > 0 {
                                                        design::mini_pill(
                                                            ui,
                                                            Some(Icon::Layers),
                                                            &format!(
                                                                "шагов: {}/{}",
                                                                tel.done_actions, tel.total_actions
                                                            ),
                                                            pod_tint,
                                                            p,
                                                        );
                                                    }
                                                    for sa in tel.subagents.iter().take(4) {
                                                        let scol = design::phase_color(
                                                            crate::model::ToolPhase::Subagent,
                                                            light,
                                                        );
                                                        design::mini_pill(
                                                            ui,
                                                            Some(Icon::Orbit),
                                                            &format!(
                                                                "{}{}",
                                                                sa.role,
                                                                if sa.running {
                                                                    " · в работе"
                                                                } else {
                                                                    " · готово"
                                                                }
                                                            ),
                                                            scol,
                                                            p,
                                                        );
                                                    }
                                                });
                                            }
                                        })
                                        .response;
                                    design::specular_rim(ui.painter(), pod_resp.rect, 13., light);
                                    ui.add_space(18.);
                                }
                            }
                        });
                    });
            });
    }
    fn composer(&mut self, ui: &mut egui::Ui, k: &str, p: Palette) {
        let send_on_enter = self.settings.appearance.send_on_enter;
        let sid = self.chats.get(k).map(|c| c.sid.clone()).unwrap_or_default();
        if let Some(server) = self.server() {
            if let Some(run_id) = self.cluster_for_chat(&server.id, &sid) {
                if let Some(r) = self
                    .cluster_store
                    .runs
                    .iter()
                    .find(|r| r.id == run_id && r.leader_sid != sid)
                {
                    let target = (r.config.leader.server.clone(), r.leader_sid.clone());
                    egui::Frame::new()
                        .fill(p.side)
                        .corner_radius(12.)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            label(
                                ui,
                                "Исполнитель выполняет поручение руководителя",
                                13.,
                                p.text,
                            );
                            if design::button(
                                ui,
                                "Написать руководителю",
                                Some(Icon::Cluster),
                                Kind::Secondary,
                                p,
                                Vec2::ZERO,
                            )
                            .clicked()
                            {
                                self.open_cluster_chat(&target.0, &target.1);
                            }
                        });
                    return;
                }
            }
        }
        let server = self.server().unwrap();
        let connected = self.connected(&server.id);
        let mut send = false;
        let mut attach = false;
        ui.horizontal(|ui| {
            let active=server.local_control && server.local_workspace.is_some();
            let auto=self.chats.get(k).is_some_and(|chat|chat.mode=="auto");
            let text=if active{if auto{"Этот ПК · Авто"}else{"Этот ПК · Вручную"}}else{"Подключить ПК"};
            let toggle=design::chip(ui,text,Icon::Computer,p,active).on_hover_text(if active{"ПК подключён. В «Авто» команды выполняются без подтверждения с правами пользователя Relay. Нажми, чтобы отключить."}else{"Подключить управление этим компьютером. Режим «Авто» выполняет команды и скачивает файлы без подтверждения."}).clicked();
            let disconnect=active && design::chip(ui,"",Icon::Close,p,false).on_hover_text("Отключить этот ПК").clicked();
            if toggle || disconnect {
                let path=if active{None}else{dirs::home_dir().or_else(||std::env::current_dir().ok()).map(|p|p.to_string_lossy().into_owned())};
                if let Some(profile)=self.settings.servers.iter_mut().find(|s|s.id==server.id){profile.local_control=path.is_some();profile.local_workspace=path.clone();}
                self.controller.send(&server.id,json!({"op":"_desktop_workspace","path":path}));
                for (key,chat) in &self.chats {if key.starts_with(&format!("{}/",server.id)){self.controller.send(&server.id,json!({"op":"_desktop_policy","sid":chat.sid,"auto":!active && chat.mode=="auto"}));}}
                self.save();
            }
        });
        ui.add_space(4.);
        let mut tests = server.tests.clone();
        let mut backend = self.backend_for_chat(&server, k);
        let previous_backend = backend.clone();
        let mut api_model = server.api.model.clone();
        let mut configure_api = false;
        let mut login_provider: Option<String> = None;
        let mut open_agents = false;
        let mut install_active_cli = false;
        let installed_flags = [
            self.backend_installed(&server.id, "claude"),
            self.backend_installed(&server.id, "codex"),
            self.backend_installed(&server.id, "agy"),
        ];
        let active_cli_missing = connected
            && matches!(backend.as_str(), "claude" | "codex" | "agy")
            && !self.backend_installed(&server.id, &backend);
        let active_cli_installing = self
            .backend_installing(&server.id, &backend)
            .map(str::to_owned);
        let models = self.models_for_backend(&server, &backend);
        let (is_worker_session, active_orchestra) = {
            let sid = self.chats.get(k).map(|c| c.sid.as_str()).unwrap_or("");
            let is_w = self.is_worker_chat(&server.id, sid);
            let orch = if !is_w {
                self.cluster_store
                    .runs
                    .iter()
                    .find(|r| !r.detached && r.config.leader.server == server.id && r.leader_sid == sid)
                    .map(|r| {
                        let w_count = r.config.workers.iter().filter(|w| w.enabled).count();
                        let w_active = r.tasks.iter().filter(|t| t.state.active()).count();
                        let (eff_b, eff_m, _) = cluster_runtime::slot_resolved_summary(
                            &r.config.model_palette.default_worker,
                            &backend,
                            Some(&server),
                        );
                        let b_short = crate::model::backend_short_label(&eff_b);
                        let m_disp = if eff_m == "default" {
                            "авто движка".into()
                        } else {
                            model_label(&self.models_for_backend(&server, &eff_b), &eff_m)
                        };
                        let role_mods = [
                            ("кодер", r.config.model_palette.coder.is_some()),
                            ("ревью", r.config.model_palette.reviewer.is_some()),
                            ("поиск", r.config.model_palette.researcher.is_some()),
                            ("быстрый", r.config.model_palette.fast.is_some()),
                        ]
                        .into_iter()
                        .filter(|(_, on)| *on)
                        .map(|(n, _)| n)
                        .collect::<Vec<_>>();
                        let summary = if role_mods.is_empty() {
                            format!("{b_short} · {m_disp}")
                        } else {
                            format!("{b_short} · {m_disp} (+{})", role_mods.join(", "))
                        };
                        (r.id.clone(), w_count, w_active, summary)
                    })
            } else {
                None
            };
            (is_w, orch)
        };
        let mut open_orchestra_dlg = false;
        let mut open_cluster_view: Option<String> = None;
        let mut disable_orchestra = false;
        self.game_test_cards(ui, &server.id, p);
        let (mut commands, live) = self
            .remote
            .get(&server.id)
            .map(|r| {
                let cmds = match backend.as_str() {
                    "codex" => r.codex_commands.clone(),
                    "agy" => r.agy_commands.clone(),
                    _ => r.commands.clone(),
                };
                (cmds, r.protocol >= 4)
            })
            .unwrap_or_default();
        for builtin in crate::model::builtin_slash_commands() {
            if !commands
                .iter()
                .any(|c| c.name.eq_ignore_ascii_case(&builtin.name))
            {
                commands.push(builtin);
            }
        }
        let chat = self.chats.get_mut(k).unwrap();
        if let Some((sent, total)) = chat.upload_progress {
            let progress = sent as f32 / total.max(1) as f32;
            ui.add(
                egui::ProgressBar::new(progress.clamp(0., 1.))
                    .desired_width(ui.available_width())
                    .text(format!(
                        "Отправка файлов · {:.1} / {:.1} МБ",
                        sent as f64 / 1048576.,
                        total as f64 / 1048576.
                    )),
            );
            ui.add_space(6.);
        }
        if !chat.queue.is_empty() {
            egui::Frame::new()
                .fill(p.side)
                .stroke(Stroke::new(1_f32, p.line))
                .corner_radius(12.)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        label(ui, format!("В очереди: {}", chat.queue.len()), 12., p.text);
                        if design::button(
                            ui,
                            if chat.queue_paused {
                                "Продолжить"
                            } else {
                                "Пауза"
                            },
                            None,
                            Kind::Ghost,
                            p,
                            Vec2::ZERO,
                        )
                        .clicked()
                        {
                            chat.queue_paused = !chat.queue_paused;
                        }
                    });
                    let mut remove = None;
                    let mut edit = None;
                    egui::ScrollArea::vertical()
                        .id_salt((k, "queue"))
                        .max_height(94.)
                        .show(ui, |ui| {
                            for (i, job) in chat.queue.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    if design::button(
                                        ui,
                                        "",
                                        Some(Icon::Close),
                                        Kind::Ghost,
                                        p,
                                        Vec2::splat(24.),
                                    )
                                    .on_hover_text("Убрать из очереди")
                                    .clicked()
                                    {
                                        remove = Some(i);
                                    }
                                    if ui
                                        .add(
                                            egui::Label::new(
                                                RichText::new(format!(
                                                    "{}  {}",
                                                    i + 1,
                                                    if job.text.is_empty() {
                                                        "Вложения"
                                                    } else {
                                                        &job.text
                                                    }
                                                ))
                                                .size(12.)
                                                .color(p.muted),
                                            )
                                            .truncate()
                                            .sense(egui::Sense::click()),
                                        )
                                        .on_hover_text("Изменить сообщение")
                                        .clicked()
                                    {
                                        edit = Some(i);
                                    }
                                });
                            }
                        });
                    if let Some(i) = remove {
                        chat.queue.remove(i);
                    } else if let Some(i) = edit {
                        if chat.draft.trim().is_empty() && chat.images.is_empty() {
                            if let Some(job) = chat.queue.remove(i) {
                                chat.draft = job.text;
                                chat.images = job.files;
                            }
                        } else {
                            chat.error = "Сначала отправь или очисти текущий черновик.".into();
                        }
                    }
                });
            ui.add_space(6.);
        }
        let suggestions = slash_matches(&chat.draft, &commands);
        if !suggestions.is_empty() {
            let tab = ui.memory(|m| m.has_focus(egui::Id::new("prompt-field")))
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
            let mut chosen = tab.then(|| suggestions[0].name.clone());
            egui::Frame::new()
                .fill(p.side)
                .stroke(Stroke::new(1_f32, p.line))
                .corner_radius(12.)
                .inner_margin(6)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (i, c) in suggestions.iter().take(8).enumerate() {
                        let row = egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(8, 5))
                            .corner_radius(8.)
                            .fill(if i == 0 { design::mix(p.side, p.line, 0.6) } else { Color32::TRANSPARENT })
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    label(ui, format!("/{}", c.name), 13., accent());
                                    if !c.hint.is_empty() {
                                        label(ui, &c.hint, 12., p.muted);
                                    }
                                    let mut job = egui::text::LayoutJob::simple(c.description.clone(), FontId::proportional(12.), p.muted, ui.available_width());
                                    job.wrap.max_rows = 1;
                                    job.wrap.overflow_character = Some('…');
                                    ui.label(job);
                                });
                            })
                            .response;
                        if ui.interact(row.rect, ui.id().with(("slash", i)), egui::Sense::click()).clicked() {
                            chosen = Some(c.name.clone());
                        }
                    }
                    label(ui, "Tab — подставить · /model, /mode и /stop срабатывают сразу, даже во время ответа", 11., p.muted);
                });
            if let Some(name) = chosen {
                chat.draft = format!("/{name} ");
                ui.memory_mut(|m| m.request_focus(egui::Id::new("prompt-field")));
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), egui::Id::new("prompt-field")) {
                    let end = egui::text::CCursor::new(chat.draft.chars().count());
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                    state.store(ui.ctx(), egui::Id::new("prompt-field"));
                }
            }
            ui.add_space(6.);
        }
        if chat.turn_summary.backend == backend
            && (chat.turn_summary.context.is_some() || chat.turn_summary.diff.is_some())
        {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.;
                if let Some((added, removed)) = chat.turn_summary.diff {
                    label(ui, format!("+{added}"), 11.5, GREEN);
                    label(ui, format!("−{removed}"), 11.5, design::RED);
                }
                if let Some((used, limit)) = chat.turn_summary.context {
                    let ratio = (used as f32 / limit as f32).clamp(0., 1.);
                    let (ring, _) = ui.allocate_exact_size(Vec2::splat(13.), egui::Sense::hover());
                    ui.painter().circle_stroke(ring.center(), 4.5, Stroke::new(1.4, p.line));
                    if ratio > 0. {
                        let points = (0..=24).map(|i| {
                            let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * ratio * i as f32 / 24.;
                            ring.center() + Vec2::angled(angle) * 4.5
                        }).collect();
                        ui.painter().add(egui::Shape::line(points, Stroke::new(1.4, accent())));
                    }
                    let short = |n: u64| match n {
                        0..=999 => n.to_string(),
                        1_000..=999_999 => format!("{:.1}k", n as f64 / 1_000.),
                        _ => format!("{:.2}M", n as f64 / 1_000_000.),
                    };
                    label(ui, format!("{}/{}", short(used), short(limit)), 11.5, p.muted)
                        .on_hover_text(format!("Токены окна контекста: {used} из {limit}"));
                }
            });
        }
        if backend != "api" && !model::explicit_model(&chat.model) {
            chat.model = model::selected_model(&chat.model, &models, &backend);
        }
        let previous = (chat.model.clone(), chat.mode.clone(), chat.effort.clone());
        if active_cli_missing || active_cli_installing.is_some() {
            egui::Frame::new()
                .fill(design::mix(p.side, p.surface, 0.7))
                .stroke(Stroke::new(1.0_f32, design::mix(p.line, accent(), 0.35)))
                .corner_radius(11.)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        if let Some(msg) = &active_cli_installing {
                            design::dots(ui, accent());
                            label(ui, msg, 12.5, accent());
                        } else {
                            let agent_title = crate::model::backend_label(&backend);
                            label(
                                ui,
                                format!("На сервере ещё не установлен {agent_title}."),
                                12.5,
                                p.text,
                            );
                            if design::button(
                                ui,
                                &format!("Установить {}", crate::model::backend_short_label(&backend)),
                                Some(Icon::Download),
                                Kind::Primary,
                                p,
                                Vec2::new(158., 28.),
                            )
                            .clicked()
                            {
                                install_active_cli = true;
                            }
                            if design::button(
                                ui,
                                "Все агенты…",
                                None,
                                Kind::Secondary,
                                p,
                                Vec2::new(112., 28.),
                            )
                            .clicked()
                            {
                                open_agents = true;
                            }
                        }
                    });
                });
            ui.add_space(6.);
        }
        if !chat.error.is_empty() {
            egui::Frame::new()
                .fill(accent().gamma_multiply(0.1))
                .corner_radius(10.)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        label(ui, chat.error.clone(), 12., accent());
                        if design::button(
                            ui,
                            "",
                            Some(Icon::Close),
                            Kind::Ghost,
                            p,
                            Vec2::splat(28.),
                        )
                        .clicked()
                        {
                            chat.error.clear();
                        }
                    });
                });
            ui.add_space(6.);
        }
        let focused = ui.memory(|m| m.has_focus(egui::Id::new("prompt-field")));
        if let Some((ref rid, w_count, w_active, ref w_summary)) = active_orchestra {
            egui::Frame::new()
                .fill(design::mix(p.side, accent(), 0.08))
                .stroke(Stroke::new(1.0_f32, accent().gamma_multiply(0.35)))
                .corner_radius(10.)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let (orb, _) = ui.allocate_exact_size(Vec2::splat(16.), egui::Sense::hover());
                        design::orbital_core(ui, orb, accent(), w_active > 0 || chat.busy);
                        label(ui, "Оркестр", 12., accent());
                        label(
                            ui,
                            if w_count == 0 {
                                format!("Модели агентов: {w_summary} · сам создаст нужных при задаче")
                            } else if w_active > 0 {
                                format!("{w_summary} · агентов: {w_count} · в работе: {w_active}")
                            } else {
                                format!("{w_summary} · агентов: {w_count}")
                            },
                            11.5,
                            p.muted,
                        );
                        if ui.small_button("Модели агентов").clicked() {
                            open_orchestra_dlg = true;
                        }
                        if ui.small_button("Кластер ↗").clicked() {
                            open_cluster_view = Some(rid.clone());
                        }
                        if ui
                            .small_button("Выкл")
                            .on_hover_text("Отключить оркестрацию в этом чате и продолжить обычной сессией")
                            .clicked()
                        {
                            disable_orchestra = true;
                        }
                    });
                });
            ui.add_space(6.);
        }
        let focus = ui
            .ctx()
            .animate_bool_with_time(egui::Id::new("composer-focus"), focused, 0.18);
        egui::Frame::new()
            .fill(design::mix(p.side,p.surface,0.6))
            .stroke(Stroke::new(
                1.0_f32,
                design::mix(p.line, accent(), focus * 0.65),
            ))
            .corner_radius(12.)
            .inner_margin(egui::Margin {
                left: 14,
                right: 8,
                top: 13,
                bottom: 9,
            })
            .shadow(egui::epaint::Shadow {
                offset: [0, 4],
                blur: 18,
                spread: 0,
                color: Color32::from_black_alpha(18),
            })
            .show(ui, |ui| {
                if !chat.images.is_empty() {
                    let mut remove = None;
                    ui.horizontal_wrapped(|ui| {
                        for (index, file) in chat.images.iter().enumerate() {
                            if attachment_view(ui, file, p, true) {
                                remove = Some(index);
                            }
                        }
                    });
                    if let Some(index) = remove {
                        chat.images.remove(index);
                    }
                    ui.add_space(10.);
                }
                let response = egui::ScrollArea::vertical()
                    .id_salt("draft-scroll")
                    .max_height(132.)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut chat.draft)
                                .id(egui::Id::new("prompt-field"))
                                .frame(false)
                                .hint_text(
                                    RichText::new(if chat.busy && live && matches!(backend.as_str(), "claude" | "codex") {
                                        format!("Добавь уточнение — {} учтёт его по ходу работы…", crate::model::backend_short_label(&backend))
                                    } else if chat.busy {
                                        "Добавь уточнение — отправится после текущего ответа…".into()
                                    } else {
                                        "Напиши задачу…".into()
                                    })
                                    .color(p.muted),
                                )
                                .font(FontId::proportional(14.))
                                .desired_width(ui.available_width())
                                .desired_rows(1),
                        )
                    })
                    .inner;
                if response.has_focus()
                    && ui.input(|i| {
                        i.key_pressed(egui::Key::Enter)
                            && !i.modifiers.shift
                            && (send_on_enter || i.modifiers.ctrl || i.modifiers.command)
                    })
                {
                    send = true;
                }
                let compact =
                    ui.available_width() < if chat.mode == "manual" { 820. } else { 740. };
                ui.add_space(4.);
                ui.horizontal(|ui| {
                    ui.add_space(-2.);
                    if design::button(ui, "", Some(Icon::Attach), Kind::Ghost, p, Vec2::splat(30.))
                        .on_hover_text("Выбор файлов, перетаскивание или Ctrl+V")
                        .clicked()
                    {
                        attach = true;
                    }
                    if !compact {
                        api_settings::backend_picker(ui, &mut backend, &mut configure_api, &mut login_provider, &mut open_agents, installed_flags, chat.busy, p);
                        let is_api = backend == "api";
                        composer_selectors(
                            ui,
                            chat,
                            &mut tests,
                            &models,
                            &backend,
                            if is_api { Some(&mut api_model) } else { None },
                            (!is_worker_session).then_some((active_orchestra.is_some(), &mut open_orchestra_dlg)),
                            p,
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.;
                        ui.add_enabled_ui(true, |ui| {
                            if design::action_button(
                                ui,
                                Icon::Send,
                                p,
                                true,
                            )
                            .on_hover_text(if chat.busy && live && matches!(backend.as_str(), "claude" | "codex") {
                                "Отправить сейчас · агент учтёт по ходу работы"
                            } else if chat.busy {
                                "В очередь · текущий ответ продолжится"
                            } else if connected {
                                "Отправить"
                            } else {
                                "Нет связи · сообщение уйдёт после переподключения"
                            })
                            .clicked()
                            {
                                send = true;
                            }
                        });
                        if chat.busy
                            && design::action_button(ui, Icon::Stop, p, false)
                                .on_hover_text("Остановить ответ и поставить очередь на паузу")
                                .clicked()
                        {
                            self.controller
                                .send(&server.id, json!({"op":"stop","sid":chat.sid}));
                            chat.status = "Останавливаем…".into();
                            chat.queue_paused = true;
                        }
                        let (pet_rect, pet_resp) = ui.allocate_exact_size(
                            Vec2::splat(26.),
                            egui::Sense::click(),
                        );
                        design::engine_icon(ui, pet_rect, &backend);
                        if pet_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Меню команд (/usage, /cost, /status, /help)")
                            .clicked()
                        {
                            if chat.draft.trim().is_empty() {
                                chat.draft = "/".into();
                            }
                            ui.memory_mut(|m| m.request_focus(egui::Id::new("prompt-field")));
                        }
                    });
                });
                if compact {
                    ui.add_space(4.);
                    ui.horizontal_wrapped(|ui| {
                        api_settings::backend_picker(ui, &mut backend, &mut configure_api, &mut login_provider, &mut open_agents, installed_flags, chat.busy, p);
                        let is_api = backend == "api";
                        composer_selectors(
                            ui,
                            chat,
                            &mut tests,
                            &models,
                            &backend,
                            if is_api { Some(&mut api_model) } else { None },
                            (!is_worker_session).then_some((active_orchestra.is_some(), &mut open_orchestra_dlg)),
                            p,
                        );
                    });
                }
            });
        let current = (chat.model.clone(), chat.mode.clone(), chat.effort.clone());
        // With the server host, model and mode switch even while the agent is answering.
        if live && chat.busy {
            if previous.0 != current.0 && backend != "api" {
                self.controller.send(&server.id, json!({"op":"control","sid":chat.sid,"backend":backend,"action":"model","value":current.0}));
            }
            if previous.1 != current.1 {
                self.controller.send(&server.id, json!({"op":"control","sid":chat.sid,"backend":backend,"action":"mode","value":current.1}));
            }
        }
        if previous.1 != current.1 {
            self.controller.send(&server.id,json!({"op":"_desktop_policy","sid":chat.sid,"auto":server.local_control && current.1=="auto"}));
        }
        if previous != current {
            if let Some(r) = self
                .cluster_store
                .runs
                .iter_mut()
                .find(|r| !r.detached && r.config.leader.server == server.id && r.leader_sid == chat.sid)
            {
                r.config.leader.model = current.0.clone();
                r.config.mode = current.1.clone();
                r.config.leader.effort = current.2.clone();
                if let Some(p) = &mut r.pending_config {
                    p.leader.model = current.0;
                    p.mode = current.1;
                    p.leader.effort = current.2;
                }
                self.cluster_store.dirty = true;
            } else if let Some(s) = self.settings.servers.iter_mut().find(|s| s.id == server.id) {
                s.set_model_for(&backend, current.0);
                s.mode = current.1;
                s.effort = current.2;
            }
            self.save();
        }
        if tests != server.tests {
            if let Some(s) = self.settings.servers.iter_mut().find(|s| s.id == server.id) {
                s.tests = tests.clone();
            }
            self.controller
                .send(&server.id, json!({"op":"_tests_policy","mode":tests}));
            self.save();
        }
        if backend != previous_backend || api_model != server.api.model {
            self.settings.set_session_backend(k.to_owned(), &backend);
            if let Some(profile) = self.settings.servers.iter_mut().find(|s| s.id == server.id) {
                profile.api.model = api_model;
            }
            if backend != previous_backend {
                let sid_copy = self.chats.get(k).map(|c| c.sid.clone()).unwrap_or_default();
                let is_dyn_leader = self
                    .cluster_store
                    .runs
                    .iter()
                    .any(|r| !r.detached && r.config.dynamic_workers && r.config.leader.server == server.id && r.leader_sid == sid_copy);
                if let Some(c) = self.chats.get_mut(k) {
                    if !c.cluster_context.is_object() || is_dyn_leader {
                        c.model = server.model_for(&backend).to_owned();
                    }
                }
                if is_dyn_leader {
                    if let Some(r) = self
                        .cluster_store
                        .runs
                        .iter_mut()
                        .find(|r| !r.detached && r.config.leader.server == server.id && r.leader_sid == sid_copy)
                    {
                        r.config.leader.backend = backend.clone();
                        r.config.leader.model = server.model_for(&backend).to_owned();
                        self.cluster_store.dirty = true;
                    }
                }
                if matches!(backend.as_str(), "codex" | "agy") {
                    self.controller.send(&server.id, json!({"op":"catalog","backend":backend}));
                }
            }
            if backend == "api" && server.api.base_url.is_empty() { configure_api = true; }
            self.save();
        }
        if disable_orchestra {
            let sid = self.chats.get(k).map(|c| c.sid.clone()).unwrap_or_default();
            self.disable_chat_orchestra(&server.id, &sid);
        }
        if open_orchestra_dlg {
            self.open_chat_orchestra_dialog(&server, k);
        }
        if let Some(rid) = open_cluster_view {
            self.cluster_store.selected = Some(rid);
            self.cluster_open = true;
            self.board_open = false;
            self.settings_open = false;
        }
        if install_active_cli {
            self.install_server_agent(&server.id, &backend, "install");
        }
        if open_agents {
            self.agents_open = Some(server.id.clone());
        }
        if let Some(prov) = login_provider {
            if let Some(profile) = self.settings.servers.iter().find(|s| s.id == server.id).cloned() {
                self.start_provider_login(&profile, &prov);
            }
        }
        if configure_api {
            if let Some(profile) = self.settings.servers.iter().find(|s| s.id == server.id).cloned() { self.open_api_settings(&profile); }
        }
        if send {
            self.send_prompt();
        }
        if attach {
            self.pick_images();
        }
    }
    fn board_panel(&mut self, ctx: &egui::Context) {
        let p = design::animated_palette(ctx, self.settings.light);
        let server = self.server();
        if let Some(s) = &server {
            if self.connected(&s.id)
                && self
                    .board_polled
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(3))
            {
                self.board_polled = Some(Instant::now());
                let since = self.boards.get(&s.id).map(|b| b["rev"].clone());
                self.controller
                    .send(&s.id, json!({"op":"board","since":since}));
            }
            ctx.request_repaint_after(Duration::from_secs(3));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.base))
            .show(ctx, |ui| {
                let Some(server) = server else {
                    ui.centered_and_justified(|ui| {
                        label(ui, "Подключи сервер, чтобы открыть доску", 14., p.muted)
                    });
                    return;
                };
                let Some(state) = self.boards.get(&server.id).cloned() else {
                    ui.centered_and_justified(|ui| {
                        label(
                            ui,
                            if self.connected(&server.id) {
                                "Загружаем доску…"
                            } else {
                                "Подключись к серверу, чтобы открыть доску"
                            },
                            14.,
                            p.muted,
                        )
                    });
                    return;
                };
                if !self.notice.is_empty() {
                    let mut clear = false;
                    egui::TopBottomPanel::top("board-notice")
                        .show_separator_line(false)
                        .frame(
                            egui::Frame::new()
                                .fill(accent().gamma_multiply(0.12))
                                .inner_margin(egui::Margin::symmetric(20, 6)),
                        )
                        .show_inside(ui, |ui| {
                            ui.horizontal(|ui| {
                                label(ui, self.notice.clone(), 12.5, p.text);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    clear = design::button(
                                        ui,
                                        "",
                                        Some(Icon::Close),
                                        Kind::Ghost,
                                        p,
                                        Vec2::splat(24.),
                                    )
                                    .clicked();
                                });
                            });
                        });
                    if clear {
                        self.notice.clear();
                    }
                }
                let me = self.settings.actor();
                let mut ops = crate::board::show(ui, &state, &mut self.board_view, p, &me);
                ops.extend(crate::board::take_queued(&mut self.board_view));
                if !ops.is_empty() {
                    self.controller.send(
                        &server.id,
                        json!({"op":"board_apply","ops":ops,"actor":me,"host":model::hostname()}),
                    );
                }
            });
    }
    fn game_test_cards(&mut self, ui: &mut egui::Ui, server: &str, p: Palette) {
        let mut stop = None;
        let mut close = None;
        for (s, status) in &self.game_tests {
            if s != server {
                continue;
            }
            let done = status["stage"] == "done";
            let result = &status["result"];
            let ok = done && result["ok"] == true;
            let purpose = status["purpose"].as_str().unwrap_or("");
            egui::Frame::new()
                .fill(design::mix(
                    p.surface,
                    if ok { GREEN } else { accent() },
                    if done { 0.08 } else { 0.05 },
                ))
                .stroke(Stroke::new(1.0_f32, p.line))
                .corner_radius(10.)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if done {
                            let (r, _) =
                                ui.allocate_exact_size(Vec2::splat(16.), egui::Sense::hover());
                            design::icon(
                                ui.painter(),
                                r,
                                if ok { Icon::Check } else { Icon::Close },
                                if ok { GREEN } else { accent() },
                            );
                        } else {
                            design::dots(ui, accent());
                        }
                        let headline = if !done {
                            format!(
                                "Игровой тест · {}",
                                status["message"].as_str().unwrap_or("")
                            )
                        } else if ok {
                            format!(
                                "Игровой тест пройден · {} с",
                                result["seconds"].as_u64().unwrap_or(0)
                            )
                        } else {
                            let failed = result["report"]["steps"]
                                .as_array()
                                .and_then(|steps| steps.iter().find(|s| s["ok"] == false))
                                .and_then(|s| s["error"].as_str())
                                .or(result["message"].as_str())
                                .unwrap_or("не пройден");
                            format!("Игровой тест: {}", model::shorten(failed, 90))
                        };
                        label(ui, headline, 12.5, p.text);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let id = status["id"].as_str().unwrap_or("").to_owned();
                            if done {
                                if design::button(
                                    ui,
                                    "",
                                    Some(Icon::Close),
                                    Kind::Ghost,
                                    p,
                                    Vec2::splat(24.),
                                )
                                .on_hover_text("Скрыть")
                                .clicked()
                                {
                                    close = Some(id);
                                }
                                if let Some(folder) = result["local_folder"].as_str() {
                                    if design::button(
                                        ui,
                                        "",
                                        Some(Icon::Folder),
                                        Kind::Ghost,
                                        p,
                                        Vec2::splat(24.),
                                    )
                                    .on_hover_text("Скриншоты и логи на этом ПК")
                                    .clicked()
                                    {
                                        open_folder(folder);
                                    }
                                }
                            } else if design::button(
                                ui,
                                "",
                                Some(Icon::Stop),
                                Kind::Ghost,
                                p,
                                Vec2::splat(24.),
                            )
                            .on_hover_text("Остановить тест")
                            .clicked()
                            {
                                stop = Some(id);
                            }
                        });
                    });
                    if !purpose.is_empty() {
                        label(ui, purpose, 11.5, p.muted);
                    }
                });
            ui.add_space(6.);
        }
        if let Some(id) = stop {
            self.controller
                .send(server, json!({"op":"_game_test_stop","id":id}));
        }
        if let Some(id) = close {
            self.game_tests.retain(|(_, v)| v["id"] != id.as_str());
        }
    }
    fn permission_card(&mut self, ui: &mut egui::Ui, k: &str, p: Palette) {
        let Some(permission) = self
            .chats
            .get(k)
            .and_then(|c| c.permissions.first())
            .cloned()
        else {
            return;
        };
        let Some(server) = self.server() else {
            return;
        };
        let sid = self.chats[k].sid.clone();
        let mut decision = None;
        egui::Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, accent().gamma_multiply(0.6)))
            .corner_radius(10.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                label(
                    ui,
                    if permission.tool == "AskUserQuestion" {
                        "Claude уточняет"
                    } else {
                        "Нужно твоё разрешение"
                    },
                    15.0,
                    p.text,
                );
                let mut ready = true;
                egui::ScrollArea::vertical()
                    .max_height(225.0)
                    .id_salt("permission-details")
                    .show(ui, |ui| {
                        if permission.tool == "AskUserQuestion" {
                            if let Some(questions) = permission.input["questions"].as_array() {
                                for question in questions {
                                    let text = question["question"].as_str().unwrap_or("");
                                    label(ui, text, 14.0, p.text);
                                    let a = self
                                        .answers
                                        .entry(format!("{}/{text}", permission.id))
                                        .or_default();
                                    if let Some(options) = question["options"].as_array() {
                                        for option in options {
                                            let option = option["label"].as_str().unwrap_or("");
                                            if question["multiSelect"].as_bool().unwrap_or(false) {
                                                let mut chosen: Vec<String> = a
                                                    .split(", ")
                                                    .filter(|s| !s.is_empty())
                                                    .map(str::to_owned)
                                                    .collect();
                                                let mut checked =
                                                    chosen.iter().any(|s| s == option);
                                                if ui.checkbox(&mut checked, option).changed() {
                                                    chosen.retain(|s| s != option);
                                                    if checked {
                                                        chosen.push(option.into());
                                                    }
                                                    *a = chosen.join(", ");
                                                }
                                            } else if ui
                                                .selectable_label(a == option, option)
                                                .clicked()
                                            {
                                                *a = option.into();
                                            }
                                        }
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(a)
                                            .hint_text("Свой ответ")
                                            .desired_width(f32::INFINITY),
                                    );
                                    ready &= !a.trim().is_empty();
                                }
                            }
                        } else {
                            label(ui, tool_name(&permission.tool), 12.0, p.muted);
                            let text = if permission.tool == "ExitPlanMode" {
                                "Перейти от плана к выполнению задачи?".into()
                            } else {
                                model::summary(&permission.input)
                            };
                            ui.add(
                                egui::Label::new(RichText::new(text).monospace().size(12.0))
                                    .selectable(true)
                                    .wrap(),
                            );
                        }
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            ready,
                            egui::Button::new(if permission.tool == "AskUserQuestion" {
                                "Ответить"
                            } else {
                                "Разрешить"
                            })
                            .fill(accent()),
                        )
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button("Отклонить").clicked() {
                        decision = Some(false);
                    }
                });
            });
        ui.add_space(10.0);
        if let Some(allow) = decision {
            let mut answers = serde_json::Map::new();
            if let Some(questions) = permission.input["questions"].as_array() {
                for q in questions {
                    if let Some(text) = q["question"].as_str() {
                        answers.insert(
                            text.into(),
                            json!(self
                                .answers
                                .remove(&format!("{}/{text}", permission.id))
                                .unwrap_or_default()),
                        );
                    }
                }
            }
            self.controller.send(&server.id,json!({"op":"permission","sid":sid,"id":permission.id,"allow":allow,"answers":answers}));
            self.chats.get_mut(k).unwrap().permissions.remove(0);
        }
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        self.api_dialog(ctx);
        let p = design::animated_palette(ctx, self.settings.light);
        if let Some((server, request)) = self.desktop_requests.first().cloned() {
            let mut decision = None;
            let profile = self.settings.servers.iter().find(|s| s.id == server);
            let name = profile.map(|s| s.name.as_str()).unwrap_or("Сервер");
            let workspace = profile
                .and_then(|s| s.local_workspace.as_deref())
                .unwrap_or("Не подключено");
            let command = request["tool"] == "run_local_command";
            let test = request["tool"] == "minecraft_test";
            let modal = egui::Modal::new(egui::Id::new("desktop-approval"))
                .frame(
                    egui::Frame::new()
                        .fill(p.surface)
                        .corner_radius(14.)
                        .inner_margin(20.),
                )
                .show(ctx, |ui| {
                    if test {
                        ui.set_width(460.);
                        label(ui, "Прогнать тест?", 18., p.text);
                        label(
                            ui,
                            format!("Claude из {name} хочет проверить изменения в игре"),
                            12.,
                            p.muted,
                        );
                        ui.add_space(10.);
                        label(
                            ui,
                            request["input"]["purpose"]
                                .as_str()
                                .unwrap_or("Проверка в игре"),
                            14.,
                            p.text,
                        );
                        ui.add_space(6.);
                        let mods: Vec<_> = request["input"]["mods"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|m| m["name"].as_str())
                            .collect();
                        let steps = request["input"]["scenario"]
                            .as_str()
                            .unwrap_or("")
                            .lines()
                            .map(str::trim)
                            .filter(|l| !l.is_empty() && !l.starts_with('#') && *l != "quit")
                            .count();
                        label(
                            ui,
                            format!(
                                "Minecraft {} · шагов: {steps} · до {} мин\nМоды: {}",
                                request["input"]["minecraft_version"]
                                    .as_str()
                                    .unwrap_or("1.21.8"),
                                request["input"]["timeout_seconds"]
                                    .as_u64()
                                    .unwrap_or(300)
                                    .div_ceil(60),
                                mods.join(", ")
                            ),
                            12.,
                            p.muted,
                        );
                        egui::CollapsingHeader::new(
                            RichText::new("Сценарий").size(12.).color(p.muted),
                        )
                        .id_salt("test-scenario")
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(160.)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(
                                                request["input"]["scenario"].as_str().unwrap_or(""),
                                            )
                                            .monospace()
                                            .size(12.),
                                        )
                                        .wrap(),
                                    );
                                });
                        });
                        label(
                            ui,
                            "Игра запустится скрыто в отдельной папке Relay и не заберёт фокус.",
                            11.5,
                            p.muted,
                        );
                        ui.add_space(12.);
                        ui.horizontal(|ui| {
                            if design::button(
                                ui,
                                "Пропустить",
                                None,
                                Kind::Secondary,
                                p,
                                Vec2::new(0., 32.),
                            )
                            .clicked()
                            {
                                decision = Some(false);
                            }
                            if design::button(
                                ui,
                                "Прогнать",
                                Some(Icon::Check),
                                Kind::Primary,
                                p,
                                Vec2::new(0., 32.),
                            )
                            .clicked()
                            {
                                decision = Some(true);
                            }
                        });
                        return;
                    }
                    ui.set_width(480.);
                    label(
                        ui,
                        if command {
                            "Команда на этом компьютере"
                        } else {
                            "Файл с сервера"
                        },
                        20.,
                        p.text,
                    );
                    label(ui, format!("Запрос из {name}"), 12., p.muted);
                    ui.add_space(8.);
                    egui::ScrollArea::vertical()
                        .max_height(220.)
                        .show(ui, |ui| {
                            let text = if command {
                                request["input"]["command"].as_str().unwrap_or("")
                            } else {
                                request["input"]["server_path"].as_str().unwrap_or("")
                            };
                            ui.add(
                                egui::Label::new(RichText::new(text).monospace().size(13.)).wrap(),
                            );
                        });
                    label(
                        ui,
                        if command {
                            format!(
                                "Папка: {workspace} · {} · до {} с",
                                std::env::consts::OS,
                                request["input"]["timeout_seconds"]
                                    .as_u64()
                                    .unwrap_or(60)
                                    .clamp(1, 120)
                            )
                        } else {
                            format!(
                                "{} · место сохранения выберешь сам",
                                format!(
                                    "{:.1} МБ",
                                    request["input"]["size"].as_u64().unwrap_or(0) as f64
                                        / 1048576.
                                )
                            )
                        },
                        12.,
                        p.muted,
                    );
                    ui.add_space(12.);
                    ui.horizontal(|ui| {
                        if ui.button("Отклонить").clicked() {
                            decision = Some(false);
                        }
                        if ui
                            .button(if command {
                                "Выполнить"
                            } else {
                                "Сохранить…"
                            })
                            .clicked()
                        {
                            decision = Some(true);
                        }
                    });
                });
            if modal.should_close() {
                decision = Some(false);
            }
            if let Some(allow) = decision {
                self.desktop_requests.remove(0);
                if allow {
                    self.controller.approve_desktop(&server, &request);
                } else {
                    self.controller
                        .send(&server, json!({"op":"_desktop_deny","id":request["id"]}));
                }
            }
        }
        let visible = self.editor.is_some()
            || self.trust.is_some()
            || self.folder_open
            || self.folder_dialog.is_some()
            || self.icon_dialog.is_some()
            || self.close_confirm
            || self.delete_chat.is_some()
            || self.lock_dialog.is_some()
            || self.agents_open.is_some()
            || self.radar_open.is_some();
        let modal_t = ctx.animate_bool_with_time_and_easing(
            egui::Id::new("modal-fade"),
            visible,
            0.28,
            egui::emath::easing::cubic_in_out,
        );
        self.deletion_dialog(ctx, p, modal_t);
        self.lock_dialog(ctx, p, modal_t);
        if let Some((id, code)) = self.phone_pairing.as_ref() {
            let mut open = true;
            egui::Window::new("Связать телефон").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
                ui.label("На телефоне открой чат на этом же сервере и выбери «Тесты на ПК».");
                ui.horizontal(|ui| {
                    ui.monospace(code.as_str());
                    if ui.button("Копировать код").clicked() { ctx.copy_text(code.to_string()); }
                });
                #[cfg(target_os = "linux")]
                if let Ok(entry) = relay_desktop::phone_tests::login_entry(id) {
                    let mut enabled = entry.exists();
                    if ui.checkbox(&mut enabled, "Запускать тесты на этом ПК при входе в систему").changed() {
                        if let Err(e) = relay_desktop::phone_tests::set_login_start(id, enabled) { self.notice = e.to_string(); }
                    }
                }
            });
            if !open { self.phone_pairing = None; }
        }
        if let Some(mut dialog) = self.folder_password.take() {
            let mut open = true;
            let mut submit = false;
            let action = match dialog.action {
                "set" => "Задать пароль",
                "remove" => "Снять пароль",
                _ => "Открыть папку",
            };
            egui::Window::new(format!("{action} · {}", dialog.name))
                .id(egui::Id::new("folder-password"))
                .open(&mut open).resizable(false).collapsible(false)
                .show(ctx, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut dialog.password).password(true).hint_text("Пароль").char_limit(256));
                    if dialog.action == "set" {
                        ui.add(egui::TextEdit::singleline(&mut dialog.confirm).password(true).hint_text("Повтори пароль").char_limit(256));
                    }
                    let valid = !dialog.password.is_empty() && (dialog.action != "set" || dialog.password == dialog.confirm);
                    submit = ui.add_enabled(valid, egui::Button::new(action)).clicked()
                        || (valid && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                });
            if submit {
                self.controller.send(&dialog.server, json!({"op":"folder_access","id":dialog.folder,"action":dialog.action,"password":dialog.password}));
            } else if open {
                self.folder_password = Some(dialog);
            }
        }
        self.folder_modal(ctx, p, modal_t);
        self.project_icon_modal(ctx, p);
        self.agents_dialog(ctx, p, modal_t);
        self.radar_dialog(ctx, p, modal_t);
        if let Some(mut editor) = self.editor.take() {
            let mut open = true;
            let mut submit = false;
            let mut delete = false;
            let mut cancel = false;
            let response = egui::Modal::new(egui::Id::new("server-editor"))
                .area(
                    egui::Modal::default_area(egui::Id::new("server-editor")).anchor(
                        egui::Align2::CENTER_CENTER,
                        Vec2::new(0., (1. - modal_t) * 9.),
                    ),
                )
                .frame(
                    egui::Frame::new()
                        .fill(p.surface)
                        .corner_radius(14.)
                        .inner_margin(22.)
                        .stroke(Stroke::new(1.0_f32, p.line)),
                )
                .backdrop_color(Color32::from_black_alpha((120. * modal_t) as u8))
                .show(ctx, |ui| {
                    ui.set_width(440.);
                    ui.set_opacity(modal_t);
                    ui.horizontal(|ui| {
                        label(
                            ui,
                            if editor.is_new {
                                "Новое подключение"
                            } else {
                                "Подключение к серверу"
                            },
                            23.,
                            p.text,
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if design::button(
                                ui,
                                "",
                                Some(Icon::Close),
                                Kind::Ghost,
                                p,
                                Vec2::splat(30.),
                            )
                            .clicked()
                            {
                                cancel = true;
                            }
                        });
                    });
                    label(ui, "Твои проекты и разговоры на этом сервере", 13., p.muted);
                    ui.add_space(20.);
                    label(ui, "Название", 12., p.muted);
                    design::field(
                        ui,
                        &mut editor.server.name,
                        "Например, рабочий сервер",
                        false,
                    );
                    ui.add_space(4.);
                    label(ui, "Адрес сервера", 12., p.muted);
                    design::field(ui, &mut editor.server.host, "IP-адрес или домен", false);
                    ui.add_space(4.);
                    ui.horizontal(|ui| {
                        let width = ui.available_width();
                        ui.allocate_ui_with_layout(
                            Vec2::new(width - 110., 70.),
                            Layout::top_down(Align::Min),
                            |ui| {
                                label(ui, "Пользователь", 12., p.muted);
                                design::field(ui, &mut editor.server.user, "root", false);
                            },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(100., 70.),
                            Layout::top_down(Align::Min),
                            |ui| {
                                label(ui, "Порт", 12., p.muted);
                                ui.add_sized(
                                    [ui.available_width(), 36.],
                                    egui::DragValue::new(&mut editor.server.port).range(1..=65535),
                                );
                            },
                        );
                    });
                    label(ui, "Пароль", 12., p.muted);
                    design::field(
                        ui,
                        &mut editor.password,
                        if editor.is_new {
                            "Пароль сервера"
                        } else {
                            "Сохранённый пароль"
                        },
                        true,
                    );
                    ui.add_space(4.);
                    label(ui, "Рабочая папка", 12., p.muted);
                    design::field(ui, &mut editor.server.cwd, "~", false);
                    ui.add_space(6.);
                    label(ui, "Агент по умолчанию", 12., p.muted);
                    let mut chosen_backend = editor.server.effective_backend().to_owned();
                    egui::ComboBox::from_id_salt("server-default-backend")
                        .width(ui.available_width())
                        .selected_text(crate::model::backend_label(&chosen_backend))
                        .icon(design::combo_icon)
                        .show_ui(ui, |ui| {
                            for id in crate::model::BACKENDS {
                                ui.selectable_value(
                                    &mut chosen_backend,
                                    id.to_owned(),
                                    crate::model::backend_label(id),
                                );
                            }
                        });
                    if chosen_backend != editor.server.effective_backend() {
                        editor.server.set_default_backend(&chosen_backend);
                    }
                    ui.add_space(4.);
                    design::toggle(ui, &mut editor.server.remember, "Запомнить пароль", p)
                        .on_hover_text("Пароль сохраняется в хранилище системы");
                    if !editor.error.is_empty() {
                        ui.add_space(5.);
                        label(ui, &editor.error, 12., accent());
                    }
                    ui.add_space(18.);
                    ui.horizontal(|ui| {
                        if !editor.is_new
                            && design::button(
                                ui,
                                "Удалить",
                                None,
                                Kind::Ghost,
                                p,
                                Vec2::new(82., 34.),
                            )
                            .clicked()
                        {
                            delete = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if design::button(
                                ui,
                                "Подключиться",
                                Some(Icon::Arrow),
                                Kind::Primary,
                                p,
                                Vec2::new(166., 34.),
                            )
                            .clicked()
                            {
                                submit = true;
                            }
                            if design::button(
                                ui,
                                "Отмена",
                                None,
                                Kind::Ghost,
                                p,
                                Vec2::new(94., 34.),
                            )
                            .clicked()
                            {
                                cancel = true;
                            }
                        });
                    });
                });
            if response.should_close() {
                open = false;
            }
            if delete {
                self.controller.disconnect(&editor.server.id);
                self.reconnect.remove(&editor.server.id);
                self.settings.servers.retain(|s| s.id != editor.server.id);
                self.login_passwords.remove(&editor.server.id);
                storage::forget(&editor.server.id);
                let _ = storage::remember_api_key(&editor.server.id, "");
                if self.settings.selected.as_deref() == Some(&editor.server.id) {
                    self.settings.selected = self.settings.servers.first().map(|s| s.id.clone());
                }
                self.save();
                editor.password.zeroize();
            } else if submit {
                match editor.server.validate() {
                    Err(e) => {
                        editor.error = e.to_string();
                        self.editor = Some(editor);
                    }
                    Ok(()) => {
                        if let Some(old) = self
                            .settings
                            .servers
                            .iter()
                            .find(|s| s.id == editor.server.id)
                        {
                            if (old.host.as_str(), old.port, old.user.as_str())
                                != (
                                    editor.server.host.as_str(),
                                    editor.server.port,
                                    editor.server.user.as_str(),
                                )
                            {
                                editor.server.fingerprint = None;
                                if editor.password.is_empty() {
                                    editor.error = "Введи пароль для нового адреса сервера.".into();
                                    self.editor = Some(editor);
                                    return;
                                }
                                self.login_passwords.remove(&editor.server.id);
                            }
                        }
                        if editor.password.is_empty() {
                            if let Some(password) = self.login_passwords.get(&editor.server.id).map(|p| p.to_string()).or_else(|| storage::password(&editor.server.id)) {
                                editor.password = password;
                            } else {
                                editor.error = "Введи пароль сервера.".into();
                                self.editor = Some(editor);
                                return;
                            }
                        }
                        if editor.server.remember {
                            if let Err(e) = storage::remember(&editor.server.id, &editor.password) {
                                self.notice = e.to_string();
                            }
                        } else {
                            storage::forget(&editor.server.id);
                        }
                        self.settings.servers.retain(|s| s.id != editor.server.id);
                        self.settings.servers.push(editor.server.clone());
                        self.settings.selected = Some(editor.server.id.clone());
                        self.save();
                        self.establish(editor.server, std::mem::take(&mut editor.password));
                    }
                }
            } else if open && !cancel {
                self.editor = Some(editor);
            } else {
                editor.password.zeroize();
            }
        }
        if let Some(trust) = self.trust.take() {
            let mut accept = false;
            let mut deny = false;
            let modal = egui::Modal::new(egui::Id::new("host-trust"))
                .area(
                    egui::Modal::default_area(egui::Id::new("host-trust")).anchor(
                        egui::Align2::CENTER_CENTER,
                        Vec2::new(0., (1. - modal_t) * 9.),
                    ),
                )
                .frame(
                    egui::Frame::new()
                        .fill(p.surface)
                        .corner_radius(14.)
                        .inner_margin(22.),
                )
                .show(ctx, |ui| {
                    ui.set_width(460.);
                    ui.set_opacity(modal_t);
                    label(ui, "Первое подключение", 18., p.text);
                    ui.add_space(10.);
                    label(ui, "Запомнить ключ этого сервера?", 15., p.text);
                    if let Some(server) =
                        self.settings.servers.iter().find(|s| s.id == trust.server)
                    {
                        label(
                            ui,
                            format!("{} · {}:{}", server.name, server.host, server.port),
                            13.,
                            p.muted,
                        );
                    }
                    ui.add_space(15.);
                    egui::Frame::new()
                        .fill(p.base)
                        .corner_radius(12.)
                        .inner_margin(14.)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&trust.fingerprint).monospace().size(12.),
                                )
                                .wrap(),
                            );
                        });
                    ui.add_space(18.);
                    ui.horizontal(|ui| {
                        if design::button(ui, "Отмена", None, Kind::Ghost, p, Vec2::new(100., 34.))
                            .clicked()
                        {
                            deny = true;
                        }
                        if design::button(
                            ui,
                            "Запомнить и продолжить",
                            Some(Icon::Check),
                            Kind::Primary,
                            p,
                            Vec2::new(260., 34.),
                        )
                        .clicked()
                        {
                            accept = true;
                        }
                    });
                });
            if modal.should_close() {
                deny = true;
            }
            if accept {
                if let Some(s) = self
                    .settings
                    .servers
                    .iter_mut()
                    .find(|s| s.id == trust.server)
                {
                    s.fingerprint = Some(trust.fingerprint);
                }
                self.save();
                let _ = trust.answer.send(true);
            } else if deny {
                let _ = trust.answer.send(false);
            } else {
                self.trust = Some(trust);
            }
        }
        if self.folder_open {
            let mut open = true;
            let mut chosen = None;
            let mut browse = None;
            let modal = egui::Modal::new(egui::Id::new("folder-picker"))
                .area(
                    egui::Modal::default_area(egui::Id::new("folder-picker")).anchor(
                        egui::Align2::CENTER_CENTER,
                        Vec2::new(0., (1. - modal_t) * 9.),
                    ),
                )
                .frame(
                    egui::Frame::new()
                        .fill(p.surface)
                        .corner_radius(14.)
                        .inner_margin(22.),
                )
                .show(ctx, |ui| {
                    ui.set_width(540.);
                    ui.set_opacity(modal_t);
                    ui.horizontal(|ui| {
                        label(ui, "Рабочая папка", 18., p.text);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if design::button(
                                ui,
                                "",
                                Some(Icon::Close),
                                Kind::Ghost,
                                p,
                                Vec2::splat(30.),
                            )
                            .clicked()
                            {
                                open = false;
                            }
                        });
                    });
                    ui.add_space(16.);
                    ui.horizontal(|ui| {
                        if design::button(
                            ui,
                            "",
                            Some(Icon::Send),
                            Kind::Secondary,
                            p,
                            Vec2::splat(43.),
                        )
                        .on_hover_text("На уровень выше")
                        .clicked()
                        {
                            browse = Some(self.folder_parent.clone());
                        }
                        let width = ui.available_width() - 94.;
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 34.),
                            Layout::top_down(Align::Min),
                            |ui| {
                                let response = design::field(
                                    ui,
                                    &mut self.folder_path,
                                    "Путь на сервере",
                                    false,
                                );
                                if response.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    browse = Some(self.folder_path.clone());
                                }
                            },
                        );
                        if design::button(
                            ui,
                            "Открыть",
                            None,
                            Kind::Secondary,
                            p,
                            Vec2::new(85., 34.),
                        )
                        .clicked()
                        {
                            browse = Some(self.folder_path.clone());
                        }
                    });
                    ui.add_space(8.);
                    if design::toggle(ui, &mut self.folder_hidden, "Скрытые папки", p).clicked()
                    {
                        browse = Some(self.folder_path.clone());
                    }
                    if let Some(server) = self.server() {
                        ui.horizontal_wrapped(|ui| {
                            for path in server.folders.iter().take(5) {
                                if design::button(
                                    ui,
                                    &model::shorten(path, 22),
                                    Some(Icon::Folder),
                                    Kind::Ghost,
                                    p,
                                    Vec2::new(0., 32.),
                                )
                                .on_hover_text(path)
                                .clicked()
                                {
                                    browse = Some(path.clone());
                                }
                            }
                        });
                    }
                    ui.add_space(10.);
                    egui::ScrollArea::vertical()
                        .max_height(320.)
                        .show(ui, |ui| {
                            for folder in &self.folders {
                                if design::button(
                                    ui,
                                    &folder.name,
                                    Some(Icon::Folder),
                                    Kind::Ghost,
                                    p,
                                    Vec2::new(ui.available_width(), 32.),
                                )
                                .clicked()
                                {
                                    browse = Some(folder.path.clone());
                                }
                            }
                        });
                    ui.add_space(16.);
                    ui.add_enabled_ui(self.folder_path == self.folder_confirmed, |ui| {
                        if design::button(
                            ui,
                            "Работать в этой папке",
                            Some(Icon::Check),
                            Kind::Primary,
                            p,
                            Vec2::new(ui.available_width(), 34.),
                        )
                        .clicked()
                        {
                            chosen = Some(self.folder_path.clone());
                        }
                    });
                });
            if modal.should_close() {
                open = false;
            }
            self.folder_open = open;
            if let Some(path) = browse {
                self.folder_path = path;
                self.request_folders();
            }
            if let Some(path) = chosen {
                if let Some(s) = self
                    .settings
                    .servers
                    .iter_mut()
                    .find(|s| Some(&s.id) == self.settings.selected.as_ref())
                {
                    s.cwd = path.clone();
                    s.last_session = None;
                    s.folders.retain(|p| p != &path);
                    s.folders.insert(0, path);
                    s.folders.truncate(10);
                }
                self.save();
                self.new_chat();
                self.folder_open = false;
            }
        }
        if self.close_confirm {
            let modal=egui::Modal::new(egui::Id::new("close-confirm")).area(egui::Modal::default_area(egui::Id::new("close-confirm" )).anchor(egui::Align2::CENTER_CENTER,Vec2::new(0.,(1.-modal_t)*9.))).frame(egui::Frame::new().fill(p.surface).corner_radius(14.).inner_margin(22.)).show(ctx,|ui|{
            ui.set_width(440.);label(ui,"Есть незавершённые задачи",18.,p.text);ui.add_space(12.);label(ui,"Неотправленная очередь сообщений будет удалена. На серверах с новым помощником Claude доведёт начатые ответы без приложения, на старых — ответы остановятся.",14.,p.muted);ui.add_space(20.);ui.horizontal(|ui|{if design::button(ui,"Остаться",None,Kind::Secondary,p,Vec2::new(130.,34.)).clicked(){self.close_confirm=false;}if design::button(ui,"Закрыть",None,Kind::Primary,p,Vec2::new(240.,34.)).clicked(){ctx.send_viewport_cmd(egui::ViewportCommand::Close);self.close_confirm=false;for chat in self.chats.values_mut(){chat.busy=false;chat.queue.clear();}}});
        });
            if modal.should_close() {
                self.close_confirm = false;
            }
        }
    }
    fn seed_demo(&mut self) {
        let s = Server {
            id: "demo".into(),
            name: "Production".into(),
            host: "192.168.1.10".into(),
            cwd: "/opt/atlas".into(),
            ..Default::default()
        };
        self.settings.servers = vec![
            s.clone(),
            Server {
                id: "stage".into(),
                name: "Stage".into(),
                host: "192.168.1.11".into(),
                ..Default::default()
            },
        ];
        self.settings.selected = Some(s.id.clone());
        self.remote.insert(
            s.id.clone(),
            Remote {
                auth_available: true,
                api_models: vec![],
                api_tokens: (0, 0),
                connected: true,
                connecting: false,
                version: "Claude Code".into(),
                home: "/root".into(),
                usage: serde_json::from_value(json!({
                    "available":true,
                    "windows":[
                        {"id":"five_hour","utilization":38,"resets_at":chrono::Local::now().timestamp()+10500},
                        {"id":"seven_day","utilization":64,"resets_at":chrono::Local::now().timestamp()+198000}
                    ],
                    "updated_at":chrono::Local::now().timestamp()
                })).unwrap_or_default(),
                usage_requested: None,
                usage_by: HashMap::new(),
                usage_polled: HashMap::new(),
                sessions_requested: None,
                sessions_loaded: true,
                sessions_retries: 0,
                sessions_error: String::new(),
                sessions_polled: None,
                protocol: 4,
                running: std::collections::HashSet::new(),
                models: serde_json::from_value(json!([
                    {"value":"default","displayName":"Default (recommended)","description":"Opus 5.5 · Best for everyday, complex tasks"},
                    {"value":"opus","displayName":"Opus 5.5","description":"For complex work and everyday tasks"},
                    {"value":"sonnet","displayName":"Sonnet 5.5","description":"Most efficient for simpler tasks"},
                    {"value":"claude-fable-5-2","displayName":"Fable 5.2","description":"For your toughest challenges"},
                    {"value":"haiku","displayName":"Haiku 5.5","description":"Fastest for quick answers"}
                ])).unwrap_or_default(),
                updating: false,
                installing_agents: HashMap::new(),
                agent_logs: HashMap::new(),
                codex_models: crate::model::default_backend_models("codex"),
                agy_models: crate::model::default_backend_models("agy"),
                codex_commands: vec![],
                agy_commands: vec![],
                backends: json!({"claude":{"installed":true},"codex":{"installed":true},"agy":{"installed":true},"api":{"installed":true}}),
                commands: serde_json::from_value(json!([
                    {"name":"compact","description":"Сжать историю разговора","hint":"[что сохранить]"},
                    {"name":"context","description":"Показать, чем занят контекст","hint":""},
                    {"name":"model","description":"Сменить модель","hint":"[модель]"},
                    {"name":"code-review","description":"Проверить изменения","hint":""}
                ])).unwrap_or_default(),
                folders: vec![
                    crate::model::SessionFolder { id: "f-ships".into(), name: "Корабли".into(), icon: String::new(), ..Default::default() },
                    crate::model::SessionFolder { id: "f-site".into(), name: "Сайт и лаунчер".into(), icon: String::new(), ..Default::default() },
                ],
                sessions: vec![
                    Session {
                        id: "demo-chat".into(),
                        title: "Упрощаем управление заказами".into(),
                        cwd: s.cwd.clone(),
                        modified: chrono::Local::now().timestamp() as f64,
                        pinned: 1.,
                        ..Default::default()
                    },
                    Session {
                        id: "d2".into(),
                        title: "Новый интерфейс мастерской".into(),
                        cwd: s.cwd.clone(),
                        modified: chrono::Local::now().timestamp() as f64 - 4000.0,
                        ..Default::default()
                    },
                    Session {
                        id: "d4".into(),
                        title: "Якорь и цепь у штурвала".into(),
                        cwd: "/opt/sbrp-client".into(),
                        modified: chrono::Local::now().timestamp() as f64 - 7000.0,
                        folder: "f-ships".into(),
                        ..Default::default()
                    },
                    Session {
                        id: "d5".into(),
                        title: "Паруса и ветер".into(),
                        cwd: "/opt/sbrp-client".into(),
                        modified: chrono::Local::now().timestamp() as f64 - 200000.0,
                        folder: "f-ships".into(),
                        ..Default::default()
                    },
                    Session {
                        id: "d6".into(),
                        title: "Подпись лаунчера".into(),
                        cwd: "/opt/sb-launcher".into(),
                        modified: chrono::Local::now().timestamp() as f64 - 30000.0,
                        folder: "f-site".into(),
                        ..Default::default()
                    },
                    Session {
                        id: "d3".into(),
                        title: "Проверка обновления сервера".into(),
                        cwd: s.cwd.clone(),
                        modified: chrono::Local::now().timestamp() as f64 - 90000.0,
                        ..Default::default()
                    },
                ],
            },
        );
        let mut c = Chat::new(
            "demo-chat".into(),
            s.cwd,
            "Упрощаем управление заказами".into(),
        );
        c.loaded = true;
        c.messages=vec![Message{role:"user".into(),text:"Давай сделаем управление заказами понятнее. Хочется выбрать работу, посмотреть условия и принять результат в одном окне.".into(),..Default::default()},Message{role:"assistant".into(),text:"Соберу всё в одном месте. Сначала посмотрю, как сейчас устроены заказы и приёмка работы.".into(),..Default::default()},Message{role:"tool".into(),tool:"Read".into(),input:json!({"file_path":"/opt/atlas/src/orders/board.rs"}),..Default::default()},Message{role:"assistant".into(),text:"Нашёл нужный экран. Предлагаю три небольших изменения:\n\n- **Условия рядом с заказом** — работа, оплата и срок видны сразу.\n- **Свои и свободные** — два понятных списка.\n- **Приёмка заказчиком** — оплата приходит после подтверждения.\n\nИсторию и текущие заявки сохраним. Начну с экрана списка.".into(),..Default::default()}];
        self.selected_chat.insert(s.id.clone(), c.sid.clone());
        self.chats.insert(key(&s.id, &c.sid), c);
    }
}
/// `cd` into the session's folder and resume it: Claude Code finds sessions by working folder.
#[cfg(test)]
fn resume_command(cwd: &str, sid: &str) -> String {
    resume_command_for(cwd, sid, "", "claude")
}

fn resume_command_for(cwd: &str, sid: &str, native_id: &str, backend: &str) -> String {
    let quoted = if cwd.chars().all(|c| c.is_ascii_alphanumeric() || "/._-~".contains(c)) {
        cwd.to_owned()
    } else {
        format!("'{}'", cwd.replace('\'', "'\\''"))
    };
    let target = if native_id.is_empty() { sid } else { native_id };
    match backend {
        "codex" => format!("cd {quoted} && codex resume {target}"),
        "agy" => format!("cd {quoted} && agy --conversation {target}"),
        _ => format!("cd {quoted} && claude --resume {sid}"),
    }
}

/// Commands for the `/` popup: the draft is a single word starting with `/`.
fn slash_matches<'a>(draft: &str, commands: &'a [crate::model::CommandInfo]) -> Vec<&'a crate::model::CommandInfo> {
    let Some(typed) = draft.strip_prefix('/') else {
        return vec![];
    };
    if typed.contains(char::is_whitespace) {
        return vec![];
    }
    let typed = typed.to_lowercase();
    let mut found: Vec<_> = commands.iter().filter(|c| c.name.to_lowercase().starts_with(&typed)).collect();
    found.extend(commands.iter().filter(|c| !c.name.to_lowercase().starts_with(&typed) && c.name.to_lowercase().contains(&typed)));
    if found.len() == 1 && found[0].name == typed {
        return vec![];
    }
    found
}

/// `fable`, `Fable 5.2` or a full id → the value Claude Code accepts.
fn resolve_model(models: &[crate::model::ModelInfo], typed: &str) -> Option<String> {
    let t = typed.to_lowercase();
    if models.is_empty() {
        return Some(typed.to_owned());
    }
    models
        .iter()
        .find(|m| m.value.to_lowercase() == t || m.display_name.to_lowercase() == t)
        .or_else(|| models.iter().find(|m| m.display_name.to_lowercase().starts_with(&t) || m.value.to_lowercase().starts_with(&t)))
        .map(|m| m.value.clone())
}

fn model_label(models: &[crate::model::ModelInfo], value: &str) -> String {
    if !model::explicit_model(value) {
        return "Выбрать модель".into();
    }
    let mut name = models
        .iter()
        .find(|m| m.value == value)
        .map(|m| m.display_name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| match value {
            "opus" => "Opus".into(),
            "sonnet" => "Sonnet".into(),
            "haiku" => "Haiku".into(),
            "fable" => "Fable".into(),
            other => other.into(),
        });
    if let Some(tail) = name.strip_prefix("claude-") {
        if let Some((family,version)) = tail.split_once('-') {
            let family = match family { "opus" => "Opus", "sonnet" => "Sonnet", "haiku" => "Haiku", "fable" => "Fable", other => other };
            name = format!("{family} {}",version.replace('-',"."));
        }
    }
    model::shorten(&name, 28)
}

fn composer_selectors(
    ui: &mut egui::Ui,
    chat: &mut Chat,
    tests: &mut String,
    models: &[crate::model::ModelInfo],
    backend: &str,
    api_model: Option<&mut String>,
    orchestra: Option<(bool, &mut bool)>,
    p: Palette,
) {
    let previous_style=ui.style().clone();
        chat.effort = model::selected_effort(&chat.effort).to_owned();
        let is_api = api_model.is_some();
        let selected_model = api_model.unwrap_or(&mut chat.model);
        let mut orchestra = orchestra;
        ui.spacing_mut().button_padding = Vec2::new(8., 5.);
        ui.spacing_mut().interact_size.y = 28.;
        ui.spacing_mut().icon_width = 10.;
        let v = ui.visuals_mut();
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.bg_stroke = Stroke::new(1.0_f32,p.line);
            w.corner_radius = egui::CornerRadius::same(8);
            w.expansion = 0.;
        }
        v.widgets.inactive.bg_fill = p.side;
        v.widgets.inactive.weak_bg_fill = p.side;
        v.widgets.hovered.weak_bg_fill = p.line;
        v.widgets.active.weak_bg_fill = p.line;
        v.widgets.open.weak_bg_fill = p.line;
        design::wrap_control(ui,&model_label(models,selected_model));
        egui::ComboBox::from_id_salt("model-selector")
            .selected_text(
                RichText::new(model_label(models, selected_model))
                .size(12.5)
                .color(p.muted),
            )
            .width(0.)
            .wrap_mode(egui::TextWrapMode::Truncate)
            .icon(design::combo_icon)
            .show_ui(ui, |ui| {
                if let Some((orch_on, open_orch)) = orchestra.as_mut() {
                    let orch_lbl = if *orch_on {
                        "✓ Оркестр активен · модели агентов…"
                    } else {
                        "⚡ Включить оркестр (модели агентов)…"
                    };
                    if ui
                        .button(RichText::new(orch_lbl).size(12.5).color(accent()))
                        .on_hover_text("Оркестратор сам создаст нужных агентов под этот чат; задай только их модели")
                        .clicked()
                    {
                        **open_orch = true;
                        ui.close_menu();
                    }
                    ui.separator();
                }
                if models.is_empty() && !is_api {
                    for (id, name) in [("sonnet", "Sonnet"), ("opus", "Opus"), ("haiku", "Haiku")] {
                        ui.selectable_value(selected_model, id.into(), name);
                    }
                }
                if models.is_empty() && is_api { ui.add(egui::TextEdit::singleline(selected_model).hint_text("Идентификатор модели API")); }
                for m in models.iter().filter(|m| model::explicit_model(&m.value)) {
                    ui.selectable_value(selected_model, m.value.clone(), model_label(models, &m.value))
                        .on_hover_text(&m.description);
                }
                if !is_api {
                    ui.separator();
                    ui.horizontal(|ui| {
                        label(ui, "Своя:", 11.5, p.muted);
                        ui.add(egui::TextEdit::singleline(selected_model).desired_width(150.).hint_text("id модели"));
                    });
                }
            });
        if let Some((orch_on, open_orch)) = orchestra {
            let orch_label = if orch_on { "Оркестр ✓" } else { "Оркестр" };
            design::wrap_control(ui, orch_label);
            let btn = egui::Button::new(
                RichText::new(orch_label)
                    .size(12.5)
                    .color(if orch_on { accent() } else { p.muted }),
            )
            .fill(if orch_on { accent().gamma_multiply(0.14) } else { p.side })
            .stroke(Stroke::new(
                1.0_f32,
                if orch_on { accent().gamma_multiply(0.55) } else { p.line },
            ))
            .corner_radius(egui::CornerRadius::same(8));
            if ui
                .add(btn)
                .on_hover_text(if orch_on {
                    "Настроить модели агентов оркестра в этом чате"
                } else {
                    "Включить оркестрацию прямо в этом чате и выбрать модели агентов"
                })
                .clicked()
            {
                *open_orch = true;
            }
        }
        design::wrap_control(ui,match chat.mode.as_str(){"auto"=>"Авто","acceptEdits"=>"Правки файлов","plan"=>"План",_=>"С подтверждением"});
        egui::ComboBox::from_id_salt("mode-selector")
            .selected_text(
                RichText::new(match chat.mode.as_str() {
                    "auto" => "Авто",
                    "acceptEdits" => "Правки файлов",
                    "plan" => "План",
                    _ => "С подтверждением",
                })
                .size(12.5)
                .color(if chat.mode == "auto" { accent() } else { p.muted }),
            )
            .width(0.)
            .wrap_mode(egui::TextWrapMode::Truncate)
            .icon(design::combo_icon)
            .show_ui(ui, |ui| {
                for (id, name) in [
                    ("auto", "Авто"),
                    ("manual", "С подтверждением"),
                    ("acceptEdits", "Правки файлов"),
                    ("plan", "Планирование"),
                ] {
                    ui.selectable_value(&mut chat.mode, id.into(), name);
                }
            });
        if backend != "agy" {
            let ultra = backend == "codex" && models.iter().any(|m| m.value == *selected_model && m.ultra);
            if chat.effort == "ultra" && !ultra && (backend != "codex" || models.iter().any(|m| m.value == *selected_model)) { chat.effort = "medium".into(); }
            design::wrap_control(ui,&format!("Глубина: {}", chat.effort));
            egui::ComboBox::from_id_salt("effort-selector")
                .selected_text(
                    RichText::new(format!(
                        "Глубина: {}",
                        chat.effort
                    ))
                    .size(12.5)
                    .color(p.muted),
                )
                .width(0.)
                .wrap_mode(egui::TextWrapMode::Truncate)
                .icon(design::combo_icon)
                .show_ui(ui, |ui| {
                    label(ui, "Глубина размышлений", 12., p.muted);
                    for (id, name) in [
                        ("low", "Низкая · Low"),
                        ("medium", "Средняя · Medium"),
                        ("high", "Высокая · High"),
                        ("xhigh", "Очень высокая · xHigh"),
                        ("max", "Максимальная · Max"),
                        ("ultra", "Ультра · Ultra"),
                    ].into_iter().filter(|(id, _)| *id != "ultra" || ultra) {
                        ui.selectable_value(&mut chat.effort, id.into(), name);
                    }
                })
                .response
                .on_hover_text("Применяется к следующему сообщению");
        }
        if is_api {ui.set_style(previous_style);return;}
        design::wrap_control(ui,&format!("Тесты: {}",match tests.as_str(){"auto"=>"авто","off"=>"выкл",_=>"спрашивать"}));
        egui::ComboBox::from_id_salt("tests-selector")
            .selected_text(
                RichText::new(format!(
                    "Тесты: {}",
                    match tests.as_str() {
                        "auto" => "авто",
                        "off" => "выкл",
                        _ => "спрашивать",
                    }
                ))
                .size(12.5)
                .color(p.muted),
            )
            .width(0.)
            .wrap_mode(egui::TextWrapMode::Truncate)
            .icon(design::combo_icon)
            .show_ui(ui, |ui| {
                label(ui, "Игровые тесты на этом ПК", 12., p.muted);
                for (id, name) in [
                    ("auto", "Авто — запускать сразу"),
                    ("ask", "Спрашивать перед запуском"),
                    ("off", "Отключены"),
                ] {
                    ui.selectable_value(tests, id.into(), name);
                }
            })
            .response
            .on_hover_text("Claude запускает скрытый Minecraft с модами и смотрит скриншоты. Окно не появляется и не забирает фокус.");
    ui.set_style(previous_style);
}
fn user_message_bubble(
    ui: &mut egui::Ui,
    text: &str,
    files: &[Attachment],
    p: Palette,
) -> egui::Rect {
    let row = ui.available_rect_before_wrap();
    let max = (row.width() * 0.86 - 28.).max(1.);
    let measured = ui
        .painter()
        .layout(text.into(), FontId::proportional(14.), p.text, max)
        .size()
        .x;
    let items: f32 = files.iter().map(|f| preview_size(f).x + 6.).sum::<f32>() - 6.;
    let width = measured.max(items).ceil().clamp(1., max);
    let target =
        egui::Rect::from_min_max(egui::pos2(row.right() - width - 28., row.top()), row.max);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(target)
            .layout(Layout::top_down(Align::Min)),
    );
    let bubble = egui::Frame::new()
        .fill(design::mix(p.surface, accent(), 0.065))
        .stroke(Stroke::new(1.0_f32, design::mix(p.line, accent(), 0.16)))
        .corner_radius(14.)
        .inner_margin(egui::Margin::symmetric(14, 9))
        .show(&mut child, |ui| {
            ui.set_width(width);
            if !text.is_empty() {
                ui.add(egui::Label::new(text).selectable(true).wrap());
            }
            if !files.is_empty() {
                ui.add_space(5.);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(6.);
                    for file in files {
                        match &file.texture {
                            Some(texture) => {
                                let size = preview_size(file);
                                ui.add(egui::Image::new(texture).fit_to_exact_size(size).corner_radius(10))
                                    .on_hover_text(format!("{} · {}", file.name, file_size(file.bytes)));
                            }
                            None => {
                                attachment_view(ui, file, p, false);
                            }
                        }
                    }
                });
            }
        })
        .response
        .rect;
    ui.advance_cursor_after_rect(egui::Rect::from_min_max(
        row.min,
        egui::pos2(row.right(), bubble.bottom()),
    ));
    bubble
}

/// Pictures show as pictures (up to 240×170), other files as a tile.
fn preview_size(file: &Attachment) -> Vec2 {
    match &file.texture {
        Some(texture) => {
            let size = texture.size_vec2().max(Vec2::splat(1.));
            size * (240. / size.x).min(170. / size.y).min(1.)
        }
        None => Vec2::new(216., 64.),
    }
}
fn file_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} МБ", bytes as f32 / 1048576.)
    } else if bytes == 0 {
        "не сохранено в истории".into()
    } else {
        format!("{} КБ", (bytes / 1024).max(1))
    }
}
fn attachment_view(ui: &mut egui::Ui, file: &Attachment, p: Palette, removable: bool) -> bool {
    // Allocate the whole tile before drawing; siblings never steal its text width.
    let width = 216_f32.min(ui.max_rect().width().max(120.));
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 64.), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 10., p.surface);
    painter.rect_stroke(
        rect,
        10.,
        Stroke::new(1_f32, p.line),
        egui::StrokeKind::Inside,
    );
    let thumb = egui::Rect::from_min_size(rect.min + Vec2::new(9., 9.), Vec2::new(46., 46.));
    painter.rect_filled(thumb, 6., p.base);
    if let Some(texture) = &file.texture {
        let size = texture.size_vec2();
        let factor = (42. / size.x).min(42. / size.y).min(1.);
        painter.image(
            texture.id(),
            egui::Rect::from_center_size(thumb.center(), size * factor),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
    } else {
        design::icon(&painter, thumb.shrink(11.), Icon::File, accent());
    }
    let left = rect.left() + 64.;
    let right = rect.right() - if removable { 30. } else { 9. };
    let text_painter = painter.with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(left, rect.top()),
        egui::pos2(right, rect.bottom()),
    ));
    let mut job = egui::text::LayoutJob::simple_singleline(
        file.name.clone(),
        FontId::proportional(12.),
        p.text,
    );
    job.wrap.max_width = (right - left).max(10.);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts(|f| f.layout_job(job));
    text_painter.galley(egui::pos2(left, rect.top() + 14.), galley, p.text);
    let size = if file.bytes >= 1024 * 1024 {
        format!("{:.1} МБ", file.bytes as f32 / 1048576.)
    } else {
        format!("{} КБ", (file.bytes / 1024).max(1))
    };
    text_painter.text(
        egui::pos2(left, rect.top() + 38.),
        egui::Align2::LEFT_TOP,
        size,
        FontId::proportional(10.5),
        p.muted,
    );
    response.clone().on_hover_text(&file.name);
    if removable {
        let close =
            egui::Rect::from_min_size(rect.right_top() + Vec2::new(-27., 5.), Vec2::splat(22.));
        let r = ui.interact(
            close,
            ui.id().with(("remove-attachment", response.id)),
            egui::Sense::click(),
        );
        if r.hovered() {
            painter.rect_filled(close, 6., p.line);
        }
        design::icon(&painter, close.shrink(4.), Icon::Close, p.muted);
        return r.on_hover_text("Убрать файл").clicked();
    }
    false
}

enum RowAction {
    Open,
    Delete,
    ManageLock,
    Relock,
    Pin(bool),
    StartRename,
    Rename(String),
    MoveToFolder,
}

fn session_menu(
    ui: &mut egui::Ui,
    session: &Session,
    _folders: &[crate::model::SessionFolder],
    connected: bool,
    blocked: Option<&str>,
    p: Palette,
) -> Option<RowAction> {
    let mut action = None;
    let locked_gate = session.is_locked_gate();
    if locked_gate && ui.button("Имя в списке…").on_hover_text("Метка на этом устройстве").clicked() {
        action = Some(RowAction::StartRename);
    }
    ui.add_enabled_ui(connected, |ui| {
        if locked_gate {
            if ui.button("Разблокировать сессию…").clicked() {
                action = Some(RowAction::Open);
            }
        } else {
            if !session.locked {
                if ui.button("Защитить паролем…").clicked() {
                    action = Some(RowAction::ManageLock);
                }
            } else {
                if ui.button("Заблокировать сейчас").clicked() {
                    action = Some(RowAction::Relock);
                }
                if ui.button("Сменить / снять пароль…").clicked() {
                    action = Some(RowAction::ManageLock);
                }
            }
            ui.separator();
            let pinned = session.pinned > 0.;
            if ui.button(if pinned { "Открепить" } else { "Закрепить сверху" }).clicked() {
                action = Some(RowAction::Pin(!pinned));
            }
            if ui.button("Переименовать").on_hover_text("Название сессии на сервере").clicked() {
                action = Some(RowAction::StartRename);
            }
            if ui.button("Скопировать команду для терминала")
                .on_hover_text("Чтобы продолжить эту сессию в CLI на сервере: вставь команду в SSH-терминал")
                .clicked()
            {
                ui.ctx().copy_text(resume_command_for(&session.cwd, &session.id, &session.native_id, &session.backend));
                ui.close_menu();
            }
            if ui
                .button(if session.folder.is_empty() {
                    "Переместить в папку…"
                } else {
                    "Сменить / убрать папку…"
                })
                .on_hover_text("Открыть окно выбора или создания папки для сессии")
                .clicked()
            {
                action = Some(RowAction::MoveToFolder);
                ui.close_menu();
            }
        }
    });
    ui.separator();
    let delete = ui.add_enabled(blocked.is_none(), egui::Button::new(RichText::new("Удалить сессию").color(design::RED)));
    if delete.on_hover_text(blocked.unwrap_or("Удалить историю этой сессии с сервера")).clicked() {
        action = Some(RowAction::Delete);
    }
    let _ = p;
    action
}

/// A sidebar section title; folders get a chevron and fold on click.
fn section_heading(ui: &mut egui::Ui, icon: Option<&str>, text: &str, count: usize, collapsed: Option<bool>, p: Palette, texture: Option<&egui::TextureHandle>) -> egui::Response {
    ui.add_space(10.);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.), egui::Sense::click());
    let painter = ui.painter();
    if collapsed.is_some() && response.hovered() {
        painter.rect_filled(rect, 7., design::mix(p.side, p.line, 0.5));
    }
    let mut x = rect.left() + 8.;
    if let Some(folded) = collapsed {
        let chevron = egui::Rect::from_center_size(egui::pos2(x + 5., rect.center().y), Vec2::splat(12.));
        let open = ui.ctx().animate_bool_with_time(response.id.with("open"), !folded, 0.15);
        let c = chevron.center();
        let turn = egui::emath::Rot2::from_angle(open * std::f32::consts::FRAC_PI_2);
        let points = [Vec2::new(-1.5, -3.5), Vec2::new(2., 0.), Vec2::new(-1.5, 3.5)].map(|v| c + turn * v).to_vec();
        painter.add(egui::Shape::line(points, Stroke::new(1.4_f32, p.muted)));
        x += 14.;
    }
    if let Some(texture) = texture {
        let thumb = egui::Rect::from_center_size(egui::pos2(x + 9., rect.center().y), Vec2::splat(18.));
        painter.image(texture.id(), thumb, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)), Color32::WHITE);
        x += 25.;
    } else if let Some(icon) = icon {
        painter.text(
            egui::pos2(x + 6., rect.center().y),
            egui::Align2::CENTER_CENTER,
            icon,
            FontId::proportional(if collapsed.is_some() { 13. } else { 11. }),
            if collapsed.is_some() { accent() } else { p.muted },
        );
        x += 19.;
    }
    let galley = painter.layout_no_wrap(
        if collapsed.is_some() { text.to_owned() } else { text.to_uppercase() },
        FontId::proportional(if collapsed.is_some() { 12.5 } else { 10.5 }),
        if collapsed.is_some() { p.text } else { p.muted },
    );
    let width = galley.size().x;
    painter.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.), galley, p.text);
    if count > 0 {
        painter.text(egui::pos2(x + width + 7., rect.center().y), egui::Align2::LEFT_CENTER, count.to_string(), FontId::proportional(11.), p.muted.gamma_multiply(0.8));
    }
    ui.add_space(2.);
    if collapsed.is_some() {
        response.on_hover_text("Щелчок — свернуть · двойной — переименовать · правый — меню")
    } else {
        response
    }
}

/// Inline name editor: Some(true) on Enter, Some(false) on Esc or focus loss.
fn folder_name_field(ui: &mut egui::Ui, text: &mut String, hint: &str, p: Palette) -> Option<bool> {
    let mut result = None;
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, accent().gamma_multiply(0.7)))
        .corner_radius(8.)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .frame(false)
                    .desired_width(f32::INFINITY)
                    .font(FontId::proportional(13.5))
                    .hint_text(RichText::new(hint).color(p.muted).size(12.)),
            );
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            if response.lost_focus() {
                result = Some(ui.input(|i| i.key_pressed(egui::Key::Enter)));
            }
        });
    result
}

/// Right-aligned caption above a user message bubble: `кто → кому · как`.
fn user_turn_caption(
    ui: &mut egui::Ui,
    attr: &crate::model::MessageAttribution,
    light: bool,
    p: Palette,
) {
    let bcol = design::backend_color(&attr.backend, light);
    let row_w = ui.available_width().max(40.);
    ui.allocate_ui_with_layout(
        Vec2::new(row_w, 16.),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 5.;
            if !attr.how.is_empty() {
                label(
                    ui,
                    format!("· {}", attr.how),
                    10.5,
                    p.muted.gamma_multiply(0.84),
                );
            }
            if !attr.to_whom.is_empty() {
                label(ui, &attr.to_whom, 10.9, bcol);
                let (mr, _) = ui.allocate_exact_size(Vec2::new(14., 12.), egui::Sense::hover());
                design::engine_icon(ui, mr, &attr.backend);
                label(ui, "→", 10.6, p.muted);
            }
            label(ui, &attr.who, 10.9, p.muted);
        },
    );
    ui.add_space(2.);
}

fn day_group(time: f64) -> &'static str {
    let now = chrono::Local::now();
    let midnight = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
        .map_or(now.timestamp(), |t| t.timestamp()) as f64;
    if time >= midnight {
        "Сегодня"
    } else if time >= midnight - 86400. {
        "Вчера"
    } else if time >= midnight - 6. * 86400. {
        "Эта неделя"
    } else if time >= midnight - 29. * 86400. {
        "Этот месяц"
    } else {
        "Ранее"
    }
}
fn folder_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or("/")
}
fn open_folder(path: &str) {
    let program = if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(path).spawn();
}
fn relative(time: f64) -> String {
    let now = chrono::Local::now().timestamp();
    let seconds = (now - time as i64).max(0);
    if seconds < 60 {
        "сейчас".into()
    } else if seconds < 3600 {
        format!("{} мин", seconds / 60)
    } else if seconds < 86400 {
        format!("{} ч", seconds / 3600)
    } else {
        format!("{} дн", seconds / 86400)
    }
}
fn tool_name(name: &str) -> &str {
    match name {
        "Read" => "Чтение файла",
        "Edit" | "MultiEdit" => "Изменение файла",
        "Write" => "Создание файла",
        "Bash" => "Команда на сервере",
        "mcp__relay_desktop__run_local_command" => "Команда на этом ПК",
        "mcp__relay_desktop__download_file" => "Скачивание на этот ПК",
        "Glob" | "Grep" => "Поиск по проекту",
        "WebSearch" | "WebFetch" => "Поиск в интернете",
        "ExitPlanMode" => "План готов",
        _ => name,
    }
}
fn reduce(c: &mut Chat, data: &Value) {
    let stamp = |m: &mut Message, c: &Chat, data: &Value| {
        let b = data["relay_backend"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&c.turn_backend);
        if m.backend.is_empty() && !b.is_empty() {
            m.backend = b.to_owned();
        }
        let mdl = data["relay_model"]
            .as_str()
            .or_else(|| data["message"]["model"].as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&c.turn_model);
        if m.model.is_empty() && !mdl.is_empty() {
            m.model = mdl.to_owned();
        }
        let md = data["relay_mode"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&c.turn_mode);
        if m.mode.is_empty() && !md.is_empty() {
            m.mode = md.to_owned();
        }
        let ef = data["relay_effort"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&c.turn_effort);
        if m.effort.is_empty() && !ef.is_empty() {
            m.effort = ef.to_owned();
        }
        let ac = data["relay_actor"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&c.turn_actor);
        if m.actor.is_empty() && !ac.is_empty() {
            m.actor = ac.to_owned();
        }
        let ag = data["relay_agent_name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&c.turn_agent_name);
        if m.agent_name.is_empty() && !ag.is_empty() {
            m.agent_name = ag.to_owned();
        }
    };
    match data["type"].as_str().unwrap_or("") {
        "stream_event" => {
            let e = &data["event"];
            let index = e["index"].as_u64().unwrap_or(0) as usize;
            match e["type"].as_str().unwrap_or("") {
                "message_start" => {
                    c.blocks.clear();
                }
                "content_block_start" => {
                    let block = &e["content_block"];
                    if block["type"] == "text" {
                        let at = c.messages.len();
                        let mut msg = Message {
                            role: "assistant".into(),
                            text: block["text"].as_str().unwrap_or("").into(),
                            ..Default::default()
                        };
                        stamp(&mut msg, c, data);
                        c.messages.push(msg);
                        c.blocks.insert(index, at);
                    } else if block["type"] == "thinking" {
                        c.status = "Думает над ответом".into();
                    }
                }
                "content_block_delta" => {
                    if let Some(text) = e["delta"]["text"].as_str() {
                        if let Some(at) = c.blocks.get(&index) {
                            c.messages[*at].text.push_str(text);
                            c.status = "Пишет ответ".into();
                        }
                    }
                }
                _ => {}
            }
        }
        "assistant" => {
            if let Some(blocks) = data["message"]["content"].as_array() {
                for b in blocks {
                    if b["type"] == "text" {
                        let text = b["text"].as_str().unwrap_or("");
                        // Content indices don't match stream indices (one block per event,
                        // thinking/tool blocks in between), so match the streamed text instead.
                        let streamed = c
                            .blocks
                            .iter()
                            .filter(|(_, at)| text.starts_with(c.messages[**at].text.as_str()))
                            .map(|(index, at)| (*index, *at))
                            .min();
                        if let Some((index, at)) = streamed {
                            c.blocks.remove(&index);
                            c.messages[at].text = text.into();
                            let mut clone_msg = c.messages[at].clone();
                            stamp(&mut clone_msg, c, data);
                            c.messages[at] = clone_msg;
                        } else if !text.is_empty() && text.trim() != "No response requested." {
                            let mut msg = Message {
                                role: "assistant".into(),
                                text: text.into(),
                                ..Default::default()
                            };
                            stamp(&mut msg, c, data);
                            c.messages.push(msg);
                        }
                    } else if b["type"] == "tool_use" {
                        let id = b["id"].as_str().unwrap_or("");
                        if !c
                            .messages
                            .iter()
                            .rev()
                            .take(20)
                            .any(|m| m.role == "tool" && m.tool_id == id)
                        {
                            let mut msg = Message {
                                role: "tool".into(),
                                tool: b["name"].as_str().unwrap_or("Действие").into(),
                                input: b["input"].clone(),
                                tool_id: id.into(),
                                ..Default::default()
                            };
                            stamp(&mut msg, c, data);
                            c.messages.push(msg);
                        }
                        c.status = tool_name(b["name"].as_str().unwrap_or("Работает")).into();
                    }
                }
            }
        }
        "user" => {
            if let Some(blocks) = data["message"]["content"].as_array() {
                for b in blocks {
                    if b["type"] == "tool_result" {
                        let text = if let Some(s) = b["content"].as_str() {
                            s.to_owned()
                        } else {
                            b["content"]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|b| b["text"].as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_default()
                        };
                        c.messages.push(Message {
                            role: "tool_result".into(),
                            text: model::shorten(&text, 24000),
                            failed: b["is_error"].as_bool().unwrap_or(false),
                            tool_id: b["tool_use_id"].as_str().unwrap_or("").into(),
                            ..Default::default()
                        });
                    }
                }
            }
        }
        "result" => {
            c.busy = false;
            c.permissions.clear();
            c.status.clear();
            if data["is_error"].as_bool().unwrap_or(false) {
                c.queue_paused = true;
                c.error = data["errors"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_else(|| {
                        data["result"]
                            .as_str()
                            .unwrap_or("Claude не смог завершить задачу.")
                            .into()
                    });
            } else if !c
                .messages
                .iter()
                .skip(c.user_at + 1)
                .any(|m| m.role == "assistant" && !m.text.is_empty())
            {
                if let Some(text) = data["result"].as_str() {
                    let mut msg = Message {
                        role: "assistant".into(),
                        text: text.into(),
                        ..Default::default()
                    };
                    stamp(&mut msg, c, data);
                    c.messages.push(msg);
                }
            }
        }
        _ => {}
    }
}
impl eframe::App for RelayApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(login)=self.login.take(){self.controller.send(&login.server,json!({"op":"auth_cancel","id":login.id}));}
        self.persist_pending(true);
        if !self.demo {
            for r in &mut self.cluster_store.runs {
                r.recover();
            }
            self.cluster_store.dirty = true;
            let _ = self.cluster_store.save();
        }
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.theme(ctx);
        self.poll();
        self.persist_pending(false);
        self.cluster_tick();
        self.auto_reconnect(ctx);
        self.drain_queues();
        if !self.demo {
            if let Some(server) = self.settings.selected.as_ref() {
                let is_claude = self
                    .settings
                    .servers
                    .iter()
                    .find(|s| &s.id == server)
                    .is_some_and(|s| {
                        self.selected_chat
                            .get(server)
                            .map(|sid| self.backend_for_chat(s, &key(server, sid)) == "claude")
                            .unwrap_or_else(|| s.effective_backend() == "claude")
                    });
                if let Some(remote) = self.remote.get_mut(server).filter(|r| r.connected) {
                    if is_claude && remote
                        .usage_requested
                        .is_none_or(|at| at.elapsed() >= Duration::from_secs(60))
                    {
                        remote.usage_requested = Some(Instant::now());
                        self.controller.send(server, json!({"op":"usage"}));
                    }
                    if remote.sessions_loaded
                        && remote
                            .sessions_polled
                            .is_none_or(|at| at.elapsed() >= Duration::from_secs(20))
                    {
                        if remote.sessions_polled.is_some() {
                            self.controller.send(server, json!({"op":"sessions"}));
                        }
                        remote.sessions_polled = Some(Instant::now());
                    }
                    ctx.request_repaint_after(Duration::from_secs(20));
                }
            }
        }
        if !self.demo {
            // Лимиты всех установленных агентов на каждом подключённом сервере: раз в 5 минут, как ручной /usage.
            let mut due = Vec::new();
            for (server, remote) in &self.remote {
                if !remote.connected {
                    continue;
                }
                for agent in ["claude", "codex", "agy"] {
                    if remote.usage_polled.get(agent).is_none_or(|at| at.elapsed() >= Duration::from_secs(300))
                        && self.backend_installed(server, agent)
                    {
                        due.push((server.clone(), agent));
                    }
                }
            }
            for (server, agent) in due {
                if let Some(remote) = self.remote.get_mut(&server) {
                    remote.usage_polled.insert(agent.to_owned(), Instant::now());
                }
                self.controller.send(&server, json!({"op":"usage","backend":agent}));
            }
            ctx.request_repaint_after(Duration::from_secs(60));
        }
        self.frames += 1;
        if !self.demo {
            for (server, remote) in &mut self.remote {
                if remote.connected
                    && !remote.sessions_loaded
                    && remote.sessions_retries < 3
                    && remote
                        .sessions_requested
                        .is_some_and(|at| at.elapsed() >= Duration::from_secs(12))
                {
                    remote.sessions_requested = Some(Instant::now());
                    remote.sessions_retries += 1;
                    remote.sessions_error = "Загрузка занимает больше времени. Повторяем…".into();
                    self.controller.send(server, json!({"op":"sessions"}));
                }
            }
            if self
                .remote
                .values()
                .any(|r| r.connected && !r.sessions_loaded)
            {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
        }
        if !self.demo
            && ctx.input(|i| i.viewport().close_requested())
            && self.chats.iter().any(|(k, c)| {
                // Hosted servers keep answering without the app; only unsent messages would be lost.
                let hosted = k.split_once('/').and_then(|(server, _)| self.remote.get(server)).is_some_and(|r| r.protocol >= 4);
                (c.busy && !hosted) || !c.queue.is_empty()
            })
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_confirm = true;
        }
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        if !dropped.is_empty() && !self.browser_visible() {
            if self.current_key().is_none() {
                self.new_chat();
            }
            if let (Some(s), Some(k)) = (self.server(), self.current_key()) {
                let sid = self.chats[&k].sid.clone();
                self.load_images(
                    s.id,
                    sid,
                    dropped.into_iter().filter_map(|f| f.path).collect(),
                );
            }
        }
        if self.editor.is_none()
            && !self.browser_visible()
            && !self.folder_open
            && ctx.input(|i| i.events.iter().any(|event|matches!(event,egui::Event::Key{key:egui::Key::V,pressed:true,modifiers,..} if modifiers.command||modifiers.ctrl)))
        {
            self.paste_image();
        }
        if !self.browser_visible() && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N)) {
            self.new_chat();
        }
        if !self.browser_expanded(){self.rail(ctx);}
        if self.browser_visible(){
            if !self.browser_expanded(){self.sidebar(ctx);}
            self.browser_panel(ctx);
        } else if self.cluster_open {
            self.cluster_panel(ctx);
        } else if self.board_open {
            self.board_panel(ctx);
        } else {
            self.sidebar(ctx);
            self.main_panel(ctx);
        }
        self.dialogs(ctx);
        self.settings_panel(ctx);
        self.cluster_settings(ctx);
        self.chat_orchestra_window(ctx);
        if self.chats.values().any(|c| c.busy) || self.remote.values().any(|r| r.connecting) {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if self.screenshot.is_some() {
            let delay = std::env::args()
                .collect::<Vec<_>>()
                .windows(2)
                .find(|a| a[0] == "--screenshot-delay-ms")
                .and_then(|a| a[1].parse::<f64>().ok())
                .unwrap_or(450.)
                / 1000.;
            if self.capture_at == 0.0 {
                self.capture_at = ctx.input(|i| i.time) + delay;
            }
            if !self.capture_requested && ctx.input(|i| i.time) >= self.capture_at {
                self.capture_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = shot {
                let path = self.screenshot.take().unwrap();
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                let _ = image::save_buffer(
                    path,
                    &bytes,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint();
            }
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn resume_command_enters_the_folder() {
        assert_eq!(resume_command("/root/relay-desktop", "0e33b7a5-123f"), "cd /root/relay-desktop && claude --resume 0e33b7a5-123f");
        assert_eq!(resume_command("/root/мой проект", "x"), "cd '/root/мой проект' && claude --resume x");
        assert_eq!(resume_command_for("/opt/app", "sid-1", "thr-codex", "codex"), "cd /opt/app && codex resume thr-codex");
        assert_eq!(resume_command_for("/opt/app", "sid-1", "conv-agy", "agy"), "cd /opt/app && agy --conversation conv-agy");
    }

    #[test]
    fn pending_store_round_trips() {
        let mut store = crate::model::PendingStore::default();
        store.chats.insert("srv/sid".into(), crate::model::PendingChat { draft: "черновик".into(), queue: vec!["потом проверь логи".into()] });
        let back: crate::model::PendingStore = serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
        assert!(back == store);
    }

    #[test]
    fn slash_popup_and_model_names() {
        let commands: Vec<crate::model::CommandInfo> = serde_json::from_value(json!([
            {"name":"compact","description":"","hint":""},{"name":"context"},{"name":"code-review"}
        ])).unwrap();
        let names = |d: &str| slash_matches(d, &commands).iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        assert_eq!(names("/co"), ["compact", "context", "code-review"]);
        assert_eq!(names("/review"), ["code-review"]);
        assert!(names("/compact").is_empty() && names("/compact now").is_empty() && names("compact").is_empty());
        let models: Vec<crate::model::ModelInfo> = serde_json::from_value(json!([
            {"value":"default","displayName":"Default"},{"value":"claude-fable-5-2","displayName":"Fable 5.2"},{"value":"opus","displayName":"Opus 5.5"}
        ])).unwrap();
        assert_eq!(resolve_model(&models, "fable").as_deref(), Some("claude-fable-5-2"));
        assert_eq!(resolve_model(&models, "Opus 5.5").as_deref(), Some("opus"));
        assert_eq!(resolve_model(&models, "gpt"), None);
        assert_eq!(model_label(&models, "claude-fable-5-2"), "Fable 5.2");
        assert_eq!(model_label(&[], "fable"), "Fable");
    }

    use super::*;
    #[test]
    fn composer_controls_stay_inside_a_wrapped_toolbar() {
        for width in [260.,420.,800.] {
            let ctx=egui::Context::default();
            let screen=egui::Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(width,600.));
            let mut chat=Chat::new("preview".into(),"/tmp".into(),"Preview".into());chat.mode="auto".into();
            let mut tests="ask".to_owned();let mut api=false;let mut configure=false;
            let output=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    ui.horizontal_wrapped(|ui|{
                        api_settings::source_picker(ui,&mut api,&mut configure,false,design::palette(false));
                        let mut open_orch = false;
                        composer_selectors(ui,&mut chat,&mut tests,&[],"claude",None,Some((false, &mut open_orch)),design::palette(false));
                    });
                });
            });
            let mut labels=0;
            for shape in output.shapes {
                if let egui::Shape::Text(text)=shape.shape {
                    if ["Claude Code","Выбрать модель","Авто","Глубина: medium","Тесты: спрашивать"].contains(&text.galley.job.text.as_str()) {
                        labels+=1;
                        assert!(screen.expand(0.5).contains_rect(egui::Rect::from_min_size(text.pos,text.galley.size())),"{} outside {width}px toolbar",text.galley.job.text);
                    }
                }
            }
            assert_eq!(labels,5);
        }
    }
    #[test]
    fn short_and_multiline_messages_share_the_right_edge() {
        for width in [320., 640., 940.] {
            let ctx = egui::Context::default();
            let mut edges = vec![];
            let _=ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,Vec2::new(width,900.))),..Default::default()},|ctx| {
                egui::CentralPanel::default().show(ctx,|ui| {
                    for text in ["привет","привет","Длинное сообщение с переносом на несколько строк, которое должно оставаться у того же правого края.","ок"] {
                        let r=user_message_bubble(ui,text,&[],design::palette(false));if text=="привет"{assert!(r.width()<100.,"short bubble was {}px",r.width());}
                        edges.push(r.right());ui.add_space(10.);
                    }
                });
            });
            for edge in &edges {
                assert!(
                    (edge - edges[0]).abs() < 1.,
                    "width={width}, edges={edges:?}"
                );
            }
        }
    }
    #[test]
    fn followups_keep_the_current_turn_and_attachment_order() {
        let mut c = Chat::new("sid".into(), "/tmp".into(), "test".into());
        c.busy = true;
        c.draft = "first".into();
        c.images.push(Attachment {
            name: "one.txt".into(),
            texture: None,
            bytes: 3,
            source: json!({"type":"file","data":"YWJj"}),
        });
        assert!(c.enqueue_draft());
        c.draft = "second".into();
        assert!(c.enqueue_draft());
        assert!(c.busy);
        assert!(c.draft.is_empty());
        assert!(c.images.is_empty());
        assert!(c.messages.is_empty());
        assert!(!c.queued_ready());
        reduce(&mut c, &json!({"type":"result","result":"done"}));
        assert!(c.queued_ready());
        let first = c.queue.pop_front().unwrap();
        assert_eq!(first.text, "first");
        assert_eq!(first.files[0].name, "one.txt");
        assert_eq!(c.queue[0].text, "second");
        c.queue_paused = true;
        assert!(!c.queued_ready());
        c.queue_paused = false;
        reduce(
            &mut c,
            &json!({"type":"result","is_error":true,"result":"failed"}),
        );
        assert!(!c.queued_ready());
    }

    #[test]
    fn failed_upload_restores_files_without_overwriting_a_new_draft() {
        for new_draft in ["", "Следующее сообщение"] {
            let mut c = Chat::new("sid".into(), "/tmp".into(), "test".into());
            c.messages.push(Message {
                role: "user".into(),
                text: "Отправь мод".into(),
                ..Default::default()
            });
            c.sent_images.insert(
                0,
                vec![Attachment {
                    name: "mod.jar".into(),
                    texture: None,
                    bytes: 32 * 1024 * 1024,
                    source: json!({"type":"file","path":"/tmp/mod.jar"}),
                }],
            );
            c.upload_progress = Some((1024, 32 * 1024 * 1024));
            c.busy = true;
            c.draft = new_draft.into();
            c.restore_failed_upload();
            assert!(!c.busy);
            assert!(c.messages.is_empty());
            assert!(c.queue_paused);
            if new_draft.is_empty() {
                assert_eq!(c.draft, "Отправь мод");
                assert_eq!(c.images[0].name, "mod.jar");
            } else {
                assert_eq!(c.draft, new_draft);
                assert_eq!(c.queue[0].text, "Отправь мод");
                assert_eq!(c.queue[0].files[0].name, "mod.jar");
            }
            c.restore_failed_upload();
            assert_eq!(c.queue.len(), usize::from(!new_draft.is_empty()));
        }
    }
    #[test]
    fn a_full_queue_keeps_the_unsent_draft() {
        let mut c = Chat::new("sid".into(), "/tmp".into(), "test".into());
        for _ in 0..12 {
            c.draft = "task".into();
            assert!(c.enqueue_draft());
        }
        c.draft = "keep this".into();
        assert!(!c.enqueue_draft());
        assert_eq!(c.draft, "keep this");
        assert_eq!(c.queue.len(), 12);
    }
    #[test]
    fn attachments_wrap_as_cards_instead_of_vertical_filenames() {
        for width in [260., 440., 720.] {
            let ctx = egui::Context::default();
            let file = Attachment {
                name: "very-long-file-name-".repeat(20),
                texture: None,
                bytes: 1234,
                source: Value::Null,
            };
            let mut used = 0.;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 600.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let top = ui.cursor().top();
                        ui.horizontal_wrapped(|ui| {
                            attachment_view(ui, &file, design::palette(false), false);
                            attachment_view(ui, &file, design::palette(false), false);
                        });
                        used = ui.cursor().top() - top;
                    });
                },
            );
            assert!(used < 150., "width={width}, tiles used {used}px");
        }
    }
    #[test]
    fn streamed_and_final_text_are_not_duplicated() {
        let mut c = Chat::new("x".into(), "/tmp".into(), "t".into());
        reduce(
            &mut c,
            &json!({"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}),
        );
        reduce(
            &mut c,
            &json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Привет"}}}),
        );
        reduce(
            &mut c,
            &json!({"type":"assistant","message":{"content":[{"type":"text","text":"Привет"}]}}),
        );
        assert_eq!(c.messages.len(), 1);
        assert_eq!(c.messages[0].text, "Привет");
    }
    #[test]
    fn text_after_thinking_is_not_duplicated() {
        // Claude Code sends each finished block as its own assistant event, so the
        // content array index differs from the stream index once thinking precedes text.
        let mut c = Chat::new("x".into(), "/tmp".into(), "t".into());
        c.messages.push(Message {
            role: "user".into(),
            text: "Сделай".into(),
            ..Default::default()
        });
        for event in [
            json!({"type":"message_start"}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Готово"}}),
        ] {
            reduce(&mut c, &json!({"type":"stream_event","event":event}));
        }
        reduce(
            &mut c,
            &json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"..."}]}}),
        );
        reduce(
            &mut c,
            &json!({"type":"assistant","message":{"content":[{"type":"text","text":"Готово"}]}}),
        );
        reduce(&mut c, &json!({"type":"result","result":"Готово"}));
        assert_eq!(c.messages.len(), 2);
        assert_eq!(c.messages[1].text, "Готово");
    }

    #[test]
    fn icon_only_buttons_center_their_glyphs_across_all_sizes() {
        for side in [22.0_f32, 24.0, 28.0, 32.0, 36.0] {
            for icon in [Icon::Download, Icon::Bolt, Icon::Settings, Icon::Refresh, Icon::Radar] {
                let ctx = egui::Context::default();
                let mut btn_rect = egui::Rect::NOTHING;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(300., 120.),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            btn_rect = design::button(
                                ui,
                                "",
                                Some(icon),
                                Kind::Secondary,
                                design::palette(false),
                                Vec2::splat(side),
                            )
                            .rect;
                        });
                    },
                );
                assert!((btn_rect.width() - side).abs() < 0.01);
                assert!((btn_rect.height() - side).abs() < 0.01);
                let mut glyph_bounds = egui::Rect::NOTHING;
                for clipped in output.shapes {
                    let b = clipped.shape.visual_bounding_rect();
                    if b.is_finite() && b.width() < side - 2.0 && b.height() < side - 2.0 {
                        glyph_bounds = glyph_bounds.union(b);
                    }
                }
                assert!(glyph_bounds.is_finite());
                assert!(
                    (glyph_bounds.center().x - btn_rect.center().x).abs() < 1.2,
                    "icon {icon:?} at {side}px off-center X: glyph={} btn={}",
                    glyph_bounds.center().x,
                    btn_rect.center().x
                );
                assert!(
                    (glyph_bounds.center().y - btn_rect.center().y).abs() < 1.2,
                    "icon {icon:?} at {side}px off-center Y: glyph={} btn={}",
                    glyph_bounds.center().y,
                    btn_rect.center().y
                );
            }
        }
    }
}
