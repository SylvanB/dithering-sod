# Film emulation (Portra 400) — implementation notes

## What this is

A new `EffectKind::FilmEmulation` variant (`src/effects.rs`) wired through
`default_for_name`, `EFFECT_NAMES`, `apply_one`, and the effects-stack UI
panel (`src/app/ui.rs`) exactly like the existing effects. The film-stock
data itself lives in `src/filmstock.rs` as a `FilmStock` struct with a
single populated instance, `PORTRA_400` — a second stock is meant to be a
new `const FilmStock { .. }` plus a name-to-stock lookup, not a rewrite of
the pipeline.

The effect models three physically distinct things, in this order:

1. **Per-channel characteristic curve** — each of R/G/B is pushed through
   its own density-vs-log-exposure response, independently. This is what's
   actually responsible for Portra's warmth and gentle contrast: the red,
   green, and blue curves have different gamma and different Dmin/Dmax, so
   a neutral gray input does not stay neutral after the curve. It's not a
   tint applied on top.
2. **Halation** — a red-dominant bloom that only originates from scene
   brightness above a threshold, modeling light scattering through the
   film base and re-exposing the emulsion from behind.
3. **Grain** — spatially-correlated per-channel noise whose amplitude
   scales with the local density the curve computed in step 1, per
   Selwyn's law.

Six real parameters, not one "amount" knob: `exposure_ev`, `grain_strength`,
`grain_size`, `halation_strength`, `halation_radius`, `halation_threshold`.

## Where the data came from

**Primary source**: Kodak Alaris, *KODAK PROFESSIONAL PORTRA 400 Film*,
Technical Data / Color Negative Film, Publication E-4050, revised
January 2025 (kodakalaris.com / kodakprofessional.com). Fetched and read
directly (not from a secondary description).

From that datasheet, taken as **ground truth**, not approximated:

- The "Characteristic Curves" panel's axis calibration: density 0–4.0
  (Status M), log exposure −4.0 to +1.0 (lux-seconds), **Log H Ref = −1.44**.
  This is `log_h_ref` / `log_e_min` / `log_e_max` in `filmstock.rs`.
- The curve *ordering and shape*: three long, nearly-straight lines with
  only a gentle toe below about log E ≈ −3, labeled B (highest density at
  any exposure) > G (middle) > R (lowest) — Portra's well-known "long
  straight-line curve, wide exposure latitude" signature.
- The Modulation Transfer Function panel's ordering: response falls off
  fastest for R and slowest for B at high spatial frequency (e.g. by
  ~50 cy/mm) — i.e. the red (cyan-dye) record is the softest/lowest-
  resolution of the three, blue the sharpest.
- Spectral sensitivity layer ordering, ISO 400 daylight speed, ESTAR base
  thickness (0.10 mm / 3.94 mil for 135/120).
- Print Grain Index table (25 at the ~4x6-from-120 visual-threshold point,
  climbing to 59–89 at 8x10/16x20 from 35mm) — Kodak's modern replacement
  for RMS granularity; confirms grain becomes visually significant at
  moderate-to-large enlargement from 35mm/120, consistent with treating
  grain as a real, non-negligible per-pixel effect rather than a texture
  overlay.

**Approximated / reconstructed** (and why):

- The individual `(log_exposure, density)` sample points in
  `PORTRA_400.curve_r/g/b` are *not* pixel-traced off the chart — I don't
  have a way to digitize a raster chart to sub-pixel curve coordinates in
  this environment. They're a hand-built curve consistent with everything
  above (axis range, Log H Ref, toe position, straight-line slope
  ordering B>G>R, and the datasheet's stated ~12-stop useful range), built
  as 9 control points per channel with `filmstock::eval_stock_curve` doing
  piecewise-linear interpolation between them (linearly extrapolating past
  the ends rather than clipping). Treat these as shape-accurate,
  value-approximate. If exact digitized values matter later, the fix is
  to pixel-trace the chart image (e.g. with a vision-capable pass over the
  rendered PDF page) rather than rebuild the model.
- `grain_weight` and `grain_radius` per channel are a reasonable but not
  directly-published translation of the MTF ordering into a grain-size/
  amplitude ordering. Kodak doesn't publish per-channel RMS granularity for
  Portra 400 (it publishes only aggregate Print Grain Index, and only per
  print size/format, not per color record). The R > G > B *ordering* is
  well-supported by the MTF chart; the specific numbers (1.15/1.0/0.75 and
  1.3/1.0/0.7) are my own calibration, not a citation.
- `halation_color = [1.0, 0.32, 0.06]` (R/G/B bloom tint) is qualitative,
  not from the datasheet. It reflects the general, widely-documented
  physical mechanism (see below) that halation is dominated by the
  red-sensitive layer because it sits nearest the film base where
  reflected light re-exposes the emulsion, with a secondary green
  contribution and minimal blue. I did not find a Portra-400-specific
  quantified halation spectrum; this is the standard color-negative
  halation story applied to Portra's known layer order, not measured.

**Physics background** (general, not Portra-specific, used to justify the
grain and halation models rather than eyeballing a look):

- Selwyn's law (R. Selwyn, Eastman Kodak, 1935): RMS granularity is
  aperture-invariant when normalized by aperture area, and is
  theoretically proportional to √density. This is the basis for scaling
  grain amplitude by `sqrt(density_frac)` in `film_emulation()` rather
  than applying uniform-amplitude noise everywhere.
- Halation mechanism: bright light passes through the emulsion, scatters
  off/through the film base, and re-exposes the emulsion from behind;
  color negative film's anti-halation backing suppresses this except when
  overwhelmed by strong highlights, and because the red-sensitive record
  sits closest to the base, the bloom that gets through is red/orange-
  dominant. This is why the effect thresholds its bloom source (only
  scene brightness above `halation_threshold` contributes) instead of
  blooming everything, and why the bloom tint is red-heavy.

Neither of these two general-physics facts is Portra-specific; they're
the standard explanation found in sensitometry references and repeated
across film-emulation write-ups (Wikipedia's Film grain and Anti-halation
backing articles, and multiple photography-technique explainers, were
used to confirm the mechanism description, not to source any number).

**Prior art consulted, not copied**: `agx-emulsion` (andreavolpato,
GitHub, published as `spektrafilm`) is an open-source spectral film
simulator that also derives its Portra 400 curves from the same Kodak
datasheet family and combines curve response + couplers + grain +
halation in one pipeline. I looked at its existence and approach as
validation that this is a reasonable architecture, but I could not get
its actual digitized data files through the tools available in this
session (GitHub's code search and the raw tree API were rate-limited /
not browsable here), so nothing from it was copied into `filmstock.rs`;
the curve points here are my own reconstruction from the Kodak PDF as
described above.

## What I'd refine with more time

- Digitize the actual chart pixels (log-exposure, density) instead of a
  shape-consistent reconstruction — would tighten the curve accuracy
  materially, especially the toe.
- Source or measure actual per-channel RMS granularity / grain power
  spectra instead of inferring an ordering from MTF response.
- Model halation radius from the actual ESTAR base thickness (0.10 mm) and
  a scattering angle rather than a free UI parameter in pixels — right now
  `halation_radius` is an image-space blur radius the user sets directly,
  with no physical units attached.
- Gamma-correct/linearize before the curve lookup. Right now the input's
  0..255 value is mapped linearly onto the log-exposure axis, treating the
  incoming (likely sRGB-ish) image values as if they were linear scene
  exposure. Real film response is driven by actual scene exposure, not
  gamma-encoded pixel values, so this is a simplification shared with
  every other effect in this file (e.g. `ToneCurve`, `bcg`), not unique to
  film emulation — but it's the least physically accurate part of the
  otherwise curve-accurate pipeline.
- Couplers / inter-layer color masking (DIR couplers) — real Portra 400
  negatives have dye-coupler interactions that desaturate and shift color
  in ways a per-channel-independent curve can't reproduce. `agx-emulsion`
  models this explicitly; this implementation doesn't.
- A second stock (e.g. Portra 160, Ektar 100, Tri-X) to prove out the
  "data addition, not a rewrite" claim in practice.

## Validation

`cargo run --example film_validation` generates synthetic test images
(grayscale ramp, a 7-patch shadow/mid/highlight chart, a hard specular
highlight on a dark background), runs the effect through the real
`apply_effects` pipeline (not a reimplementation), saves before/after PNGs
to `validation/images/`, and runs quantitative checks — see
`validation/REPORT.md` for the latest run's pass/fail results.

All four checks pass as of this writing:

- Grain std across the 7 patches increases essentially monotonically with
  input level (0.00 → 4.94 → 7.49 → 9.39 → 11.44 → 13.44 → 13.35), which is
  the sqrt(density) relationship the grain model claims, not just "noise
  everywhere."
- The halation bloom source is exactly zero for luminance at or below
  `halation_threshold` and strictly increasing above it (checked against
  the formula directly, independent of blur spread).
- Isolating halation's own contribution against a curve-only baseline: a
  pixel just outside a hard highlight picks up delta ≈ (+35R, +12G, +2B) —
  clearly red-dominant, matching `halation_color` — while a far corner
  outside the bloom radius is unchanged (delta = 0.000).
- A neutral gray input (127,127,127) comes out non-neutral after the
  curve alone (97, 102, 87 at the ramp's midpoint) — confirms the
  color/contrast shift is coming from genuinely different per-channel
  curves, not a tint layered on top.

The validation run itself caught a real bug before any of this got
written up as working: the first version inverted brightness (dark inputs
rendered white and vice versa), because the density-to-display mapping
subtracted from 1 as if reproducing the intermediate negative's
appearance, rather than the final print/scan appearance. Fixed in
`src/effects.rs` (`curved[ch] = frac`, not `1.0 - frac`) — see that
commit's message for the reasoning. Worth noting because it's exactly the
kind of directionally-plausible-looking bug that's easy to miss by eye on
one test image and would have been shipped without the quantitative
far-corner/near-highlight halation check flagging an unexplained
160-value delta where ~0 was expected.
