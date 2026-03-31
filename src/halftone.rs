//! Halftone rendering.
//!
//! Converts an image into a halftone screen by sampling average tone in a
//! rotated grid, then drawing a scaled dot at each grid center whose size
//! is proportional to the local tone value.
//!
//! Two modes are supported:
//! - **Grayscale** — single screen at a configurable angle
//! - **CMYK** — four screens at standard print angles (C=105°, M=75°, Y=90°, K=45°)
//!   composited subtractively onto a white background

use image::{DynamicImage, RgbImage};

/// Shape of each halftone dot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DotShape {
    Circle,
    Square,
    Diamond,
    Line,
}

impl DotShape {
    pub fn all() -> &'static [DotShape] {
        &[DotShape::Circle, DotShape::Square, DotShape::Diamond, DotShape::Line]
    }
    pub fn name(&self) -> &'static str {
        match self {
            DotShape::Circle  => "Circle",
            DotShape::Square  => "Square",
            DotShape::Diamond => "Diamond",
            DotShape::Line    => "Line",
        }
    }
}

/// Parameters for halftone rendering.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HalftoneParams {
    /// Grid cell size in pixels (controls dot spacing / "lpi").
    pub cell_size: f32,
    pub dot_shape: DotShape,
    /// Screen angle in degrees; only used in grayscale mode.
    pub angle: f32,
    /// Scales the dot radius relative to the cell. 1.0 means the dot just
    /// fills the cell at 100% coverage; values >1.0 allow dots to overlap.
    pub dot_scale: f32,
    /// If true, render white dots on black instead of black on white.
    pub invert: bool,
    /// Use four CMYK screens instead of a single grayscale screen.
    pub cmyk: bool,
}

impl Default for HalftoneParams {
    fn default() -> Self {
        Self {
            cell_size: 8.0,
            dot_shape: DotShape::Circle,
            angle: 45.0,
            dot_scale: 1.0,
            invert: false,
            cmyk: false,
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Render `src` as a halftone image according to `params`.
pub fn apply_halftone(src: &DynamicImage, params: &HalftoneParams) -> DynamicImage {
    if params.cmyk {
        halftone_cmyk(src, params)
    } else {
        halftone_gray(src, params)
    }
}

// ── Grayscale halftone ────────────────────────────────────────────────────────

fn halftone_gray(src: &DynamicImage, params: &HalftoneParams) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let cell = params.cell_size;
    let angle = params.angle.to_radians();
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    let mut out = RgbImage::new(w, h);

    for y in 0..h {
        for x in 0..w {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            // Pixel coords in rotated grid space
            let u = px * cos_a + py * sin_a;
            let v = -px * sin_a + py * cos_a;

            // Nearest grid center in rotated space
            let gu = (u / cell).round() * cell;
            let gv = (v / cell).round() * cell;

            // Grid center back in image space
            let gx = (gu * cos_a - gv * sin_a).clamp(0.0, (w - 1) as f32) as u32;
            let gy = (gu * sin_a + gv * cos_a).clamp(0.0, (h - 1) as f32) as u32;

            // Sample average luma in a small neighborhood around the grid center
            let luma = sample_luma_avg(&rgb, gx, gy, (cell * 0.4) as u32);

            let coverage = if params.invert { 1.0 - luma } else { luma };
            let half = cell * 0.5;
            let dot_r = coverage.sqrt() * half * params.dot_scale;

            let dx = px - (gx as f32 + 0.5);
            let dy = py - (gy as f32 + 0.5);

            let in_dot = dot_metric(dx, dy, &params.dot_shape) <= dot_r;

            let v = if in_dot ^ params.invert { 0u8 } else { 255u8 };
            out.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }

    DynamicImage::ImageRgb8(out)
}

// ── CMYK halftone ─────────────────────────────────────────────────────────────

// Standard print halftone angles: K=45°, C=105°, M=75°, Y=90°
const CMYK_ANGLES: [f32; 4] = [105.0, 75.0, 90.0, 45.0]; // C, M, Y, K

fn halftone_cmyk(src: &DynamicImage, params: &HalftoneParams) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let cell = params.cell_size;

    // Precompute trig for each channel
    let trig: Vec<(f32, f32)> = CMYK_ANGLES
        .iter()
        .map(|&a| {
            let r = a.to_radians();
            (r.cos(), r.sin())
        })
        .collect();

    let mut out = RgbImage::new(w, h);

    for y in 0..h {
        for x in 0..w {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let mut in_dot = [false; 4]; // C, M, Y, K

            for ch in 0..4 {
                let (cos_a, sin_a) = trig[ch];

                let u = px * cos_a + py * sin_a;
                let v = -px * sin_a + py * cos_a;

                let gu = (u / cell).round() * cell;
                let gv = (v / cell).round() * cell;

                let gx = (gu * cos_a - gv * sin_a).clamp(0.0, (w - 1) as f32) as u32;
                let gy = (gu * sin_a + gv * cos_a).clamp(0.0, (h - 1) as f32) as u32;

                // Sample CMYK at grid center
                let [c, m, yy, k] = sample_cmyk_avg(&rgb, gx, gy, (cell * 0.4) as u32);
                let ch_val = [c, m, yy, k][ch];

                let half = cell * 0.5;
                let dot_r = ch_val.sqrt() * half * params.dot_scale;

                let dx = px - (gx as f32 + 0.5);
                let dy = py - (gy as f32 + 0.5);

                in_dot[ch] = dot_metric(dx, dy, &params.dot_shape) <= dot_r;
            }

            // Subtractive CMYK → RGB on white paper
            // R = (1 - C) × (1 - K)
            // G = (1 - M) × (1 - K)
            // B = (1 - Y) × (1 - K)
            let k = if in_dot[3] { 1.0f32 } else { 0.0 };
            let r = (if in_dot[0] { 0.0f32 } else { 1.0 }) * (1.0 - k);
            let g = (if in_dot[1] { 0.0f32 } else { 1.0 }) * (1.0 - k);
            let b = (if in_dot[2] { 0.0f32 } else { 1.0 }) * (1.0 - k);

            out.put_pixel(
                x, y,
                image::Rgb([(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]),
            );
        }
    }

    DynamicImage::ImageRgb8(out)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Distance metric for dot shape. Returns a value comparable to `dot_r`.
#[inline]
fn dot_metric(dx: f32, dy: f32, shape: &DotShape) -> f32 {
    match shape {
        DotShape::Circle  => (dx * dx + dy * dy).sqrt(),
        DotShape::Square  => dx.abs().max(dy.abs()),
        DotShape::Diamond => (dx.abs() + dy.abs()) * std::f32::consts::FRAC_1_SQRT_2,
        DotShape::Line    => dy.abs(), // horizontal lines — width varies with coverage
    }
}

/// Average luma in a square neighborhood around (cx, cy) with radius `r`.
fn sample_luma_avg(img: &image::RgbImage, cx: u32, cy: u32, r: u32) -> f32 {
    let (w, h) = img.dimensions();
    let r = r.max(1);
    let x0 = cx.saturating_sub(r);
    let y0 = cy.saturating_sub(r);
    let x1 = (cx + r).min(w - 1);
    let y1 = (cy + r).min(h - 1);
    let mut sum = 0.0f32;
    let mut count = 0u32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = img.get_pixel(x, y);
            sum += luma_f(p[0], p[1], p[2]);
            count += 1;
        }
    }
    if count == 0 { 0.5 } else { sum / count as f32 }
}

/// Average CMYK in a neighborhood around (cx, cy) with radius `r`.
fn sample_cmyk_avg(img: &image::RgbImage, cx: u32, cy: u32, r: u32) -> [f32; 4] {
    let (w, h) = img.dimensions();
    let r = r.max(1);
    let x0 = cx.saturating_sub(r);
    let y0 = cy.saturating_sub(r);
    let x1 = (cx + r).min(w - 1);
    let y1 = (cy + r).min(h - 1);
    let mut sum = [0.0f32; 4];
    let mut count = 0u32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = img.get_pixel(x, y);
            let cmyk = rgb_to_cmyk(p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
            sum[0] += cmyk[0];
            sum[1] += cmyk[1];
            sum[2] += cmyk[2];
            sum[3] += cmyk[3];
            count += 1;
        }
    }
    if count == 0 {
        return [0.0; 4];
    }
    [sum[0] / count as f32, sum[1] / count as f32, sum[2] / count as f32, sum[3] / count as f32]
}

#[inline]
fn luma_f(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * r as f32 / 255.0 + 0.7152 * g as f32 / 255.0 + 0.0722 * b as f32 / 255.0
}

#[inline]
fn rgb_to_cmyk(r: f32, g: f32, b: f32) -> [f32; 4] {
    let k = 1.0 - r.max(g).max(b);
    if k >= 1.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let d = 1.0 - k;
    [(1.0 - r - k) / d, (1.0 - g - k) / d, (1.0 - b - k) / d, k]
}
