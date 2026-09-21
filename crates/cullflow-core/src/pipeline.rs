use crate::face::FaceDetector;
use crate::ffmpeg::extract_proxy_frames;
use crate::landmarks::LandmarkDetector;
use crate::models::{AnalyzedClip, ClipInfo, Tolerance};
use crate::scoring::{classify_clip, score_frames};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub sample_every_secs: f64,
    pub tolerance: Tolerance,
    pub proxy_root: PathBuf,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            sample_every_secs: 0.5,
            tolerance: Tolerance::Conservative,
            proxy_root: std::env::temp_dir().join("cullflow-proxies"),
        }
    }
}

/// Runs proxy extraction + CV scoring for every clip in parallel (rayon; the
/// blueprint's "60x real-time" target comes from doing this pass on a cheap
/// downscaled proxy rather than the source media, plus running clips
/// concurrently rather than one at a time). One clip's failure (corrupt file,
/// unsupported codec) doesn't abort the batch - it's reported per-clip.
pub fn analyze_clips(
    ffmpeg_path: &Path,
    clips: &[ClipInfo],
    config: &PipelineConfig,
) -> Vec<Result<AnalyzedClip, String>> {
    clips
        .par_iter()
        .map(|clip| analyze_one(ffmpeg_path, clip.clone(), config))
        .collect()
}

fn analyze_one(
    ffmpeg_path: &Path,
    clip: ClipInfo,
    config: &PipelineConfig,
) -> Result<AnalyzedClip, String> {
    let out_dir = config.proxy_root.join(&clip.id);
    let frame_paths = extract_proxy_frames(
        ffmpeg_path,
        Path::new(&clip.path),
        &out_dir,
        config.sample_every_secs,
    )
    .map_err(|e| format!("{} : {e}", clip.file_name))?;

    // Face and landmark detection are informational-only (see
    // FrameMetrics::face_detected / eyes_closed) and gracefully unavailable
    // when the face-detection feature isn't compiled in or a model fails to
    // load - neither ever fails the clip.
    let mut face_detector = FaceDetector::load().ok();
    let mut landmark_detector = LandmarkDetector::load().ok();
    let frame_metrics = score_frames(
        &frame_paths,
        config.sample_every_secs,
        face_detector.as_mut(),
        landmark_detector.as_mut(),
    )
    .map_err(|e| format!("{} : {e}", clip.file_name))?;

    Ok(classify_clip(clip, frame_metrics, config.tolerance))
}
