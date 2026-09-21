//! Manual smoke test: run the real pipeline against a folder of footage and
//! print a report. Useful for sanity-checking the engine on synthetic or
//! real clips without the full Tauri app.
//!
//! Usage: cargo run -p cullflow-core --example smoke_test -- <footage-dir>
//! (add `--features face-detection,audio-vad` for face/blink/speech signals,
//! which need network access to fetch ONNX Runtime the first time.)

use cullflow_core::ffmpeg::locate_ffmpeg;
use cullflow_core::ingest::scan_folder;
use cullflow_core::pipeline::{analyze_clips, PipelineConfig};
use cullflow_core::Tolerance;
use std::path::Path;

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: smoke_test <footage-dir>");
    let ffmpeg_path = locate_ffmpeg()
        .expect("ffmpeg not found (set CULLFLOW_FFMPEG_PATH or install ffmpeg on PATH)");
    let clips = scan_folder(Path::new(&dir)).expect("failed to scan folder");
    println!("ffmpeg: {}", ffmpeg_path.display());
    println!("found {} clip(s) in {dir}\n", clips.len());

    let config = PipelineConfig {
        sample_every_secs: 0.5,
        tolerance: Tolerance::Conservative,
        proxy_root: std::env::temp_dir().join("cullflow-smoke-test-proxies"),
    };

    for result in analyze_clips(&ffmpeg_path, &clips, &config) {
        match result {
            Ok(c) => println!(
                "{:<20} score={:>5.1}  {:<12} flags={:<20} sharp={:>7.1} motion={:>5.2} lum={:>5.1}  face={:<5} blink={:<5} audio(has={:<5} speech={:<6} clip%={:.3})",
                c.clip.file_name,
                c.score,
                format!("{:?}", c.classification),
                format!("{:?}", c.flags),
                c.min_sharpness,
                c.max_motion_incoherence,
                c.min_luminance,
                c.contains_face,
                c.contains_blink,
                c.has_audio,
                c.speech_ratio
                    .map(|r| format!("{r:.2}"))
                    .unwrap_or_else(|| "n/a".to_string()),
                c.audio_clipping_ratio,
            ),
            Err(e) => println!("ERROR: {e}"),
        }
    }
}
