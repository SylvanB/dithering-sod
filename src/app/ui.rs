//! Pure UI helper functions for the dithering tool.
//!
//! These are free functions (not methods on [`super::DitheringApp`]) because
//! they don't need access to application state — they operate only on the
//! data types they receive as arguments. Keeping them here reduces the length
//! of `mod.rs` and makes them easy to test in isolation.

use egui::{Color32, Frame, Margin, Pos2, Rect, Rounding, Sense, Stroke, TextureHandle, Vec2};

use crate::effects::{eval_curve, EffectKind};

// ── Theme ─────────────────────────────────────────────────────────────────────

/// Centralised colour + spacing constants for the custom dark theme.
pub(super) mod theme {
    use egui::Color32;

    // Background layers (darkest → lightest)
    pub const BG_APP:      Color32 = Color32::from_rgb(13,  13,  20);
    pub const BG_PANEL:    Color32 = Color32::from_rgb(18,  18,  28);
    pub const BG_CARD:     Color32 = Color32::from_rgb(28,  28,  42);
    pub const BG_CARD_HI:  Color32 = Color32::from_rgb(38,  38,  58);

    // Borders
    pub const BORDER:      Color32 = Color32::from_rgb(48,  48,  72);
    pub const BORDER_SEP:  Color32 = Color32::from_rgb(34,  34,  52);

    // Accent (muted teal)
    pub const ACCENT:      Color32 = Color32::from_rgb(78, 201, 176);
    pub const ACCENT_DIM:  Color32 = Color32::from_rgb(42, 107,  94);

    // Text
    pub const TEXT_STRONG: Color32 = Color32::from_rgb(224, 224, 239);
    pub const TEXT_WEAK:   Color32 = Color32::from_rgb(110, 110, 145);

    // Top bar
    pub const BAR_FILL:    Color32 = Color32::from_rgb(20,  20,  32);
}

// ── Global visuals setup ──────────────────────────────────────────────────────

/// Apply the custom dark theme visuals. Call once from `DitheringApp::new`.
pub(super) fn setup_visuals(ctx: &egui::Context) {
    use egui::{FontId, Style, TextStyle, Visuals};

    let mut visuals = Visuals::dark();

    // Window / panel fills
    visuals.window_fill        = theme::BG_APP;
    visuals.panel_fill         = theme::BG_PANEL;
    visuals.faint_bg_color     = theme::BG_CARD;
    visuals.extreme_bg_color   = theme::BG_APP;
    visuals.code_bg_color      = theme::BG_CARD;
    visuals.hyperlink_color    = theme::ACCENT;
    visuals.override_text_color = Some(theme::TEXT_STRONG);

    // Selection highlight
    visuals.selection.bg_fill = theme::ACCENT_DIM;
    visuals.selection.stroke  = Stroke::new(1.0, theme::ACCENT);

    // Widget states — bg_fill, bg_stroke, fg_stroke, rounding, expansion
    let rounding = Rounding::same(5.0);
    let border   = Stroke::new(1.0, theme::BORDER);
    let fg       = Stroke::new(1.5, theme::TEXT_STRONG);

    visuals.widgets.noninteractive.bg_fill    = theme::BG_CARD;
    visuals.widgets.noninteractive.bg_stroke  = Stroke::new(1.0, theme::BORDER_SEP);
    visuals.widgets.noninteractive.fg_stroke  = Stroke::new(1.0, theme::TEXT_WEAK);
    visuals.widgets.noninteractive.rounding   = rounding;
    visuals.widgets.noninteractive.expansion  = 0.0;

    visuals.widgets.inactive.bg_fill   = theme::BG_CARD;
    visuals.widgets.inactive.bg_stroke = border;
    visuals.widgets.inactive.fg_stroke = fg;
    visuals.widgets.inactive.rounding  = rounding;
    visuals.widgets.inactive.expansion = 0.0;

    visuals.widgets.hovered.bg_fill   = theme::BG_CARD_HI;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, theme::ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, theme::ACCENT);
    visuals.widgets.hovered.rounding  = rounding;
    visuals.widgets.hovered.expansion = 1.0;

    visuals.widgets.active.bg_fill   = theme::ACCENT_DIM;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, theme::ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(2.0, theme::ACCENT);
    visuals.widgets.active.rounding  = rounding;
    visuals.widgets.active.expansion = 1.0;

    visuals.widgets.open.bg_fill   = theme::BG_CARD_HI;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, theme::ACCENT);
    visuals.widgets.open.fg_stroke = Stroke::new(1.5, theme::TEXT_STRONG);
    visuals.widgets.open.rounding  = rounding;
    visuals.widgets.open.expansion = 0.0;

    ctx.set_visuals(visuals);

    // Spacing / typography tweaks
    ctx.style_mut(|s: &mut Style| {
        s.spacing.button_padding  = egui::vec2(8.0, 4.0);
        s.spacing.item_spacing    = egui::vec2(6.0, 4.0);
        s.spacing.window_margin   = Margin::same(12.0);
        s.spacing.menu_margin     = Margin::same(6.0);
        s.text_styles.insert(
            TextStyle::Body,
            FontId::proportional(13.0),
        );
        s.text_styles.insert(
            TextStyle::Button,
            FontId::proportional(13.0),
        );
        s.text_styles.insert(
            TextStyle::Small,
            FontId::proportional(11.0),
        );
    });
}

// ── Card helpers ──────────────────────────────────────────────────────────────

/// Render `add_contents` inside a styled "card" frame with rounded corners,
/// a subtle border, and consistent inner padding.
pub(super) fn card<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    Frame::none()
        .fill(theme::BG_CARD)
        .rounding(Rounding::same(6.0))
        .stroke(Stroke::new(1.0, theme::BORDER))
        .inner_margin(Margin::same(10.0))
        .outer_margin(Margin { left: 0.0, right: 0.0, top: 0.0, bottom: 6.0 })
        .show(ui, add_contents)
}

/// A clickable section header row rendered directly with the painter so the
/// layout stays predictable inside a card.
pub(super) fn section_header(ui: &mut egui::Ui, label: &str, open: &mut bool) {
    let desired = egui::vec2(ui.available_width(), 22.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        if resp.hovered() || resp.is_pointer_button_down_on() {
            painter.rect_filled(rect, 4.0, theme::BG_CARD_HI);
        }

        let arrow = if *open { "▾" } else { "▸" };
        painter.text(
            Pos2::new(rect.left() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            arrow,
            egui::FontId::proportional(11.0),
            theme::ACCENT,
        );
        painter.text(
            Pos2::new(rect.left() + 24.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            theme::TEXT_STRONG,
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

    // Background — deep, darker than card
    painter.rect_filled(rect, 4.0, theme::BG_APP);

    // Grid lines
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let x = rect.left() + t * rect.width();
        let y = rect.top() + t * rect.height();
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(0.5, theme::BORDER_SEP),
        );
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(0.5, theme::BORDER_SEP),
        );
    }

    // Identity diagonal
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(0.5, theme::BORDER),
    );

    // Curve (128 samples) — accent colour
    let curve_pts: Vec<Pos2> = (0..=128)
        .map(|i| {
            let t = i as f32 / 128.0;
            let v = eval_curve(points, t);
            Pos2::new(rect.left() + t * rect.width(), rect.bottom() - v * rect.height())
        })
        .collect();
    painter.add(egui::Shape::line(curve_pts, Stroke::new(1.5, theme::ACCENT)));

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
            theme::TEXT_STRONG
        };
        painter.circle_filled(cp, 5.0, col);
        painter.circle_stroke(cp, 5.0, Stroke::new(1.0, theme::BORDER));
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
