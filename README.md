# CullFlow AI

Offline desktop engine for automated raw-footage culling, quality scoring, and
NLE timeline generation. Built per `cullflow_execution_blueprint.pdf`. Zero
cloud uploads: everything runs on the editor's own machine.

## Status: MVP scaffold (blueprint Weeks 1–5)

This pass implements the core pipeline end to end - folder ingest, proxy
extraction, CV scoring, classification, and FCPXML export - plus a working
desktop UI. It intentionally does **not** yet implement facial/blink
detection, audio VAD, payment/licensing, or a real dense-optical-flow
implementation; see [Known simplifications](#known-simplifications) and
[Not yet built](#not-yet-built-blueprint-phase-2) below.

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
    src/scoring.rs            Per-frame sharpness / luminance / jitter metrics
                              + clip-level classification
    src/xml_export.rs         FCPXML 1.10 sequence generator
    src/pipeline.rs           Parallel (rayon) per-clip analysis pipeline
    src/models.rs             Shared data types (ClipInfo, AnalyzedClip, Tolerance, ...)

  src-tauri/                Tauri (Rust) desktop shell
    src/commands.rs           Tauri commands exposed to the frontend
    src/state.rs              In-memory app state (scanned/analyzed clips)
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
     means lens-cap/blackout), and **jitter** (mean luminance delta between
     downsampled consecutive frames - a cheap proxy for camera shake).
3. **Classification** (`scoring::classify_clip`) - combines the worst
   per-frame values into a 0–100 composite score and a Green/Cyan/Red verdict:
   - **Blackout** in any frame is an instant Discard, regardless of the rest
     of the clip (matches the blueprint's "instant purge" rule).
   - Otherwise, score = `0.6 * sharpness_score + 0.4 * jitter_score`, banded
     into Best Take (≥70), Usable B-Roll (≥35), Discard (<35).
   - **Aggressive** vs **Conservative** tolerance changes the sharpness/jitter
     thresholds; switching it in the UI reclassifies already-scored clips
     instantly (`scoring::reclassify`) without re-running ffmpeg.
4. **Export** (`xml_export::generate_fcpxml`) - writes a standard FCPXML 1.10
   sequence (imports into both DaVinci Resolve and Premiere Pro) with one
   `asset-clip` per source clip and a marker carrying its classification +
   score. Discard clips are written with `enabled="0"` (present but disabled
   in the timeline) rather than deleted, matching the blueprint's "muted
   secondary track for editor safety" intent without needing multi-lane math.

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

- **Jitter/shake detection** is a mean-luminance-delta heuristic between
  downsampled consecutive frames, not the blueprint's dense Farneback optical
  flow. It catches gross motion (drops, whip-pans) but won't distinguish
  intentional cinematic pans from shake as precisely as real optical flow
  would. Swapping in a proper flow implementation (e.g. via an `opencv-rust`
  binding once that native dependency is worth taking on) is a drop-in
  replacement for `scoring::jitter_delta`.
- **Clip duration in the FCPXML export** is approximated from the number of
  sampled proxy frames × the sampling interval, not the source's true frame
  rate/duration (which would need an `ffprobe` call). Good enough to land
  clips in roughly the right place and see the right marker; not
  frame-accurate for a conform pass.
- **Format choice**: the blueprint says "FCPXML / Premiere XML" - this
  implementation writes FCPXML only, since it's a single well-documented
  format both DaVinci Resolve and Premiere Pro import natively, versus
  maintaining two serializers.

## Not yet built (blueprint Phase 2)

- Facial/blink gate (MediaPipe FaceMesh, Eye Aspect Ratio) and audio VAD
  (Silero VAD) - these pull in an ONNX Runtime + model-file dependency the
  blueprint's own 8-week roadmap also defers past the Week 1–5 core engine.
- Payment/licensing integration (LemonSqueezy/Stripe + machine-fingerprint
  license keys) - Week 7 in the roadmap.
- Code signing / notarization for macOS and Windows (needs a paid developer
  certificate - not something CI can do on its own).
- Go-to-market execution (Reddit/Discord/Instagram playbook) - not
  engineering work.
