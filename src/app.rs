use std::path::PathBuf;

use egui::{Color32, ColorImage, Context, Pos2, Rect, RichText, Sense, Stroke, TextureHandle,
           TextureOptions, Vec2};
use image::DynamicImage;

use crate::dither::{Algorithm, BayerSize, DitherParams};
use crate::effects::{eval_curve, Effect, EffectKind, EFFECT_NAMES};
use crate::halftone::{DotShape, HalftoneParams};
use crate::palette::{extract_palette, load_gpl, save_gpl, Palette, PaletteKind};

// ── View mode ─────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Debug)]
pub enum ViewMode {
    Split,   // side-by-side
    Compare, // draggable slider
    Output,  // output only
}

// ── App mode ──────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Debug)]
pub enum AppMode {
    Dither,
    Halftone,
}

// ── App state ─────────────────────────────────────────────────────────────────

pub struct DitheringApp {
    original: Option<DynamicImage>,
    preprocessed: Option<DynamicImage>,
    dithered: Option<DynamicImage>,

    original_tex: Option<TextureHandle>,
    dithered_tex: Option<TextureHandle>,

    // Modes
    mode: AppMode,
    view_mode: ViewMode,

    // Effects stack
    effects: Vec<Effect>,
    add_effect_choice: usize,

    // Dither params
    dither_params: DitherParams,
    palette_kind: PaletteKind,
    user_palette: Vec<[f32; 3]>, // backing store for Extracted + Custom
    extract_num_colors: usize,

    // Halftone params
    halftone_params: HalftoneParams,

    // Dirty flags
    pre_dirty: bool,
    dither_dirty: bool,

    // View state
    zoom: f32,
    compare_pos: f32,   // 0.0–1.0, position of comparison divider
    export_scale: u32,  // 1, 2, 4, 8 — nearest-neighbour upscale on save

    status: String,
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
            pre_dirty: false,
            dither_dirty: false,
            zoom: 1.0,
            compare_pos: 0.5,
            export_scale: 1,
            status: "Drop an image or use \"Open Image\"".to_string(),
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
                    img.width(), img.height()
                );
                self.original_tex = Some(dynamic_to_texture(ctx, "original", &img));
                self.original = Some(img);
                self.pre_dirty = true;
                self.dither_dirty = true;
            }
            Err(e) => self.status = format!("Error: {e}"),
        }
    }

    fn save_image(&self, format: SaveFormat) {
        if let Some(ref img) = self.dithered {
            let export = if self.export_scale > 1 {
                img.resize_exact(
                    img.width() * self.export_scale,
                    img.height() * self.export_scale,
                    image::imageops::FilterType::Nearest,
                )
            } else {
                img.clone()
            };

            let (filter, exts, default) = match format {
                SaveFormat::Png  => ("PNG",  vec!["png"],        "dithered.png"),
                SaveFormat::Gif  => ("GIF",  vec!["gif"],        "dithered.gif"),
                SaveFormat::Bmp  => ("BMP",  vec!["bmp"],        "dithered.bmp"),
                SaveFormat::Jpeg => ("JPEG", vec!["jpg", "jpeg"],"dithered.jpg"),
            };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(filter, &exts)
                .set_file_name(default)
                .save_file()
            {
                let result = match format {
                    SaveFormat::Gif => save_gif(&export, &path),
                    _ => export.save(&path).map_err(|e| e.to_string()),
                };
                if let Err(e) = result { eprintln!("Save error: {e}"); }
            }
        }
    }

    fn copy_to_clipboard(&self) {
        if let Some(ref img) = self.dithered {
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
    }

    // ── Recompute ─────────────────────────────────────────────────────────────

    fn recompute(&mut self, ctx: &Context) {
        let src = match &self.original { Some(i) => i.clone(), None => return };

        if self.pre_dirty {
            self.preprocessed = Some(crate::effects::apply_effects(&src, &self.effects));
            self.pre_dirty = false;
            self.dither_dirty = true;
        }

        if self.dither_dirty {
            if let Some(ref pre) = self.preprocessed {
                let result = match self.mode {
                    AppMode::Halftone => crate::halftone::apply_halftone(pre, &self.halftone_params),
                    AppMode::Dither => {
                        let palette = self.resolved_palette();
                        crate::dither::apply_dither(pre, &self.dither_params, &palette)
                    }
                };
                self.dithered_tex = Some(dynamic_to_texture(ctx, "dithered", &result));
                self.dithered = Some(result);
            }
            self.dither_dirty = false;
        }
    }

    fn resolved_palette(&self) -> Palette {
        match self.palette_kind {
            PaletteKind::Extracted | PaletteKind::Custom if !self.user_palette.is_empty() => {
                Palette { name: "User", colors: self.user_palette.clone() }
            }
            _ => self.palette_kind.build(),
        }
    }

    // ── eframe update ─────────────────────────────────────────────────────────

    // (see impl eframe::App below)

    // ── Controls ──────────────────────────────────────────────────────────────

    fn ui_controls(&mut self, ui: &mut egui::Ui) {
        let effects_snap: Vec<(bool, EffectKind)> =
            self.effects.iter().map(|e| (e.enabled, e.kind.clone())).collect();
        let palette_before  = self.palette_kind.clone();
        let dither_before   = self.dither_params.clone();
        let halftone_before = self.halftone_params.clone();
        let mode_before     = self.mode.clone();

        // Mode toggle
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, AppMode::Dither,   "Dither");
            ui.selectable_value(&mut self.mode, AppMode::Halftone, "Halftone");
        });
        ui.separator();
        ui.add_space(4.0);

        self.ui_effects_stack(ui);
        ui.add_space(4.0);

        match self.mode {
            AppMode::Dither   => self.ui_dither_controls(ui),
            AppMode::Halftone => self.ui_halftone_controls(ui),
        }
        ui.add_space(4.0);
        self.ui_view_controls(ui);

        // Dirty propagation
        let effects_now: Vec<(bool, EffectKind)> =
            self.effects.iter().map(|e| (e.enabled, e.kind.clone())).collect();
        if effects_now != effects_snap {
            self.pre_dirty   = true;
            self.dither_dirty = true;
        }
        if self.mode != mode_before
            || self.palette_kind != palette_before
            || self.dither_params != dither_before
            || self.halftone_params != halftone_before
        {
            self.dither_dirty = true;
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
                            .color(if e.enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() });
                        ui.toggle_value(&mut e.open, label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() { delete_idx = Some(i); }
                            if i + 1 < n && ui.small_button("▼").clicked() { swap_next = Some(i); }
                            if i > 0      && ui.small_button("▲").clicked() { swap_prev = Some(i); }
                        });
                    });
                    if e.open {
                        ui.indent(format!("eff_{i}"), |ui| {
                            effect_params_ui(ui, &mut e.kind);
                        });
                    }
                    if i + 1 < n { ui.separator(); }
                }

                if let Some(i) = swap_prev  { self.effects.swap(i - 1, i); }
                if let Some(i) = swap_next  { self.effects.swap(i, i + 1); }
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
        let palette_dirty = self.ui_palette_section(ui);
        if palette_dirty { self.dither_dirty = true; }
        ui.add_space(4.0);
        self.ui_algorithm_section(ui);
    }

    fn ui_palette_section(&mut self, ui: &mut egui::Ui) -> bool {
        let mut dirty = false;
        egui::CollapsingHeader::new(RichText::new("Palette").strong())
            .default_open(true)
            .show(ui, |ui| {
                let w = ui.available_width();

                // Dropdown
                let before = self.palette_kind.clone();
                egui::ComboBox::from_id_salt("palette")
                    .selected_text(self.palette_kind.name())
                    .width(w - 8.0)
                    .show_ui(ui, |ui| {
                        for kind in PaletteKind::all() {
                            ui.selectable_value(&mut self.palette_kind, kind.clone(), kind.name());
                        }
                    });
                if self.palette_kind != before { dirty = true; }

                // Swatch preview
                ui.add_space(4.0);
                let palette = self.resolved_palette();
                let sw = ((w - 8.0) / palette.colors.len().max(1) as f32).clamp(4.0, 24.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(1.0, 1.0);
                    for &[r, g, b] in &palette.colors {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(sw, sw), Sense::hover());
                        ui.painter().rect_filled(rect, 2.0,
                            Color32::from_rgb((r*255.0) as u8, (g*255.0) as u8, (b*255.0) as u8));
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
                let can = self.preprocessed.is_some();
                if ui.add_enabled(can, egui::Button::new("Extract Palette"))
                    .on_disabled_hover_text("Load an image first").clicked()
                {
                    if let Some(ref pre) = self.preprocessed {
                        let pal = extract_palette(pre, self.extract_num_colors);
                        self.user_palette = pal.colors;
                        self.palette_kind = PaletteKind::Extracted;
                        dirty = true;
                    }
                }

                // Custom palette editor (shown for Extracted and Custom)
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
                            // Color picker swatch
                            if egui::color_picker::color_edit_button_rgb(ui, &mut self.user_palette[i]).changed() {
                                dirty = true;
                            }
                            // Reorder + delete
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("✕").clicked() { delete_color = Some(i); dirty = true; }
                                if i + 1 < nc && ui.small_button("▼").clicked() { swap_next = Some(i); dirty = true; }
                                if i > 0       && ui.small_button("▲").clicked() { swap_prev = Some(i); dirty = true; }
                            });
                        });
                    }

                    if let Some(i) = swap_prev  { self.user_palette.swap(i - 1, i); }
                    if let Some(i) = swap_next  { self.user_palette.swap(i, i + 1); }
                    if let Some(i) = delete_color { self.user_palette.remove(i); }

                    if ui.small_button("+ Add Color").clicked() {
                        self.user_palette.push([1.0, 1.0, 1.0]);
                        dirty = true;
                    }

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.small_button("Save .gpl").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("GIMP Palette", &["gpl"])
                                .set_file_name("palette.gpl")
                                .save_file()
                            {
                                let _ = save_gpl(&self.user_palette, "Custom", &path);
                            }
                        }
                        if ui.small_button("Load .gpl").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("GIMP Palette", &["gpl"])
                                .pick_file()
                            {
                                if let Ok(colors) = load_gpl(&path) {
                                    self.user_palette = colors;
                                    self.palette_kind = PaletteKind::Custom;
                                    dirty = true;
                                }
                            }
                        }
                    });
                }
            });
        dirty
    }

    fn ui_algorithm_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("Dithering").strong())
            .default_open(true)
            .show(ui, |ui| {
                let d = &mut self.dither_params;
                let w = ui.available_width();
                egui::ComboBox::from_id_salt("algo")
                    .selected_text(d.algorithm.name())
                    .width(w - 8.0)
                    .show_ui(ui, |ui| {
                        for algo in Algorithm::all() {
                            ui.selectable_value(&mut d.algorithm, algo.clone(), algo.name());
                        }
                    });
                ui.add_space(4.0);
                if matches!(d.algorithm, Algorithm::Threshold) {
                    ui.label("Threshold");
                    ui.add(egui::Slider::new(&mut d.threshold, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.2}", v)));
                    ui.add_space(4.0);
                }
                if d.algorithm.needs_bayer_size() {
                    ui.label("Matrix size");
                    egui::ComboBox::from_id_salt("bayer_size")
                        .selected_text(d.bayer_size.name())
                        .width(w - 8.0)
                        .show_ui(ui, |ui| {
                            for bs in BayerSize::all() {
                                ui.selectable_value(&mut d.bayer_size, bs.clone(), bs.name());
                            }
                        });
                    ui.add_space(4.0);
                    ui.label("Spread");
                    ui.add(egui::Slider::new(&mut d.bayer_spread, 0.1..=2.0)
                        .custom_formatter(|v, _| format!("{:.2}", v)));
                    ui.add_space(4.0);
                }
                if d.algorithm.needs_noise_scale() {
                    ui.label("Noise scale");
                    ui.add(egui::Slider::new(&mut d.noise_scale, 0.1..=2.0)
                        .custom_formatter(|v, _| format!("{:.2}", v)));
                    ui.add_space(4.0);
                }
                if d.algorithm.needs_serpentine() {
                    ui.checkbox(&mut d.serpentine, "Serpentine scan");
                }
            });
    }

    // ── Halftone controls ─────────────────────────────────────────────────────

    fn ui_halftone_controls(&mut self, ui: &mut egui::Ui) {
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
                ui.add(egui::Slider::new(&mut h.cell_size, 2.0..=64.0)
                    .custom_formatter(|v, _| format!("{:.0}px", v)));
                ui.add_space(4.0);
                ui.label("Dot scale");
                ui.add(egui::Slider::new(&mut h.dot_scale, 0.1..=2.0)
                    .custom_formatter(|v, _| format!("{:.2}×", v)));
                ui.add_space(4.0);
                ui.label("Dot shape");
                egui::ComboBox::from_id_salt("dot_shape")
                    .selected_text(h.dot_shape.name())
                    .width(w - 8.0)
                    .show_ui(ui, |ui| {
                        for shape in DotShape::all() {
                            ui.selectable_value(&mut h.dot_shape, shape.clone(), shape.name());
                        }
                    });
                ui.add_space(4.0);
                if !h.cmyk {
                    ui.label("Angle");
                    ui.add(egui::Slider::new(&mut h.angle, 0.0..=180.0)
                        .suffix("°").custom_formatter(|v, _| format!("{:.0}°", v)));
                    ui.add_space(4.0);
                } else {
                    ui.label(RichText::new("C 105° · M 75° · Y 90° · K 45°").weak().small());
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
                    ui.selectable_value(&mut self.view_mode, ViewMode::Split,   "Split");
                    ui.selectable_value(&mut self.view_mode, ViewMode::Output,  "Output");
                });
                ui.add_space(4.0);
                ui.label("Zoom");
                ui.add(egui::Slider::new(&mut self.zoom, 0.1..=8.0)
                    .logarithmic(true)
                    .custom_formatter(|v, _| format!("{:.1}×", v)));
                if ui.button("Reset zoom").clicked() { self.zoom = 1.0; }
                ui.add_space(4.0);
                ui.label("Export scale");
                egui::ComboBox::from_id_salt("export_scale")
                    .selected_text(format!("{}×", self.export_scale))
                    .show_ui(ui, |ui| {
                        for s in [1u32, 2, 4, 8] {
                            ui.selectable_value(&mut self.export_scale, s, format!("{s}×"));
                        }
                    });
                ui.label(RichText::new("Nearest-neighbour upscale on save").weak().small());
            });
    }

    // ── Canvas ────────────────────────────────────────────────────────────────

    fn ui_canvas(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        let available = ui.available_rect_before_wrap();

        if self.original.is_none() {
            ui.allocate_rect(available, Sense::hover());
            ui.painter().rect_filled(available, 8.0, ui.visuals().faint_bg_color);
            ui.painter().text(available.center(), egui::Align2::CENTER_CENTER,
                "Drop an image here\nor use  Open Image",
                egui::FontId::proportional(20.0), ui.visuals().weak_text_color());
            return;
        }

        match self.view_mode {
            ViewMode::Compare => self.draw_compare(ctx, ui, available),
            ViewMode::Split   => self.draw_split(ui),
            ViewMode::Output  => self.draw_output(ui),
        }
    }

    fn draw_compare(&mut self, ctx: &Context, ui: &mut egui::Ui, available: Rect) {
        let (orig_tex, dith_tex) = match (&self.original_tex, &self.dithered_tex) {
            (Some(o), Some(d)) => (o, d),
            _ => return,
        };

        let [iw, ih] = orig_tex.size();
        let fit_scale = (available.width() / iw as f32).min(available.height() / ih as f32);
        let scale = fit_scale * self.zoom;
        let img_size = Vec2::new(iw as f32 * scale, ih as f32 * scale);
        let img_rect = Rect::from_center_size(available.center(), img_size);

        let split_x = img_rect.left() + self.compare_pos * img_rect.width();
        let uv_full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let painter = ui.painter_at(available);

        // Original on left of divider
        painter.with_clip_rect(
            Rect::from_min_max(img_rect.min, Pos2::new(split_x, img_rect.max.y))
        ).image(orig_tex.id(), img_rect, uv_full, Color32::WHITE);

        // Dithered on right of divider (nearest-neighbour)
        painter.with_clip_rect(
            Rect::from_min_max(Pos2::new(split_x, img_rect.min.y), img_rect.max)
        ).image(dith_tex.id(), img_rect, uv_full, Color32::WHITE);

        // Divider line + handle
        painter.line_segment(
            [Pos2::new(split_x, img_rect.min.y), Pos2::new(split_x, img_rect.max.y)],
            Stroke::new(2.0, Color32::WHITE),
        );
        painter.circle_filled(Pos2::new(split_x, img_rect.center().y), 8.0, Color32::WHITE);
        painter.circle_stroke(
            Pos2::new(split_x, img_rect.center().y), 8.0, Stroke::new(1.5, Color32::from_gray(80))
        );

        // Labels
        if self.compare_pos > 0.08 {
            painter.text(
                Pos2::new(img_rect.left() + 8.0, img_rect.top() + 8.0),
                egui::Align2::LEFT_TOP, "Original",
                egui::FontId::proportional(12.0), Color32::from_rgba_premultiplied(0,0,0,160),
            );
        }
        if self.compare_pos < 0.92 {
            painter.text(
                Pos2::new(img_rect.right() - 8.0, img_rect.top() + 8.0),
                egui::Align2::RIGHT_TOP, "Output",
                egui::FontId::proportional(12.0), Color32::from_rgba_premultiplied(0,0,0,160),
            );
        }

        // Drag interaction on the divider strip
        let divider_rect = Rect::from_center_size(
            Pos2::new(split_x, img_rect.center().y),
            Vec2::new(24.0, img_rect.height()),
        );
        let divider_resp = ui.interact(divider_rect, ui.id().with("compare_div"), Sense::drag());
        if divider_resp.dragged() && img_rect.width() > 0.0 {
            self.compare_pos = (self.compare_pos + divider_resp.drag_delta().x / img_rect.width())
                .clamp(0.0, 1.0);
        }
        if divider_resp.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        ui.allocate_rect(available, Sense::hover());
    }

    fn draw_split(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(tex) = &self.original_tex {
                    let size = tex_display_size(tex, self.zoom);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Original").weak());
                        ui.add(egui::Image::new(tex).fit_to_exact_size(size));
                    });
                }
                ui.separator();
                if let Some(tex) = &self.dithered_tex {
                    let size = tex_display_size(tex, self.zoom);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Output").weak());
                        ui.add(egui::Image::new(tex)
                            .fit_to_exact_size(size)
                            .texture_options(TextureOptions::NEAREST));
                    });
                }
            });
        });
    }

    fn draw_output(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            if let Some(tex) = &self.dithered_tex {
                let size = tex_display_size(tex, self.zoom);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new(tex)
                        .fit_to_exact_size(size)
                        .texture_options(TextureOptions::NEAREST));
                });
            }
        });
    }
}

// ── eframe::App ───────────────────────────────────────────────────────────────

impl eframe::App for DitheringApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Drag and drop
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if let Some(path) = dropped.into_iter().next() {
            self.load_image(path, ctx);
        }

        if (self.pre_dirty || self.dither_dirty) && self.original.is_some() {
            self.recompute(ctx);
        }

        // Top bar
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Dithering Tool");
                ui.separator();
                if ui.button("Open Image").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Images", &["png","jpg","jpeg","bmp","gif","tiff","webp"])
                        .pick_file() { self.load_image(path, ctx); }
                }
                ui.separator();
                let has = self.dithered.is_some();
                ui.menu_button("Save ▾", |ui| {
                    for (label, fmt) in [("PNG", SaveFormat::Png), ("GIF", SaveFormat::Gif),
                                         ("JPEG", SaveFormat::Jpeg), ("BMP", SaveFormat::Bmp)]
                    {
                        if ui.add_enabled(has, egui::Button::new(label)).clicked() {
                            self.save_image(fmt); ui.close_menu();
                        }
                    }
                });
                if ui.add_enabled(has, egui::Button::new("Copy")).clicked() {
                    self.copy_to_clipboard();
                }
                ui.separator();
                ui.label(RichText::new(&self.status).weak());
            });
        });

        // Left panel
        egui::SidePanel::left("controls")
            .resizable(true).default_width(240.0).min_width(200.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(4.0);
                    self.ui_controls(ui);
                    ui.add_space(8.0);
                });
            });

        // Canvas
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_canvas(ctx, ui);
        });
    }
}

// ── Effect parameter UI ───────────────────────────────────────────────────────

fn effect_params_ui(ui: &mut egui::Ui, kind: &mut EffectKind) {
    match kind {
        EffectKind::BrightnessContrastGamma { brightness, contrast, gamma } => {
            ui.label("Brightness");
            ui.add(egui::Slider::new(brightness, -1.0..=1.0).custom_formatter(|v,_| format!("{:+.2}",v)));
            ui.label("Contrast");
            ui.add(egui::Slider::new(contrast, 0.1..=4.0).custom_formatter(|v,_| format!("{:.2}×",v)));
            ui.label("Gamma");
            ui.add(egui::Slider::new(gamma, 0.1..=4.0).custom_formatter(|v,_| format!("{:.2}",v)));
        }
        EffectKind::Posterize { levels } => {
            ui.label("Levels");
            ui.add(egui::Slider::new(levels, 2..=16).suffix(" levels"));
        }
        EffectKind::Sharpen { amount } => {
            ui.label("Amount");
            ui.add(egui::Slider::new(amount, 0.0..=4.0).custom_formatter(|v,_| format!("{:.2}",v)));
        }
        EffectKind::Pixelate { block_size } => {
            ui.label("Block size");
            egui::ComboBox::from_id_salt("pix_blk")
                .selected_text(format!("{block_size}px"))
                .show_ui(ui, |ui| {
                    for s in [2u32, 4, 8, 12, 16, 24, 32, 48, 64] {
                        ui.selectable_value(block_size, s, format!("{s}px"));
                    }
                });
        }
        EffectKind::JpegGlitch { quality } => {
            ui.label("Quality");
            ui.add(egui::Slider::new(quality, 1..=95).suffix("%"));
        }
        EffectKind::ChromaticAberration { x_shift, y_shift } => {
            ui.label("X shift");
            ui.add(egui::Slider::new(x_shift, -50.0..=50.0).custom_formatter(|v,_| format!("{:+.0}px",v)));
            ui.label("Y shift");
            ui.add(egui::Slider::new(y_shift, -50.0..=50.0).custom_formatter(|v,_| format!("{:+.0}px",v)));
        }
        EffectKind::ToneCurve { points } => {
            tone_curve_widget(ui, points);
        }
    }
}

fn tone_curve_widget(ui: &mut egui::Ui, points: &mut [f32; 5]) {
    let size = ui.available_width().min(180.0);
    let (response, painter) = ui.allocate_painter(Vec2::splat(size), Sense::hover());
    let rect = response.rect;

    // Background + grid
    painter.rect_filled(rect, 4.0, Color32::from_gray(25));
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let x = rect.left() + t * rect.width();
        let y = rect.top()  + t * rect.height();
        let dim = Color32::from_gray(50);
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(0.5, dim));
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(0.5, dim));
    }
    // Identity diagonal
    painter.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(0.5, Color32::from_gray(70)));

    // Curve (sample 128 points)
    let curve_pts: Vec<Pos2> = (0..=128).map(|i| {
        let t = i as f32 / 128.0;
        let v = eval_curve(points, t);
        Pos2::new(rect.left() + t * rect.width(), rect.bottom() - v * rect.height())
    }).collect();
    painter.add(egui::Shape::line(curve_pts, Stroke::new(1.5, Color32::WHITE)));

    // Control points — drag interaction
    let inputs = [0.0f32, 0.25, 0.5, 0.75, 1.0];
    for (i, (&in_v, out_v)) in inputs.iter().zip(points.iter_mut()).enumerate() {
        let cx = rect.left() + in_v * rect.width();
        let cy = rect.bottom() - *out_v * rect.height();
        let cp = Pos2::new(cx, cy);

        let cp_resp = ui.interact(
            Rect::from_center_size(cp, Vec2::splat(14.0)),
            response.id.with(i),
            Sense::drag(),
        );
        if cp_resp.dragged() {
            *out_v = (*out_v - cp_resp.drag_delta().y / rect.height()).clamp(0.0, 1.0);
        }

        let col = if cp_resp.hovered() || cp_resp.dragged() { Color32::YELLOW } else { Color32::WHITE };
        painter.circle_filled(cp, 5.0, col);
        painter.circle_stroke(cp, 5.0, Stroke::new(1.0, Color32::from_gray(80)));
    }

    if ui.small_button("Reset curve").clicked() {
        *points = [0.0, 0.25, 0.5, 0.75, 1.0];
    }
}

// ── Save helpers ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum SaveFormat { Png, Gif, Jpeg, Bmp }

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

// ── Texture helpers ───────────────────────────────────────────────────────────

fn dynamic_to_texture(ctx: &Context, name: &str, img: &DynamicImage) -> TextureHandle {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let ci = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
    ctx.load_texture(name, ci, TextureOptions::LINEAR)
}

fn tex_display_size(tex: &TextureHandle, zoom: f32) -> Vec2 {
    let [tw, th] = tex.size();
    Vec2::new(tw as f32 * zoom, th as f32 * zoom)
}
