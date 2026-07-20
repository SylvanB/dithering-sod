//! Pre-processing effects pipeline.
//!
//! Effects are stacked in a [`Vec<Effect>`] and applied in order by
//! [`apply_effects`] before dithering or halftone rendering. Each [`Effect`]
//! wraps an [`EffectKind`] with an enabled toggle and a UI open/collapsed flag.

use image::{DynamicImage, RgbImage};
use serde::{Deserialize, Serialize};

use crate::filmstock::{curve_range, eval_stock_curve, PORTRA_400};

// ── Effect types ──────────────────────────────────────────────────────────────

/// The type and parameters of a single effect in the stack.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum EffectKind {
    BrightnessContrastGamma {
        brightness: f32, // -1.0 to 1.0
        contrast: f32,   // 0.1 to 4.0
        gamma: f32,      // 0.1 to 4.0
    },
    Posterize {
        levels: u32, // 2–16
    },
    Sharpen {
        amount: f32, // 0.0–4.0
    },
    Pixelate {
        block_size: u32, // 2–64
    },
    JpegGlitch {
        quality: u8, // 1–95
    },
    ChromaticAberration {
        x_shift: f32, // pixels, R shifts +, B shifts −
        y_shift: f32,
    },
    /// Tone curve: 5 output values at inputs [0, 0.25, 0.5, 0.75, 1.0]
    ToneCurve {
        points: [f32; 5],
    },
    /// CRT-style bloom glow: blurs the image and blends it back additively.
    CrtGlow {
        radius: u32,   // 1–32 pixels
        strength: f32, // 0.0–3.0 blend multiplier
    },
    Scanlines {
        spacing: u32, // 2–16 pixels
        opacity: f32, // 0.0–1.0
    },
    /// Physically-modeled film emulation (currently hardcoded to Kodak
    /// Portra 400 — see `crate::filmstock`): per-channel characteristic
    /// curve, density-dependent grain, and highlight halation.
    FilmEmulation {
        exposure_ev: f32,        // -2.0–2.0 stops, shifts input along the curve's log-exposure axis
        grain_strength: f32,     // 0.0–2.0 grain amplitude multiplier
        grain_size: f32,         // 0.5–3.0 grain "clump" radius multiplier
        halation_strength: f32,  // 0.0–2.0 bloom amplitude multiplier
        halation_radius: u32,    // 2–64 px bloom blur radius
        halation_threshold: f32, // 0.0–1.0 luminance above which halation blooms
    },
}

impl EffectKind {
    pub fn default_for_name(name: &str) -> Self {
        match name {
            "bcg"       => EffectKind::BrightnessContrastGamma { brightness: 0.0, contrast: 1.0, gamma: 1.0 },
            "posterize" => EffectKind::Posterize { levels: 4 },
            "sharpen"   => EffectKind::Sharpen { amount: 1.0 },
            "pixelate"  => EffectKind::Pixelate { block_size: 8 },
            "jpeg"      => EffectKind::JpegGlitch { quality: 20 },
            "chroma"    => EffectKind::ChromaticAberration { x_shift: 5.0, y_shift: 0.0 },
            "curve"     => EffectKind::ToneCurve { points: [0.0, 0.25, 0.5, 0.75, 1.0] },
            "crtglow"   => EffectKind::CrtGlow { radius: 8, strength: 0.6 },
            "scanlines" => EffectKind::Scanlines { spacing: 4, opacity: 0.7 },
            "film"      => EffectKind::FilmEmulation {
                exposure_ev: 0.0,
                grain_strength: 0.6,
                grain_size: 1.0,
                halation_strength: 0.5,
                halation_radius: 12,
                halation_threshold: 0.75,
            },
            _           => EffectKind::BrightnessContrastGamma { brightness: 0.0, contrast: 1.0, gamma: 1.0 },
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            EffectKind::BrightnessContrastGamma { .. } => "Brightness / Contrast / Gamma",
            EffectKind::Posterize { .. }               => "Posterize",
            EffectKind::Sharpen { .. }                 => "Sharpen",
            EffectKind::Pixelate { .. }                => "Pixelate",
            EffectKind::JpegGlitch { .. }              => "JPEG Glitch",
            EffectKind::ChromaticAberration { .. }     => "Chromatic Aberration",
            EffectKind::ToneCurve { .. }               => "Tone Curve",
            EffectKind::CrtGlow { .. }                 => "CRT Glow",
            EffectKind::Scanlines { .. }               => "Scanlines",
            EffectKind::FilmEmulation { .. }           => "Film Emulation (Portra 400)",
        }
    }
}

// Available effect types for the "Add Effect" menu
pub const EFFECT_NAMES: &[(&str, &str)] = &[
    ("bcg",       "Brightness / Contrast / Gamma"),
    ("curve",     "Tone Curve"),
    ("posterize", "Posterize"),
    ("sharpen",   "Sharpen"),
    ("pixelate",  "Pixelate"),
    ("jpeg",      "JPEG Glitch"),
    ("chroma",    "Chromatic Aberration"),
    ("crtglow",   "CRT Glow"),
    ("scanlines", "Scanlines"),
    ("film",      "Film Emulation (Portra 400)"),
];

// ── Effect entry (data + UI state) ────────────────────────────────────────────

/// A single entry in the effects stack.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub enabled: bool,
    /// Whether the effect's UI panel is expanded in the stack.
    pub open: bool,
}

impl Effect {
    pub fn new(kind: EffectKind) -> Self {
        Self { kind, enabled: true, open: true }
    }
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Apply every enabled effect in `effects` to `src` in order.
pub fn apply_effects(src: &DynamicImage, effects: &[Effect]) -> DynamicImage {
    let mut img = src.clone();
    for e in effects {
        if e.enabled {
            img = apply_one(&img, &e.kind);
        }
    }
    img
}

fn apply_one(img: &DynamicImage, kind: &EffectKind) -> DynamicImage {
    match *kind {
        EffectKind::BrightnessContrastGamma { brightness, contrast, gamma } => {
            bcg(img, brightness, contrast, gamma)
        }
        EffectKind::Posterize { levels }         => posterize(img, levels),
        EffectKind::Sharpen { amount }           => sharpen(img, amount),
        EffectKind::Pixelate { block_size }      => pixelate(img, block_size),
        EffectKind::JpegGlitch { quality }       => jpeg_glitch(img, quality),
        EffectKind::ChromaticAberration { x_shift, y_shift } => {
            chromatic_aberration(img, x_shift, y_shift)
        }
        EffectKind::ToneCurve { points } => tone_curve(img, &points),
        EffectKind::CrtGlow { radius, strength } => crt_glow(img, radius, strength),
        EffectKind::Scanlines { spacing, opacity } => scanlines(img, spacing, opacity),
        EffectKind::FilmEmulation {
            exposure_ev, grain_strength, grain_size,
            halation_strength, halation_radius, halation_threshold,
        } => film_emulation(
            img, exposure_ev, grain_strength, grain_size,
            halation_strength, halation_radius, halation_threshold,
        ),
    }
}

// ── Tone curve helpers (exported for UI) ──────────────────────────────────────

/// Evaluate piecewise-linear tone curve at input t ∈ [0,1].
/// control points are at inputs [0, 0.25, 0.5, 0.75, 1.0].
pub fn eval_curve(points: &[f32; 5], t: f32) -> f32 {
    const INPUTS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
    if t <= 0.0 { return points[0]; }
    if t >= 1.0 { return points[4]; }
    for i in 0..4 {
        if t <= INPUTS[i + 1] {
            let alpha = (t - INPUTS[i]) / (INPUTS[i + 1] - INPUTS[i]);
            return points[i] + alpha * (points[i + 1] - points[i]);
        }
    }
    points[4]
}

fn tone_curve(src: &DynamicImage, points: &[f32; 5]) -> DynamicImage {
    // Build 256-entry LUT once, then apply per-pixel
    let lut: Vec<u8> = (0u32..256)
        .map(|i| {
            let v = eval_curve(points, i as f32 / 255.0);
            (v.clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect();

    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut out = RgbImage::new(w, h);
    for (x, y, px) in rgb.enumerate_pixels() {
        out.put_pixel(x, y, image::Rgb([lut[px[0] as usize], lut[px[1] as usize], lut[px[2] as usize]]));
    }
    DynamicImage::ImageRgb8(out)
}

// ── Individual effects ────────────────────────────────────────────────────────

fn bcg(src: &DynamicImage, brightness: f32, contrast: f32, gamma: f32) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let inv_gamma = 1.0 / gamma.max(0.001);
    let mut out = RgbImage::new(w, h);
    for (x, y, px) in rgb.enumerate_pixels() {
        let apply = |v: u8| -> u8 {
            let mut f = v as f32 / 255.0;
            f = f.powf(inv_gamma);
            f = (f - 0.5) * contrast + 0.5 + brightness;
            (f.clamp(0.0, 1.0) * 255.0).round() as u8
        };
        out.put_pixel(x, y, image::Rgb([apply(px[0]), apply(px[1]), apply(px[2])]));
    }
    DynamicImage::ImageRgb8(out)
}

fn posterize(src: &DynamicImage, levels: u32) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let steps = (levels.max(2) - 1) as f32;
    let mut out = RgbImage::new(w, h);
    for (x, y, px) in rgb.enumerate_pixels() {
        let q = |v: u8| -> u8 {
            ((( v as f32 / 255.0) * steps).round() / steps * 255.0).clamp(0.0, 255.0) as u8
        };
        out.put_pixel(x, y, image::Rgb([q(px[0]), q(px[1]), q(px[2])]));
    }
    DynamicImage::ImageRgb8(out)
}

fn sharpen(src: &DynamicImage, amount: f32) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let blurred = box_blur_3x3(&rgb);
    let mut out = RgbImage::new(w, h);
    for (x, y, px) in rgb.enumerate_pixels() {
        let bl = blurred.get_pixel(x, y);
        let s = |orig: u8, blur: u8| -> u8 {
            ((orig as f32 + amount * (orig as f32 - blur as f32)).clamp(0.0, 255.0)) as u8
        };
        out.put_pixel(x, y, image::Rgb([s(px[0], bl[0]), s(px[1], bl[1]), s(px[2], bl[2])]));
    }
    DynamicImage::ImageRgb8(out)
}

fn box_blur_3x3(rgb: &RgbImage) -> RgbImage {
    let (w, h) = rgb.dimensions();
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (mut sr, mut sg, mut sb, mut cnt) = (0u32, 0u32, 0u32, 0u32);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
                    let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
                    let p = rgb.get_pixel(nx, ny);
                    sr += p[0] as u32; sg += p[1] as u32; sb += p[2] as u32;
                    cnt += 1;
                }
            }
            out.put_pixel(x, y, image::Rgb([(sr/cnt) as u8, (sg/cnt) as u8, (sb/cnt) as u8]));
        }
    }
    out
}

fn pixelate(src: &DynamicImage, block: u32) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let block = block.max(1);
    let mut out = RgbImage::new(w, h);
    let mut y = 0u32;
    while y < h {
        let mut x = 0u32;
        while x < w {
            let x1 = (x + block).min(w);
            let y1 = (y + block).min(h);
            let cnt = ((x1 - x) * (y1 - y)) as u32;
            let (mut sr, mut sg, mut sb) = (0u32, 0u32, 0u32);
            for py in y..y1 { for px in x..x1 {
                let p = rgb.get_pixel(px, py);
                sr += p[0] as u32; sg += p[1] as u32; sb += p[2] as u32;
            }}
            let avg = image::Rgb([(sr/cnt) as u8, (sg/cnt) as u8, (sb/cnt) as u8]);
            for py in y..y1 { for px in x..x1 { out.put_pixel(px, py, avg); }}
            x += block;
        }
        y += block;
    }
    DynamicImage::ImageRgb8(out)
}

fn jpeg_glitch(src: &DynamicImage, quality: u8) -> DynamicImage {
    use image::codecs::jpeg::JpegEncoder;
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut buf = Vec::new();
    {
        let mut enc = JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 95));
        if enc.encode(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8).is_err() {
            return src.clone();
        }
    }
    image::load_from_memory_with_format(&buf, image::ImageFormat::Jpeg)
        .unwrap_or_else(|_| src.clone())
}

/// Separable box blur with the given pixel radius (horizontal then vertical pass).
fn box_blur_r(rgb: &RgbImage, radius: u32) -> RgbImage {
    let (w, h) = rgb.dimensions();
    let r = radius.max(1) as i32;
    let diam = (2 * r + 1) as u32;

    // Horizontal pass
    let mut horiz = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (mut sr, mut sg, mut sb) = (0u32, 0u32, 0u32);
            for dx in -r..=r {
                let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
                let p = rgb.get_pixel(nx, y);
                sr += p[0] as u32; sg += p[1] as u32; sb += p[2] as u32;
            }
            horiz.put_pixel(x, y, image::Rgb([(sr/diam) as u8, (sg/diam) as u8, (sb/diam) as u8]));
        }
    }

    // Vertical pass
    let mut vert = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (mut sr, mut sg, mut sb) = (0u32, 0u32, 0u32);
            for dy in -r..=r {
                let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
                let p = horiz.get_pixel(x, ny);
                sr += p[0] as u32; sg += p[1] as u32; sb += p[2] as u32;
            }
            vert.put_pixel(x, y, image::Rgb([(sr/diam) as u8, (sg/diam) as u8, (sb/diam) as u8]));
        }
    }
    vert
}

/// CRT-style glow: blurs the image twice (approximate Gaussian) then adds
/// the result back onto the original with `strength` as the blend factor.
fn crt_glow(src: &DynamicImage, radius: u32, strength: f32) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();

    // Two box-blur passes ≈ Gaussian
    let blurred = box_blur_r(&box_blur_r(&rgb, radius), radius);

    let mut out = RgbImage::new(w, h);
    for (x, y, px) in rgb.enumerate_pixels() {
        let bl = blurred.get_pixel(x, y);
        let add = |orig: u8, glow: u8| -> u8 {
            (orig as f32 + strength * glow as f32).clamp(0.0, 255.0) as u8
        };
        out.put_pixel(x, y, image::Rgb([add(px[0], bl[0]), add(px[1], bl[1]), add(px[2], bl[2])]));
    }
    DynamicImage::ImageRgb8(out)
}

fn scanlines(src: &DynamicImage, spacing: u32, opacity: f32) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let spacing = spacing.max(2);
    let mut out = rgb.clone();
    for y in 0..h {
        if y % spacing == 0 {
            for x in 0..w {
                let p = out.get_pixel(x, y);
                let darken = |v: u8| -> u8 { (v as f32 * (1.0 - opacity)).round() as u8 };
                out.put_pixel(x, y, image::Rgb([darken(p[0]), darken(p[1]), darken(p[2])]));
            }
        }
    }
    DynamicImage::ImageRgb8(out)
}

fn chromatic_aberration(src: &DynamicImage, x_shift: f32, y_shift: f32) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let xs = x_shift.round() as i32;
    let ys = y_shift.round() as i32;
    let mut out = RgbImage::new(w, h);
    let sample = |x: u32, y: u32, dx: i32, dy: i32, ch: usize| -> u8 {
        let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
        let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
        rgb.get_pixel(nx, ny)[ch]
    };
    for y in 0..h {
        for x in 0..w {
            let r = sample(x, y, xs, ys, 0);          // R shifted +
            let g = rgb.get_pixel(x, y)[1];            // G unchanged
            let b = sample(x, y, -xs, -ys, 2);        // B shifted −
            out.put_pixel(x, y, image::Rgb([r, g, b]));
        }
    }
    DynamicImage::ImageRgb8(out)
}

// ── Film emulation (Portra 400) ────────────────────────────────────────────────
//
// Physically-modeled pipeline, in order:
//   1. Per-channel characteristic curve (density vs. log exposure) maps each
//      input channel through Portra 400's actual H&D response, independently
//      per channel — this is what produces the color-balance shift (warmth)
//      and gentle, straight-line contrast, rather than a hand-tuned tint.
//   2. Halation: scene brightness above `halation_threshold` blooms outward
//      with a red-dominant tint, modeling light scattering through the film
//      base and re-exposing the emulsion from behind.
//   3. Grain: spatially-correlated per-channel noise whose amplitude follows
//      Selwyn's law (RMS granularity ∝ sqrt(density)), so grain is
//      near-absent in Dmin (shadow) regions and strongest where density is
//      highest.
//
// See NOTES.md for what's derived from the Kodak Alaris E-4050 datasheet vs.
// approximated, and `crate::filmstock` for the underlying curve data.

const LOG10_2: f32 = 0.301_029_99;

#[allow(clippy::too_many_arguments)]
fn film_emulation(
    src: &DynamicImage,
    exposure_ev: f32,
    grain_strength: f32,
    grain_size: f32,
    halation_strength: f32,
    halation_radius: u32,
    halation_threshold: f32,
) -> DynamicImage {
    let stock = &PORTRA_400;
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let n = (w * h) as usize;

    let curves = [stock.curve_r, stock.curve_g, stock.curve_b];
    let ranges: Vec<(f32, f32)> = curves.iter().map(|c| curve_range(c)).collect();
    let log_span = stock.log_e_max - stock.log_e_min;
    // Input midtone (f=0.5) is anchored at the datasheet's sensitometric
    // reference exposure (Log H Ref) rather than an arbitrary domain edge,
    // so `exposure_ev` reads as stops of compensation around that reference.
    let ev_shift = exposure_ev * LOG10_2;

    // Pass 1: per-channel characteristic curve. Keeps each channel's density
    // normalized to 0..1 within its own Dmin..Dmax for grain scaling later,
    // and the pre-curve scene luminance as halation's bloom source.
    let mut curved = vec![0f32; n * 3];
    let mut density_frac = vec![0f32; n * 3];
    let mut luma_src = vec![0f32; n];

    for (i, px) in rgb.pixels().enumerate() {
        let mut luma = 0.0;
        for ch in 0..3 {
            let f = px[ch] as f32 / 255.0;
            luma += f;
            let log_e = stock.log_h_ref + (f - 0.5) * log_span + ev_shift;
            let d = eval_stock_curve(curves[ch], log_e);
            let (dmin, dmax) = ranges[ch];
            let frac = ((d - dmin) / (dmax - dmin)).clamp(0.0, 1.0);
            density_frac[i * 3 + ch] = frac;
            // Negative density inverts to a positive display value.
            curved[i * 3 + ch] = (1.0 - frac).clamp(0.0, 1.0);
        }
        luma_src[i] = luma / 3.0;
    }

    // Pass 2: halation. The bloom source is scene brightness *before* the
    // curve compresses it, since halation happens at the moment of exposure,
    // and it's thresholded so only genuinely bright highlights punch through
    // — mirroring the anti-halation backing suppressing all but the
    // strongest light.
    let denom = (1.0 - halation_threshold.min(0.98)).max(0.02);
    let bloom_src: Vec<f32> = luma_src
        .iter()
        .map(|&l| ((l - halation_threshold) / denom).clamp(0.0, 1.0))
        .collect();
    let bloom = box_blur_f32(&bloom_src, w, h, halation_radius.max(1) as i32);

    for i in 0..n {
        for ch in 0..3 {
            let add = halation_strength * bloom[i] * stock.halation_color[ch];
            curved[i * 3 + ch] = (curved[i * 3 + ch] + add).clamp(0.0, 1.0);
        }
    }

    // Pass 3: grain. Spatially-correlated Gaussian noise per channel, blurred
    // to the channel's characteristic grain radius, then rescaled to unit std
    // (box blur reduces variance; an empirical rescale keeps `grain_strength`
    // meaningful across grain sizes) and modulated by sqrt(local density) per
    // Selwyn's law.
    let mut out = RgbImage::new(w, h);
    for ch in 0..3usize {
        let raw: Vec<f32> = (0..n)
            .map(|i| {
                let x = (i as u32) % w;
                let y = (i as u32) / w;
                gaussian_noise(x, y, ch as u32)
            })
            .collect();
        let radius = (grain_size * stock.grain_radius[ch]).max(0.5).round() as i32;
        let mut blurred = box_blur_f32(&raw, w, h, radius);

        let mean: f32 = blurred.iter().sum::<f32>() / n as f32;
        let var: f32 = blurred.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n as f32;
        let std = var.sqrt().max(1e-6);
        for v in blurred.iter_mut() {
            *v = (*v - mean) / std;
        }

        const GRAIN_SCALE: f32 = 0.10;
        for (i, item) in blurred.iter().enumerate() {
            let frac = density_frac[i * 3 + ch];
            let g = item * grain_strength * stock.grain_weight[ch] * frac.sqrt() * GRAIN_SCALE;
            let v = (curved[i * 3 + ch] + g).clamp(0.0, 1.0);
            let px = out.get_pixel_mut(i as u32 % w, i as u32 / w);
            px[ch] = (v * 255.0).round() as u8;
        }
    }

    DynamicImage::ImageRgb8(out)
}

/// Separable box blur over a single-channel f32 buffer, clamp-to-edge.
fn box_blur_f32(buf: &[f32], w: u32, h: u32, radius: i32) -> Vec<f32> {
    let r = radius.max(1);
    let diam = (2 * r + 1) as f32;

    let mut horiz = vec![0f32; buf.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0f32;
            for dx in -r..=r {
                let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
                s += buf[(y * w + nx) as usize];
            }
            horiz[(y * w + x) as usize] = s / diam;
        }
    }

    let mut vert = vec![0f32; buf.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0f32;
            for dy in -r..=r {
                let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
                s += horiz[(ny * w + x) as usize];
            }
            vert[(y * w + x) as usize] = s / diam;
        }
    }
    vert
}

/// Deterministic hash-based pseudo-random unit-Gaussian sample for pixel
/// `(x, y)` on channel `ch` — avoids a `rand` crate dependency while staying
/// reproducible run-to-run (same input image always grains identically).
fn gaussian_noise(x: u32, y: u32, ch: u32) -> f32 {
    let seed = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263))
        .wrapping_add(ch.wrapping_mul(2_246_822_519));
    let u1 = rand01(seed).max(1e-6);
    let u2 = rand01(hash_u32(seed ^ 0x9E37_79B9));
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

fn hash_u32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

fn rand01(seed: u32) -> f32 {
    (hash_u32(seed) as f32) / (u32::MAX as f32)
}
