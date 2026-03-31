//! Palette definitions, median-cut extraction, and GIMP `.gpl` I/O.
//!
//! A [`Palette`] is a list of RGB colors in 0.0–1.0 float range. The
//! [`PaletteKind`] enum names every built-in palette and also the two
//! dynamic variants (`Extracted` and `Custom`) that are managed by app state.

use image::DynamicImage;

/// A set of RGB colors used as the quantization target during dithering.
///
/// All color components are stored in 0.0–1.0 float range.
#[derive(Clone, PartialEq, Debug)]
pub struct Palette {
    pub name: &'static str,
    pub colors: Vec<[f32; 3]>,
}

impl Palette {
    /// Find the nearest palette color to the given RGB value using weighted Euclidean distance.
    #[inline]
    pub fn nearest(&self, r: f32, g: f32, b: f32) -> [f32; 3] {
        let mut best = self.colors[0];
        let mut best_dist = f32::MAX;
        for &c in &self.colors {
            // Perceptually weighted distance
            let dr = (r - c[0]) * 0.2126_f32.sqrt();
            let dg = (g - c[1]) * 0.7152_f32.sqrt();
            let db = (b - c[2]) * 0.0722_f32.sqrt();
            let dist = dr * dr + dg * dg + db * db;
            if dist < best_dist {
                best_dist = dist;
                best = c;
            }
        }
        best
    }

    fn from_u8s(name: &'static str, raw: &[[u8; 3]]) -> Self {
        Self {
            name,
            colors: raw.iter().map(|&[r, g, b]| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]).collect(),
        }
    }

    pub fn gray_ramp(levels: u32) -> Self {
        let steps = levels.max(2);
        Self {
            name: "Gray Ramp",
            colors: (0..steps)
                .map(|i| {
                    let v = i as f32 / (steps - 1) as f32;
                    [v, v, v]
                })
                .collect(),
        }
    }
}

// ── Built-in palette definitions ─────────────────────────────────────────────

/// Named palette presets available in the UI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaletteKind {
    GrayRamp2,
    GrayRamp4,
    GrayRamp8,
    GrayRamp16,
    GameBoy,
    GameBoyPocket,
    CgaMode1,
    CgaMode2,
    Pico8,
    C64,
    ZxSpectrum,
    Ega,
    AppleII,
    Teletext,
    Amber,
    GreenPhosphor,
    Extracted, // populated from the image via median-cut
    Custom,    // manually edited by the user
}

impl PaletteKind {
    pub fn all() -> &'static [PaletteKind] {
        &[
            PaletteKind::Extracted,
            PaletteKind::Custom,
            PaletteKind::GrayRamp2,
            PaletteKind::GrayRamp4,
            PaletteKind::GrayRamp8,
            PaletteKind::GrayRamp16,
            PaletteKind::GameBoy,
            PaletteKind::GameBoyPocket,
            PaletteKind::CgaMode1,
            PaletteKind::CgaMode2,
            PaletteKind::Pico8,
            PaletteKind::C64,
            PaletteKind::ZxSpectrum,
            PaletteKind::Ega,
            PaletteKind::AppleII,
            PaletteKind::Teletext,
            PaletteKind::Amber,
            PaletteKind::GreenPhosphor,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            PaletteKind::GrayRamp2    => "1-bit (B&W)",
            PaletteKind::GrayRamp4    => "2-bit Gray (4)",
            PaletteKind::GrayRamp8    => "3-bit Gray (8)",
            PaletteKind::GrayRamp16   => "4-bit Gray (16)",
            PaletteKind::GameBoy      => "Game Boy",
            PaletteKind::GameBoyPocket => "Game Boy Pocket",
            PaletteKind::CgaMode1     => "CGA Mode 4 (Cyan/Magenta)",
            PaletteKind::CgaMode2     => "CGA Mode 5 (Red/Cyan)",
            PaletteKind::Pico8        => "PICO-8",
            PaletteKind::C64          => "Commodore 64",
            PaletteKind::ZxSpectrum   => "ZX Spectrum",
            PaletteKind::Ega          => "EGA",
            PaletteKind::AppleII      => "Apple II",
            PaletteKind::Teletext     => "Teletext",
            PaletteKind::Amber        => "Amber Phosphor",
            PaletteKind::GreenPhosphor => "Green Phosphor",
            PaletteKind::Extracted    => "From Image (extracted)",
            PaletteKind::Custom       => "Custom",
        }
    }

    pub fn build(&self) -> Palette {
        match self {
            PaletteKind::GrayRamp2  => Palette::gray_ramp(2),
            PaletteKind::GrayRamp4  => Palette::gray_ramp(4),
            PaletteKind::GrayRamp8  => Palette::gray_ramp(8),
            PaletteKind::GrayRamp16 => Palette::gray_ramp(16),

            PaletteKind::GameBoy => Palette::from_u8s("Game Boy", &[
                [0x9b, 0xbc, 0x0f],
                [0x8b, 0xac, 0x0f],
                [0x30, 0x62, 0x30],
                [0x0f, 0x38, 0x0f],
            ]),

            PaletteKind::GameBoyPocket => Palette::from_u8s("Game Boy Pocket", &[
                [0xc4, 0xcf, 0xa1],
                [0x8b, 0x95, 0x6d],
                [0x4d, 0x54, 0x3d],
                [0x1f, 0x1f, 0x1f],
            ]),

            PaletteKind::CgaMode1 => Palette::from_u8s("CGA Mode 4", &[
                [0x00, 0x00, 0x00],
                [0x00, 0xaa, 0xaa],
                [0xaa, 0x00, 0xaa],
                [0xaa, 0xaa, 0xaa],
            ]),

            PaletteKind::CgaMode2 => Palette::from_u8s("CGA Mode 5", &[
                [0x00, 0x00, 0x00],
                [0x00, 0xaa, 0xaa],
                [0xaa, 0x00, 0x00],
                [0xaa, 0xaa, 0xaa],
            ]),

            PaletteKind::Pico8 => Palette::from_u8s("PICO-8", &[
                [0x00, 0x00, 0x00],
                [0x1d, 0x2b, 0x53],
                [0x7e, 0x25, 0x53],
                [0x00, 0x87, 0x51],
                [0xab, 0x52, 0x36],
                [0x5f, 0x57, 0x4f],
                [0xc2, 0xc3, 0xc7],
                [0xff, 0xf1, 0xe8],
                [0xff, 0x00, 0x4d],
                [0xff, 0xa3, 0x00],
                [0xff, 0xec, 0x27],
                [0x00, 0xe4, 0x36],
                [0x29, 0xad, 0xff],
                [0x83, 0x76, 0x9c],
                [0xff, 0x77, 0xa8],
                [0xff, 0xcc, 0xaa],
            ]),

            PaletteKind::C64 => Palette::from_u8s("C64", &[
                [0x00, 0x00, 0x00],
                [0xff, 0xff, 0xff],
                [0x88, 0x00, 0x00],
                [0xaa, 0xff, 0xee],
                [0xcc, 0x44, 0xcc],
                [0x00, 0xcc, 0x55],
                [0x00, 0x00, 0xaa],
                [0xee, 0xee, 0x77],
                [0xdd, 0x88, 0x55],
                [0x66, 0x44, 0x00],
                [0xff, 0x77, 0x77],
                [0x33, 0x33, 0x33],
                [0x77, 0x77, 0x77],
                [0xaa, 0xff, 0x66],
                [0x00, 0x88, 0xff],
                [0xbb, 0xbb, 0xbb],
            ]),

            PaletteKind::ZxSpectrum => Palette::from_u8s("ZX Spectrum", &[
                [0x00, 0x00, 0x00],
                [0x00, 0x00, 0xd7],
                [0xd7, 0x00, 0x00],
                [0xd7, 0x00, 0xd7],
                [0x00, 0xd7, 0x00],
                [0x00, 0xd7, 0xd7],
                [0xd7, 0xd7, 0x00],
                [0xd7, 0xd7, 0xd7],
                [0x00, 0x00, 0xff],
                [0xff, 0x00, 0x00],
                [0xff, 0x00, 0xff],
                [0x00, 0xff, 0x00],
                [0x00, 0xff, 0xff],
                [0xff, 0xff, 0x00],
                [0xff, 0xff, 0xff],
            ]),

            PaletteKind::Ega => Palette::from_u8s("EGA", &[
                [0x00, 0x00, 0x00],
                [0x00, 0x00, 0xaa],
                [0x00, 0xaa, 0x00],
                [0x00, 0xaa, 0xaa],
                [0xaa, 0x00, 0x00],
                [0xaa, 0x00, 0xaa],
                [0xaa, 0x55, 0x00],
                [0xaa, 0xaa, 0xaa],
                [0x55, 0x55, 0x55],
                [0x55, 0x55, 0xff],
                [0x55, 0xff, 0x55],
                [0x55, 0xff, 0xff],
                [0xff, 0x55, 0x55],
                [0xff, 0x55, 0xff],
                [0xff, 0xff, 0x55],
                [0xff, 0xff, 0xff],
            ]),

            PaletteKind::AppleII => Palette::from_u8s("Apple II", &[
                [0x00, 0x00, 0x00],
                [0x72, 0x26, 0x40],
                [0x40, 0x33, 0x7f],
                [0xe4, 0x34, 0xfe],
                [0x0e, 0x59, 0x40],
                [0x80, 0x80, 0x80],
                [0x1b, 0x52, 0xf5],
                [0xbf, 0xbf, 0xfb],
                [0x40, 0x40, 0x01],
                [0xe4, 0x60, 0x0a],
                [0x80, 0x80, 0x80],
                [0xfe, 0xac, 0x80],
                [0x1b, 0xcb, 0x01],
                [0x80, 0xfe, 0x81],
                [0xbc, 0xca, 0x40],
                [0xff, 0xff, 0xff],
            ]),

            PaletteKind::Teletext => Palette::from_u8s("Teletext", &[
                [0x00, 0x00, 0x00],
                [0xff, 0x00, 0x00],
                [0x00, 0xff, 0x00],
                [0xff, 0xff, 0x00],
                [0x00, 0x00, 0xff],
                [0xff, 0x00, 0xff],
                [0x00, 0xff, 0xff],
                [0xff, 0xff, 0xff],
            ]),

            PaletteKind::Amber => Palette::from_u8s("Amber Phosphor", &[
                [0x00, 0x00, 0x00],
                [0x2a, 0x10, 0x00],
                [0x55, 0x20, 0x00],
                [0x7f, 0x40, 0x00],
                [0xaa, 0x60, 0x00],
                [0xd4, 0x88, 0x00],
                [0xff, 0xb0, 0x00],
                [0xff, 0xd8, 0x55],
            ]),

            PaletteKind::GreenPhosphor => Palette::from_u8s("Green Phosphor", &[
                [0x00, 0x00, 0x00],
                [0x00, 0x1a, 0x00],
                [0x00, 0x33, 0x00],
                [0x00, 0x4d, 0x00],
                [0x00, 0x66, 0x00],
                [0x00, 0x99, 0x00],
                [0x00, 0xcc, 0x00],
                [0x33, 0xff, 0x33],
            ]),

            // Extracted and Custom are resolved externally in app state; fallback to gray
            PaletteKind::Extracted | PaletteKind::Custom => Palette::gray_ramp(16),
        }
    }
}

// ── Median-cut palette extraction ─────────────────────────────────────────────

/// Extract a palette of `num_colors` from an image using the median-cut algorithm.
pub fn extract_palette(img: &DynamicImage, num_colors: usize) -> Palette {
    let num_colors = num_colors.clamp(2, 256);

    // Downsample large images for speed (512×512 is plenty for palette extraction)
    let small = if img.width() * img.height() > 512 * 512 {
        img.resize(512, 512, image::imageops::FilterType::Nearest)
    } else {
        img.clone()
    };

    let pixels: Vec<[u8; 3]> = small
        .to_rgb8()
        .pixels()
        .map(|p| [p[0], p[1], p[2]])
        .collect();

    let mut buckets: Vec<Vec<[u8; 3]>> = vec![pixels];

    while buckets.len() < num_colors {
        // Find bucket with the largest range along any channel
        let mut best_idx = 0;
        let mut best_range = 0u16;
        for (i, bucket) in buckets.iter().enumerate() {
            if bucket.len() <= 1 {
                continue;
            }
            let (rmin, rmax, gmin, gmax, bmin, bmax) = bucket_range(bucket);
            let range = (rmax - rmin)
                .max(gmax - gmin)
                .max(bmax - bmin) as u16;
            if range > best_range {
                best_range = range;
                best_idx = i;
            }
        }

        if best_range == 0 {
            break;
        }

        let bucket = buckets.remove(best_idx);
        let (rmin, rmax, gmin, gmax, bmin, bmax) = bucket_range(&bucket);
        let channel = if (rmax - rmin) >= (gmax - gmin) && (rmax - rmin) >= (bmax - bmin) {
            0usize
        } else if (gmax - gmin) >= (bmax - bmin) {
            1
        } else {
            2
        };

        let mut sorted = bucket;
        sorted.sort_unstable_by_key(|p| p[channel]);
        let mid = sorted.len() / 2;
        buckets.push(sorted[..mid].to_vec());
        buckets.push(sorted[mid..].to_vec());
    }

    // Average each bucket → palette color
    let mut colors: Vec<[f32; 3]> = buckets
        .iter()
        .filter(|b| !b.is_empty())
        .map(|bucket| {
            let n = bucket.len() as f64;
            let r: u64 = bucket.iter().map(|p| p[0] as u64).sum();
            let g: u64 = bucket.iter().map(|p| p[1] as u64).sum();
            let b: u64 = bucket.iter().map(|p| p[2] as u64).sum();
            [
                (r as f64 / n / 255.0) as f32,
                (g as f64 / n / 255.0) as f32,
                (b as f64 / n / 255.0) as f32,
            ]
        })
        .collect();

    // Sort by perceptual brightness for consistent swatch display
    colors.sort_by(|a, b| {
        let la = 0.2126 * a[0] + 0.7152 * a[1] + 0.0722 * a[2];
        let lb = 0.2126 * b[0] + 0.7152 * b[1] + 0.0722 * b[2];
        la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
    });

    Palette { name: "Extracted", colors }
}

fn bucket_range(bucket: &[[u8; 3]]) -> (u8, u8, u8, u8, u8, u8) {
    let (mut rmin, mut gmin, mut bmin) = (255u8, 255u8, 255u8);
    let (mut rmax, mut gmax, mut bmax) = (0u8, 0u8, 0u8);
    for &[r, g, b] in bucket {
        rmin = rmin.min(r); rmax = rmax.max(r);
        gmin = gmin.min(g); gmax = gmax.max(g);
        bmin = bmin.min(b); bmax = bmax.max(b);
    }
    (rmin, rmax, gmin, gmax, bmin, bmax)
}

// ── .gpl (GIMP Palette) I/O ───────────────────────────────────────────────────

/// Save a list of colors as a GIMP .gpl palette file.
pub fn save_gpl(colors: &[[f32; 3]], name: &str, path: &std::path::Path) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    writeln!(f, "GIMP Palette").map_err(|e| e.to_string())?;
    writeln!(f, "Name: {name}").map_err(|e| e.to_string())?;
    writeln!(f, "#").map_err(|e| e.to_string())?;
    for &[r, g, b] in colors {
        let ri = (r * 255.0).round() as u8;
        let gi = (g * 255.0).round() as u8;
        let bi = (b * 255.0).round() as u8;
        writeln!(f, "{ri:3} {gi:3} {bi:3}   Untitled").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Load colors from a GIMP .gpl palette file.
pub fn load_gpl(path: &std::path::Path) -> Result<Vec<[f32; 3]>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut colors = Vec::new();
    let mut past_header = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "#" { past_header = true; continue; }
        if !past_header || line.starts_with('#') || line.is_empty() { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let r: u8 = parts[0].parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            let g: u8 = parts[1].parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            let b: u8 = parts[2].parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
            colors.push([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]);
        }
    }
    if colors.is_empty() {
        Err("No colors found in palette file".to_string())
    } else {
        Ok(colors)
    }
}
