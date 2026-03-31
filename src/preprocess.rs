//! Legacy pre-processing module.
//!
//! This module predates the composable [`crate::effects`] pipeline and is no
//! longer used by the app. It is kept to avoid breaking any external callers
//! and is allowed to be dead code.

#![allow(dead_code)]
use image::{DynamicImage, RgbImage};

#[derive(Clone, PartialEq, Debug)]
pub struct PreParams {
    pub brightness: f32,       // -1.0 to 1.0
    pub contrast: f32,         // 0.5 to 3.0
    pub gamma: f32,            // 0.1 to 4.0
    pub pixelate: u32,         // 1 = off, otherwise block size in pixels
    pub jpeg_enabled: bool,
    pub jpeg_quality: u8,      // 1–95
    pub sharpen_enabled: bool,
    pub sharpen_amount: f32,   // 0.0 to 3.0
    pub posterize_enabled: bool,
    pub posterize_levels: u32, // 2–16
}

impl Default for PreParams {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            pixelate: 1,
            jpeg_enabled: false,
            jpeg_quality: 20,
            sharpen_enabled: false,
            sharpen_amount: 1.0,
            posterize_enabled: false,
            posterize_levels: 4,
        }
    }
}

pub fn apply_preprocess(src: &DynamicImage, params: &PreParams) -> DynamicImage {
    let mut img = src.clone();

    // Posterize first — hard-step tones before anything else
    if params.posterize_enabled {
        img = posterize(&img, params.posterize_levels);
    }

    // Sharpen
    if params.sharpen_enabled && params.sharpen_amount > 0.0 {
        img = sharpen(&img, params.sharpen_amount);
    }

    // Pixelate — downsample then upsample
    if params.pixelate > 1 {
        img = pixelate(&img, params.pixelate);
    }

    // JPEG glitch — encode + decode at low quality
    if params.jpeg_enabled {
        img = jpeg_glitch(&img, params.jpeg_quality);
    }

    // Brightness / Contrast / Gamma on the float buffer
    let apply_bcg = params.brightness != 0.0
        || params.contrast != 1.0
        || (params.gamma - 1.0).abs() > 0.01;

    if apply_bcg {
        img = apply_brightness_contrast_gamma(&img, params.brightness, params.contrast, params.gamma);
    }

    img
}

// ── Individual effects ───────────────────────────────────────────────────────

fn apply_brightness_contrast_gamma(
    src: &DynamicImage,
    brightness: f32,
    contrast: f32,
    gamma: f32,
) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut out = RgbImage::new(w, h);
    let inv_gamma = 1.0 / gamma.max(0.001);

    for (x, y, px) in rgb.enumerate_pixels() {
        let apply = |v: u8| -> u8 {
            let mut f = v as f32 / 255.0;
            // Gamma
            f = f.powf(inv_gamma);
            // Contrast around midpoint
            f = (f - 0.5) * contrast + 0.5;
            // Brightness
            f += brightness;
            (f.clamp(0.0, 1.0) * 255.0).round() as u8
        };
        out.put_pixel(x, y, image::Rgb([apply(px[0]), apply(px[1]), apply(px[2])]));
    }

    DynamicImage::ImageRgb8(out)
}

fn pixelate(src: &DynamicImage, block: u32) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut out = RgbImage::new(w, h);

    let bx = block.max(1);
    let by = block.max(1);

    let mut y = 0u32;
    while y < h {
        let mut x = 0u32;
        while x < w {
            // Average pixels in this block
            let x1 = (x + bx).min(w);
            let y1 = (y + by).min(h);
            let count = ((x1 - x) * (y1 - y)) as u32;
            let (mut sr, mut sg, mut sb) = (0u32, 0u32, 0u32);
            for py in y..y1 {
                for px in x..x1 {
                    let p = rgb.get_pixel(px, py);
                    sr += p[0] as u32;
                    sg += p[1] as u32;
                    sb += p[2] as u32;
                }
            }
            let avg = image::Rgb([(sr / count) as u8, (sg / count) as u8, (sb / count) as u8]);
            for py in y..y1 {
                for px in x..x1 {
                    out.put_pixel(px, py, avg);
                }
            }
            x += bx;
        }
        y += by;
    }

    DynamicImage::ImageRgb8(out)
}

fn jpeg_glitch(src: &DynamicImage, quality: u8) -> DynamicImage {
    use image::codecs::jpeg::JpegEncoder;

    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut buf = Vec::new();
    {
        let mut encoder = JpegEncoder::new_with_quality(&mut buf, quality.max(1).min(95));
        if encoder.encode(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8).is_err() {
            return src.clone();
        }
    }
    image::load_from_memory_with_format(&buf, image::ImageFormat::Jpeg)
        .unwrap_or_else(|_| src.clone())
}

fn sharpen(src: &DynamicImage, amount: f32) -> DynamicImage {
    // Unsharp mask: out = original + amount * (original - blurred)
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();

    // Box blur with radius 1
    let blurred = box_blur_3x3(&rgb);

    let mut out = RgbImage::new(w, h);
    for (x, y, px) in rgb.enumerate_pixels() {
        let bl = blurred.get_pixel(x, y);
        let sharpen_channel = |orig: u8, blur: u8| -> u8 {
            let o = orig as f32;
            let b = blur as f32;
            let v = o + amount * (o - b);
            v.clamp(0.0, 255.0).round() as u8
        };
        out.put_pixel(x, y, image::Rgb([
            sharpen_channel(px[0], bl[0]),
            sharpen_channel(px[1], bl[1]),
            sharpen_channel(px[2], bl[2]),
        ]));
    }

    DynamicImage::ImageRgb8(out)
}

fn box_blur_3x3(rgb: &RgbImage) -> RgbImage {
    let (w, h) = rgb.dimensions();
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (mut sr, mut sg, mut sb, mut count) = (0u32, 0u32, 0u32, 0u32);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                        let p = rgb.get_pixel(nx as u32, ny as u32);
                        sr += p[0] as u32;
                        sg += p[1] as u32;
                        sb += p[2] as u32;
                        count += 1;
                    }
                }
            }
            out.put_pixel(x, y, image::Rgb([(sr / count) as u8, (sg / count) as u8, (sb / count) as u8]));
        }
    }
    out
}

fn posterize(src: &DynamicImage, levels: u32) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut out = RgbImage::new(w, h);
    let steps = (levels.max(2) - 1) as f32;

    for (x, y, px) in rgb.enumerate_pixels() {
        let quant = |v: u8| -> u8 {
            let f = v as f32 / 255.0;
            ((f * steps).round() / steps * 255.0).round().clamp(0.0, 255.0) as u8
        };
        out.put_pixel(x, y, image::Rgb([quant(px[0]), quant(px[1]), quant(px[2])]));
    }

    DynamicImage::ImageRgb8(out)
}
