# Plan: Share Once, Melt Away Forever

Turn RustyParts into a **shareable social “moment”** that plays **once** when opened, then **melts away forever** (no replay of that same view).

---

## 1. What “it” is

- **Current**: Interactive particle field with intro (title forms → holds → melts → “Touch!”), then full controls.
- **Target**: A **shareable link** that:
  - Opens to a **single, automatic playback**: your message forms in particles, holds, then melts away.
  - After the melt, the experience is **over** for that view (no “play again” of the same content).
  - Optional: **true one-time link** (first open plays; any later open sees “already gone”).

---

## 2. Shareable over social media

### 2.1 Shareable URL

- **Same app, two modes** (or a dedicated route):
  - **Playground** (current): full UI, replay, all controls.
  - **Story / moment**: minimal UI, one-shot playback, optional “gone” state.
- **URL shape** (e.g.):
  - `https://yoursite.com/` → playground.
  - `https://yoursite.com/m?t=Hello` or `.../moment#Hello` → story mode with message `Hello`.
- **Query/hash**: `t` or `text` (or hash) = pre-filled message for the particle shape (max length already 18 in app). Optional: `theme`, `fx` for preset.

### 2.2 Social preview (Open Graph / Twitter)

- **og:title**: e.g. “A moment” or the shared message (truncated).
- **og:description**: e.g. “Open to watch it once, then it’s gone.”
- **og:image**: Static or pre-rendered image (particle blob, logo, or “open to see your message”).
- **og:url**: Canonical URL of the moment (with same `t=` so shares show the right preview).
- **Twitter card**: summary_large_image or player if you ever add a tiny preview.

**Caveat**: For **per-message** OG images you’d need either:
- Server or serverless that renders an image per `t=`, or
- A single default OG image that says “Open to see a message that melts away.”

---

## 3. Plays once

### 3.1 Story mode flow (client-side)

1. **Entry**: URL has story param (e.g. `?m=1` or path `/m`) and optional `t=YourMessage`.
2. **Start**: No or minimal HUD. Use `t` as shape text (or default “RUSTY PARTS” / “Touch!”). No manual Form/Melt buttons in this mode.
3. **Timeline** (reuse existing intro logic):
   - **Form**: Particles form the message (reuse `SHAPE_FORM_DURATION_S` / snap + shape mix).
   - **Hold**: Message stays visible for a fixed time (e.g. `INTRO_HOLD_S` or configurable 2–4 s).
   - **Melt**: Trigger melt (same as current intro melt + burst).
   - **Done**: Transition to “gone” state (see below). No switch to “Touch!” interactive mode.

### 3.2 One-shot enforcement (client-only)

- **In story mode**, after “melt done”:
  - Set a flag in **localStorage** (e.g. key = `rustyparts_seen_` + hash of the **full URL** or `url + t`).
  - Show **“Gone” screen**: e.g. “This moment is gone.” and optionally “It was only for you, once.”
- **On load** in story mode:
  - If `localStorage` already has the “seen” flag for this URL:
    - **Don’t** run the particle app (or don’t load WASM).
    - Show only the “already seen” screen (same “gone” message, no replay).

So: **plays once per URL per browser**, then “melts away forever” in that browser.

### 3.3 Optional: true one-time link (server)

- **Backend** (e.g. small Cloudflare Worker or Netlify function):
  - **Create**: POST with `{ "text": "Hello" }` → returns short link `https://yoursite.com/m?id=abc123` and marks `id` as unused.
  - **Open**: GET `/m?id=abc123`:
    - If unused: serve the app with `t=Hello` (or pass text server-side); then **mark id as used** (e.g. on first load or when client calls “mark viewed”).
    - If already used: serve a static “already viewed” page (or redirect to a “gone” page).
- **Result**: Link works **once globally**; every subsequent open sees “already viewed,” even in another browser/device.

---

## 4. Melts away forever

### 4.1 Visual “melt away”

- **Already there**: Your intro melt (particles explode off the shape) is the “melts away” moment.
- **After melt** in story mode:
  - **Option A**: Freeze the last frame (scattered particles) and fade to black/dark, then show copy: “This moment is gone.”
  - **Option B**: Fade canvas out and show a full-screen “gone” message (no canvas).
  - **Option C**: Short “dust settling” (e.g. 1–2 s), then fade to “gone” screen.

### 4.2 “Forever” semantics

| Approach              | Meaning of “forever”                         |
|-----------------------|----------------------------------------------|
| **Client-only (localStorage)** | Once per URL in this browser; no replay here. |
| **Server one-time link**       | Once per link, globally; any reopen = “gone”. |
| **No storage**                 | Poetic: “that view” is gone; refresh = new view. |

Recommendation: **Start with client-only** (simplest, no backend). Add **server one-time** later if you want true “one view per link” for sharing.

---

## 5. Implementation checklist

### Phase 1 – Story mode (plays once, client-only “forever”)

- [ ] **URL parsing**: Detect story mode (e.g. `?m=1` or `/m`) and read `t=` (or hash) for message; pass into Rust (e.g. via `init` or a `#[wasm_bindgen]` setter called from JS after parsing).
- [ ] **Rust**: Add a “story mode” flag (e.g. from `App::new` or early JS call). When true:
  - Use URL message (or default) as the only shape text; no “Touch!” switch after intro.
  - After intro phase 2 (melt) completes, set a “story_done” flag and **notify JS** (e.g. `story_complete()` callback or custom event).
- [ ] **JS**: On `story_complete`: write `localStorage` key for this URL; replace canvas with “gone” view (or overlay full-screen message and stop animation loop).
- [ ] **JS**: On load in story mode: if localStorage says “seen” for this URL, don’t start WASM (or start and immediately show “gone”); show “This moment is gone.” (and optionally “You’ve already seen this.”).
- [ ] **UI**: In story mode hide or simplify HUD (no controls panel, no text-entry dot, or only a minimal “…” until gone).

### Phase 2 – Shareable links and preview

- [ ] **OG / Twitter meta**: Add `og:title`, `og:description`, `og:image`, `og:url` (and Twitter equivalents). Prefer dynamic if you have a server (e.g. `/m?id=xyz` sets title/description from message).
- [ ] **Copy link**: Optional “Copy link” or “Share” button that appears **before** the moment plays (or after, for “share your own”), with the current URL (including `t=`).
- [ ] **Base URL**: Document the canonical base URL (e.g. for Trunk deploy or static host) so shares look correct.

### Phase 3 (optional) – True one-time links

- [ ] **Backend**: Tiny service that creates short IDs and stores “viewed” state; GET `/m?id=...` serves app + message or “already viewed” page.
- [ ] **Client**: On first frame or on “story_complete”, call backend to “mark viewed” for `id` so future opens get “already viewed” from server.

---

## 6. File / surface changes (summary)

| Area        | Changes |
|------------|---------|
| **index.html** | Optional OG/Twitter meta; maybe a second “story” HTML or same page with different root state. |
| **JS (inline or small .js)** | Parse `?m=1&t=...` or `/m#text`; call into WASM to set message and story mode; on `story_complete` set localStorage and show “gone”; on load in story mode check localStorage and skip WASM or show “gone” only. |
| **Rust (lib.rs)** | Story mode flag; use provided message through intro and don’t switch to “Touch!” when story; after melt in story mode call `story_complete()` (exported to JS). Optional: `set_message(text)`, `set_story_mode(bool)` from JS. |
| **styles.css** | Styles for “gone” screen (full-screen message, no canvas). |
| **Backend** (optional) | Small serverless create + resolve + mark-viewed for one-time links. |

---

## 7. Suggested order

1. **Story mode in Rust**: flag + single timeline (form → hold → melt) and `story_complete()` callback; no “Touch!” after melt in story mode.
2. **URL + JS**: Parse `m` and `t`; set message and story mode before or right after WASM start; on `story_complete` set localStorage and show “gone” screen.
3. **“Already seen”**: On load, if story URL and localStorage key present, show “gone” only (no WASM run or immediate “gone”).
4. **OG meta** and share UX (copy link, default image).
5. **Server one-time** only if you want link-level “once ever” guarantees.

This keeps the current app intact (playground by default) and adds a parallel “moment” experience that’s shareable, plays once, and melts away forever in the sense you choose (per-browser or per-link).
