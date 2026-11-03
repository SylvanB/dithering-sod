# Dithering Tool

A desktop image processing app built with Rust and [egui](https://github.com/emilk/egui). Apply dithering algorithms, halftone effects, and a composable pre-processing pipeline to any image — then export the result.

## Features

### Dithering
- **Threshold** — hard cutoff with adjustable bias
- **Ordered (Bayer)** — 2×2, 4×4, 8×8, 16×16 matrices
- **White Noise** — deterministic per-pixel hash noise
- **Error diffusion** — Floyd-Steinberg, Atkinson, Burkes, Jarvis-Judice-Ninke, Stucki, Sierra, Sierra Lite, Sierra Two-Row, Shiau-Fan
- **Serpentine scanning** toggle for error-diffusion algorithms

### Halftone
- Grayscale single-screen and CMYK four-screen modes
- Dot shapes: Circle, Square, Diamond, Line
- Adjustable cell size, angle (grayscale), and dot scale (allows overlap)
- Standard print angles for CMYK (C=105°, M=75°, Y=90°, K=45°)

### Pre-processing effects stack
Effects are applied in order before dithering. Any number of effects can be stacked, reordered, toggled, or removed:
- **Brightness / Contrast / Gamma**
- **Tone Curve** — piecewise-linear with 5 draggable control points
- **Posterize**
- **Sharpen** — unsharp mask
- **Pixelate**
- **JPEG Glitch** — encode/decode round-trip at low quality
- **Chromatic Aberration** — per-channel RGB shift

### Palettes
- 1-bit B&W through 4-bit gray ramps
- Retro system palettes: Game Boy, Game Boy Pocket, CGA, PICO-8, C64, ZX Spectrum, EGA, Apple II, Teletext, Amber Phosphor, Green Phosphor
- **Extract from image** — median-cut algorithm, configurable color count
- **Custom** — manual swatch editor with GIMP `.gpl` import/export

### View modes
- **Split** — original and output side-by-side
- **Compare** — draggable divider overlay
- **Output** — output only

### Input downscaling
Optionally cap the input image to a maximum dimension (128px–2048px) before processing, using Lanczos3 resampling. Useful for speeding up rendering on large source images.

### Export
- Save as PNG, BMP, or JPEG
- Nearest-neighbour upscale on export (1×, 2×, 4×, 8×)
- Copy to clipboard

## Building

Requires Rust 1.75+ and the standard native platform dependencies for egui/eframe.

```sh
cargo build --release
cargo run --release
```

## Validation

The repository includes a deterministic film-emulation validation example. It
generates before/after fixtures and checks tone curves, grain, and halation:

```sh
cargo run --example film_validation
```

Generated fixtures and the validation report live in `validation/`.

## Project structure

```
src/
  main.rs        — entry point, window setup
  app.rs         — DitheringApp: UI, state, background render loop
  dither.rs      — dithering algorithms and error-diffusion engine
  halftone.rs    — halftone rendering (grayscale + CMYK)
  effects.rs     — pre-processing effects pipeline
  palette.rs     — palette definitions, median-cut extraction, GPL I/O
  preprocess.rs  — legacy pre-processing module (superseded by effects.rs)
```

## Architecture notes

All image processing (effects + dither/halftone) runs on a background thread via an `mpsc` channel. The UI thread clones the current parameters, spawns the worker, and polls for the result each frame. Only one render runs at a time; parameter changes while a render is in progress queue another pass automatically.

## License

This project is licensed under the [MIT License](LICENSE).
