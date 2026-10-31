#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod artifacts;
mod connection;
mod desktop;
mod images;
mod limits;
mod model;
mod storage;
mod timeline;
mod ui;

fn main() -> eframe::Result {
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
