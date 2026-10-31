#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod artifacts;
mod board;
mod connection;
mod desktop;
mod gametest;
mod images;
mod limits;
mod model;
mod storage;
mod timeline;
mod ui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--game-test") {
        // Debug entry: relay --game-test <scenario> <mods dir> <out dir>; writes <out>/result.json.
        let arg = |n: usize| args.get(i + n).cloned().unwrap_or_default();
        let job = gametest::Job {
            version: "1.21.8".into(),
            loader: String::new(),
            scenario: std::fs::read_to_string(arg(1)).unwrap_or_default(),
            timeout: std::time::Duration::from_secs(300),
            mods: arg(2).into(),
            out: arg(3).into(),
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let progress_file = job.out.join("progress.txt");
        let _ = std::fs::create_dir_all(&job.out);
        let result = gametest::run(&job, &cancel, &|m| {
            let _ = std::fs::write(&progress_file, m);
        })
        .unwrap_or_else(|e| serde_json::json!({"ok":false,"message":format!("{e:#}")}));
        let _ = std::fs::write(job.out.join("result.json"), result.to_string());
        return Ok(());
    }
    if std::env::args().any(|a| a == "--probe") {
        return connection::probe().map_err(|e| eframe::Error::AppCreation(e.into()));
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Relay")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([640.0, 460.0]),
        vsync: true,
        ..Default::default()
    };
    eframe::run_native(
        "Relay",
        options,
        Box::new(|cc| Ok(Box::new(app::RelayApp::new(cc)))),
    )
}
