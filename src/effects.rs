//! Pre-processing effects pipeline.
//!
//! Effects are stacked in a [`Vec<Effect>`] and applied in order by
//! [`apply_effects`] before dithering or halftone rendering. Each [`Effect`]
//! wraps an [`EffectKind`] with an enabled toggle and a UI open/collapsed flag.

use image::{DynamicImage, RgbImage};

// ── Effect types ──────────────────────────────────────────────────────────────

/// The type and parameters of a single effect in the stack.
#[derive(Clone, PartialEq, Debug)]
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
];

// ── Effect entry (data + UI state) ────────────────────────────────────────────

/// A single entry in the effects stack.
#[derive(Clone, Debug)]
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
        EffectKind::ToneCurve { ref points } => tone_curve(img, points),
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
