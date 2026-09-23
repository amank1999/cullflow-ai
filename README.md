# CullFlow AI

Offline desktop engine for automated raw-footage culling, quality scoring, and
NLE timeline generation. Built per `cullflow_execution_blueprint.pdf`. Zero
cloud uploads: everything runs on the editor's own machine.

## Status: MVP scaffold (blueprint Weeks 1–5)

This pass implements the core pipeline end to end - folder ingest, proxy
extraction, CV scoring, classification, NLE XML export, an offline license-key
system, block-matching motion detection, face/blink detection, and audio
VAD - plus a working desktop UI. It intentionally does **not** yet implement
payment-processor integration; see [Known
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
    src/xml_export.rs         XMEML ("Final Cut Pro XML") sequence generator
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
   - In parallel, `ffmpeg::extract_proxy_audio` + `audio::SileroVad` pull
     each clip's audio track and measure how much of it has detected speech
     and how much is clipped/over-driven (see "Audio VAD" below).
3. **Classification** (`scoring::classify_clip`) - combines the worst
   per-frame values into a 0–100 composite score and a Green/Cyan/Red verdict:
   - **Blackout** in any frame is an instant Discard, regardless of the rest
     of the clip (matches the blueprint's "instant purge" rule).
   - Otherwise, score = `0.6 * sharpness_score + 0.4 * motion_score`, banded
     into Best Take (≥70), Usable B-Roll (≥35), Discard (<35).
   - **Aggressive** vs **Conservative** tolerance changes the sharpness/motion
     thresholds; switching it in the UI reclassifies already-scored clips
     instantly (`scoring::reclassify`) without re-running ffmpeg.
4. **Export** (`xml_export::generate_premiere_xml`) - writes an XMEML v5
   sequence (the legacy "Final Cut Pro 7 XML Interchange Format" - despite
   the name, this is what Adobe Premiere Pro's File > Import actually
   understands, and DaVinci Resolve accepts it too) with one `clipitem` per
   source clip and a marker carrying its classification + score. See "NLE
   export format" below for why this isn't modern FCPXML. Discard clips are
   written with `<enabled>FALSE</enabled>` (present but disabled in the
   timeline) rather than deleted, matching the blueprint's "muted secondary
   track for editor safety" intent without needing multi-lane math.

Each frame is also checked for a face and, when one's found, whether its
eyes are closed (`face::FaceDetector` / `landmarks::LandmarkDetector`, see
[Face and blink detection](#face-and-blink-detection) below) - both shown in
the UI as informational indicators, not used in scoring.

## Face and blink detection

The blueprint's face/blink gate needs two models: a face detector and an eye
landmark model (to compute Eye Aspect Ratio). Both are now built:

- **Face detector, bundled**: [Ultra-Light-Fast-Generic-Face-Detector-1MB](https://github.com/Linzaer/Ultra-Light-Fast-Generic-Face-Detector-1MB)
  (RFB-320 variant, MIT licensed - see
  `crates/cullflow-core/assets/THIRD_PARTY_LICENSES.md`), run via
  [`ort`](https://ort.pyke.io/) (ONNX Runtime bindings). It answers "is
  there a face in this frame?".
- **Landmark model, bundled**: [PIPNet](https://github.com/jhb86253817/PIPNet)
  (ResNet-18, 300W+CelebA GSSL checkpoint, 68 points), via the ONNX export
  and decode logic from [yakhyo/pipnet-onnx](https://github.com/yakhyo/pipnet-onnx)
  - both MIT licensed, see `THIRD_PARTY_LICENSES.md`. It locates 68 facial
  landmarks inside the highest-confidence face box per frame; `landmarks::
  eye_aspect_ratio` computes the Soukupova/Cech Eye Aspect Ratio from the
  two eyes' points (indices 36-41, 42-47), and `landmarks::eyes_closed`
  flags a blink when both eyes fall below `DEFAULT_EAR_THRESHOLD` (0.20, per
  the blueprint spec).
- **Deliberately not used for scoring or classification**: a face-free frame
  is often legitimate B-roll (rings, venue, decor), not a bad take, and a
  single sampled frame catching a natural blink doesn't mean the take is
  bad either - proxy frames are sampled every `sample_every_secs`, sparsely
  enough that one closed-eye sample is an unreliable signal to auto-discard
  a clip on. Both are exposed as UI-only indicators (🙂 / 😑 in the clip
  table) so editors can spot talking-head shots and blinks at a glance and
  decide for themselves.
- **Behind a Cargo feature** (`face-detection`, default off): `ort`'s
  `download-binaries` build step fetches a prebuilt ONNX Runtime binary over
  the network, which this project's own dev sandbox couldn't do (see "Known
  simplifications"). `cargo test -p cullflow-core` therefore stays fully
  offline/local by default; `src-tauri` enables the feature for real builds,
  which is why `.github/workflows/ci.yml` exists - it's the only place this
  path gets built and linked with real network access, on every push.

## Audio VAD

The blueprint's "Facial & Audio Analytics" layer also calls for Silero VAD
to "cut uncalibrated audio peaks or dead mic takes" - the audio counterpart
to the visual blackout gate. Built:

- **Bundled**: [Silero VAD](https://github.com/snakers4/silero-vad)'s
  combined 8kHz/16kHz ONNX model, MIT licensed - see
  `crates/cullflow-core/assets/THIRD_PARTY_LICENSES.md`. `ffmpeg::
  extract_proxy_audio` pulls each clip's audio track as raw 16kHz mono PCM;
  `audio::SileroVad` runs it in 32ms windows (carrying the model's
  recurrent state and windowing context between windows, exactly like the
  reference implementation) and reports the fraction classified as speech
  (`AnalyzedClip::speech_ratio`). A second, model-free metric,
  `audio::clipping_ratio`, measures the fraction of samples near full-scale
  - the "uncalibrated audio peaks" half of the same gate.
- **Deliberately not used for scoring or classification**: a clip with no
  audio track at all is common and legitimate (a video-only B-roll rig), so
  `has_audio: false` isn't a defect. Less obviously, a clip *with* an audio
  track but a near-zero speech ratio isn't reliably a "dead mic" either -
  it's equally what a legitimate ambient/scenery shot with no dialogue
  looks like. CullFlow has no way to know from the footage alone whether a
  given take was *supposed* to have dialogue, so - consistent with the
  face/blink signals above - this stays an informational indicator (an
  Audio column showing speech % and a ⚠️ for noticeable clipping) rather
  than an auto-discard rule.
- **Behind its own Cargo feature** (`audio-vad`, default off, same
  `dep:ort` reasoning as `face-detection`): needs network access to fetch
  ONNX Runtime, so it's off for `cargo test -p cullflow-core` and on for
  `src-tauri` / CI, same split as face/blink detection.

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

To sanity-check the real engine against actual footage without the full
Tauri app (useful with no display, or before trusting a change):

```bash
cargo run -p cullflow-core --example smoke_test -- /path/to/footage
```

Add `--features face-detection,audio-vad` for face/blink/speech signals too
(needs network access to fetch ONNX Runtime the first time). See
`crates/cullflow-core/examples/smoke_test.rs`.

## Known simplifications

- **Motion thresholds are calibrated against ffmpeg-generated synthetic
  footage, not real camera footage.** A smoke test
  (`examples/smoke_test.rs`) against real (H.264-encoded, JPEG-proxied)
  clips found the original thresholds badly miscalibrated: a bit-identical,
  truly zero-motion clip measured ~1.33 incoherence (from encoding/proxy
  noise alone) and a clean deliberate pan measured ~2.32 - both above the
  original Conservative threshold of 1.5, meaning good takes were getting
  flagged shaky. Thresholds were raised (see `Tolerance::
  motion_incoherence_threshold`'s doc comment for the full numbers) to
  clear that measured noise floor, fixing the false-positive case. The
  margin between a clean pan and genuine shake is still thin in this test,
  so shake *sensitivity* on real camera footage isn't validated yet - this
  needs real wedding footage to tune properly, not more synthetic clips.
- **Linux build needs glibc 2.39+** (Ubuntu 24.04 or newer, or an equivalent
  distro release): the `ort` crate's prebuilt ONNX Runtime binary references
  glibc symbols (e.g. `__isoc23_strtoll`) only present from glibc 2.38
  onward, so building (and therefore running) on glibc 2.35 (Ubuntu 22.04)
  fails to link. This is a real behavior change from before face detection
  was added, not just a build-environment quirk - a Linux user on an older
  distro won't be able to run the built binary.
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
- **Clip duration in the NLE export** is approximated from the number of
  sampled proxy frames × the sampling interval, not the source's true frame
  rate/duration (which would need an `ffprobe` call). Good enough to land
  clips in roughly the right place and see the right marker; not
  frame-accurate for a conform pass.
- **NLE export format**: the first implementation of this feature generated
  modern FCPXML (`<fcpxml version="1.10">`, Final Cut Pro X's own format),
  on the assumption - stated directly in the blueprint - that this is a
  single well-documented format both DaVinci Resolve and Premiere Pro
  import natively. A real user's first test proved that assumption wrong
  for Premiere Pro: its File > Import rejects modern FCPXML outright as an
  unsupported file type. It turns out "Final Cut Pro XML" casually refers
  to two unrelated schemas - modern FCPXML (FCP X) and the older XMEML
  ("Final Cut Pro 7 XML Interchange Format", `<xmeml version="5">`), and
  Premiere Pro's importer only understands the latter. CullFlow now
  generates XMEML instead, which both DaVinci Resolve and Premiere Pro
  import; this cost the FCP X-native features modern FCPXML has (styled
  markers rendering as colored to-do checkmarks, keyword collections) in
  exchange for actually opening in the two NLEs this project targets.
- **Landmark model size**: the bundled PIPNet ONNX model
  (`pipnet_landmarks_300w_68.onnx`) is ~48MB, embedded directly into the
  binary via `include_bytes!` alongside the ~1.3MB face detector, for the
  same reason both are embedded rather than downloaded at runtime (no
  install-time network dependency, consistent with the "0 bytes leave this
  machine" offline guarantee). This meaningfully grows the installer size
  versus the pre-blink-detection MVP - an acceptable tradeoff for a paid
  desktop tool, but worth knowing about if installer size becomes a concern.

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
