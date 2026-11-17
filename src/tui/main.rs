mod app;
mod conversation;
mod editor;
mod markdown;
mod render;

use anyhow::{Context, Result};
use app::App;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};
use std::{
    io::IsTerminal,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const HELP: &str = "Relay TUI — Claude Code и собственный API на ваших серверах

Использование: relay-tui [--connect ИМЯ] [--session UUID | --new] [--cwd ПАПКА]

  --list                Показать сохранённые серверы
  --connect ИМЯ         Подключиться к профилю по имени, ID или адресу
  --session UUID        Открыть существующую сессию выбранного сервера
  --new                 Начать новую сессию
  --cwd ПАПКА           Рабочая папка для новой сессии на сервере
  --check ИМЯ           Проверить SSH и список сессий без запроса к модели
  --demo                Посмотреть интерфейс без подключения и записи настроек
  --snapshot ФАЙЛ       Сохранить демонстрационный экран (текст + JSON ячеек)
  --width N --height N  Размер демонстрационного снимка (по умолчанию 120×40)
  --no-mouse            Сохранить обычное выделение/прокрутку терминала мышью
  -V, --version         Версия
  -h, --help            Справка

F2 — серверы; Ctrl+A в списке — добавить сервер; F3 — сессии; F1 — справка.
Enter — отправить, Alt+Enter / Ctrl+J — строка, Ctrl+C — остановить ответ.
Профили и история общие с Relay. TUI-черновики хранятся отдельно в tui-state.json.
";

#[derive(Default)]
struct Args {
    demo: bool,
    list: bool,
    connect: Option<String>,
    session: Option<String>,
    new: bool,
    cwd: Option<String>,
    check: Option<String>,
    snapshot: Option<PathBuf>,
    width: u16,
    height: u16,
    no_mouse: bool,
}
fn parse() -> Result<Args> {
    let mut out = Args {
        width: 120,
        height: 40,
        ..Default::default()
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().context(format!("После {arg} нужно значение"));
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("Relay TUI {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--demo" => out.demo = true,
            "--list" => out.list = true,
            "--connect" => out.connect = Some(value()?),
            "--session" => out.session = Some(value()?),
            "--new" => out.new = true,
            "--cwd" => out.cwd = Some(value()?),
            "--check" => out.check = Some(value()?),
            "--snapshot" => out.snapshot = Some(value()?.into()),
            "--width" => out.width = value()?.parse().context("Ширина должна быть числом")?,
            "--height" => out.height = value()?.parse().context("Высота должна быть числом")?,
            "--no-mouse" => out.no_mouse = true,
            _ => anyhow::bail!("Неизвестный аргумент: {arg}. --help — справка"),
        }
    }
    anyhow::ensure!(
        !out.new || out.session.is_none(),
        "--new и --session нельзя использовать вместе"
    );
    anyhow::ensure!(
        out.cwd.is_none() || out.session.is_none(),
        "--cwd меняет папку новой сессии; не используй вместе с --session"
    );
    anyhow::ensure!(
        (16..=400).contains(&out.width) && (6..=200).contains(&out.height),
        "Размер снимка: ширина 16–400, высота 6–200"
    );
    Ok(out)
}
fn resolve(app: &App, name: &str) -> Result<String> {
    let profiles = app.servers();
    let matches: Vec<_> = profiles
        .iter()
        .filter(|s| s.id == name || s.name.eq_ignore_ascii_case(name) || s.host == name)
        .collect();
    anyhow::ensure!(
        matches.len() == 1,
        "Профиль не найден или имя неоднозначно. relay-tui --list покажет серверы."
    );
    Ok(matches[0].id.clone())
}

fn check(mut app: App, name: &str) -> Result<()> {
    app.read_only = true;
    let id = resolve(&app, name)?;
    app.active = Some(id.clone());
    let server = app.server().unwrap();
    anyhow::ensure!(
        server.fingerprint.is_some()
            || app.store.trusted.contains_key(&format!(
                "{}@{}:{}#{}",
                server.user, server.host, server.port, server.id
            )),
        "Сначала подтверди SSH-ключ в интерактивном Relay"
    );
    app.connect();
    let until = Instant::now() + Duration::from_secs(75);
    loop {
        app.tick();
        if let Some(remote) = app.remote.get(&id) {
            if remote.connected && remote.loaded {
                println!(
                    "{}",
                    serde_json::json!({"ok":true,"version":env!("CARGO_PKG_VERSION"),"server":server.name,"sessions":remote.sessions.len(),"models":remote.models.len(),"window_system":false})
                );
                app.controller.disconnect(&id);
                return Ok(());
            }
            anyhow::ensure!(remote.error.is_empty(), "{}", remote.error);
        }
        anyhow::ensure!(
            !matches!(app.popup, Some(app::Popup::Password { .. })),
            "Пароль отсутствует в системном хранилище. Подключись интерактивно."
        );
        anyhow::ensure!(Instant::now() < until, "Сервер не ответил за 75 секунд");
        std::thread::sleep(Duration::from_millis(25));
    }
}

struct InputGuard {
    mouse: bool,
}
impl Drop for InputGuard {
    fn drop(&mut self) {
        restore_input(self.mouse);
    }
}
fn restore_input(mouse: bool) {
    let mut out = std::io::stdout();
    let _ = execute!(out, DisableBracketedPaste, PopKeyboardEnhancementFlags);
    if mouse {
        let _ = execute!(out, DisableMouseCapture);
    }
}

fn run() -> Result<()> {
    let args = parse()?;
    let mut app = App::new(args.demo || args.snapshot.is_some())?;
    if args.list {
        for server in app.servers() {
            println!(
                "{}\t{}\t{}@{}:{}",
                server.id,
                markdown::clean(&server.name),
                markdown::clean(&server.user),
                markdown::clean(&server.host),
                server.port
            );
        }
        return Ok(());
    }
    if let Some(name) = args.check {
        return check(app, &name);
    }
    if app.demo {
        app.demo();
    }
    if let Some(name) = args.connect {
        app.active = Some(resolve(&app, &name)?);
    }
    if let Some(cwd) = args.cwd {
        if let Some(host) = app.active.clone() {
            app.store.cwd.insert(host, cwd);
        }
        app.new_chat();
    }
    if let Some(sid) = args.session {
        uuid::Uuid::parse_str(&sid).context("Укажи UUID сессии")?;
        let host = app
            .active
            .clone()
            .context("Сначала выбери сервер через --connect")?;
        app.selected.insert(host.clone(), sid.clone());
        app.store.last.insert(host, sid);
    } else if args.new {
        app.new_chat();
    }
    if let Some(path) = args.snapshot {
        let buffer = render::snapshot(&mut app, args.width, args.height)?;
        let mut lines = vec![];
        let mut cells = vec![];
        for y in 0..args.height {
            let mut line = String::new();
            let mut row = vec![];
            for x in 0..args.width {
                let cell = &buffer[(x, y)];
                line.push_str(cell.symbol());
                row.push(serde_json::json!({"text":cell.symbol(),"fg":format!("{:?}",cell.fg),"bg":format!("{:?}",cell.bg),"bold":cell.modifier.contains(ratatui::style::Modifier::BOLD)}));
            }
            lines.push(line);
            cells.push(row);
        }
        std::fs::write(&path, lines.join("\n"))?;
        std::fs::write(
            path.with_extension("json"),
            serde_json::to_vec(
                &serde_json::json!({"width":args.width,"height":args.height,"cells":cells}),
            )?,
        )?;
        return Ok(());
    }
    anyhow::ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "Relay TUI нужен интерактивный терминал. --help — справка; --list — серверы."
    );
    let shutdown = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    for signal in [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(signal, shutdown.clone())?;
    }
    if !app.demo {
        if let Some(host) = app.active.clone() {
            app.select_host(host);
        } else {
            app.picker(app::Picker::Hosts);
        }
    }
    ratatui::run(|terminal| -> Result<()> {
        let _guard = InputGuard {
            mouse: !args.no_mouse,
        };
        execute!(
            std::io::stdout(),
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        if !args.no_mouse {
            execute!(std::io::stdout(), EnableMouseCapture)?;
        }
        let old_hook = std::panic::take_hook();
        let mouse = !args.no_mouse;
        std::panic::set_hook(Box::new(move |info| {
            restore_input(mouse);
            old_hook(info);
        }));
        let mut view = render::View::default();
        let mut redraw = true;
        let mut drawn = Instant::now();
        while !app.quit && !shutdown.load(Ordering::Relaxed) {
            redraw |= app.tick();
            let busy = app.chat().is_some_and(|c| c.busy);
            if redraw || busy && drawn.elapsed() >= Duration::from_millis(120) {
                terminal.draw(|frame| render::draw(frame, &mut app, &mut view))?;
                redraw = false;
                drawn = Instant::now();
            }
            if event::poll(Duration::from_millis(50))? {
                app.input(event::read()?);
                redraw = true;
            }
        }
        app.persist()?;
        Ok(())
    })?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("Relay TUI: {}", markdown::clean(&format!("{error:#}")));
        std::process::exit(1);
    }
}
