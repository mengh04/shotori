# Shotori — Architecture Decisions & Roadmap

Archive of conclusions from the research phase (before any code was written).
Principle: **trust live wayland-info probes over docs and over memory.**

## Environment (measured 2026-09-25)

- Arch Linux + niri 26.04 (zwlr_layer_shell_v1 v5, zwlr_screencopy_manager_v1
  v3, both old and new foreign-toplevel, virtual-pointer, data-control — all
  present)
- Mixed-DPI multi-monitor: HDMI-A-1 1920×1080@1.0 (0,0) + eDP-1 logical
  1536×960@1.25 (1920,0)
- `ext-image-copy-capture`: only merged into niri main on 2026-09-13 (and
  without window capture at that) — 26.04 **does not have it**. The official
  wiki documents main; don't be fooled.

## Capture backend matrix (origin of the CaptureBackend trait)

| Path | Protocol | Fits | Notes |
|------|----------|------|-------|
| ① portal | xdg-desktop-portal ScreenCast + PipeWire | every desktop (GNOME's only option) | consent dialog, video-stream frame extraction |
| ② wlr | zwlr_screencopy-unstable-v1 | niri/sway/Hyprland etc. | what grim uses; today's workhorse |
| ③ ext | the ext-image-copy-capture family | the standardized future | waiting for distro rollout; includes window capture |

Window geometry: foreign-toplevel gives a list but no coordinates →
prototyping used the `niri msg --json windows` backdoor.

## Overlay (settled in lecture #2)

- A plain xdg window as an overlay is a disaster under a tiling compositor
  → layer-shell or nothing
- **gpui-pre 0.3.6 has `WindowKind::LayerShell(LayerShellOptions)`** (nobody
  in the gpui-kit ecosystem had used it; we went first — spike #1 verified)
- The toolbar must live inside the overlay window (layering deadlock: a
  normal window would be buried under its own dim layer)
- Pin (floating image) = a `Layer::Top` layer-shell window (Wayland has no
  "keep normal window on top" protocol)

## Spike #1 verification points

1. Can a layer-shell window open on niri at all (protocol handshake)
2. Is `WindowBackgroundAppearance::Transparent` actually transparent (EGL
   alpha)
3. Does the Esc focus chain work under `KeyboardInteractivity::Exclusive`
4. Does the `exclusive_zone: Some(px(-1.))` negative sentinel map to the
   protocol's -1 correctly
5. Do four-edge anchors + configure cover the whole output (including a
   1.25-scale screen)

## Findings log

### Closed cases (2026-09-25)
- **The Root/CSD poisoning case**: base::Root's WindowState plugin
  (component) paints a themed background onto layer-shell windows (white
  wall → gray haze 76 = 0.3×255), WindowBorder calls set_client_inset(20)
  (window grows by +40), plus inward padding. Overlays must always use a
  bare cx.open_window; normal windows can use Root.
- **The 240Hz invisibility case (final ruling, late 2026-09-25)**: nothing
  to do with the refresh rate — the real culprit was **layer-surface
  placement lottery**. When gpui doesn't pin a layer surface to an output,
  niri picks one (the focused screen); the overlay sometimes landed on DP-2
  (720×1280) while grim photographed HDMI — hence "invisible". The 60Hz
  test windows happened to land on HDMI, manufacturing the
  "refresh-rate-related" illusion. Fix: pass `display_id` when opening the
  window to pin it to the captured screen (the upstream pipeline already
  existed: WindowOptions.display_id → wl_outputs match →
  get_layer_surface(output)). **Lesson: in a mixed-scale multi-monitor
  setup, when "some screen can't be captured", check placement before
  rendering.**
- **The true shape of empty displays() at startup (zed#46378)**: always
  empty during synchronous startup, but all 3 screens appear after the
  first event-loop pass (first cx.spawn update). Workaround: move window
  opening into a spawn'd async task; first shot gets it. display_id
  matching: bounds size == capture size (exact at scale=1; mixed-scale
  matching needs the vendor to expose output names — backlog).
- **scale_factor() misreports across monitors**: window pinned to HDMI
  (rendering at 1.0) while `window.scale_factor()` reports 1.5 (DP-2's).
  Cropping instead computes "captured physical width ÷ window logical
  width", naturally consistent with rendering.
- **niri's zwlr_virtual_pointer appears dead**: motion_absolute/motion/button
  all vanish (WAYLAND_DEBUG confirmed requests go on the wire, zero client
  events; with output/without, absolute/relative — all the same). The lab's
  vinput test bench is therefore unusable; no GUI click automation for now
  (keyboard side untested). Worth reporting upstream.
- **The RenderImage contract**: BGRA bytes (Vulkan backend); feeding memory
  directly requires swap(0,2); the PNG path is RGBA.
- **wl_shm format is an ordinal** (xrgb8888=1), not a DRM fourcc; the
  format name describes the word's byte order (MSB→LSB), little-endian
  memory is reversed.
- **Multi-monitor output selection**: upstream zed#46378 (displays() empty
  at startup), fix PR #61578 stuck in review; when vendoring, consider
  taking the roundtrip patch along.

### Debug backdoors
- `SHOTORI_DEBUG_SELECTION=x,y,w,h`: inject a ready-made selection (for
  automated selection-UI verification)
- `SHOTORI_DEBUG_ACTION=copy|quit|save|ocr|ocrsetup`: fire the action 1.5s
  after startup — the only entry point for headless e2e (recipe:
  `SHOTORI_DEBUG_TARGET=HDMI-A-1 SHOTORI_DEBUG_SELECTION=...
  SHOTORI_DEBUG_ACTION=copy ./shotori & sleep 4; wl-paste --type
  image/png | size assertion`)
- `SHOTORI_DEBUG_TARGET=<output name>`: restrict the backdoor to one
  overlay (multiple overlays all firing fight each other)

## v0.3 multi-monitor support (2026-09-26 early AM)

### Features
- capture_all_outputs(): one connection captures every output (three
  screens ~350ms including encoding); one Capture per screen
- **One overlay window per output** (display_id pinning), selection/crop/
  copy independent; Enter/Esc act on "the screen you're interacting with"
  (niri's exclusive-layer keyboard focus follows the focused output —
  **user-verified live**)
- The self-computed crop scale naturally handles per-screen scales
  (eDP 2.0 / DP-2 1.5 / HDMI 1.0)
- screencap --all: one debug image per screen

### Closed record (multi-monitor)
- **gpui display bounds coordinates = output logical position ÷ wl_output
  integer scale** (the backend does the division; measured by comparison:
  eDP 1920,0→960,0; DP-2 -720,-100→-360,-50). Display matching must use
  the same algorithm.
- **wl_output.scale is an integer**: a 1.5x screen reports 2 (ceil); the
  true value needs the fractional protocol (unavailable per-output). Size
  matching is therefore infeasible — match on position (layout origins are
  unique).
- **Transform semantics, measured**: niri's "90° counter-clockwise"
  (Transform::_90) actually fills the panel by rotating the buffer
  **clockwise** 90° — opposite of the protocol wording. rotate_rgba was
  calibrated against grim; the Flipped family is rare and unhandled.
- **Lesson: wallpaper rotation destroys comparison testing** — DP-2's photo
  wallpaper changes orientation; cross-time grim comparisons correlate as
  badly as 0.54. Verification posture: grim→screencap→grim within a
  one-second window, three-way compare.
- Remaining limits: selections can't span screens; rotation+flip combos
  (Flipped90 etc.) unimplemented; keyboard behavior of multiple Exclusive
  overlays on non-niri compositors unknown.

## v0.2.1 clipboard copy (2026-09-25, late night)

### Features
- Enter / Ctrl+C / toolbar [Copy]: selection → PNG → clipboard → exit; no
  selection = full screen
- Ctrl+S / toolbar [Save]: to disk (the original Enter behavior); toolbar
  becomes [Copy][Save][Cancel] (pin button deferred)
- **Resident-offer model** (same as wl-copy): copy = re-exec ourselves as
  a `--clipboard-daemon` twin, PNG bytes via stdin; the twin serves pastes
  as a `zwlr_data_control` source and exits on `cancelled` when replaced
- wayland-rs pitfall: the compositor creates new objects inside
  data_offer events on the client's behalf; the parent interface must
  specialize `event_created_child` (default panics; the
  `event_created_child!` macro fixes it in one line)
- Fully automated e2e verified: byte-exact read-back / repeated pastes /
  old-new twin replacement / full chain (600×400 exact after display
  pinning)

### Fixed along the way (details above)
- Overlay/pin display_id pinning (the placement-lottery bug, the "240Hz
  invisibility" culprit)
- Self-computed crop scale (scale_factor() multi-monitor misreport)
- Toolbar button clicks still unverified by a real mouse (vinput dead) —
  propagation-chain analysis says fine; pending day-to-day confirmation

## v0.2 basics cleanup (2026-09-25)

### Modularization
overlay.rs (408 lines) split into: `selection.rs` (state machine, pure
logic + tests), `export.rs` (crop/PNG/disk, pure functions + tests),
`toolbar.rs`, `image_util.rs` (BGRA contract in one place, deduped for
pin). overlay keeps only assembly. First 12 unit tests (no compositor
needed).

### Behavior fixes (v0.1 → v0.2)
- Dragging up-left produced negative-width "invisible selections"
  (`Bounds::from_corners` doesn't normalize) — a latent bug the tests
  flushed out; fixed
- In-place click (<2px) = clear selection; no more 0×0 selection +
  toolbar weirdness
- Two-stage Esc: while dragging = abandon this drag; after release = exit
- Enter with no selection = save full screen
- Save failures no longer panic: print the error, stay in the overlay for
  retry
- Filenames `Shotori_<date>_<time>.png`, same-second conflicts get
  `_2`/`_3`
- Toolbar only appears after release (no flicker while dragging)

### Suspended: pin (floating image)
Dragging to edges has a bug (suspects: ① moving reference frame in
window-relative coordinates ② niri clamping out-of-bounds margins ③
whether implicit grab keeps delivering once the cursor leaves the small
window). Test bench ready: the lab's `vinput` (zwlr_virtual_pointer,
can fully automate drags). When resuming: the user's remote mouse will
fight the virtual mouse.

### Pending manual acceptance (user back at the screen)
- Two-stage Esc feel, click-to-clear, Enter-full-screen, new filenames
- Toolbar button clicks (the earlier virtual-pointer "pin" click didn't
  trigger; coordinates corrected to the button text cluster center
  x≈298 — to re-verify)

## v0.3.1 refactor: anti-big-ball-of-mud (2026-09-26 early AM)

- capture.rs (449 lines) split into capture/{mod,wayland,pixels}:
  orchestration / event state machine / pure pixels
- display.rs created: display matching moved out of main.rs; the match
  predicate unit-locks "gpui coords = position ÷ integer scale"
- hud.rs created: dim_strips/selection_chrome/hint_bar moved out of
  overlay.rs
- overlay.rs backdoors split into debug_targeted/debug_selection/
  spawn_debug_action private functions
- **A unit test caught a real bug**: rotated_size's _180 fell into the
  catch-all (180° would wrongly swap width/height; it had fake-passed via
  unwritten memory) — fixed + four-corner assertions locked
- pin_selection cropping switched to the self-computed scale (a leftover
  of the scale_factor() misreport path)
- Dead code capture_first_output removed; tests 12 → 21

## Release route (2026-09-26)

- **Local install**: `cargo install --path . --bin shotori` →
  ~/.cargo/bin (no blockers)
- **crates.io**: three blockers ① publish=false ② [patch.crates-io] local
  paths (forbidden on crates.io) ③ set_layer_margin unmerged upstream.
  Route: gate pin behind a feature → drop the patch in patchless builds →
  publishable
- **AUR / GitHub Release**: more realistic for Arch users; a PKGBUILD can
  carry the vendor patch
- User context: niri `Mod+Shift+S` was originally bound to the old shotori;
  this project took over the binding

## v0.4.0 renamed shotori + cancel/Esc fix (2026-09-26)

- Project renamed saccade → shotori (the user kept the old tool's name;
  the old ~/Projects/shotori source stays untouched, the cargo bin just
  gets overwritten)
- **Fixed: Cancel button / Esc doing nothing** — located with a real user
  mouse + probe logs: the button click chain works end-to-end (container
  intercept → on_click → dispatch_action → overlay handler), but a
  dispatch_action inside a gpui window **stops at the end of the focus
  path and does not bubble to App::on_action** — the exit logic was riding
  on an app-level backstop and had been dead since the toolbar was born.
  Fix: two-stage Esc handled right in the overlay handler (dragging =
  cancel drag, otherwise quit)
- Backdoor upgrade: SHOTORI_DEBUG_ACTION=copy|quit (quit goes through the
  real dispatch_action pipeline, making exit e2e-testable)

## v0.5.0 pin removed to unlock publishing (2026-09-26)

- pin moved wholesale to the `pin` branch (including the vendored
  set_layer_margin patch dependency)
- main drops [patch.crates-io] (upstream gpui-pre), publish=false, and
  gains crates.io metadata (license/repository pending user confirmation)
- `cargo publish --dry-run --allow-dirty` passed: no path deps, packaging
  compliant
- Real publish: `cargo login` → `cargo publish`

## v0.6.0 selection OCR (2026-09-26)

### Features
- Ctrl+O: selection → PP-OCRv6 small (rapidocr-core + ort/ONNX Runtime) →
  text to clipboard
- First use auto-downloads models from ModelScope (4 files, ~31MB) to
  `~/.local/share/shotori/ocr-models/`; subsequent calls are instant
  (engine stays resident via OnceLock<Mutex>)
- feature gate: `--features ocr`; default build has zero added weight
  (crates.io publish unburdened by heavy deps)
- Clipboard daemon generalized: `--clipboard-daemon <MIME>`;
  copy_image/copy_text share the twin framework; text offers
  `text/plain;charset=utf-8` with a `text/plain` fallback

### Choice record (why rapidocr-core)
- Candidates: rapidocr-core (ONNX/ort) vs rusto-rs (MNN) vs paddle-ocr-rs
- rusto-rs's mnn-sys build chain is three-strategy
  (vendor/prebuilt/source) + bindgen/cmake — fragile
- rapidocr-core: `run_image(&RgbImage)` takes in-memory pixels directly;
  mature model-cache machinery (ModelCache + SHA256 verification); ort
  auto-downloads a prebuilt libonnxruntime at build time, statically
  linked
- Accuracy: PP-OCRv6 small is solid on mixed Chinese/English; text under
  ~16px on a 1080p screen struggles (HiDPI screens do better — more
  physical pixels)

### e2e verification record
- First run: models auto-downloaded ✓ → full-screen OCR → clipboard text
  read back (terminal content, mixed CN/EN transcribed) ✓
- Second run: instant, complete logs ✓
- Exact selection 1500x800 → 32 lines, 612 chars ✓ (line structure
  preserved)
- Image copy regression 600x400 ✓ (daemon rework broke nothing)
- Real user Ctrl+O acceptance: file sidebar 631x328 → 14 lines ✓
- Known niggles: stdout redirected to a file is fully buffered — SIGTERM
  kills drop the last log lines (no impact in foreground use); missing
  first characters are usually selection edges cutting glyphs, not model
  issues

### Notes
- SHOTORI_DEBUG_* env vars don't leak between `opencode run` shells
- No GUI preview of OCR results in v1 (straight to clipboard); floating
  preview/edit is a candidate

## v0.6.1 OCR implementation review (2026-09-26)

### Review findings, fixed
- **🔴 Corrupt model bricked OCR forever**: rapidocr-core's download_asset
  writes straight to the target file (no temp+rename); an interrupted
  download leaves a truncated file → every later sha256 check fails →
  permanent error. Fix: wipe the model cache dir when initialization
  fails; the next attempt re-downloads from scratch (tested: plant a
  garbage file → sha mismatch error → dir cleaned → rerun auto-
  re-downloads successfully)
- **🔴 init via panic behaves unpredictably**: get_or_init + expect on
  failure paths (first run offline, unwritable dir) panics through gpui's
  background executor. Fix: init_engine returns Result, failures are not
  cached (OnceLock stays unset) → the overlay prints the error and stays
  usable; the next Ctrl+O retries. Tested: XDG_DATA_HOME pointed at an
  unwritable path → clean error, process alive, Esc works
- **Self-inflicted bug**: during the refactor I dropped the ENG.set() —
  download + engine construction succeeded and then got thrown away,
  reporting "vanished after init". Caught by e2e (the retry-success
  path); fixed
- **🟡 hint bar / keybinding feature gating**: non-ocr builds no longer
  advertise or bind Ctrl+O (OCR later became a default feature; the gate
  remains as the slim-build exit)
- **🟡 text paste compatibility**: text mode now also offers
  UTF8_STRING/STRING (old xwayland apps); the Send handler writes on any
  offered-MIME hit
- **🟢 first-download feedback**: prints "downloading PP-OCRv6 small
  models…" before starting
- **🟢 the real reason for the dedicated download thread**:
  reqwest::blocking cannot run in an async context (gpui's background
  executor is one) — comment corrected

### Design change: OCR became a default feature
- `default = ["ocr"]`: crates.io/AUR users get full functionality on a
  bare install; OCR is no longer a hidden feature
- Slim-build exit: `--no-default-features`
- Rationale: product identity = screenshots + OCR; next to the gpui dep
  tree, ort+reqwest are a rounding error

### Review methodology notes
- Failure paths (offline / corrupt files / retries) are mandatory testing
  for lazy-loading designs; success-path e2e is not enough
- "wl-paste -l suddenly lost MIMEs" → suspect yourself first, then the
  compositor, and last remember the user is also using the computer
  (their copies replace test state)

## v0.6.2 toolbar OCR button + full English codebase (2026-09-26)

### Features
- Toolbar gains [OCR]: [Copy][Save][OCR][Cancel], same dispatch_action
  pipeline as the keyboard; compiles away without the feature
  (`.children(Option)`)
- Hint bar / buttons / logs all English (UI faces international
  crates.io/AUR users)

### English-conversion scope & principles
- All 16 files under src/: doc comments, inline comments, string
  literals, test function names
- Translation preserved all the war stories (the wayland-rs pits, the
  gpui pits, the grim calibration, …) — language changed, content kept
- Both feature combos build/test/clippy green; OCR/copy e2e regressions
  pass

### Pit notes
- `#[cfg]` cannot hang in the middle of a method chain (an attribute on
  a `.child()` link isn't legal Rust) — absorb via `.children(Option<E>)`
  (children takes an IntoIterator; Option is one)
- gpui-kit doesn't implement IntoElement for Option<impl IntoElement>
  (upstream gpui does), nor for Infallible — the non-ocr stub returning
  Option<&'static str> is the cheapest way out

## v0.6.3 OCR speedup: prewarm + skip re-hashing (2026-09-26)

### Problem
- One-shot process × in-process engine cache = full cold start on every
  Ctrl+O (read 31MB from disk + build 3 ort sessions + sha256-hash 31MB);
  measured OCR net time 1464ms (release, 800x400 selection, 34 lines)
- Diagnostic methodology: debug builds run the pure-Rust pre/post
  processing 10-100× slower (10.8s) — benchmark with the release install
  (3.36s full chain)

### Fixes
- **A. Prewarm**: the overlay warmups in the background on open (own
  thread, only when models are already cached — a first-ever run must not
  surprise-download 31MB during a plain screenshot). The init hides
  inside the user's 2-5s of drawing a selection
- **B. Skip the repeated sha256**: when all model files exist, skip
  `ensure_*` entirely (it re-hashes all 31MB per call); corruption
  detection is instead covered by "engine init fails → clean cache"
- Effect: worst-case (debug hook) OCR net time 1464ms → 890ms; in real
  use (2-5s drawing) Ctrl+O leaves only inference, ~300-500ms

### Self-healing upgrade (unexpected bonus)
- A corrupt model file is now silently digested by the warmup thread:
  warmup hits the corruption → init fails → cache cleaned; the subsequent
  real OCR finds the cache empty → auto re-downloads → the user never
  sees it (v0.6.1 required an explicit error + manual retry)

### Notes
- `let _ = engine()` trips the let_underscore_lock lint (even for
  deliberately dropping a lock) — explicit `drop(engine())` states the
  intent

## v0.6.4 first-download: confirm dialog + progress bar + cancel (2026-09-26)

### Features
- Ctrl+O with no models: centered confirm card ("OCR needs models",
  showing ~31MB, ModelScope source, storage path) → [Download] / [Cancel]
- While downloading: byte-accurate progress bar (total = Σ
  content-length, growing as each file starts), current file name +
  (2/4) + MB readout; Esc/[Cancel] aborts anytime
- Failure card [Retry]/[Close]; on success the selection snapshot frozen
  at Ctrl+O time is fed to OCR automatically
- Model location: ~/.local/share/shotori/ocr-models/ (XDG_DATA_HOME
  respected). Reset for testing: `rm -rf ~/.local/share/shotori/ocr-models`

### Implementation
- src/ocr_setup.rs (new): Stage state machine (Confirm/Downloading/
  Failed) + card rendering; actions OcrSetupConfirm/OcrSetupCancel ride
  the same pipeline as keyboard actions
- ocr.rs: own downloader replaces ensure (progress/cancel hooks), writes
  via **temp + atomic rename** (half-written models become impossible) +
  post-download sha256 verification (sha2)
- overlay: the dialog is modal (Enter/Ctrl+S/copy/new selections all
  blocked), Esc = cancel; an 80ms poll loop drives the bar via the Entity
  handle + notify; completion handover goes through entity.update
- warmup unaffected (it already skips when models are missing)
- reqwest/sha2 both under the ocr feature; slim build unchanged

### Pit records
- window_handle.update's closure receives an AnyView (can't touch the
  concrete view's fields) — touching view state from async requires the
  Entity handle's entity.update
- gpui-kit's Entity::update return = the closure's return passed through
  (not zed's Result wrapping); returning () trips clippy's
  let_unit_value
- Almost put #[cfg] mid-chain again (render layer ⑥) — precompute an
  Option<AnyElement> and .children() unconditionally is the idiom

### e2e
- ocrsetup backdoor: 1.5s opens the dialog → 6s auto-[Download] →
  download → OCR → clipboard (verified after deleting models; no .part
  leftovers)
- The old headless path (DEBUG_ACTION=ocr) remains: still downloads
  inline; regression passes

## v0.6.5 desktop notifications (2026-09-26)

### Features (verified live against noctalia / org.freedesktop.Notifications)
- Save success → "Saved 500×300 → ~/Pictures/Shotori/…png" (the path is
  the real need; stdout is lost when launched from a keybinding — the
  notification is the only feedback)
- OCR success → "N lines → clipboard + preview"; OCR failure → error
  summary (no in-UI error display yet; the notification covers the
  keybinding case)
- Image copy also notifies (user's call: all three exits give uniform
  feedback — Copied WxH → clipboard)

### Architecture: notification child process (the clipboard-twin pattern)
- `shotori --notify <summary> <body>`: the parent spawns it and exits
  immediately; **the detached child outlives the parent** — a plain
  background thread would be killed by the process::exit after
  cx.quit(), cutting the notification mid-send
- notify-rust 4 (zbus/D-Bus); the child fails quietly (one stderr line);
  a missing daemon never affects screenshots

### Notes
- debug backdoor gains a save action (for notification-path e2e)
- Known phenomenon reconfirmed: stdout full buffering on the quit path
  can swallow the last log line (notifications go through the child and
  are unaffected)

## v0.6.6 notification image previews (2026-09-26)

### Features
- Copy/save notifications carry a screenshot thumbnail (image-path hint +
  file:// URL; noctalia renders it ✓ — probed the spec support with a raw
  busctl call before writing any code)
- OCR notifications stay plain text (the preview IS the content)

### Implementation
- notify::send_with_preview(w, h, rgba): raw pixels → image::imageops
  thumbnail (≤256px) → ~/.cache/shotori/preview-<ts>.png → child gets
  the path
- The preview file must outlive the notification: lazy cleanup (each
  send removes previews older than 24h)
- notify child argv extended: --notify <summary> <body> [image]
- Thumbnail-write failure → silently degrades to a plain text
  notification

### Lesson
- The pkill in an e2e script must happen after the action (the 1.5s
  backdoor) fires — otherwise you kill a process that hasn't done its
  work yet; this round's "copy had no preview" was a test race, not a
  code bug (wait for foreground exit before checking)

## v0.6.7 white-line fix + OCR busy badge (2026-09-26)

### 🔴 The white-line bug (pixel-level forensics + fix)
- Symptom: an occasional 1px full-width pure white line just under the
  selection's bottom edge (reported by the user with a screenshot; "only
  at certain positions")
- Forensic chain: pasted-image pixel analysis (the white line is
  sandwiched between two dimmed regions) → orange-rectangle geometry
  reconstruction (line = selection bottom + 1 row; toolbar top = bottom +
  8 ✓ matching the code) → mechanism identified
- Mechanism: remote mice produce fractional selection coordinates →
  dim_strips (4 dim bands) and selection_chrome (border) each round
  independently inside gpui → at certain fractional phases the two
  roundings diverge → a 1px row covered by neither → raw content bleeds
  through (pure white on light backgrounds, invisible on dark — hence
  "only at certain positions")
- Fix: round_px() — all four edges rounded once each (round(l)+round(w) ≠
  round(r); edges must be rounded independently), dim bands / border /
  toolbar share the same integer bounds; the rounding ambiguity is gone
- Verification: a 10-phase fractional sweep (.0-.9) of boundary rows —
  zero leak rows ✅

### OCR busy badge (spinner)
- During inference a spinner badge shows at the selection's center
  ("OCR…" + an orbiting dot)
- gpui with_animation (respects reduce_motion automatically; max_fps 15
  caps redraws)
- ocr_busy flipped via the Entity handle (cleared on both success and
  failure — no eternal spinner)
- Burst verification: absent at 1.6s (not yet triggered) → present at
  1.75s/1.9s (19574 chip pixels) ✅

### Pit records
- Test probes must adapt: after the fix the boundary rounds to 301; a
- hardcoded x=300 orange-line self-check went all false-negative — prove
  "the overlay is up" before probing the target, and don't hardcode
  coordinates in the check itself
- #[cfg] mid-method-chain, third offense (busy_el again) — the dual cfg
  let binding is the only correct posture; burn it into muscle memory
- Capturing animations with grim needs burst frames (a single frame
  misses 0.3s-scale windows)

## v0.6.8 size label stacking vertically (2026-09-26)

- Symptom: on narrow selections (e.g. 22px wide) the "W × H" label wraps
  one character per line into a vertical tower
- Root cause: the label div was a child of the selection border box; its
  auto width was clamped to the selection's width (Taffy clamps the
  fit-content of absolute children to the parent's content box) — a
  narrow selection leaves ~22px of usable width → per-character wrapping
- Fix: selection_chrome now emits two window-anchored elements (border
  box + label); the label is absolutely positioned against the overlay
  root, content-sized, independent of the selection's width
- Verified: a 22×140 selection renders a normal 64×32 horizontal chip ✅;
  border verticals 44/44 ✅

## v0.7.0 release-ready (2026-09-26)

- Version 0.6.0 → 0.7.0 (aggregates: the full OCR suite + notifications/
  previews + the download confirm UI + white-line/label fixes + the
  spinner badge; a major step over the 0.5.x line)
- cargo publish --dry-run passes; the actual publish is done by the user
  (ceremony preserved)
- Repo fully anglicized for publication (ROADMAP included); default
  README in English, Chinese README at README.zh-CN.md
- Outstanding confirmation before publishing: the repository link points
  at a not-yet-created GitHub repo (create it or drop the line)

## Pre-publish snags: reqwest 0.13 / sha2 patch (2026-09-26)

- A stray bump to reqwest 0.13 → the `rustls-tls` feature was renamed to
  `rustls` in 0.13; immediate error. **Decision: stay on 0.12** —
  rapidocr-core also uses 0.12; upgrading would compile two full
  hyper/tokio/rustls trees for the sake of one GET
- Knock-on: the lock re-resolution bumped sha2 to the hybrid-array
  version whose finalize() output no longer implements LowerHex — digest
  formatting is now manual and version-agnostic

## v0.7.0 addendum: screencap removed (2026-09-26)

- The spike-era debug front end (`screencap --all`) is gone from the
  package: the user prefers a single-purpose crate, and it had no runtime
  role. Its calibration history (grim cross-checks) stays recorded above.
- README restructured to the standard layout (badges, features,
  requirements, install, usage, OCR notes, build, license); Chinese
  README mirrors it.

## v0.8.0-dev: save via the system file picker (2026-09-26)

`Ctrl+S` no longer writes to a fixed path — it opens the desktop's native
"save as" dialog (xdg-desktop-portal FileChooser via rfd 0.17 / ashpd, the
`xdg-portal` feature, no GTK link time; zenity fallback if the portal is
dead). Suggested name pre-filled (`Shotori_<date>_<time>.png`, default dir
`~/Pictures/Shotori`), extension re-appended if dropped while renaming.

Flow: the overlay action crops, stashes RGBA pixels in a static slot and
tears the overlays down; after the run loop returns, the main thread runs
the dialog (blocking), writes the PNG and fires the thumbnail notification.
Headless e2e keeps working via `SHOTORI_DEBUG_SAVE_PATH=<file>` (skips the
dialog).

Three measured gotchas along the way, all worth remembering:

1. **A portal dialog cannot coexist with the overlay.** The overlays are
   layer-shell surfaces on the Overlay layer with exclusive keyboard — a
   regular toplevel (the dialog) renders below them and gets no input. The
   overlays must be unmapped first.
2. **`cx.quit()` does not unmap surfaces.** It stops the run loop; the
   window-destroy requests may still be sitting unflushed in the wayland
   connection buffer. With the process exiting immediately (every other
   action path) nobody notices — the socket close cleans up. With the
   process alive waiting on the dialog, frozen frames stay mapped on
   screen forever. Fix: `QuitMode::Explicit`, remove every window, then
   quit from a 150 ms timer so the loop gets a few iterations to flush.
3. **`handle.update()` on the window currently running an action handler
   is a no-op** (gpui takes the window out of the map during its update,
   so the nested update finds nothing). The current window must remove
   itself through its own `window` reference; `close_overlays` therefore
   takes both.

Also: removing the last window auto-quits gpui on Linux
(`QuitMode::Default == LastWindowClosed` off macOS) — irrelevant now that
we set Explicit, but good to know. The fixed-path `export::next_path`
machinery and its collision-suffix logic were deleted (the dialog asks
before overwriting).

### Addendum: superseding the atomic fixed-path writer (2026-09-26)

Between the dialog work starting and landing, a parallel change introduced
millisecond timestamps + an atomic `create_new` collision-suffix writer for
the fixed-path flow (concurrent instances racing on the same second).
Merged resolution: the millisecond stamp lives on in the dialog's suggested
name (`..._%3f`), while the atomic writer itself has no caller anymore —
with a picker in front, overwrite confirmation is the dialog's business and
`save_png` writes the chosen path plainly.

## Label / toolbar: never off-screen (2026-09-26)

A selection touching the top and bottom of the screen had nowhere to put
the size label (below) or the toolbar (below/above) — both ran off-screen
(user-reported with screenshots). Both now have a third fallback state:
drawn INSIDE the selection box, pinned to its top edge. Geometry extracted
into pure anchor functions (`label_anchor`, `toolbar_anchor`) with unit
tests for all three states plus horizontal clamping (which also gained a
max() guard against a clamp(min, max) panic on very narrow windows).

## Label / toolbar placement v2: two disjoint zones (2026-09-26)

The v1 fallback chain (label above→below→inside, toolbar below→above→inside)
kept colliding as user reports rolled in: overlap when both flipped to the
same side, a label that visibly "reserved room" for a toolbar that only
exists after release, and elements glued flush to the screen edge at
exactly-zero margin.

An intermediate fix computed both Y anchors in one six-state matrix —
correct, but the label↔toolbar coupling was the drag-jump bug in disguise,
and the matrix only grew.

Final scheme (user-designed, and better): **two disjoint zones**.
- Label: ABOVE the box; if the box hugs the screen top, inside its
  TOP-LEFT corner.
- Toolbar: BELOW the box; if the box reaches the screen bottom, inside its
  BOTTOM-LEFT corner.

No overlap is possible by construction (the zones cannot intersect), the
label is toolbar-independent (drag-stable), and off-screen is impossible.
Inside corners carry a 12 px horizontal / 8 px vertical inset; the
below-fit decision keeps 12 px of breathing room at the screen edge
(zero-margin still looks glued on — measured). Anchors stay pure and
unit-tested, including a grid sweep asserting the disjoint-and-on-screen
invariant over 35 selection geometries.


## Annotation tools — incremental implementation

Reference: [PixPin annotation basics](https://pixpin.cn/docs/mark/base-use)
and [geometry tools](https://pixpin.cn/docs/mark/geo). Implement one tool at a
time, with shared desktop coordinates, preview, history, and PNG export.

1. Implemented: rectangle outlines — drag, Shift-square, preset colors/widths, undo/redo,
   multi-output preview and export. OCR continues to use the original image.
2. Ellipse outlines and Shift-circle.
3. Lines and polylines.
4. Arrows.
5. Sequence numbers.
6. Pencil.
7. Highlighter.
8. Mosaic and blur (smart erasing requires a separate feasibility review).
9. Text.
10. Eraser.
11. Spotlight.
12. Watermark.
13. Magnifier.

Follow-up geometry enhancements: select existing annotations, move/resize,
delete, fill, line styles, rounded corners, rotation, sectors and arcs.
Rectangle strokes use the same four inward bands for GPU preview and raster
export; history and drafts belong to the shared screenshot session. Starting
a new screenshot selection clears its old annotations and redo history.
