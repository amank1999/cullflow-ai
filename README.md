# CullFlow AI

Offline desktop engine for automated raw-footage culling, quality scoring, and
NLE timeline generation. Built per `cullflow_execution_blueprint.pdf`. Zero
cloud uploads: everything runs on the editor's own machine.

## Status: MVP scaffold (blueprint Weeks 1–5)

This pass implements the core pipeline end to end - folder ingest, proxy
extraction, CV scoring, classification, FCPXML export, an offline license-key
system, and block-matching motion detection - plus a working desktop UI. It
intentionally does **not** yet implement facial/blink detection, audio VAD,
or payment-processor integration; see [Known
simplifications](#known-simplifications) and [Not yet
built](#not-yet-built-blueprint-phase-2) below.

## Downloading a build

Push a tag like `v0.1.0` (or run the **Release** workflow manually from the
*Actions* tab) and GitHub Actions builds installers for macOS (Apple
Silicon), Windows, and Linux, publishing them as a **draft GitHub
Release** with downloadable `.dmg` / `.msi` / `.exe` / `.deb` / `.rpm` /
`.AppImage` files - see `.github/workflows/release.yml`. Un-draft the
release once you've smoke tested a build to make it publicly downloadable.

Intel Macs aren't built (GitHub's Intel-Mac runner pool proved too slow/
congested in practice to hold releases on); Apple Silicon covers every Mac
sold since 2020. Add `macos-13` back to the workflow's matrix if Intel Mac
support becomes necessary.

ffmpeg is bundled into each installer (see next section), so anyone
downloading a release does **not** need ffmpeg installed separately.

Neither macOS nor Windows builds are code-signed yet (that needs a paid
Apple Developer / code-signing certificate), so first launches will show an
"unidentified developer" / SmartScreen warning until that's set up - normal
for a pre-signing MVP, not a build error.

## Architecture

```
cullflow-ai/
  crates/cullflow-core/     Pure Rust engine - no GUI dependency, fully
                             unit-testable headlessly (cargo test -p cullflow-core)
    src/ingest.rs            Recursive folder scan -> ClipInfo list
    src/ffmpeg.rs             Locates + shells out to ffmpeg for proxy frame extraction
    src/scoring.rs            Per-frame sharpness / luminance / motion metrics
                              + clip-level classification
    src/motion.rs             Block-matching motion estimation (shake vs. pan)
    src/face.rs               Bundled ONNX face detector (informational "contains a face" signal)
    src/license.rs            Ed25519 offline license-key signing/verification
    src/xml_export.rs         FCPXML 1.10 sequence generator
    src/pipeline.rs           Parallel (rayon) per-clip analysis pipeline
    src/models.rs             Shared data types (ClipInfo, AnalyzedClip, Tolerance, ...)

  crates/cullflow-keygen/    Seller-side CLI: issues signed license keys.
                              Never ships inside the desktop app.

  src-tauri/                Tauri (Rust) desktop shell
    src/commands.rs           Tauri commands exposed to the frontend
    src/state.rs              In-memory app state (scanned/analyzed clips)
    src/license_store.rs      Local license activation + machine binding
    src/sidecar.rs            Resolves the bundled ffmpeg binary at runtime
    src/lib.rs                App wiring

  src/                       React + TypeScript frontend (Vite)
    App.tsx                   4-step workflow UI
    api.ts                    Typed wrappers around Tauri invoke() calls
    types.ts                  TS mirrors of the Rust data types
```

`cullflow-core` has no Tauri/GUI dependency by design: it builds and its test
suite runs on any machine (including a headless CI box), while the Tauri
shell around it needs the platform's native webview toolkit (see
Prerequisites).

## Pipeline

1. **Folder Ingest** (`ingest::scan_folder`) - recursively walks the chosen
   folder for video files (`mp4`, `mov`, `mxf`, `mkv`, `avi`, `braw`, `r3d`,
   `m4v`).
2. **Proxy & Analysis** (`ffmpeg::extract_proxy_frames` + `scoring::score_frames`,
   run in parallel across clips via `pipeline::analyze_clips`):
   - ffmpeg extracts a 360p JPEG frame every 0.5s (hardware-accelerated decode
     requested via `-hwaccel videotoolbox`/`cuda` where available, falling
     back to software silently if that fails).
   - Each frame is scored for **sharpness** (variance of the Laplacian - low
     variance means an out-of-focus frame), **mean luminance** (near-zero
     means lens-cap/blackout), and **motion incoherence** (`motion::motion_incoherence`
     - block-matching motion estimation between consecutive downsampled
     frames, measuring how much blocks *disagree* with each other about
     which way the frame moved). That disagreement is the useful signal: a
     deliberate pan moves every block the same way (low incoherence,
     whatever its speed), while handheld shake or a dropped camera moves
     blocks inconsistently (high incoherence) - one metric distinguishes the
     two without needing the blueprint's dense Farneback optical flow.
3. **Classification** (`scoring::classify_clip`) - combines the worst
   per-frame values into a 0–100 composite score and a Green/Cyan/Red verdict:
   - **Blackout** in any frame is an instant Discard, regardless of the rest
     of the clip (matches the blueprint's "instant purge" rule).
   - Otherwise, score = `0.6 * sharpness_score + 0.4 * motion_score`, banded
     into Best Take (≥70), Usable B-Roll (≥35), Discard (<35).
   - **Aggressive** vs **Conservative** tolerance changes the sharpness/motion
     thresholds; switching it in the UI reclassifies already-scored clips
     instantly (`scoring::reclassify`) without re-running ffmpeg.
4. **Export** (`xml_export::generate_fcpxml`) - writes a standard FCPXML 1.10
   sequence (imports into both DaVinci Resolve and Premiere Pro) with one
   `asset-clip` per source clip and a marker carrying its classification +
   score. Discard clips are written with `enabled="0"` (present but disabled
   in the timeline) rather than deleted, matching the blueprint's "muted
   secondary track for editor safety" intent without needing multi-lane math.

Each frame is also checked for a face (`face::FaceDetector`, see
[Face detection](#face-detection-not-yet-blinkexpression) below) - shown in
the UI as an informational "contains a face" indicator, not used in scoring.

## Face detection (not yet blink/expression)

The blueprint's face/blink gate needs two models: a face detector and an eye
landmark model (to compute Eye Aspect Ratio). Only the first is built so
far:

- **Bundled**: [Ultra-Light-Fast-Generic-Face-Detector-1MB](https://github.com/Linzaer/Ultra-Light-Fast-Generic-Face-Detector-1MB)
  (RFB-320 variant, MIT licensed - see
  `crates/cullflow-core/assets/THIRD_PARTY_LICENSES.md`), run via
  [`ort`](https://ort.pyke.io/) (ONNX Runtime bindings). It answers "is
  there a face in this frame?", not "are the eyes open?".
- **Deliberately not used for scoring or classification**: a face-free frame
  is often legitimate B-roll (rings, venue, decor), not a bad take, so
  auto-flagging "no face" as a defect would be wrong. It's exposed as a
  UI-only indicator (a 🙂 in the clip table) so editors can spot talking-head
  shots at a glance.
- **Behind a Cargo feature** (`face-detection`, default off): `ort`'s
  `download-binaries` build step fetches a prebuilt ONNX Runtime binary over
  the network, which this project's own dev sandbox couldn't do (see "Known
  simplifications"). `cargo test -p cullflow-core` therefore stays fully
  offline/local by default; `src-tauri` enables the feature for real builds,
  which is why `.github/workflows/ci.yml` exists - it's the only place this
  path gets built and linked with real network access, on every push.
- **Sourcing a landmark model** (68/106-point face landmarks, needed for
  eye state) hit the same network restriction from a different angle: the
  candidates found were either Git-LFS pointers (not real binaries) when
  fetched via `raw.githubusercontent.com`, or had complex, undocumented
  output decoding this project couldn't verify correctness of without a
  reference implementation and real face footage to test against. Picking
  up that model and wiring EAR-based blink detection is the next step for
  this feature (see "Not yet built" below).

## Prerequisites

- **Rust** (stable) + **Node.js 18+**
- Tauri's native build prerequisites for your OS - see
  <https://tauri.app/start/prerequisites/> (on Linux: `libwebkit2gtk-4.1-dev`,
  `libgtk-3-dev`, `librsvg2-dev`, `libsoup-3.0-dev`, `libayatana-appindicator3-dev`,
  `libxdo-dev`; macOS/Windows need no extra system packages beyond Xcode CLT /
  the MSVC toolchain).

## Bundling ffmpeg (sidecar binary)

CullFlow ships its own ffmpeg next to the app - no separate ffmpeg install
required for anyone running a built installer. Tauri calls this an
**external binary / sidecar**: `tauri.conf.json`'s `bundle.externalBin`
points at `src-tauri/binaries/ffmpeg`, and Tauri looks for a matching file
per platform named `ffmpeg-<target-triple>[.exe]` at build time (e.g.
`ffmpeg-aarch64-apple-darwin`, `ffmpeg-x86_64-pc-windows-msvc.exe`). Once
bundled, it's placed next to the app's own executable at runtime, and
`src-tauri/src/sidecar.rs` resolves it from there automatically.

These binaries are **not committed to the repo** (large, platform-specific,
and not ours to redistribute via git) - `.gitignore` excludes
`src-tauri/binaries/*`. To build locally:

```bash
./scripts/fetch-ffmpeg.sh        # macOS / Linux
./scripts/fetch-ffmpeg.ps1       # Windows (PowerShell)
```

The release CI workflow (`.github/workflows/release.yml`) does the
equivalent download for each target platform before building, so a
published release always has ffmpeg bundled in.

At runtime, `resolve_ffmpeg()` (`src-tauri/src/sidecar.rs`) checks, in
order: `CULLFLOW_FFMPEG_PATH` env var → the bundled sidecar next to the app
→ `ffmpeg` on `PATH`. That last fallback means dev mode still works with a
system ffmpeg install even without running the fetch script.

## Running it

```bash
./scripts/fetch-ffmpeg.sh   # or fetch-ffmpeg.ps1 on Windows - one-time setup
npm install
npm run tauri dev           # desktop app with hot reload
```

Headless-only checks (no native webview toolkit required):

```bash
cargo test -p cullflow-core   # engine unit tests
npm run build                  # frontend typecheck + Vite build
```

## Known simplifications

- **Motion/shake detection** (`motion::motion_incoherence`) is block-matching
  motion estimation (search each block's best match in a small neighborhood,
  like classic video-codec motion estimation), not the blueprint's dense
  per-pixel Farneback optical flow. It correctly separates deliberate pans
  from shake (see the Pipeline section above) and needs no native
  dependency, but is coarser than dense flow: an 8x8 block size and a ±4px
  search window (`crates/cullflow-core/src/motion.rs`) won't catch very fine
  or very fast motion as precisely as per-pixel flow would. The
  `Tolerance::motion_incoherence_threshold` values are hand-picked against
  this metric's theoretical range, not tuned against real footage yet.
  Swapping in dense flow (e.g. via an `opencv-rust` binding once that native
  dependency is worth taking on) would be a drop-in replacement for this
  function's body without changing its callers.
- **Clip duration in the FCPXML export** is approximated from the number of
  sampled proxy frames × the sampling interval, not the source's true frame
  rate/duration (which would need an `ffprobe` call). Good enough to land
  clips in roughly the right place and see the right marker; not
  frame-accurate for a conform pass.
- **Format choice**: the blueprint says "FCPXML / Premiere XML" - this
  implementation writes FCPXML only, since it's a single well-documented
  format both DaVinci Resolve and Premiere Pro import natively, versus
  maintaining two serializers.

## Licensing

Scanning and scoring a project (steps 1-3 of the workflow) work with no
license at all, so an editor can see CullFlow actually cull their footage
before paying. **Exporting** the NLE sequence (step 4) requires an
activated license - this is what makes the desktop app safe to distribute
freely (via GitHub Releases, a website, wherever): without a paid key it's
a fully-working preview that can't produce the deliverable.

**How it works** (`crates/cullflow-core/src/license.rs`, `crates/cullflow-keygen/`,
`src-tauri/src/license_store.rs`):

1. A license key is an Ed25519-signed payload (customer email, tier, an
   optional expiry) - `CFAI1.<payload>.<signature>`, verifiable **fully
   offline** against a public key embedded in the app. No server call is
   needed to check a key, matching the app's zero-cloud design.
2. Keys are issued with **`cullflow-keygen`**, a separate CLI that holds the
   private signing key - it never ships inside the desktop app:
   ```bash
   cargo run -p cullflow-keygen -- genkey   # once, to create the real signing keypair
   export CULLFLOW_SIGNING_KEY=<private key from genkey>
   cargo run -p cullflow-keygen -- issue --email buyer@example.com --tier founding
   ```
   Paste `genkey`'s public key into `LICENSE_PUBLIC_KEY_B64` in
   `license.rs` before shipping a build - the copy in this repo right now
   is a real keypair generated during development, already swapped into
   that constant, but treat it as a placeholder to rotate before any real
   sale (see the security note in that file).
3. Entering a key in the app's License panel calls `activate_license`,
   which re-verifies it and stores `{key, machine fingerprint}` locally
   (`license_store.rs`). Every later check re-verifies the signature *and*
   confirms the stored fingerprint still matches the current machine.

**What this does and doesn't protect against:** the signature stops anyone
from forging a key out of thin air, and the machine-fingerprint binding
stops casually copying your app's local config folder to a friend's
computer. It does **not** stop someone sharing the raw key *string* itself
with a friend, who could then activate that same string on their own
machine - true single-seat enforcement needs an online activation server
to track redemptions, which is a deliberate scope cut for this offline-first
MVP (see "Not yet built" below).

Wiring `cullflow-keygen issue` up to a Stripe/LemonSqueezy payment webhook
(so a key is generated and emailed automatically on purchase, instead of
run by hand) is the next step once a payment processor account exists.

## Not yet built (blueprint Phase 2)

- Blink/expression gate (Eye Aspect Ratio from face landmarks) - face
  *detection* is built (see "Face detection" above); an eye landmark model
  still needs to be sourced and verified. Audio VAD (Silero VAD) is
  similarly not started. Both pull in ONNX model files the blueprint's own
  8-week roadmap also defers past the Week 1–5 core engine.
- Automated payment → license-key issuance (Stripe/LemonSqueezy webhook
  calling `cullflow-keygen`) - the signing/verification/activation pipeline
  itself is built (see "Licensing" above); wiring it to a real payment
  processor needs that processor's account and API keys.
- Online seat/activation-count enforcement (stopping the same key being
  shared and activated on multiple machines) - would need an activation
  server, which conflicts with the fully-offline verification this MVP
  intentionally uses instead.
- Code signing / notarization for macOS and Windows (needs a paid developer
  certificate - not something CI can do on its own).
- Go-to-market execution (Reddit/Discord/Instagram playbook) - not
  engineering work.
