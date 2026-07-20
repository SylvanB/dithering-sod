//! Physically-derived per-stock data for the film emulation effect.
//!
//! A [`FilmStock`] bundles everything [`crate::effects`]'s `FilmEmulation`
//! effect needs that is intrinsic to a particular film — its characteristic
//! (H&D) curves, and the relative grain/halation behavior implied by its
//! published MTF and construction. Only [`PORTRA_400`] is populated today,
//! but the type makes adding a second stock a data addition (another
//! `const FilmStock`), not a pipeline rewrite. See `NOTES.md` at the repo
//! root for exactly where these numbers came from and where they're
//! approximated rather than read off a chart.

/// A single (log-exposure, density) sample on a characteristic curve.
///
/// `log_exposure` is log10(lux-seconds), matching the x-axis convention
/// used on Kodak/Kodak Alaris technical data sheets. Points must be sorted
/// by ascending `log_exposure`.
pub type CurvePoint = (f32, f32);

/// Characteristic-curve and grain/halation data for one film stock.
pub struct FilmStock {
    pub name: &'static str,

    /// Status M density vs. log10 exposure, red / green / blue records
    /// (i.e. the density read back through red/green/blue filters — for a
    /// color negative this corresponds to the cyan/magenta/yellow dye
    /// layers respectively). Monotonically increasing in both fields.
    pub curve_r: &'static [CurvePoint],
    pub curve_g: &'static [CurvePoint],
    pub curve_b: &'static [CurvePoint],

    /// Reference log exposure (the datasheet's "Log H Ref" speed point).
    /// Exposure-compensation in the UI is expressed relative to this.
    pub log_h_ref: f32,
    /// Log-exposure domain the curve data covers; an input's 0..1 tone
    /// range is mapped linearly onto this before curve lookup.
    pub log_e_min: f32,
    pub log_e_max: f32,

    /// Relative grain amplitude per channel (R, G, B), before Selwyn
    /// density-scaling. Portra 400's red (cyan-dye) record has the lowest
    /// MTF response of the three at high spatial frequency, which in
    /// practice reads as coarser, more visible grain than the blue record.
    pub grain_weight: [f32; 3],
    /// Relative grain correlation radius per channel — same MTF ordering:
    /// the sharper (higher-resolution) record produces finer-grained noise.
    pub grain_radius: [f32; 3],

    /// Halation bloom tint (R, G, B), applied to light that blooms back
    /// through from the film base. Dominated by red because the red-
    /// sensitive record sits nearest the base, where reflected light
    /// re-exposes the emulsion.
    pub halation_color: [f32; 3],
}

/// Kodak Professional Portra 400, digitized from Kodak Alaris technical
/// data sheet E-4050 (Revised 1-25, kodakalaris.com).
///
/// The datasheet's "Characteristic Curves" panel plots Status-M density
/// (0-4) against log exposure in lux-seconds (-4 to +1), daylight exposure,
/// Log H Ref -1.44, with the three curves labeled B (top/highest density),
/// G (middle), R (bottom/lowest density) at any given exposure, and drawn
/// as long, nearly straight lines with only a gentle toe below roughly
/// logE -3 — the "long straight-line curve, wide exposure latitude" the
/// stock is known for. The axis calibration (range, Log H Ref) and the
/// curve ordering/shape below are read directly off that chart; the
/// individual sample densities are our own reconstruction of a curve
/// consistent with that description (the chart itself isn't available as
/// machine-readable point data), so treat this as "shape-accurate,
/// values-approximate." See NOTES.md.
///
/// The MTF panel on the same sheet shows response-vs-spatial-frequency for
/// R, G, B with the ordering B > G > R at high spatial frequency (e.g. by
/// ~50 cy/mm), which is what `grain_weight`/`grain_radius` below encode:
/// the red record is the softest of the three and reads as the coarsest
/// grain, blue the finest.
pub const PORTRA_400: FilmStock = FilmStock {
    name: "Kodak Portra 400",

    curve_r: &[
        (-3.2, 0.12), (-3.0, 0.18), (-2.4, 0.40), (-1.8, 0.65),
        (-1.44, 0.85), (-1.0, 1.08), (-0.4, 1.42), (0.0, 1.68), (0.6, 2.02),
    ],
    curve_g: &[
        (-3.2, 0.48), (-3.0, 0.56), (-2.4, 0.85), (-1.8, 1.10),
        (-1.44, 1.28), (-1.0, 1.50), (-0.4, 1.85), (0.0, 2.12), (0.6, 2.50),
    ],
    curve_b: &[
        (-3.2, 0.88), (-3.0, 0.96), (-2.4, 1.22), (-1.8, 1.45),
        (-1.44, 1.62), (-1.0, 1.84), (-0.4, 2.20), (0.0, 2.50), (0.6, 3.00),
    ],

    log_h_ref: -1.44,
    log_e_min: -3.2,
    log_e_max: 0.6,

    // R softest/coarsest, G reference, B sharpest/finest — see doc comment.
    grain_weight: [1.15, 1.0, 0.75],
    grain_radius: [1.3, 1.0, 0.7],

    halation_color: [1.0, 0.32, 0.06],
};

/// Evaluate a characteristic curve at `log_e` by piecewise-linear
/// interpolation, extending the end segments' slope beyond the plotted
/// domain instead of hard-clipping (so extreme exposure compensation
/// still produces a smooth, if extrapolated, response).
pub fn eval_stock_curve(points: &[CurvePoint], log_e: f32) -> f32 {
    let last = points.len() - 1;
    if log_e <= points[0].0 {
        let (x0, y0) = points[0];
        let (x1, y1) = points[1];
        let slope = (y1 - y0) / (x1 - x0);
        return y0 + slope * (log_e - x0);
    }
    if log_e >= points[last].0 {
        let (x0, y0) = points[last - 1];
        let (x1, y1) = points[last];
        let slope = (y1 - y0) / (x1 - x0);
        return y1 + slope * (log_e - x1);
    }
    for i in 0..last {
        let (x0, y0) = points[i];
        let (x1, y1) = points[i + 1];
        if log_e <= x1 {
            let t = (log_e - x0) / (x1 - x0);
            return y0 + t * (y1 - y0);
        }
    }
    points[last].1
}

/// Density at the minimum and maximum of a curve's plotted domain — used
/// to normalize a channel's density back to a 0..1 display value.
pub fn curve_range(points: &[CurvePoint]) -> (f32, f32) {
    (points[0].1, points[points.len() - 1].1)
}
