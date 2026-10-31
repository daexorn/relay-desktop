#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod connection;
mod desktop;
mod images;
mod limits;
mod model;
mod storage;
mod ui;

fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--probe") {
        return connection::probe().map_err(|e| eframe::Error::AppCreation(e.into()));
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Relay")
            .with_inner_size([1260.0, 840.0])
            .with_min_inner_size([860.0, 580.0]),
        vsync: true,
        ..Default::default()
    };
    eframe::run_native(
        "Relay",
        options,
        Box::new(|cc| Ok(Box::new(app::RelayApp::new(cc)))),
    )
}
