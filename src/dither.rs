//! Dithering algorithms.
//!
//! The public entry point is [`apply_dither`], which dispatches to one of:
//! - **Threshold** — hard cutoff with adjustable bias
//! - **Ordered (Bayer)** — threshold matrix dithering at 2×2, 4×4, 8×8, or 16×16
//! - **White Noise** — deterministic per-pixel hash noise
//! - **Error diffusion** — a family of kernels (Floyd-Steinberg, Atkinson, etc.)
//!   with optional serpentine scanning
//!
//! All algorithms quantize each pixel to the nearest color in the supplied
//! [`Palette`] using a perceptually-weighted Euclidean distance.

use crate::palette::Palette;
use image::{DynamicImage, RgbImage};
use serde::{Deserialize, Serialize};

// ── Bayer matrices ──────────────────────────────────────────────────────────

#[rustfmt::skip]
const BAYER_2: [[f32; 2]; 2] = [
    [0.0/4.0,  2.0/4.0],
    [3.0/4.0,  1.0/4.0],
];

#[rustfmt::skip]
const BAYER_4: [[f32; 4]; 4] = [
    [ 0.0/16.0,  8.0/16.0,  2.0/16.0, 10.0/16.0],
    [12.0/16.0,  4.0/16.0, 14.0/16.0,  6.0/16.0],
    [ 3.0/16.0, 11.0/16.0,  1.0/16.0,  9.0/16.0],
    [15.0/16.0,  7.0/16.0, 13.0/16.0,  5.0/16.0],
];

#[rustfmt::skip]
const BAYER_8: [[f32; 8]; 8] = [
    [ 0.0/64.0, 32.0/64.0,  8.0/64.0, 40.0/64.0,  2.0/64.0, 34.0/64.0, 10.0/64.0, 42.0/64.0],
    [48.0/64.0, 16.0/64.0, 56.0/64.0, 24.0/64.0, 50.0/64.0, 18.0/64.0, 58.0/64.0, 26.0/64.0],
    [12.0/64.0, 44.0/64.0,  4.0/64.0, 36.0/64.0, 14.0/64.0, 46.0/64.0,  6.0/64.0, 38.0/64.0],
    [60.0/64.0, 28.0/64.0, 52.0/64.0, 20.0/64.0, 62.0/64.0, 30.0/64.0, 54.0/64.0, 22.0/64.0],
    [ 3.0/64.0, 35.0/64.0, 11.0/64.0, 43.0/64.0,  1.0/64.0, 33.0/64.0,  9.0/64.0, 41.0/64.0],
    [51.0/64.0, 19.0/64.0, 59.0/64.0, 27.0/64.0, 49.0/64.0, 17.0/64.0, 57.0/64.0, 25.0/64.0],
    [15.0/64.0, 47.0/64.0,  7.0/64.0, 39.0/64.0, 13.0/64.0, 45.0/64.0,  5.0/64.0, 37.0/64.0],
    [63.0/64.0, 31.0/64.0, 55.0/64.0, 23.0/64.0, 61.0/64.0, 29.0/64.0, 53.0/64.0, 21.0/64.0],
];

#[rustfmt::skip]
const BAYER_16: [[f32; 16]; 16] = [
    [  0.0/256.0, 128.0/256.0,  32.0/256.0, 160.0/256.0,   8.0/256.0, 136.0/256.0,  40.0/256.0, 168.0/256.0,   2.0/256.0, 130.0/256.0,  34.0/256.0, 162.0/256.0,  10.0/256.0, 138.0/256.0,  42.0/256.0, 170.0/256.0],
    [192.0/256.0,  64.0/256.0, 224.0/256.0,  96.0/256.0, 200.0/256.0,  72.0/256.0, 232.0/256.0, 104.0/256.0, 194.0/256.0,  66.0/256.0, 226.0/256.0,  98.0/256.0, 202.0/256.0,  74.0/256.0, 234.0/256.0, 106.0/256.0],
    [ 48.0/256.0, 176.0/256.0,  16.0/256.0, 144.0/256.0,  56.0/256.0, 184.0/256.0,  24.0/256.0, 152.0/256.0,  50.0/256.0, 178.0/256.0,  18.0/256.0, 146.0/256.0,  58.0/256.0, 186.0/256.0,  26.0/256.0, 154.0/256.0],
    [240.0/256.0, 112.0/256.0, 208.0/256.0,  80.0/256.0, 248.0/256.0, 120.0/256.0, 216.0/256.0,  88.0/256.0, 242.0/256.0, 114.0/256.0, 210.0/256.0,  82.0/256.0, 250.0/256.0, 122.0/256.0, 218.0/256.0,  90.0/256.0],
    [ 12.0/256.0, 140.0/256.0,  44.0/256.0, 172.0/256.0,   4.0/256.0, 132.0/256.0,  36.0/256.0, 164.0/256.0,  14.0/256.0, 142.0/256.0,  46.0/256.0, 174.0/256.0,   6.0/256.0, 134.0/256.0,  38.0/256.0, 166.0/256.0],
    [204.0/256.0,  76.0/256.0, 236.0/256.0, 108.0/256.0, 196.0/256.0,  68.0/256.0, 228.0/256.0, 100.0/256.0, 206.0/256.0,  78.0/256.0, 238.0/256.0, 110.0/256.0, 198.0/256.0,  70.0/256.0, 230.0/256.0, 102.0/256.0],
    [ 60.0/256.0, 188.0/256.0,  28.0/256.0, 156.0/256.0,  52.0/256.0, 180.0/256.0,  20.0/256.0, 148.0/256.0,  62.0/256.0, 190.0/256.0,  30.0/256.0, 158.0/256.0,  54.0/256.0, 182.0/256.0,  22.0/256.0, 150.0/256.0],
    [252.0/256.0, 124.0/256.0, 220.0/256.0,  92.0/256.0, 244.0/256.0, 116.0/256.0, 212.0/256.0,  84.0/256.0, 254.0/256.0, 126.0/256.0, 222.0/256.0,  94.0/256.0, 246.0/256.0, 118.0/256.0, 214.0/256.0,  86.0/256.0],
    [  3.0/256.0, 131.0/256.0,  35.0/256.0, 163.0/256.0,  11.0/256.0, 139.0/256.0,  43.0/256.0, 171.0/256.0,   1.0/256.0, 129.0/256.0,  33.0/256.0, 161.0/256.0,   9.0/256.0, 137.0/256.0,  41.0/256.0, 169.0/256.0],
    [195.0/256.0,  67.0/256.0, 227.0/256.0,  99.0/256.0, 203.0/256.0,  75.0/256.0, 235.0/256.0, 107.0/256.0, 193.0/256.0,  65.0/256.0, 225.0/256.0,  97.0/256.0, 201.0/256.0,  73.0/256.0, 233.0/256.0, 105.0/256.0],
    [ 51.0/256.0, 179.0/256.0,  19.0/256.0, 147.0/256.0,  59.0/256.0, 187.0/256.0,  27.0/256.0, 155.0/256.0,  49.0/256.0, 177.0/256.0,  17.0/256.0, 145.0/256.0,  57.0/256.0, 185.0/256.0,  25.0/256.0, 153.0/256.0],
    [243.0/256.0, 115.0/256.0, 211.0/256.0,  83.0/256.0, 251.0/256.0, 123.0/256.0, 219.0/256.0,  91.0/256.0, 241.0/256.0, 113.0/256.0, 209.0/256.0,  81.0/256.0, 249.0/256.0, 121.0/256.0, 217.0/256.0,  89.0/256.0],
    [ 15.0/256.0, 143.0/256.0,  47.0/256.0, 175.0/256.0,   7.0/256.0, 135.0/256.0,  39.0/256.0, 167.0/256.0,  13.0/256.0, 141.0/256.0,  45.0/256.0, 173.0/256.0,   5.0/256.0, 133.0/256.0,  37.0/256.0, 165.0/256.0],
    [207.0/256.0,  79.0/256.0, 239.0/256.0, 111.0/256.0, 199.0/256.0,  71.0/256.0, 231.0/256.0, 103.0/256.0, 205.0/256.0,  77.0/256.0, 237.0/256.0, 109.0/256.0, 197.0/256.0,  69.0/256.0, 229.0/256.0, 101.0/256.0],
    [ 63.0/256.0, 191.0/256.0,  31.0/256.0, 159.0/256.0,  55.0/256.0, 183.0/256.0,  23.0/256.0, 151.0/256.0,  61.0/256.0, 189.0/256.0,  29.0/256.0, 157.0/256.0,  53.0/256.0, 181.0/256.0,  21.0/256.0, 149.0/256.0],
    [255.0/256.0, 127.0/256.0, 223.0/256.0,  95.0/256.0, 247.0/256.0, 119.0/256.0, 215.0/256.0,  87.0/256.0, 253.0/256.0, 125.0/256.0, 221.0/256.0,  93.0/256.0, 245.0/256.0, 117.0/256.0, 213.0/256.0,  85.0/256.0],
];

// ── Types ────────────────────────────────────────────────────────────────────

/// Dithering algorithm to apply.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Algorithm {
    Threshold,
    Ordered,
    WhiteNoise,
    FloydSteinberg,
    Atkinson,
    Burkes,
    JarvisJudiceNinke,
    Stucki,
    Sierra,
    SierraLite,
    SierraTwoRow,
    ShiauFan,
}

impl Algorithm {
    pub fn all() -> &'static [Algorithm] {
        &[
            Algorithm::Threshold,
            Algorithm::Ordered,
            Algorithm::WhiteNoise,
            Algorithm::FloydSteinberg,
            Algorithm::Atkinson,
            Algorithm::Burkes,
            Algorithm::JarvisJudiceNinke,
            Algorithm::Stucki,
            Algorithm::Sierra,
            Algorithm::SierraLite,
            Algorithm::SierraTwoRow,
            Algorithm::ShiauFan,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Algorithm::Threshold => "Threshold",
            Algorithm::Ordered => "Ordered (Bayer)",
            Algorithm::WhiteNoise => "White Noise",
            Algorithm::FloydSteinberg => "Floyd-Steinberg",
            Algorithm::Atkinson => "Atkinson",
            Algorithm::Burkes => "Burkes",
            Algorithm::JarvisJudiceNinke => "Jarvis-Judice-Ninke",
            Algorithm::Stucki => "Stucki",
            Algorithm::Sierra => "Sierra",
            Algorithm::SierraLite => "Sierra Lite",
            Algorithm::SierraTwoRow => "Sierra Two-Row",
            Algorithm::ShiauFan => "Shiau-Fan",
        }
    }

    pub fn needs_bayer_size(&self) -> bool {
        matches!(self, Algorithm::Ordered)
    }

    pub fn needs_serpentine(&self) -> bool {
        matches!(
            self,
            Algorithm::FloydSteinberg
                | Algorithm::Atkinson
                | Algorithm::Burkes
                | Algorithm::JarvisJudiceNinke
                | Algorithm::Stucki
                | Algorithm::Sierra
                | Algorithm::SierraLite
                | Algorithm::SierraTwoRow
                | Algorithm::ShiauFan
        )
    }

    pub fn needs_noise_scale(&self) -> bool {
        matches!(self, Algorithm::WhiteNoise)
    }
}

/// Ordered-dithering matrix size.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BayerSize {
    B2,
    B4,
    B8,
    B16,
}

impl BayerSize {
    pub fn name(&self) -> &'static str {
        match self {
            BayerSize::B2 => "2×2",
            BayerSize::B4 => "4×4",
            BayerSize::B8 => "8×8",
            BayerSize::B16 => "16×16",
        }
    }
    pub fn all() -> &'static [BayerSize] {
        &[BayerSize::B2, BayerSize::B4, BayerSize::B8, BayerSize::B16]
    }
}

/// Parameters controlling which dithering algorithm is applied and how.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct DitherParams {
    pub algorithm: Algorithm,
    /// Bias applied before quantization in Threshold mode (0.0 = midpoint).
    pub threshold: f32,
    pub bayer_size: BayerSize,
    /// Amplitude of the Bayer offset; 1.0 fills the full ±0.5 range.
    pub bayer_spread: f32,
    /// Amplitude of the white-noise offset; 1.0 fills the full ±0.5 range.
    pub noise_scale: f32,
    /// Alternate scan direction each row to reduce directional banding (error diffusion only).
    pub serpentine: bool,
}

impl Default for DitherParams {
    fn default() -> Self {
        Self {
            algorithm: Algorithm::FloydSteinberg,
            threshold: 0.5,
            bayer_size: BayerSize::B4,
            bayer_spread: 1.0,
            noise_scale: 1.0,
            serpentine: true,
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

#[inline]
fn u8_to_f(v: u8) -> f32 {
    v as f32 / 255.0
}
#[inline]
fn f_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Deterministic per-pixel hash noise in [0, 1]
#[inline]
fn pixel_hash(x: usize, y: usize) -> f32 {
    let mut h = (x as u64).wrapping_mul(2654435761) ^ (y as u64).wrapping_mul(2246822519);
    h ^= h >> 16;
    h = h.wrapping_mul(0x45d9f3b57ad8b0c1);
    h ^= h >> 16;
    (h & 0xFFFF) as f32 / 65535.0
}

// ── Main entry point ─────────────────────────────────────────────────────────

/// Apply the dithering algorithm described by `params` to `src`, quantizing
/// each output pixel to the nearest color in `palette`.
pub fn apply_dither(src: &DynamicImage, params: &DitherParams, palette: &Palette) -> DynamicImage {
    let rgb = src.to_rgb8();
    let (width, height) = rgb.dimensions();
    let w = width as usize;
    let h = height as usize;

    // Working buffer: one [f32; 3] per pixel, layout [y * w + x]
    let mut buf: Vec<[f32; 3]> = rgb
        .pixels()
        .map(|p| [u8_to_f(p[0]), u8_to_f(p[1]), u8_to_f(p[2])])
        .collect();

    match &params.algorithm {
        Algorithm::Threshold => dither_threshold(&mut buf, w, h, params, palette),
        Algorithm::Ordered => dither_ordered(&mut buf, w, h, params, palette),
        Algorithm::WhiteNoise => dither_white_noise(&mut buf, w, h, params, palette),
        Algorithm::FloydSteinberg => {
            dither_error_diffusion(&mut buf, w, h, params, palette, &FLOYD_STEINBERG)
        }
        Algorithm::Atkinson => dither_error_diffusion(&mut buf, w, h, params, palette, &ATKINSON),
        Algorithm::Burkes => dither_error_diffusion(&mut buf, w, h, params, palette, &BURKES),
        Algorithm::JarvisJudiceNinke => {
            dither_error_diffusion(&mut buf, w, h, params, palette, &JJN)
        }
        Algorithm::Stucki => dither_error_diffusion(&mut buf, w, h, params, palette, &STUCKI),
        Algorithm::Sierra => dither_error_diffusion(&mut buf, w, h, params, palette, &SIERRA),
        Algorithm::SierraLite => {
            dither_error_diffusion(&mut buf, w, h, params, palette, &SIERRA_LITE)
        }
        Algorithm::SierraTwoRow => {
            dither_error_diffusion(&mut buf, w, h, params, palette, &SIERRA_TWO_ROW)
        }
        Algorithm::ShiauFan => dither_error_diffusion(&mut buf, w, h, params, palette, &SHIAU_FAN),
    }

    // Reconstruct
    let mut out = RgbImage::new(width, height);
    for (i, px) in buf.iter().enumerate() {
        let x = (i % w) as u32;
        let y = (i / w) as u32;
        out.put_pixel(
            x,
            y,
            image::Rgb([f_to_u8(px[0]), f_to_u8(px[1]), f_to_u8(px[2])]),
        );
    }

    DynamicImage::ImageRgb8(out)
}

// ── Threshold ────────────────────────────────────────────────────────────────

fn dither_threshold(
    buf: &mut [[f32; 3]],
    w: usize,
    h: usize,
    params: &DitherParams,
    palette: &Palette,
) {
    let bias = params.threshold - 0.5;
    for y in 0..h {
        for x in 0..w {
            let px = &mut buf[y * w + x];
            let r = (px[0] - bias).clamp(0.0, 1.0);
            let g = (px[1] - bias).clamp(0.0, 1.0);
            let b = (px[2] - bias).clamp(0.0, 1.0);
            let c = palette.nearest(r, g, b);
            *px = c;
        }
    }
}

// ── Ordered (Bayer) ──────────────────────────────────────────────────────────

fn dither_ordered(
    buf: &mut [[f32; 3]],
    w: usize,
    h: usize,
    params: &DitherParams,
    palette: &Palette,
) {
    let spread = params.bayer_spread;
    for y in 0..h {
        for x in 0..w {
            let noise = bayer_value(x, y, &params.bayer_size);
            let offset = (noise - 0.5) * spread;
            let px = &mut buf[y * w + x];
            let r = (px[0] + offset).clamp(0.0, 1.0);
            let g = (px[1] + offset).clamp(0.0, 1.0);
            let b = (px[2] + offset).clamp(0.0, 1.0);
            *px = palette.nearest(r, g, b);
        }
    }
}

#[inline]
fn bayer_value(x: usize, y: usize, size: &BayerSize) -> f32 {
    match size {
        BayerSize::B2 => BAYER_2[y % 2][x % 2],
        BayerSize::B4 => BAYER_4[y % 4][x % 4],
        BayerSize::B8 => BAYER_8[y % 8][x % 8],
        BayerSize::B16 => BAYER_16[y % 16][x % 16],
    }
}

// ── White noise ──────────────────────────────────────────────────────────────

fn dither_white_noise(
    buf: &mut [[f32; 3]],
    w: usize,
    h: usize,
    params: &DitherParams,
    palette: &Palette,
) {
    let scale = params.noise_scale;
    for y in 0..h {
        for x in 0..w {
            let noise = pixel_hash(x, y);
            let offset = (noise - 0.5) * scale;
            let px = &mut buf[y * w + x];
            let r = (px[0] + offset).clamp(0.0, 1.0);
            let g = (px[1] + offset).clamp(0.0, 1.0);
            let b = (px[2] + offset).clamp(0.0, 1.0);
            *px = palette.nearest(r, g, b);
        }
    }
}

// ── Error diffusion kernels ───────────────────────────────────────────────────

/// An error-diffusion kernel defined as a list of `(dx, dy, weight)` offsets
/// relative to the current pixel. Weights should sum to 1.0.
struct DiffusionKernel {
    entries: &'static [(i32, i32, f32)],
}

const FLOYD_STEINBERG: DiffusionKernel = DiffusionKernel {
    entries: &[
        (1, 0, 7.0 / 16.0),
        (-1, 1, 3.0 / 16.0),
        (0, 1, 5.0 / 16.0),
        (1, 1, 1.0 / 16.0),
    ],
};

const ATKINSON: DiffusionKernel = DiffusionKernel {
    entries: &[
        (1, 0, 1.0 / 8.0),
        (2, 0, 1.0 / 8.0),
        (-1, 1, 1.0 / 8.0),
        (0, 1, 1.0 / 8.0),
        (1, 1, 1.0 / 8.0),
        (0, 2, 1.0 / 8.0),
    ],
};

const BURKES: DiffusionKernel = DiffusionKernel {
    entries: &[
        (1, 0, 8.0 / 32.0),
        (2, 0, 4.0 / 32.0),
        (-2, 1, 2.0 / 32.0),
        (-1, 1, 4.0 / 32.0),
        (0, 1, 8.0 / 32.0),
        (1, 1, 4.0 / 32.0),
        (2, 1, 2.0 / 32.0),
    ],
};

const JJN: DiffusionKernel = DiffusionKernel {
    entries: &[
        (1, 0, 7.0 / 48.0),
        (2, 0, 5.0 / 48.0),
        (-2, 1, 3.0 / 48.0),
        (-1, 1, 5.0 / 48.0),
        (0, 1, 7.0 / 48.0),
        (1, 1, 5.0 / 48.0),
        (2, 1, 3.0 / 48.0),
        (-2, 2, 1.0 / 48.0),
        (-1, 2, 3.0 / 48.0),
        (0, 2, 5.0 / 48.0),
        (1, 2, 3.0 / 48.0),
        (2, 2, 1.0 / 48.0),
    ],
};

const STUCKI: DiffusionKernel = DiffusionKernel {
    entries: &[
        (1, 0, 8.0 / 42.0),
        (2, 0, 4.0 / 42.0),
        (-2, 1, 2.0 / 42.0),
        (-1, 1, 4.0 / 42.0),
        (0, 1, 8.0 / 42.0),
        (1, 1, 4.0 / 42.0),
        (2, 1, 2.0 / 42.0),
        (-2, 2, 1.0 / 42.0),
        (-1, 2, 2.0 / 42.0),
        (0, 2, 4.0 / 42.0),
        (1, 2, 2.0 / 42.0),
        (2, 2, 1.0 / 42.0),
    ],
};

const SIERRA: DiffusionKernel = DiffusionKernel {
    entries: &[
        (1, 0, 5.0 / 32.0),
        (2, 0, 3.0 / 32.0),
        (-2, 1, 2.0 / 32.0),
        (-1, 1, 4.0 / 32.0),
        (0, 1, 5.0 / 32.0),
        (1, 1, 4.0 / 32.0),
        (2, 1, 2.0 / 32.0),
        (-1, 2, 2.0 / 32.0),
        (0, 2, 3.0 / 32.0),
        (1, 2, 2.0 / 32.0),
    ],
};

const SIERRA_LITE: DiffusionKernel = DiffusionKernel {
    entries: &[(1, 0, 2.0 / 4.0), (-1, 1, 1.0 / 4.0), (0, 1, 1.0 / 4.0)],
};

// Sierra Two-Row (shorter than Sierra, two rows)
const SIERRA_TWO_ROW: DiffusionKernel = DiffusionKernel {
    entries: &[
        (1, 0, 4.0 / 16.0),
        (2, 0, 3.0 / 16.0),
        (-2, 1, 1.0 / 16.0),
        (-1, 1, 2.0 / 16.0),
        (0, 1, 3.0 / 16.0),
        (1, 1, 2.0 / 16.0),
        (2, 1, 1.0 / 16.0),
    ],
};

// Shiau-Fan: compact, low-wander, 1-row right + 1-row below
const SHIAU_FAN: DiffusionKernel = DiffusionKernel {
    entries: &[
        (1, 0, 4.0 / 8.0),
        (-1, 1, 1.0 / 8.0),
        (0, 1, 2.0 / 8.0),
        (1, 1, 1.0 / 8.0),
    ],
};

// ── Error diffusion engine ────────────────────────────────────────────────────

fn dither_error_diffusion(
    buf: &mut [[f32; 3]],
    w: usize,
    h: usize,
    params: &DitherParams,
    palette: &Palette,
    kernel: &DiffusionKernel,
) {
    let mut work: Vec<[f32; 3]> = buf.to_vec();

    for y in 0..h {
        let left_to_right = !params.serpentine || (y % 2 == 0);
        let xs: Box<dyn Iterator<Item = usize>> = if left_to_right {
            Box::new(0..w)
        } else {
            Box::new((0..w).rev())
        };

        for x in xs {
            let old = work[y * w + x];
            let quantized = palette.nearest(
                old[0].clamp(0.0, 1.0),
                old[1].clamp(0.0, 1.0),
                old[2].clamp(0.0, 1.0),
            );
            work[y * w + x] = quantized;

            let err = [
                old[0] - quantized[0],
                old[1] - quantized[1],
                old[2] - quantized[2],
            ];

            // Skip if error is negligible
            if err[0].abs() + err[1].abs() + err[2].abs() < f32::EPSILON {
                continue;
            }

            for &(mut dx, dy, weight) in kernel.entries {
                if !left_to_right {
                    dx = -dx;
                }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                    let n = &mut work[ny as usize * w + nx as usize];
                    n[0] += err[0] * weight;
                    n[1] += err[1] * weight;
                    n[2] += err[2] * weight;
                }
            }
        }
    }

    buf.copy_from_slice(&work);
}
