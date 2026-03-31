//! Pure UI helper functions for the dithering tool.
//!
//! These are free functions (not methods on [`super::DitheringApp`]) because
//! they don't need access to application state — they operate only on the
//! data types they receive as arguments. Keeping them here reduces the length
//! of `mod.rs` and makes them easy to test in isolation.

use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};

use crate::effects::{eval_curve, EffectKind};

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
                    .custom_formatter(|v, _| format!("{v:.2}×")),
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
                    .custom_formatter(|v, _| format!("{v:.2}×")),
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
    }
}

// ── Tone curve widget ─────────────────────────────────────────────────────────

/// Interactive tone-curve editor. Renders a square canvas with a draggable
/// piecewise-linear curve over five evenly-spaced control points.
fn tone_curve_widget(ui: &mut egui::Ui, points: &mut [f32; 5]) {
    let size = ui.available_width().min(180.0);
    let (response, painter) = ui.allocate_painter(Vec2::splat(size), Sense::hover());
    let rect = response.rect;

    // Background
    painter.rect_filled(rect, 4.0, Color32::from_gray(25));

    // Grid lines
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let x = rect.left() + t * rect.width();
        let y = rect.top() + t * rect.height();
        let dim = Color32::from_gray(50);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(0.5, dim),
        );
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(0.5, dim),
        );
    }

    // Identity diagonal
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(0.5, Color32::from_gray(70)),
    );

    // Curve (128 samples)
    let curve_pts: Vec<Pos2> = (0..=128)
        .map(|i| {
            let t = i as f32 / 128.0;
            let v = eval_curve(points, t);
            Pos2::new(rect.left() + t * rect.width(), rect.bottom() - v * rect.height())
        })
        .collect();
    painter.add(egui::Shape::line(curve_pts, Stroke::new(1.5, Color32::WHITE)));

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
            Color32::YELLOW
        } else {
            Color32::WHITE
        };
        painter.circle_filled(cp, 5.0, col);
        painter.circle_stroke(cp, 5.0, Stroke::new(1.0, Color32::from_gray(80)));
    }

    if ui.small_button("Reset curve").clicked() {
        *points = [0.0, 0.25, 0.5, 0.75, 1.0];
    }
}

// ── Display helpers ───────────────────────────────────────────────────────────

/// Human-readable label for the input downscale dimension selector.
pub(super) fn input_dim_label(dim: u32) -> &'static str {
    match dim {
        0 => "Full size",
        128 => "128px",
        256 => "256px",
        512 => "512px",
        1024 => "1024px",
        2048 => "2048px",
        _ => "Custom",
    }
}

/// Compute the display size of a texture at the given zoom level.
pub(super) fn tex_display_size(tex: &TextureHandle, zoom: f32) -> Vec2 {
    let [tw, th] = tex.size();
    Vec2::new(tw as f32 * zoom, th as f32 * zoom)
}

