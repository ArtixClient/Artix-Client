#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod auth;
mod game;
mod mods;
mod net;
mod skin;
mod skin_ui;
mod store;
mod sys;
mod ui;

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([980.0, 620.0]).with_min_inner_size([860.0, 540.0]).with_title("Artix Launcher"),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Artix Launcher",
        opts,
        Box::new(|cc| {
            ui::style(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc.egui_ctx.clone())))
        }),
    )
}
