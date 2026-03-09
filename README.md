# RustyParts

Interactive particle-field app built with Rust + WebAssembly and rendered with WebGL2.

## Features

- High-density particle simulation on GPU
- Shape form/melt interaction
- Brush modes: push, pull, vortex
- FX presets and color controls
- Attractor controls and live particle count display
- **Shareable moment**: one-shot link that plays once and melts away (see below)

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

## Shareable moment (story mode)

Share a link that plays **once** and then shows “This moment is gone.”

- **URL**: `?m=1` for story mode; optional `&t=Your+Message` (up to 18 chars, forms then melts).
- **Example**: `https://yoursite.com/?m=1&t=Hello` — opens to your message forming, holding, then melting; after that, reopening the same link in that browser shows only “This moment is gone.”
- **OG/Twitter**: Basic meta tags are set for social previews.

## Mobile

- **Haptics**: Touch interactions trigger short vibration (light on touch, pattern on release burst). Form/Melt buttons trigger medium haptics. Requires a device and browser that support the Vibration API (e.g. Android Chrome; not supported in Safari).
- **Tilt / motion**: Particles react to device orientation. Tilting the phone adds a gravity-like bias so the field responds to motion. Uses `devicemotion` (accelerometer); on iOS 13+ the browser may prompt for motion permission.

## Project Files

- src/lib.rs: Rust app logic + shader sources
- styles.css: UI styling
- index.html: Trunk entry page and controls
