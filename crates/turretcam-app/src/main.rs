//! TurretCAM desktop app: draw or import parts, nest them on a sheet, generate and check
//! turret punch programs, export the .CNC.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod canvas;
mod icons;
mod machine_view;
mod parts_view;
mod program_view;
mod sheet_view;
mod theme;
mod widgets;

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1480.0, 900.0]).with_min_inner_size([1100.0, 700.0]).with_title("TurretCAM"),
        ..Default::default()
    };
    eframe::run_native("TurretCAM", opts, Box::new(|cc| Ok(Box::new(app::App::new(cc, app::Startup::from_args())))))
}
