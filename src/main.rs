//! Dithering Tool — desktop image processing app.
//!
//! Entry point. Sets up the native window via [`eframe`] and launches
//! [`app::DitheringApp`].

mod app;
mod dither;
mod effects;
mod halftone;
mod palette;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("Dithering Tool")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "Dithering Tool",
        native_options,
        Box::new(|cc| Ok(Box::new(app::DitheringApp::new(cc)))),
    )
}
