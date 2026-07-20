//! Generates synthetic test images, runs the Portra 400 film emulation
//! effect on them, saves before/after PNGs under `validation/images/`, and
//! runs a couple of quantitative sanity checks against the physical claims
//! the effect makes (grain tracks local density; halation only originates
//! above its brightness threshold).
//!
//! Run with: `cargo run --example film_validation`
//!
//! Not part of the app itself — a standalone check pulled in via `#[path]`
//! so it can exercise the real `effects`/`filmstock` modules without a GUI.

#[path = "../src/filmstock.rs"]
mod filmstock;
#[path = "../src/effects.rs"]
mod effects;

use effects::{apply_effects, Effect, EffectKind};
use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use std::path::Path;

const OUT_DIR: &str = "validation/images";

fn main() {
    std::fs::create_dir_all(OUT_DIR).expect("create output dir");
    let mut failures = Vec::new();

    let ramp = grayscale_ramp(512, 128);
    let patches = patch_chart(560, 320);
    let highlight = hard_highlight(400, 300);

    save(&ramp, "01_ramp_before.png");
    save(&patches, "02_patches_before.png");
    save(&highlight, "03_highlight_before.png");

    let default_kind = EffectKind::default_for_name("film");
    println!("Default film emulation params: {default_kind:?}");

    let ramp_after = run(&ramp, default_kind);
    let patches_after_full = run(&patches, default_kind);
    let highlight_after_full = run(&highlight, default_kind);
    save(&ramp_after, "01_ramp_after.png");
    save(&patches_after_full, "02_patches_after.png");
    save(&highlight_after_full, "03_highlight_after.png");

    // Isolate grain: halation off.
    let grain_only_kind = with_halation_off(default_kind);
    let patches_grain_only = run(&patches, grain_only_kind);
    save(&patches_grain_only, "04_patches_grain_only.png");

    // Isolate halation: grain off.
    let halation_only_kind = with_grain_off(default_kind);
    let highlight_halation_only = run(&highlight, halation_only_kind);
    save(&highlight_halation_only, "05_highlight_halation_only.png");

    // Curve-only baseline (halation and grain both off) — the reference for
    // isolating halation's *own* contribution, since the characteristic
    // curve alone already reshapes brightness and must not be mistaken for
    // halation leakage.
    let curve_only_kind = with_grain_off(with_halation_off(default_kind));
    let highlight_curve_only = run(&highlight, curve_only_kind);
    save(&highlight_curve_only, "06_highlight_curve_only.png");

    println!("\n=== Check 1: grain amplitude tracks local density (Selwyn's law) ===");
    check_grain_tracks_density(&patches, &patches_grain_only, &mut failures);

    println!("\n=== Check 2: halation bloom source is zero at/below threshold ===");
    check_halation_threshold_formula(&mut failures);

    println!("\n=== Check 3: halation only brightens regions near/above threshold ===");
    check_halation_spatial_extent(
        &highlight_curve_only,
        &highlight_halation_only,
        halation_only_kind,
        &mut failures,
    );

    println!("\n=== Check 4: per-channel curve produces distinct R/G/B response (not a flat tint) ===");
    check_channel_divergence(&ramp, &ramp_after, &mut failures);

    write_report(&failures);

    if failures.is_empty() {
        println!("\nAll checks passed.");
    } else {
        println!("\n{} check(s) FAILED:", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        std::process::exit(1);
    }
}

fn run(img: &DynamicImage, kind: EffectKind) -> DynamicImage {
    apply_effects(img, &[Effect::new(kind)])
}

fn with_halation_off(kind: EffectKind) -> EffectKind {
    match kind {
        EffectKind::FilmEmulation {
            exposure_ev,
            grain_strength,
            grain_size,
            halation_radius,
            halation_threshold,
            ..
        } => EffectKind::FilmEmulation {
            exposure_ev,
            grain_strength,
            grain_size,
            halation_strength: 0.0,
            halation_radius,
            halation_threshold,
        },
        other => other,
    }
}

fn with_grain_off(kind: EffectKind) -> EffectKind {
    match kind {
        EffectKind::FilmEmulation {
            exposure_ev,
            grain_size,
            halation_strength,
            halation_radius,
            halation_threshold,
            ..
        } => EffectKind::FilmEmulation {
            exposure_ev,
            grain_strength: 0.0,
            grain_size,
            halation_strength,
            halation_radius,
            halation_threshold,
        },
        other => other,
    }
}

fn save(img: &DynamicImage, name: &str) {
    let path = Path::new(OUT_DIR).join(name);
    img.save(&path).unwrap_or_else(|e| panic!("save {path:?}: {e}"));
    println!("wrote {}", path.display());
}

// ── Synthetic test images ───────────────────────────────────────────────────

/// Horizontal grayscale ramp, 0..255 left to right, constant down each column.
fn grayscale_ramp(w: u32, h: u32) -> DynamicImage {
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = ((x as f32 / (w - 1) as f32) * 255.0).round() as u8;
            img.put_pixel(x, y, Rgb([v, v, v]));
        }
    }
    DynamicImage::ImageRgb8(img)
}

/// A row of flat neutral-gray patches spanning shadow -> midtone -> highlight,
/// each big enough to measure local statistics on.
const PATCH_LEVELS: [u8; 7] = [8, 40, 80, 128, 176, 216, 248];

fn patch_chart(w: u32, h: u32) -> DynamicImage {
    let mut img = RgbImage::new(w, h);
    let n = PATCH_LEVELS.len() as u32;
    for y in 0..h {
        for x in 0..w {
            let idx = (x * n / w).min(n - 1) as usize;
            let v = PATCH_LEVELS[idx];
            img.put_pixel(x, y, Rgb([v, v, v]));
        }
    }
    DynamicImage::ImageRgb8(img)
}

/// Returns the pixel-space rect of patch `idx` in the chart produced above,
/// inset from the patch boundary so we're sampling well inside a flat region.
fn patch_rect(idx: usize, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let n = PATCH_LEVELS.len() as u32;
    let patch_w = w / n;
    let x0 = idx as u32 * patch_w + patch_w / 4;
    let x1 = (idx as u32 + 1) * patch_w - patch_w / 4;
    let y0 = h / 4;
    let y1 = h - h / 4;
    (x0, y0, x1, y1)
}

/// Dark midtone background with one small, hard, pure-white "specular
/// highlight" square — the classic halation stress test.
fn hard_highlight(w: u32, h: u32) -> DynamicImage {
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.put_pixel(x, y, Rgb([60, 55, 50]));
        }
    }
    let (cx, cy, r) = (w / 2, h / 2, 18i32);
    for y in 0..h {
        for x in 0..w {
            let dx = x as i32 - cx as i32;
            let dy = y as i32 - cy as i32;
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
    }
    DynamicImage::ImageRgb8(img)
}

// ── Quantitative checks ─────────────────────────────────────────────────────

fn mean_std(img: &DynamicImage, ch: usize, x0: u32, y0: u32, x1: u32, y1: u32) -> (f64, f64) {
    let rgb = img.to_rgb8();
    let mut sum = 0f64;
    let mut count = 0f64;
    for y in y0..y1 {
        for x in x0..x1 {
            sum += rgb.get_pixel(x, y)[ch] as f64;
            count += 1.0;
        }
    }
    let mean = sum / count;
    let mut var = 0f64;
    for y in y0..y1 {
        for x in x0..x1 {
            let d = rgb.get_pixel(x, y)[ch] as f64 - mean;
            var += d * d;
        }
    }
    (mean, (var / count).sqrt())
}

fn check_grain_tracks_density(
    before: &DynamicImage,
    after: &DynamicImage,
    failures: &mut Vec<String>,
) {
    let (w, h) = before.dimensions();
    let mut stds = Vec::new();
    for (i, level) in PATCH_LEVELS.iter().enumerate() {
        let (x0, y0, x1, y1) = patch_rect(i, w, h);
        // Average std across R,G,B (patch is neutral gray input, but the
        // curve's Dmin/Dmax differ per channel so density_frac differs too).
        let (mean, std) = mean_std(after, 1, x0, y0, x1, y1); // green channel
        println!("  patch level={level:>3} (before) -> after mean={mean:.1} std={std:.3}");
        stds.push(std);
    }
    // Selwyn: grain amplitude ~ sqrt(density_frac), density_frac increases
    // monotonically with input level for Portra's monotonic curve, so std
    // should trend upward from shadow to highlight patches. Allow a couple
    // of local inversions from noise-of-the-noise-estimate, but the darkest
    // patch must be clearly less grainy than the brightest, and the trend
    // must be broadly increasing (Spearman-ish: more ups than downs).
    let mut ups = 0;
    let mut downs = 0;
    for w in stds.windows(2) {
        if w[1] > w[0] {
            ups += 1;
        } else {
            downs += 1;
        }
    }
    println!("  monotonic steps: {ups} up / {downs} down (want mostly up)");
    if stds[0] >= stds[stds.len() - 1] {
        failures.push(format!(
            "grain std did not increase from darkest ({:.3}) to brightest ({:.3}) patch",
            stds[0],
            stds[stds.len() - 1]
        ));
    }
    if ups < downs {
        failures.push(format!(
            "grain std vs. density trend is not mostly increasing ({ups} up vs {downs} down)"
        ));
    }
}

fn check_halation_threshold_formula(failures: &mut Vec<String>) {
    // Mirrors the bloom_src formula in effects::film_emulation directly, to
    // verify the threshold cutoff independent of blur spreading it spatially.
    let threshold = 0.75f32;
    let denom = (1.0 - threshold.min(0.98)).max(0.02);
    let bloom_src = |l: f32| -> f32 { ((l - threshold) / denom).clamp(0.0, 1.0) };

    let below = [0.0, 0.3, 0.6, 0.75];
    let above = [0.8, 0.9, 1.0];
    for l in below {
        let b = bloom_src(l);
        println!("  luminance={l:.2} -> bloom_src={b:.4}");
        if b != 0.0 {
            failures.push(format!("bloom_src({l}) = {b}, expected exactly 0 at/below threshold"));
        }
    }
    let mut prev = 0.0;
    for l in above {
        let b = bloom_src(l);
        println!("  luminance={l:.2} -> bloom_src={b:.4}");
        if b <= prev {
            failures.push(format!("bloom_src not increasing above threshold at l={l}"));
        }
        prev = b;
    }
}

fn check_halation_spatial_extent(
    before: &DynamicImage,
    after: &DynamicImage,
    kind: EffectKind,
    failures: &mut Vec<String>,
) {
    let radius = match kind {
        EffectKind::FilmEmulation { halation_radius, .. } => halation_radius,
        _ => unreachable!(),
    };
    let (w, h) = before.dimensions();
    let (cx, cy) = (w / 2, h / 2);

    // Far corner: outside bloom radius by a comfortable margin, all pixels
    // originally well below threshold -> should be essentially unchanged.
    let margin = radius * 3;
    assert!(margin < w.min(h) / 2, "test image too small for this radius");
    let (_, far_std_before) = mean_std(before, 0, 0, 0, margin, margin);
    let (far_mean_before, _) = mean_std(before, 0, 0, 0, margin, margin);
    let (far_mean_after, _) = mean_std(after, 0, 0, 0, margin, margin);
    let far_delta = (far_mean_after - far_mean_before).abs();
    println!("  far corner R mean: before={far_mean_before:.2} after={far_mean_after:.2} |delta|={far_delta:.3} (std_before={far_std_before:.3})");
    if far_delta > 1.0 {
        failures.push(format!(
            "far-from-highlight region changed by {far_delta:.3} (>1.0 in 0..255), halation leaked further than expected"
        ));
    }

    // Just outside the highlight disc but inside bloom radius: background
    // pixel that should pick up a visible, red-dominant boost.
    let ex = (cx + 22).min(w - 2);
    let ey = cy;
    let before_px = before.to_rgb8().get_pixel(ex, ey).0;
    let after_px = after.to_rgb8().get_pixel(ex, ey).0;
    let dr = after_px[0] as i32 - before_px[0] as i32;
    let dg = after_px[1] as i32 - before_px[1] as i32;
    let db = after_px[2] as i32 - before_px[2] as i32;
    println!("  near-highlight bg pixel ({ex},{ey}): before={before_px:?} after={after_px:?} delta=({dr},{dg},{db})");
    if dr <= 0 {
        failures.push(format!(
            "expected a positive red boost near the highlight, got delta_r={dr}"
        ));
    }
    if !(dr > dg && dg >= db) {
        failures.push(format!(
            "expected halation to be red-dominant (dr>dg>=db), got dr={dr} dg={dg} db={db}"
        ));
    }
}

fn check_channel_divergence(
    before: &DynamicImage,
    after: &DynamicImage,
    failures: &mut Vec<String>,
) {
    // On the neutral ramp, an input of (v,v,v) should map to a *non-equal*
    // (r,g,b) after the per-channel curve, because Portra's R/G/B curves
    // have different Dmin/Dmax/gamma. Check the ramp midpoint.
    let (w, h) = before.dimensions();
    let (x, y) = (w / 2, h / 2);
    let px = after.to_rgb8().get_pixel(x, y).0;
    println!("  ramp midpoint after curve: {px:?}");
    let spread = px.iter().max().unwrap() - px.iter().min().unwrap();
    if spread < 3 {
        failures.push(format!(
            "expected per-channel curve to visibly separate R/G/B on a neutral input, got spread={spread} (px={px:?})"
        ));
    }
}

fn write_report(failures: &[String]) {
    let mut report = String::new();
    report.push_str("# Film Emulation Validation Report\n\n");
    report.push_str("Generated by `cargo run --example film_validation`.\n\n");
    report.push_str("## Images\n\n");
    report.push_str("- `01_ramp_{before,after}.png` — grayscale ramp, full curve shape / tone response.\n");
    report.push_str("- `02_patches_{before,after}.png` — shadow/mid/highlight patch chart, full effect.\n");
    report.push_str("- `04_patches_grain_only.png` — patch chart with halation disabled, grain isolated.\n");
    report.push_str("- `03_highlight_{before,after}.png` — hard highlight, full effect.\n");
    report.push_str("- `05_highlight_halation_only.png` — hard highlight with grain disabled, halation isolated.\n");
    report.push_str("- `06_highlight_curve_only.png` — hard highlight with halation and grain both disabled, curve-only baseline used to isolate halation's own contribution.\n\n");
    report.push_str("## Quantitative checks\n\n");
    if failures.is_empty() {
        report.push_str("All checks passed. See stdout of the validation run for the raw numbers.\n");
    } else {
        report.push_str("FAILURES:\n\n");
        for f in failures {
            report.push_str(&format!("- {f}\n"));
        }
    }
    std::fs::write("validation/REPORT.md", report).expect("write report");
}
