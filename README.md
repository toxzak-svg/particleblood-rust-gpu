# RustyParts

Interactive particle-field app built with Rust + WebAssembly and rendered with WebGL2.

## Features

- High-density particle simulation on GPU
- Shape form/melt interaction
- Brush modes: push, pull, vortex
- FX presets and color controls
- Attractor controls and live particle count display

## Run (Web)

From workspace root:

```bash
cd rustyparts
trunk serve
```

Then open the local URL shown by Trunk.

## Build Check

```bash
cargo check
```

## Controls

- Pointer move: apply brush force
- Hold click/touch: enable attractor at pointer
- Tap/click: toggle shape visibility
- Wheel: adjust attractor mass
- Q/W/E: brush mode
- Z/X/C: quality preset
- 1/2/3: FX preset
- Enter: form shape text
- M: melt shape
- T: switch shape layout

## Project Files

- src/lib.rs: Rust app logic + shader sources
- styles.css: UI styling
- index.html: Trunk entry page and controls
