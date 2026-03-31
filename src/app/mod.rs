//! Main application state and UI.
//!
//! [`DitheringApp`] implements [`eframe::App`] and owns all persistent state:
//! the loaded image, effects stack, dither/halftone parameters, palette
//! selection, and the background render channel.
//!
//! # Render loop
//!
//! Image processing is intentionally kept off the UI thread to prevent
//! freezing. When parameters change, a dirty flag is set (`pre_dirty` or
//! `dither_dirty`). At the start of each [`eframe::App::update`] call:
//!
//! 1. [`DitheringApp::poll_render`] checks the `mpsc` channel for a completed
//!    [`RenderOutput`] and, if present, updates the displayed textures.
//! 2. If dirty and no render is in flight, [`DitheringApp::kick_render`]
//!    snapshots the current parameters and spawns a background thread.
//! 3. The thread sends its result back through the channel and calls
//!    [`egui::Context::request_repaint`] to wake the UI.

mod ui;

use std::path::PathBuf;
use std::sync::mpsc;

use egui::{ColorImage, Context, RichText, TextureHandle, TextureOptions};
use image::DynamicImage;
use serde::{Deserialize, Serialize};

use crate::dither::DitherParams;
use crate::effects::{Effect, EffectKind, EFFECT_NAMES};
use crate::halftone::HalftoneParams;
use crate::palette::{Palette, PaletteKind};

// ── Background render output ──────────────────────────────────────────────────

/// Images produced by a background render pass.
struct RenderOutput {
    preprocessed: DynamicImage,
    dithered: DynamicImage,
}

/// Snapshot of all creative state — used for undo/redo and preset serialization.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct AppSnapshot {
    mode: AppMode,
    effects: Vec<Effect>,
    dither_params: DitherParams,
    halftone_params: HalftoneParams,
    palette_kind: PaletteKind,
    user_palette: Vec<[f32; 3]>,
    input_max_dim: u32,
}

// ── View mode ─────────────────────────────────────────────────────────────────

/// How the canvas area displays the before/after images.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    /// Original and output shown side-by-side.
    Split,
    /// Single image with a draggable divider: left = original, right = output.
    Compare,
    /// Output image only.
    Output,
}

// ── App mode ──────────────────────────────────────────────────────────────────

/// Top-level processing mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AppMode {
    Dither,
    Halftone,
}

// ── App state ─────────────────────────────────────────────────────────────────

/// Root application state. One instance is created at startup and lives for
/// the entire session.
pub struct DitheringApp {
    // ── Images ────────────────────────────────────────────────────────────────
    /// The raw image as loaded from disk (or dropped in). Never modified.
    original: Option<DynamicImage>,
    /// `original` after the effects stack has been applied; the input to dither/halftone.
    preprocessed: Option<DynamicImage>,
    /// Final output after dithering or halftone rendering.
    dithered: Option<DynamicImage>,

    // ── GPU textures (uploaded lazily) ────────────────────────────────────────
    original_tex: Option<TextureHandle>,
    dithered_tex: Option<TextureHandle>,

    // ── Modes ─────────────────────────────────────────────────────────────────
    mode: AppMode,
    view_mode: ViewMode,

    // ── Effects stack ─────────────────────────────────────────────────────────
    effects: Vec<Effect>,
    /// Index into `EFFECT_NAMES` for the "Add effect" combo box.
    add_effect_choice: usize,

    // ── Dither params ─────────────────────────────────────────────────────────
    dither_params: DitherParams,
    palette_kind: PaletteKind,
    /// Colors backing both `Extracted` (filled by median-cut) and `Custom`
    /// (edited by the user) palette kinds.
    user_palette: Vec<[f32; 3]>,
    /// Target color count for median-cut extraction.
    extract_num_colors: usize,

    // ── Halftone params ───────────────────────────────────────────────────────
    halftone_params: HalftoneParams,

    // ── Input downscaling ─────────────────────────────────────────────────────
    /// Maximum dimension (width or height) to cap the input image before
    /// processing. 0 means use the full original resolution.
    input_max_dim: u32,

    // ── Dirty flags ───────────────────────────────────────────────────────────
    /// Set when the effects stack changes; causes the pre-process step to re-run.
    pre_dirty: bool,
    /// Set when dither/halftone params change; causes only the final step to re-run.
    dither_dirty: bool,

    // ── Background render ─────────────────────────────────────────────────────
    /// Receiver half of the channel used to receive results from the background
    /// render thread. `None` when no render is in progress.
    render_rx: Option<mpsc::Receiver<RenderOutput>>,

    // ── View state ────────────────────────────────────────────────────────────
    zoom: f32,
    /// Horizontal position of the compare-mode divider, normalised 0.0–1.0.
    compare_pos: f32,
    /// Nearest-neighbour upscale factor applied when saving (1, 2, 4, or 8).
    export_scale: u32,

    /// Text shown in the status bar.
    status: String,

    // ── Undo / redo ───────────────────────────────────────────────────────────────
    undo_stack: std::collections::VecDeque<AppSnapshot>,
    redo_stack: Vec<AppSnapshot>,
    /// Snapshot captured at the start of a pointer-drag gesture for grouping continuous edits.
    undo_pending: Option<AppSnapshot>,
}

impl Default for DitheringApp {
    fn default() -> Self {
        Self {
            original: None,
            preprocessed: None,
            dithered: None,
            original_tex: None,
            dithered_tex: None,
            mode: AppMode::Dither,
            view_mode: ViewMode::Compare,
            effects: Vec::new(),
            add_effect_choice: 0,
            dither_params: DitherParams::default(),
            palette_kind: PaletteKind::GrayRamp2,
            user_palette: Vec::new(),
            extract_num_colors: 8,
            halftone_params: HalftoneParams::default(),
            input_max_dim: 0,
            pre_dirty: false,
            dither_dirty: false,
            render_rx: None,
            zoom: 1.0,
            compare_pos: 0.5,
            export_scale: 1,
            status: "Drop an image or use \"Open Image\"".to_string(),
            undo_stack: std::collections::VecDeque::new(),
            redo_stack: Vec::new(),
            undo_pending: None,
        }
    }
}

impl DitheringApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    // ── I/O ───────────────────────────────────────────────────────────────────

    fn load_image(&mut self, path: PathBuf, ctx: &Context) {
        match image::open(&path) {
            Ok(img) => {
                self.status = format!(
                    "{}  ({}×{})",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    img.width(),
                    img.height(),
                );
                self.original_tex = Some(dynamic_to_texture(ctx, "original", &img));
                self.original = Some(img);
                self.pre_dirty = true;
                self.dither_dirty = true;
            }
            Err(e) => self.status = format!("Error: {e}"),
        }
    }

    fn save_image(&mut self, format: SaveFormat) {
        // Compute the export image first, releasing the borrow of self before
        // the status update (NLL ends the borrow when `export` is fully built).
        let export = match self.dithered.as_ref() {
            None => return,
            Some(img) => {
                if self.export_scale > 1 {
                    img.resize_exact(
                        img.width() * self.export_scale,
                        img.height() * self.export_scale,
                        image::imageops::FilterType::Nearest,
                    )
                } else {
                    img.clone()
                }
            }
        };

        let Some(path) = rfd::FileDialog::new()
            .add_filter(format.filter_name(), format.extensions())
            .set_file_name(format.default_filename())
            .save_file()
        else {
            return;
        };

        let result = match format {
            SaveFormat::Gif => save_gif(&export, &path),
            _ => export.save(&path).map_err(|e| e.to_string()),
        };
        match result {
            Ok(()) => self.status = format!("Saved: {}", path.display()),
            Err(e) => self.status = format!("Save error: {e}"),
        }
    }

    fn copy_to_clipboard(&self) {
        let Some(ref img) = self.dithered else { return };
        use arboard::ImageData;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let data = ImageData {
            width: w as usize,
            height: h as usize,
            bytes: rgba.as_raw().to_vec().into(),
        };
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_image(data);
        }
    }

    // ── Background render ─────────────────────────────────────────────────────

    /// Return `original` downscaled to `input_max_dim` if set, otherwise the
    /// full image. Returns `None` if no image has been loaded.
    fn scaled_original(&self) -> Option<DynamicImage> {
        let img = self.original.as_ref()?;
        if self.input_max_dim == 0
            || (img.width() <= self.input_max_dim && img.height() <= self.input_max_dim)
        {
            return Some(img.clone());
        }
        Some(img.resize(
            self.input_max_dim,
            self.input_max_dim,
            image::imageops::FilterType::Lanczos3,
        ))
    }

    /// Snapshot the current parameters and spawn a background render thread.
    /// Clears both dirty flags immediately so that further changes during the
    /// render set them again and trigger a second pass on completion.
    fn kick_render(&mut self, ctx: &Context) {
        let Some(src) = self.scaled_original() else { return };

        let need_pre = self.pre_dirty;
        let pre_base = self.preprocessed.clone();
        let effects = self.effects.clone();
        let mode = self.mode;               // Copy
        let dither_params = self.dither_params;   // Copy
        let halftone_params = self.halftone_params; // Copy
        let palette = self.resolved_palette();

        self.pre_dirty = false;
        self.dither_dirty = false;

        let (tx, rx) = mpsc::channel();
        self.render_rx = Some(rx); // previous in-flight receiver dropped here

        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let pre = if need_pre {
                crate::effects::apply_effects(&src, &effects)
            } else {
                pre_base.unwrap_or_else(|| src.clone())
            };

            let dithered = match mode {
                AppMode::Halftone => crate::halftone::apply_halftone(&pre, &halftone_params),
                AppMode::Dither => crate::dither::apply_dither(&pre, &dither_params, &palette),
            };

            let _ = tx.send(RenderOutput { preprocessed: pre, dithered });
            ctx2.request_repaint();
        });
    }

    /// Check the background channel for a completed render result. If one is
    /// ready, update the stored images and GPU textures. If dirty flags were
    /// set during the render, kick another pass immediately.
    fn poll_render(&mut self, ctx: &Context) {
        // Use a block so the shared borrow of `self.render_rx` ends before the
        // later mutable assignments (NLL ensures this even without the block,
        // but the block makes the intent explicit).
        let output = {
            let Some(ref rx) = self.render_rx else { return };
            match rx.try_recv() {
                Ok(o) => o,
                Err(_) => return,
            }
        };
        self.render_rx = None;
        self.dithered_tex = Some(dynamic_to_texture(ctx, "dithered", &output.dithered));
        self.preprocessed = Some(output.preprocessed);
        self.dithered = Some(output.dithered);
        // Re-render if settings changed while we were busy.
        if (self.pre_dirty || self.dither_dirty) && self.original.is_some() {
            self.kick_render(ctx);
        }
    }

    /// Return the active [`Palette`], resolving `Extracted`/`Custom` from
    /// `user_palette` and falling back to the kind's built-in definition.
    fn resolved_palette(&self) -> Palette {
        match self.palette_kind {
            PaletteKind::Extracted | PaletteKind::Custom if !self.user_palette.is_empty() => {
                Palette { name: "User", colors: self.user_palette.clone() }
            }
            _ => self.palette_kind.build(),
        }
    }

    fn make_snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            mode: self.mode,
            effects: self.effects.clone(),
            dither_params: self.dither_params,
            halftone_params: self.halftone_params,
            palette_kind: self.palette_kind,
            user_palette: self.user_palette.clone(),
            input_max_dim: self.input_max_dim,
        }
    }

    fn apply_snapshot(&mut self, snap: AppSnapshot) {
        self.mode = snap.mode;
        self.effects = snap.effects;
        self.dither_params = snap.dither_params;
        self.halftone_params = snap.halftone_params;
        self.palette_kind = snap.palette_kind;
        self.user_palette = snap.user_palette;
        self.input_max_dim = snap.input_max_dim;
        self.pre_dirty = true;
        self.dither_dirty = true;
    }

    fn push_undo(&mut self, snap: AppSnapshot) {
        const MAX_UNDO: usize = 50;
        self.undo_stack.push_back(snap);
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.pop_front();
        }
        self.redo_stack.clear();
    }

    fn do_undo(&mut self) {
        if let Some(snap) = self.undo_stack.pop_back() {
            let current = self.make_snapshot();
            self.redo_stack.push(current);
            self.apply_snapshot(snap);
            self.status = format!("Undo ({} left)", self.undo_stack.len());
        }
    }

    fn do_redo(&mut self) {
        if let Some(snap) = self.redo_stack.pop() {
            let current = self.make_snapshot();
            self.undo_stack.push_back(current);
            self.apply_snapshot(snap);
            self.status = format!("Redo ({} left)", self.redo_stack.len());
        }
    }
}

// ── eframe::App ───────────────────────────────────────────────────────────────

impl eframe::App for DitheringApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Drag-and-drop file loading.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.load_image(path, ctx);
        }

        self.poll_render(ctx);

        // Undo: Cmd+Z (Mac) / Ctrl+Z (Windows/Linux)
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z) && !i.modifiers.shift) {
            self.do_undo();
        }
        // Redo: Cmd+Shift+Z or Cmd+Y
        if ctx.input(|i| {
            i.modifiers.command
                && (i.key_pressed(egui::Key::Y) || (i.key_pressed(egui::Key::Z) && i.modifiers.shift))
        }) {
            self.do_redo();
        }

        // Rebuild the "before" texture whenever the input scale changes so the
        // compare view stays in sync with the processing resolution.
        if self.pre_dirty {
            if let Some(scaled) = self.scaled_original() {
                self.original_tex = Some(dynamic_to_texture(ctx, "original", &scaled));
            }
        }

        if (self.pre_dirty || self.dither_dirty) && self.original.is_some() && self.render_rx.is_none() {
            self.kick_render(ctx);
        }

        // ── Top bar ───────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Dithering Tool");
                ui.separator();

                if ui.button("Open Image").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tiff", "webp"])
                        .pick_file()
                    {
                        self.load_image(path, ctx);
                    }
                }

                ui.separator();

                let has_output = self.dithered.is_some();
                ui.menu_button("Save ▾", |ui| {
                    for (label, fmt) in [
                        ("PNG", SaveFormat::Png),
                        ("GIF", SaveFormat::Gif),
                        ("JPEG", SaveFormat::Jpeg),
                        ("BMP", SaveFormat::Bmp),
                    ] {
                        if ui.add_enabled(has_output, egui::Button::new(label)).clicked() {
                            self.save_image(fmt);
                            ui.close_menu();
                        }
                    }
                });

                if ui.add_enabled(has_output, egui::Button::new("Copy")).clicked() {
                    self.copy_to_clipboard();
                }

                ui.separator();

                // Undo / Redo buttons
                if ui.add_enabled(!self.undo_stack.is_empty(), egui::Button::new("↩ Undo")).on_hover_text("Cmd+Z").clicked() {
                    self.do_undo();
                }
                if ui.add_enabled(!self.redo_stack.is_empty(), egui::Button::new("↪ Redo")).on_hover_text("Cmd+Shift+Z").clicked() {
                    self.do_redo();
                }

                ui.separator();

                // Preset save/load
                if ui.button("Save Preset").clicked() {
                    let snap = self.make_snapshot();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Preset", &["json"])
                        .set_file_name("preset.json")
                        .save_file()
                    {
                        match serde_json::to_string_pretty(&snap) {
                            Ok(json) => match std::fs::write(&path, json) {
                                Ok(()) => self.status = format!("Preset saved: {}", path.display()),
                                Err(e) => self.status = format!("Save error: {e}"),
                            },
                            Err(e) => self.status = format!("Serialize error: {e}"),
                        }
                    }
                }
                if ui.button("Load Preset").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Preset", &["json"])
                        .pick_file()
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(json) => match serde_json::from_str::<AppSnapshot>(&json) {
                                Ok(snap) => {
                                    self.push_undo(self.make_snapshot());
                                    self.apply_snapshot(snap);
                                    self.status = format!("Preset loaded: {}", path.display());
                                }
                                Err(e) => self.status = format!("Invalid preset: {e}"),
                            },
                            Err(e) => self.status = format!("Read error: {e}"),
                        }
                    }
                }

                ui.separator();
                ui.label(RichText::new(&self.status).weak());
                if self.render_rx.is_some() {
                    ui.label(RichText::new("rendering…").weak().italics());
                }
            });
        });

        // ── Left panel ────────────────────────────────────────────────────────
        egui::SidePanel::left("controls")
            .resizable(true)
            .default_width(240.0)
            .min_width(200.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(4.0);
                    self.ui_controls(ui);
                    ui.add_space(8.0);
                });
            });

        // ── Canvas ────────────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_canvas(ctx, ui);
        });
    }
}

// ── Controls (implemented in ui.rs) ──────────────────────────────────────────

impl DitheringApp {
    fn ui_controls(&mut self, ui: &mut egui::Ui) {
        // ── Undo tracking ─────────────────────────────────────────────────────────────
        let pointer_down = ui.input(|i| i.pointer.any_down());
        let pre_undo = self.make_snapshot();
        if pointer_down && self.undo_pending.is_none() {
            self.undo_pending = Some(pre_undo.clone());
        }

        // Snapshot Copy params before rendering — no allocations needed.
        let effects_snap: Vec<Effect> = self.effects.clone();
        let dither_snap = self.dither_params;
        let halftone_snap = self.halftone_params;
        let mode_snap = self.mode;
        let palette_snap = self.palette_kind;
        let input_snap = self.input_max_dim;

        // Mode toggle
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, AppMode::Dither, "Dither");
            ui.selectable_value(&mut self.mode, AppMode::Halftone, "Halftone");
        });
        ui.separator();
        ui.add_space(4.0);

        self.ui_effects_stack(ui);
        ui.add_space(4.0);

        match self.mode {
            AppMode::Dither => self.ui_dither_controls(ui),
            AppMode::Halftone => self.ui_halftone_controls(ui),
        }
        ui.add_space(4.0);
        self.ui_view_controls(ui);

        // ── Dirty propagation ─────────────────────────────────────────────────
        if self.effects != effects_snap {
            self.pre_dirty = true;
            self.dither_dirty = true;
        }
        if self.dither_params != dither_snap
            || self.palette_kind != palette_snap
            || self.mode != mode_snap
            || self.halftone_params != halftone_snap
        {
            self.dither_dirty = true;
        }
        if self.input_max_dim != input_snap {
            self.pre_dirty = true;
            self.dither_dirty = true;
        }

        // Push undo on discrete changes or when a drag gesture completes.
        let post_undo = self.make_snapshot();
        if !pointer_down {
            if let Some(pending) = self.undo_pending.take() {
                if pending != post_undo {
                    self.push_undo(pending);
                }
            } else if pre_undo != post_undo {
                self.push_undo(pre_undo);
            }
        }
    }

    // ── Effects stack ─────────────────────────────────────────────────────────

    fn ui_effects_stack(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("Effects").strong())
            .default_open(true)
            .show(ui, |ui| {
                let mut swap_prev: Option<usize> = None;
                let mut swap_next: Option<usize> = None;
                let mut delete_idx: Option<usize> = None;
                let n = self.effects.len();

                for i in 0..n {
                    let e = &mut self.effects[i];
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut e.enabled, "");
                        let label = RichText::new(e.kind.label())
                            .strong()
                            .color(if e.enabled {
                                ui.visuals().text_color()
                            } else {
                                ui.visuals().weak_text_color()
                            });
                        ui.toggle_value(&mut e.open, label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() { delete_idx = Some(i); }
                            if i + 1 < n && ui.small_button("▼").clicked() { swap_next = Some(i); }
                            if i > 0 && ui.small_button("▲").clicked() { swap_prev = Some(i); }
                        });
                    });
                    if e.open {
                        ui.indent(format!("eff_{i}"), |ui| {
                            ui::effect_params_ui(ui, &mut self.effects[i].kind);
                        });
                    }
                    if i + 1 < n {
                        ui.separator();
                    }
                }

                if let Some(i) = swap_prev { self.effects.swap(i - 1, i); }
                if let Some(i) = swap_next { self.effects.swap(i, i + 1); }
                if let Some(i) = delete_idx { self.effects.remove(i); }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let w = ui.available_width() - 50.0;
                    egui::ComboBox::from_id_salt("add_eff")
                        .selected_text(EFFECT_NAMES[self.add_effect_choice].1)
                        .width(w)
                        .show_ui(ui, |ui| {
                            for (idx, &(_, label)) in EFFECT_NAMES.iter().enumerate() {
                                ui.selectable_value(&mut self.add_effect_choice, idx, label);
                            }
                        });
                    if ui.button("Add").clicked() {
                        let key = EFFECT_NAMES[self.add_effect_choice].0;
                        self.effects.push(Effect::new(EffectKind::default_for_name(key)));
                    }
                });
            });
    }

    // ── Dither controls ───────────────────────────────────────────────────────

    fn ui_dither_controls(&mut self, ui: &mut egui::Ui) {
        self.ui_palette_section(ui);
        ui.add_space(4.0);
        self.ui_algorithm_section(ui);
    }

    fn ui_palette_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("Palette").strong())
            .default_open(true)
            .show(ui, |ui| {
                let w = ui.available_width();

                egui::ComboBox::from_id_salt("palette")
                    .selected_text(self.palette_kind.name())
                    .width(w - 8.0)
                    .show_ui(ui, |ui| {
                        for &kind in PaletteKind::all() {
                            ui.selectable_value(&mut self.palette_kind, kind, kind.name());
                        }
                    });

                // Swatch preview
                ui.add_space(4.0);
                let palette = self.resolved_palette();
                let sw = ((w - 8.0) / palette.colors.len().max(1) as f32).clamp(4.0, 24.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(1.0, 1.0);
                    for &[r, g, b] in &palette.colors {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(sw, sw),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            rect,
                            2.0,
                            egui::Color32::from_rgb(
                                (r * 255.0) as u8,
                                (g * 255.0) as u8,
                                (b * 255.0) as u8,
                            ),
                        );
                    }
                });

                // Extraction controls
                ui.add_space(6.0);
                ui.separator();
                ui.label(RichText::new("Extract from image").strong());
                ui.horizontal(|ui| {
                    ui.label("Colors:");
                    ui.add(egui::Slider::new(&mut self.extract_num_colors, 2..=32).integer());
                });
                let can_extract = self.preprocessed.is_some();
                if ui
                    .add_enabled(can_extract, egui::Button::new("Extract Palette"))
                    .on_disabled_hover_text("Load an image first")
                    .clicked()
                {
                    if let Some(ref pre) = self.preprocessed {
                        let pal = crate::palette::extract_palette(pre, self.extract_num_colors);
                        self.user_palette = pal.colors;
                        self.palette_kind = PaletteKind::Extracted;
                    }
                }

                // Custom/extracted palette editor
                if matches!(self.palette_kind, PaletteKind::Extracted | PaletteKind::Custom)
                    && !self.user_palette.is_empty()
                {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(RichText::new("Edit Palette").strong());

                    let mut delete_color: Option<usize> = None;
                    let mut swap_prev: Option<usize> = None;
                    let mut swap_next: Option<usize> = None;
                    let nc = self.user_palette.len();

                    for i in 0..nc {
                        ui.horizontal(|ui| {
                            egui::color_picker::color_edit_button_rgb(
                                ui,
                                &mut self.user_palette[i],
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("✕").clicked() { delete_color = Some(i); }
                                    if i + 1 < nc && ui.small_button("▼").clicked() { swap_next = Some(i); }
                                    if i > 0 && ui.small_button("▲").clicked() { swap_prev = Some(i); }
                                },
                            );
                        });
                    }

                    if let Some(i) = swap_prev { self.user_palette.swap(i - 1, i); }
                    if let Some(i) = swap_next { self.user_palette.swap(i, i + 1); }
                    if let Some(i) = delete_color { self.user_palette.remove(i); }

                    if ui.small_button("+ Add Color").clicked() {
                        self.user_palette.push([1.0, 1.0, 1.0]);
                    }

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.small_button("Save .gpl").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("GIMP Palette", &["gpl"])
                                .set_file_name("palette.gpl")
                                .save_file()
                            {
                                let _ = crate::palette::save_gpl(
                                    &self.user_palette,
                                    "Custom",
                                    &path,
                                );
                            }
                        }
                        if ui.small_button("Load .gpl").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("GIMP Palette", &["gpl"])
                                .pick_file()
                            {
                                if let Ok(colors) = crate::palette::load_gpl(&path) {
                                    self.user_palette = colors;
                                    self.palette_kind = PaletteKind::Custom;
                                }
                            }
                        }
                    });
                }
            });
    }

    fn ui_algorithm_section(&mut self, ui: &mut egui::Ui) {
        use crate::dither::{Algorithm, BayerSize};
        egui::CollapsingHeader::new(RichText::new("Dithering").strong())
            .default_open(true)
            .show(ui, |ui| {
                let d = &mut self.dither_params;
                let w = ui.available_width();

                egui::ComboBox::from_id_salt("algo")
                    .selected_text(d.algorithm.name())
                    .width(w - 8.0)
                    .show_ui(ui, |ui| {
                        for &algo in Algorithm::all() {
                            ui.selectable_value(&mut d.algorithm, algo, algo.name());
                        }
                    });
                ui.add_space(4.0);

                if matches!(d.algorithm, Algorithm::Threshold) {
                    ui.label("Threshold");
                    ui.add(
                        egui::Slider::new(&mut d.threshold, 0.0..=1.0)
                            .custom_formatter(|v, _| format!("{v:.2}")),
                    );
                    ui.add_space(4.0);
                }
                if d.algorithm.needs_bayer_size() {
                    ui.label("Matrix size");
                    egui::ComboBox::from_id_salt("bayer_size")
                        .selected_text(d.bayer_size.name())
                        .width(w - 8.0)
                        .show_ui(ui, |ui| {
                            for &bs in BayerSize::all() {
                                ui.selectable_value(&mut d.bayer_size, bs, bs.name());
                            }
                        });
                    ui.add_space(4.0);
                    ui.label("Spread");
                    ui.add(
                        egui::Slider::new(&mut d.bayer_spread, 0.1..=2.0)
                            .custom_formatter(|v, _| format!("{v:.2}")),
                    );
                    ui.add_space(4.0);
                }
                if d.algorithm.needs_noise_scale() {
                    ui.label("Noise scale");
                    ui.add(
                        egui::Slider::new(&mut d.noise_scale, 0.1..=2.0)
                            .custom_formatter(|v, _| format!("{v:.2}")),
                    );
                    ui.add_space(4.0);
                }
                if d.algorithm.needs_serpentine() {
                    ui.checkbox(&mut d.serpentine, "Serpentine scan");
                }
            });
    }

    // ── Halftone controls ─────────────────────────────────────────────────────

    fn ui_halftone_controls(&mut self, ui: &mut egui::Ui) {
        use crate::halftone::DotShape;
        egui::CollapsingHeader::new(RichText::new("Halftone").strong())
            .default_open(true)
            .show(ui, |ui| {
                let h = &mut self.halftone_params;
                let w = ui.available_width();

                ui.horizontal(|ui| {
                    ui.selectable_value(&mut h.cmyk, false, "Grayscale");
                    ui.selectable_value(&mut h.cmyk, true, "CMYK");
                });
                ui.add_space(4.0);

                ui.label("Cell size");
                ui.add(
                    egui::Slider::new(&mut h.cell_size, 2.0..=64.0)
                        .custom_formatter(|v, _| format!("{v:.0}px")),
                );
                ui.add_space(4.0);

                ui.label("Dot scale");
                ui.add(
                    egui::Slider::new(&mut h.dot_scale, 0.1..=2.0)
                        .custom_formatter(|v, _| format!("{v:.2}×")),
                );
                ui.add_space(4.0);

                ui.label("Dot shape");
                egui::ComboBox::from_id_salt("dot_shape")
                    .selected_text(h.dot_shape.name())
                    .width(w - 8.0)
                    .show_ui(ui, |ui| {
                        for &shape in DotShape::all() {
                            ui.selectable_value(&mut h.dot_shape, shape, shape.name());
                        }
                    });
                ui.add_space(4.0);

                if !h.cmyk {
                    ui.label("Angle");
                    ui.add(
                        egui::Slider::new(&mut h.angle, 0.0..=180.0)
                            .suffix("°")
                            .custom_formatter(|v, _| format!("{v:.0}°")),
                    );
                    ui.add_space(4.0);
                } else {
                    ui.label(
                        RichText::new("C 105° · M 75° · Y 90° · K 45°")
                            .weak()
                            .small(),
                    );
                    ui.add_space(4.0);
                }
                ui.checkbox(&mut h.invert, "Invert");
            });
    }

    // ── View controls ─────────────────────────────────────────────────────────

    fn ui_view_controls(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("View").strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.view_mode, ViewMode::Compare, "Compare");
                    ui.selectable_value(&mut self.view_mode, ViewMode::Split, "Split");
                    ui.selectable_value(&mut self.view_mode, ViewMode::Output, "Output");
                });
                ui.add_space(4.0);

                ui.label("Zoom");
                ui.add(
                    egui::Slider::new(&mut self.zoom, 0.1..=8.0)
                        .logarithmic(true)
                        .custom_formatter(|v, _| format!("{v:.1}×")),
                );
                if ui.button("Reset zoom").clicked() {
                    self.zoom = 1.0;
                }
                ui.add_space(4.0);

                ui.label("Downscale input");
                egui::ComboBox::from_id_salt("input_scale")
                    .selected_text(ui::input_dim_label(self.input_max_dim))
                    .show_ui(ui, |ui| {
                        for &dim in &[0u32, 2048, 1024, 512, 256, 128] {
                            ui.selectable_value(
                                &mut self.input_max_dim,
                                dim,
                                ui::input_dim_label(dim),
                            );
                        }
                    });
                if let Some(ref img) = self.original {
                    let (w, h) = if self.input_max_dim > 0
                        && (img.width() > self.input_max_dim || img.height() > self.input_max_dim)
                    {
                        let scale =
                            self.input_max_dim as f32 / img.width().max(img.height()) as f32;
                        (
                            (img.width() as f32 * scale) as u32,
                            (img.height() as f32 * scale) as u32,
                        )
                    } else {
                        (img.width(), img.height())
                    };
                    ui.label(RichText::new(format!("→ {w}×{h}px")).weak().small());
                }
                ui.add_space(4.0);

                ui.label("Export scale");
                egui::ComboBox::from_id_salt("export_scale")
                    .selected_text(format!("{}×", self.export_scale))
                    .show_ui(ui, |ui| {
                        for s in [1u32, 2, 4, 8] {
                            ui.selectable_value(&mut self.export_scale, s, format!("{s}×"));
                        }
                    });
                ui.label(
                    RichText::new("Nearest-neighbour upscale on save")
                        .weak()
                        .small(),
                );
            });
    }

    // ── Canvas ────────────────────────────────────────────────────────────────

    fn ui_canvas(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        let available = ui.available_rect_before_wrap();

        if self.original.is_none() {
            ui.allocate_rect(available, egui::Sense::hover());
            ui.painter().rect_filled(available, 8.0, ui.visuals().faint_bg_color);
            ui.painter().text(
                available.center(),
                egui::Align2::CENTER_CENTER,
                "Drop an image here\nor use  Open Image",
                egui::FontId::proportional(20.0),
                ui.visuals().weak_text_color(),
            );
            return;
        }

        match self.view_mode {
            ViewMode::Compare => self.draw_compare(ctx, ui, available),
            ViewMode::Split => self.draw_split(ui),
            ViewMode::Output => self.draw_output(ui),
        }
    }

    fn draw_compare(&mut self, ctx: &Context, ui: &mut egui::Ui, available: egui::Rect) {
        let (orig_tex, dith_tex) = match (&self.original_tex, &self.dithered_tex) {
            (Some(o), Some(d)) => (o, d),
            _ => return,
        };

        let [iw, ih] = orig_tex.size();
        let fit_scale =
            (available.width() / iw as f32).min(available.height() / ih as f32);
        let scale = fit_scale * self.zoom;
        let img_size = egui::Vec2::new(iw as f32 * scale, ih as f32 * scale);
        let img_rect = egui::Rect::from_center_size(available.center(), img_size);

        let split_x = img_rect.left() + self.compare_pos * img_rect.width();
        let uv_full = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0));
        let painter = ui.painter_at(available);

        painter
            .with_clip_rect(egui::Rect::from_min_max(
                img_rect.min,
                egui::Pos2::new(split_x, img_rect.max.y),
            ))
            .image(orig_tex.id(), img_rect, uv_full, egui::Color32::WHITE);

        painter
            .with_clip_rect(egui::Rect::from_min_max(
                egui::Pos2::new(split_x, img_rect.min.y),
                img_rect.max,
            ))
            .image(dith_tex.id(), img_rect, uv_full, egui::Color32::WHITE);

        painter.line_segment(
            [
                egui::Pos2::new(split_x, img_rect.min.y),
                egui::Pos2::new(split_x, img_rect.max.y),
            ],
            egui::Stroke::new(2.0, egui::Color32::WHITE),
        );
        painter.circle_filled(
            egui::Pos2::new(split_x, img_rect.center().y),
            8.0,
            egui::Color32::WHITE,
        );
        painter.circle_stroke(
            egui::Pos2::new(split_x, img_rect.center().y),
            8.0,
            egui::Stroke::new(1.5, egui::Color32::from_gray(80)),
        );

        if self.compare_pos > 0.08 {
            painter.text(
                egui::Pos2::new(img_rect.left() + 8.0, img_rect.top() + 8.0),
                egui::Align2::LEFT_TOP,
                "Original",
                egui::FontId::proportional(12.0),
                egui::Color32::from_rgba_premultiplied(0, 0, 0, 160),
            );
        }
        if self.compare_pos < 0.92 {
            painter.text(
                egui::Pos2::new(img_rect.right() - 8.0, img_rect.top() + 8.0),
                egui::Align2::RIGHT_TOP,
                "Output",
                egui::FontId::proportional(12.0),
                egui::Color32::from_rgba_premultiplied(0, 0, 0, 160),
            );
        }

        let divider_rect = egui::Rect::from_center_size(
            egui::Pos2::new(split_x, img_rect.center().y),
            egui::Vec2::new(24.0, img_rect.height()),
        );
        let divider_resp =
            ui.interact(divider_rect, ui.id().with("compare_div"), egui::Sense::drag());
        if divider_resp.dragged() && img_rect.width() > 0.0 {
            self.compare_pos =
                (self.compare_pos + divider_resp.drag_delta().x / img_rect.width())
                    .clamp(0.0, 1.0);
        }
        if divider_resp.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        ui.allocate_rect(available, egui::Sense::hover());
    }

    fn draw_split(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(tex) = &self.original_tex {
                        let size = ui::tex_display_size(tex, self.zoom);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Original").weak());
                            ui.add(egui::Image::new(tex).fit_to_exact_size(size));
                        });
                    }
                    ui.separator();
                    if let Some(tex) = &self.dithered_tex {
                        let size = ui::tex_display_size(tex, self.zoom);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Output").weak());
                            ui.add(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(size)
                                    .texture_options(TextureOptions::NEAREST),
                            );
                        });
                    }
                });
            });
    }

    fn draw_output(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(tex) = &self.dithered_tex {
                    let size = ui::tex_display_size(tex, self.zoom);
                    ui.vertical_centered(|ui| {
                        ui.add(
                            egui::Image::new(tex)
                                .fit_to_exact_size(size)
                                .texture_options(TextureOptions::NEAREST),
                        );
                    });
                }
            });
    }
}

// ── Save format ───────────────────────────────────────────────────────────────

/// Output format for the Save dialog.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SaveFormat {
    Png,
    Gif,
    Jpeg,
    Bmp,
}

impl SaveFormat {
    fn filter_name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Gif => "GIF",
            Self::Jpeg => "JPEG",
            Self::Bmp => "BMP",
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png"],
            Self::Gif => &["gif"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Bmp => &["bmp"],
        }
    }

    fn default_filename(self) -> &'static str {
        match self {
            Self::Png => "dithered.png",
            Self::Gif => "dithered.gif",
            Self::Jpeg => "dithered.jpg",
            Self::Bmp => "dithered.bmp",
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn save_gif(img: &DynamicImage, path: &std::path::Path) -> Result<(), String> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::Frame;
    let rgba = img.to_rgba8();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = GifEncoder::new(file);
    enc.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;
    enc.encode_frame(Frame::new(rgba)).map_err(|e| e.to_string())?;
    Ok(())
}

/// Upload a [`DynamicImage`] to the GPU as an egui texture.
fn dynamic_to_texture(ctx: &Context, name: &str, img: &DynamicImage) -> TextureHandle {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let ci = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
    ctx.load_texture(name, ci, TextureOptions::LINEAR)
}
