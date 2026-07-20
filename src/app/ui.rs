//! Pure UI helper functions for the dithering tool.
//!
//! These are free functions (not methods on [`super::DitheringApp`]) because
//! they don't need access to application state — they operate only on the
//! data types they receive as arguments. Keeping them here reduces the length
//! of `mod.rs` and makes them easy to test in isolation.

use egui::{Color32, Pos2, Rect, Rounding, Sense, Stroke, TextureHandle, Vec2};

use crate::effects::{eval_curve, EffectKind};

// ── Theme ─────────────────────────────────────────────────────────────────────

/// Green phosphor terminal colour constants.
pub(super) mod theme {
    use egui::Color32;

    pub const BG:             Color32 = Color32::from_rgb(0x05, 0x0a, 0x05);
    pub const BG_PANEL:       Color32 = Color32::from_rgb(0x07, 0x0c, 0x07);
    pub const BG_WIDGET:      Color32 = Color32::from_rgb(0x0c, 0x16, 0x0c);
    pub const BG_WIDGET_HI:   Color32 = Color32::from_rgb(0x14, 0x20, 0x14);
    pub const GREEN:          Color32 = Color32::from_rgb(0x33, 0xff, 0x77);
    pub const GREEN_DIM:      Color32 = Color32::from_rgb(0x1a, 0x66, 0x33);
    pub const GREEN_FAINT:    Color32 = Color32::from_rgb(0x0c, 0x24, 0x14);
    pub const BORDER:         Color32 = Color32::from_rgb(0x1a, 0x4a, 0x1a);
    pub const BORDER_BRIGHT:  Color32 = Color32::from_rgb(0x33, 0xff, 0x77);
}

// ── Global visuals setup ──────────────────────────────────────────────────────

/// Apply the phosphor-green terminal theme. Call once from `DitheringApp::new`.
pub(super) fn setup_visuals(ctx: &egui::Context) {
    use egui::{FontFamily::Monospace, FontId, Style, TextStyle, Visuals};
    use std::collections::BTreeMap;

    let mut v = Visuals::dark();

    v.override_text_color  = Some(theme::GREEN);
    v.panel_fill           = theme::BG_PANEL;
    v.window_fill          = theme::BG;
    v.extreme_bg_color     = theme::BG;
    v.faint_bg_color       = theme::BG_WIDGET;
    v.code_bg_color        = theme::BG_WIDGET;
    v.hyperlink_color      = theme::GREEN;
    v.selection.bg_fill    = theme::BG_WIDGET_HI;
    v.selection.stroke     = Stroke::new(1.0, theme::GREEN);

    let zero = Rounding::ZERO;

    v.widgets.noninteractive = egui::style::WidgetVisuals {
        bg_fill: theme::BG_WIDGET, weak_bg_fill: theme::BG_WIDGET,
        bg_stroke: Stroke::new(1.0, theme::BORDER),
        fg_stroke: Stroke::new(1.0, theme::GREEN_DIM),
        rounding: zero, expansion: 0.0,
    };
    v.widgets.inactive = egui::style::WidgetVisuals {
        bg_fill: theme::BG_WIDGET, weak_bg_fill: theme::BG_WIDGET,
        bg_stroke: Stroke::new(1.0, theme::BORDER),
        fg_stroke: Stroke::new(1.0, theme::GREEN),
        rounding: zero, expansion: 0.0,
    };
    v.widgets.hovered = egui::style::WidgetVisuals {
        bg_fill: theme::BG_WIDGET_HI, weak_bg_fill: theme::BG_WIDGET_HI,
        bg_stroke: Stroke::new(1.0, theme::BORDER_BRIGHT),
        fg_stroke: Stroke::new(1.5, theme::GREEN),
        rounding: zero, expansion: 0.0,
    };
    v.widgets.active = egui::style::WidgetVisuals {
        bg_fill: theme::BG_WIDGET, weak_bg_fill: theme::BG_WIDGET,
        bg_stroke: Stroke::new(1.5, theme::BORDER_BRIGHT),
        fg_stroke: Stroke::new(2.0, theme::GREEN),
        rounding: zero, expansion: 0.0,
    };
    v.widgets.open = egui::style::WidgetVisuals {
        bg_fill: theme::BG_WIDGET_HI, weak_bg_fill: theme::BG_WIDGET_HI,
        bg_stroke: Stroke::new(1.0, theme::BORDER_BRIGHT),
        fg_stroke: Stroke::new(1.5, theme::GREEN),
        rounding: zero, expansion: 0.0,
    };

    ctx.set_visuals(v);

    ctx.style_mut(|s: &mut Style| {
        s.text_styles = BTreeMap::from([
            (TextStyle::Body,      FontId::new(13.0, Monospace)),
            (TextStyle::Button,    FontId::new(13.0, Monospace)),
            (TextStyle::Heading,   FontId::new(14.0, Monospace)),
            (TextStyle::Small,     FontId::new(11.0, Monospace)),
            (TextStyle::Monospace, FontId::new(13.0, Monospace)),
        ]);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.spacing.item_spacing   = egui::vec2(4.0, 3.0);
    });
}

// ── Section helpers ───────────────────────────────────────────────────────────

/// Wraps a section body with minimal vertical spacing and a faint bottom rule.
pub(super) fn section<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.add_space(4.0);
    let result = add_contents(ui);
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 1.0),
        Sense::hover(),
    );
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, theme::BORDER));
    result
}

/// ASCII-rule style collapsible section header.
///
/// Renders as: `[- LABEL ]──────────────────`
/// Clicking anywhere on the row toggles `open`.
pub(super) fn section_header(ui: &mut egui::Ui, label: &str, open: &mut bool) {
    let desired = egui::vec2(ui.available_width(), 20.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let line_y  = rect.center().y;

        // Full-width rule
        painter.hline(rect.x_range(), line_y, Stroke::new(1.0, theme::BORDER));

        let indicator = if *open { "-" } else { "+" };
        let text = format!("[{indicator} {} ]", label.to_uppercase());
        let font = egui::FontId::new(12.0, egui::FontFamily::Monospace);

        // Measure, paint opaque bg to "cut" the rule behind the label
        let galley  = painter.layout_no_wrap(text.clone(), font.clone(), theme::GREEN);
        let text_w  = galley.size().x + 8.0;
        let cut_rect = Rect::from_center_size(
            Pos2::new(rect.left() + text_w / 2.0 + 4.0, line_y),
            Vec2::new(text_w, rect.height()),
        );
        painter.rect_filled(cut_rect, Rounding::ZERO, theme::BG_PANEL);

        let col = if resp.hovered() { theme::GREEN } else { theme::GREEN_DIM };
        painter.text(
            Pos2::new(rect.left() + 4.0, line_y),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            col,
        );
    }

    if resp.clicked() {
        *open = !*open;
    }
}

// ── Effect parameter widgets ──────────────────────────────────────────────────

/// Render the parameter controls for a single effect kind, mutating it in place.
pub(super) fn effect_params_ui(ui: &mut egui::Ui, kind: &mut EffectKind) {
    match kind {
        EffectKind::BrightnessContrastGamma { brightness, contrast, gamma } => {
            ui.label("Brightness");
            ui.add(
                egui::Slider::new(brightness, -1.0..=1.0)
                    .custom_formatter(|v, _| format!("{v:+.2}")),
            );
            ui.label("Contrast");
            ui.add(
                egui::Slider::new(contrast, 0.1..=4.0)
                    .custom_formatter(|v, _| format!("{v:.2}x")),
            );
            ui.label("Gamma");
            ui.add(
                egui::Slider::new(gamma, 0.1..=4.0)
                    .custom_formatter(|v, _| format!("{v:.2}")),
            );
        }
        EffectKind::Posterize { levels } => {
            ui.label("Levels");
            ui.add(egui::Slider::new(levels, 2..=16).suffix(" levels"));
        }
        EffectKind::Sharpen { amount } => {
            ui.label("Amount");
            ui.add(
                egui::Slider::new(amount, 0.0..=4.0)
                    .custom_formatter(|v, _| format!("{v:.2}")),
            );
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
            ui.add(
                egui::Slider::new(x_shift, -50.0..=50.0)
                    .custom_formatter(|v, _| format!("{v:+.0}px")),
            );
            ui.label("Y shift");
            ui.add(
                egui::Slider::new(y_shift, -50.0..=50.0)
                    .custom_formatter(|v, _| format!("{v:+.0}px")),
            );
        }
        EffectKind::ToneCurve { points } => {
            tone_curve_widget(ui, points);
        }
        EffectKind::CrtGlow { radius, strength } => {
            ui.label("Radius");
            ui.add(egui::Slider::new(radius, 1..=32).suffix("px"));
            ui.label("Strength");
            ui.add(
                egui::Slider::new(strength, 0.0..=3.0)
                    .custom_formatter(|v, _| format!("{v:.2}x")),
            );
        }
        EffectKind::Scanlines { spacing, opacity } => {
            ui.label("Spacing");
            ui.add(egui::Slider::new(spacing, 2..=16).suffix("px"));
            ui.label("Opacity");
            ui.add(
                egui::Slider::new(opacity, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
        }
        EffectKind::FilmEmulation {
            exposure_ev,
            grain_strength,
            grain_size,
            halation_strength,
            halation_radius,
            halation_threshold,
        } => {
            ui.label("Kodak Portra 400");
            ui.label("Exposure");
            ui.add(
                egui::Slider::new(exposure_ev, -2.0..=2.0)
                    .custom_formatter(|v, _| format!("{v:+.2} EV")),
            );
            ui.add_space(4.0);
            ui.label("Grain strength");
            ui.add(
                egui::Slider::new(grain_strength, 0.0..=2.0)
                    .custom_formatter(|v, _| format!("{v:.2}")),
            );
            ui.label("Grain size");
            ui.add(
                egui::Slider::new(grain_size, 0.5..=3.0)
                    .custom_formatter(|v, _| format!("{v:.2}")),
            );
            ui.add_space(4.0);
            ui.label("Halation strength");
            ui.add(
                egui::Slider::new(halation_strength, 0.0..=2.0)
                    .custom_formatter(|v, _| format!("{v:.2}")),
            );
            ui.label("Halation radius");
            ui.add(egui::Slider::new(halation_radius, 2..=64).suffix("px"));
            ui.label("Halation threshold");
            ui.add(
                egui::Slider::new(halation_threshold, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
        }
    }
}

// ── Tone curve widget ─────────────────────────────────────────────────────────

/// Interactive tone-curve editor with phosphor-green styling.
fn tone_curve_widget(ui: &mut egui::Ui, points: &mut [f32; 5]) {
    let size = ui.available_width().min(180.0);
    let (response, painter) = ui.allocate_painter(Vec2::splat(size), Sense::hover());
    let rect = response.rect;

    painter.rect_filled(rect, Rounding::ZERO, theme::BG);
    painter.rect_stroke(rect, Rounding::ZERO, Stroke::new(1.0, theme::BORDER));

    // Grid lines
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let x = rect.left() + t * rect.width();
        let y = rect.top()  + t * rect.height();
        painter.line_segment(
            [Pos2::new(x, rect.top()),    Pos2::new(x, rect.bottom())],
            Stroke::new(0.5, theme::GREEN_FAINT),
        );
        painter.line_segment(
            [Pos2::new(rect.left(), y),   Pos2::new(rect.right(), y)],
            Stroke::new(0.5, theme::GREEN_FAINT),
        );
    }

    // Identity diagonal
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(0.5, theme::GREEN_DIM),
    );

    // Curve (128 samples) — bright phosphor green
    let curve_pts: Vec<Pos2> = (0..=128)
        .map(|i| {
            let t = i as f32 / 128.0;
            let v = eval_curve(points, t);
            Pos2::new(rect.left() + t * rect.width(), rect.bottom() - v * rect.height())
        })
        .collect();
    painter.add(egui::Shape::line(curve_pts, Stroke::new(1.5, theme::GREEN)));

    // Draggable control points
    const INPUTS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
    for (i, (&in_v, out_v)) in INPUTS.iter().zip(points.iter_mut()).enumerate() {
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

        let col = if cp_resp.hovered() || cp_resp.dragged() {
            Color32::WHITE
        } else {
            theme::GREEN
        };
        painter.circle_filled(cp, 4.0, col);
        painter.circle_stroke(cp, 4.0, Stroke::new(1.0, theme::GREEN_DIM));
    }

    if ui.small_button("reset curve").clicked() {
        *points = [0.0, 0.25, 0.5, 0.75, 1.0];
    }
}

// ── Display helpers ───────────────────────────────────────────────────────────

/// Human-readable label for the input downscale dimension selector.
pub(super) fn input_dim_label(dim: u32) -> &'static str {
    match dim {
        0    => "full size",
        128  => "128px",
        256  => "256px",
        512  => "512px",
        1024 => "1024px",
        2048 => "2048px",
        _    => "custom",
    }
}

/// Compute the display size of a texture at the given zoom level.
pub(super) fn tex_display_size(tex: &TextureHandle, zoom: f32) -> Vec2 {
    let [tw, th] = tex.size();
    Vec2::new(tw as f32 * zoom, th as f32 * zoom)
}
