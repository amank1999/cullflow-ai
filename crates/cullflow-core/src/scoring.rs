use crate::face::FaceDetector;
use crate::landmarks::{self, LandmarkDetector};
use crate::models::{AnalyzedClip, Classification, ClipInfo, FrameMetrics, Tolerance};
use crate::motion::motion_incoherence;
use image::imageops::FilterType;
use image::{GrayImage, ImageBuffer, Luma};
use imageproc::filter::laplacian_filter;

const MOTION_THUMB_WIDTH: u32 = 64;
const MOTION_THUMB_HEIGHT: u32 = 36;

/// Variance of the Laplacian - the standard "how sharp is this frame" proxy.
/// Low variance means few strong edges, i.e. an out-of-focus or blurry frame.
fn sharpness_of(gray: &GrayImage) -> f64 {
    let laplacian = laplacian_filter(gray);
    let values: Vec<f64> = laplacian.pixels().map(|p| p.0[0] as f64).collect();
    variance(&values)
}

fn mean_luminance_of(gray: &GrayImage) -> f64 {
    let sum: u64 = gray.pixels().map(|p| p.0[0] as u64).sum();
    sum as f64 / gray.len() as f64
}

fn variance(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64
}

fn thumbnail(gray: &GrayImage) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    image::imageops::resize(
        gray,
        MOTION_THUMB_WIDTH,
        MOTION_THUMB_HEIGHT,
        FilterType::Triangle,
    )
}

/// Scores an already-extracted proxy frame sequence for one clip. `frame_paths`
/// must be in timestamp order (as returned by `ffmpeg::extract_proxy_frames`).
/// `face_detector` and `landmark_detector` are optional (and
/// informational-only, see `FrameMetrics::face_detected` /
/// `FrameMetrics::eyes_closed`) so callers where they're unavailable or
/// disabled can still get every other metric.
pub fn score_frames(
    frame_paths: &[std::path::PathBuf],
    sample_every_secs: f64,
    mut face_detector: Option<&mut FaceDetector>,
    mut landmark_detector: Option<&mut LandmarkDetector>,
) -> image::ImageResult<Vec<FrameMetrics>> {
    let mut metrics = Vec::with_capacity(frame_paths.len());
    let mut prev_thumb: Option<ImageBuffer<Luma<u8>, Vec<u8>>> = None;

    for (i, path) in frame_paths.iter().enumerate() {
        let dyn_image = image::open(path)?;
        let gray = dyn_image.to_luma8();
        let sharpness = sharpness_of(&gray);
        let mean_luminance = mean_luminance_of(&gray);
        let cur_thumb = thumbnail(&gray);

        let motion = match &prev_thumb {
            Some(prev) => motion_incoherence(prev, &cur_thumb),
            None => 0.0,
        };

        let faces = if let Some(detector) = &mut face_detector {
            detector.detect(&dyn_image).unwrap_or_default()
        } else {
            Vec::new()
        };
        let face_detected = !faces.is_empty();

        // Landmarks (and therefore the blink signal) run only against the
        // most confident detected face - one subject's eye state per frame
        // is enough for the informational signal this drives.
        let eyes_closed = match (faces.first(), &mut landmark_detector) {
            (Some(best_face), Some(detector)) => detector
                .get_landmarks(&dyn_image, best_face)
                .map(|pts| landmarks::eyes_closed(&pts, landmarks::DEFAULT_EAR_THRESHOLD))
                .unwrap_or(false),
            _ => false,
        };

        metrics.push(FrameMetrics {
            timestamp_secs: i as f64 * sample_every_secs,
            sharpness,
            mean_luminance,
            motion_incoherence: motion,
            face_detected,
            eyes_closed,
        });

        prev_thumb = Some(cur_thumb);
    }

    Ok(metrics)
}

fn sharpness_score(min_sharpness: f64, threshold: f64) -> f64 {
    if threshold <= 0.0 {
        return 100.0;
    }
    ((min_sharpness / threshold).min(1.5) / 1.5 * 100.0).clamp(0.0, 100.0)
}

fn motion_score(max_incoherence: f64, threshold: f64) -> f64 {
    if threshold <= 0.0 {
        return 100.0;
    }
    (1.0 - (max_incoherence / threshold).clamp(0.0, 1.0)) * 100.0
}

/// Combines per-frame metrics into a single clip-level verdict: a 0-100
/// composite score, a Green/Cyan/Red classification, and the flags that drove
/// it. Sharpness is weighted higher than motion incoherence because an
/// out-of-focus take is unusable in a way that mild handheld motion often
/// isn't.
pub fn classify_clip(
    clip: ClipInfo,
    frames: Vec<FrameMetrics>,
    tolerance: Tolerance,
) -> AnalyzedClip {
    if frames.is_empty() {
        return AnalyzedClip {
            clip,
            frames,
            min_sharpness: 0.0,
            max_motion_incoherence: 0.0,
            min_luminance: 0.0,
            contains_face: false,
            contains_blink: false,
            score: 0.0,
            classification: Classification::DiscardTake,
            flags: vec!["unreadable".to_string()],
        };
    }

    let min_sharpness = frames
        .iter()
        .map(|f| f.sharpness)
        .fold(f64::INFINITY, f64::min);
    let max_motion_incoherence = frames
        .iter()
        .map(|f| f.motion_incoherence)
        .fold(0.0, f64::max);
    let min_luminance = frames
        .iter()
        .map(|f| f.mean_luminance)
        .fold(f64::INFINITY, f64::min);
    let contains_face = frames.iter().any(|f| f.face_detected);
    let contains_blink = frames.iter().any(|f| f.eyes_closed);

    let mut flags = Vec::new();
    if min_sharpness < tolerance.sharpness_threshold() {
        flags.push("blurry".to_string());
    }
    if max_motion_incoherence > tolerance.motion_incoherence_threshold() {
        flags.push("shaky".to_string());
    }
    if min_luminance < tolerance.blackout_threshold() {
        flags.push("blackout".to_string());
    }

    // Blackout / lens-cap is an instant purge per the blueprint's blackout
    // gate, regardless of how sharp or stable the rest of the clip is.
    if flags.contains(&"blackout".to_string()) {
        return AnalyzedClip {
            clip,
            frames,
            min_sharpness,
            max_motion_incoherence,
            min_luminance,
            contains_face,
            contains_blink,
            score: 0.0,
            classification: Classification::DiscardTake,
            flags,
        };
    }

    let score = 0.6 * sharpness_score(min_sharpness, tolerance.sharpness_threshold())
        + 0.4
            * motion_score(
                max_motion_incoherence,
                tolerance.motion_incoherence_threshold(),
            );

    let classification = if score >= 70.0 {
        Classification::BestTake
    } else if score >= 35.0 {
        Classification::UsableBRoll
    } else {
        Classification::DiscardTake
    };

    AnalyzedClip {
        clip,
        frames,
        min_sharpness,
        max_motion_incoherence,
        min_luminance,
        contains_face,
        contains_blink,
        score,
        classification,
        flags,
    }
}

/// Re-classifies already-scored clips against a new tolerance without
/// re-running ffmpeg or re-scoring frames - this is what backs the UI's
/// live sensitivity slider.
pub fn reclassify(clips: Vec<AnalyzedClip>, tolerance: Tolerance) -> Vec<AnalyzedClip> {
    clips
        .into_iter()
        .map(|c| classify_clip(c.clip, c.frames, tolerance))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Luma};

    fn make_clip() -> ClipInfo {
        ClipInfo {
            id: "test".into(),
            path: "/tmp/test.mp4".into(),
            file_name: "test.mp4".into(),
            size_bytes: 0,
        }
    }

    fn solid_frame(luminance: u8) -> GrayImage {
        ImageBuffer::from_pixel(80, 45, Luma([luminance]))
    }

    fn checkerboard_frame() -> GrayImage {
        ImageBuffer::from_fn(80, 45, |x, y| {
            if (x / 4 + y / 4) % 2 == 0 {
                Luma([250])
            } else {
                Luma([5])
            }
        })
    }

    #[test]
    fn blackout_frame_is_discarded_regardless_of_sharpness() {
        let frames = vec![FrameMetrics {
            timestamp_secs: 0.0,
            sharpness: 500.0,
            mean_luminance: 2.0,
            motion_incoherence: 0.0,
            face_detected: false,
            eyes_closed: false,
        }];
        let analyzed = classify_clip(make_clip(), frames, Tolerance::Conservative);
        assert_eq!(analyzed.classification, Classification::DiscardTake);
        assert_eq!(analyzed.score, 0.0);
        assert!(analyzed.flags.contains(&"blackout".to_string()));
    }

    #[test]
    fn sharp_stable_clip_is_best_take() {
        let frames = vec![
            FrameMetrics {
                timestamp_secs: 0.0,
                sharpness: 400.0,
                mean_luminance: 128.0,
                motion_incoherence: 0.1,
                face_detected: false,
                eyes_closed: false,
            },
            FrameMetrics {
                timestamp_secs: 0.5,
                sharpness: 420.0,
                mean_luminance: 130.0,
                motion_incoherence: 0.2,
                face_detected: false,
                eyes_closed: false,
            },
        ];
        let analyzed = classify_clip(make_clip(), frames, Tolerance::Conservative);
        assert_eq!(analyzed.classification, Classification::BestTake);
        assert!(analyzed.flags.is_empty());
    }

    #[test]
    fn blurry_clip_is_flagged_and_scored_low() {
        let frames = vec![FrameMetrics {
            timestamp_secs: 0.0,
            sharpness: 5.0,
            mean_luminance: 128.0,
            motion_incoherence: 0.0,
            face_detected: false,
            eyes_closed: false,
        }];
        let analyzed = classify_clip(make_clip(), frames, Tolerance::Conservative);
        assert!(analyzed.flags.contains(&"blurry".to_string()));
        assert_ne!(analyzed.classification, Classification::BestTake);
    }

    #[test]
    fn shaky_clip_is_flagged_and_scored_low() {
        let frames = vec![FrameMetrics {
            timestamp_secs: 0.0,
            sharpness: 400.0,
            mean_luminance: 128.0,
            motion_incoherence: 3.0, // well above Conservative's 1.5 threshold
            face_detected: false,
            eyes_closed: false,
        }];
        let analyzed = classify_clip(make_clip(), frames, Tolerance::Conservative);
        assert!(analyzed.flags.contains(&"shaky".to_string()));
        assert_ne!(analyzed.classification, Classification::BestTake);
    }

    #[test]
    fn checkerboard_scores_sharper_than_flat_frame() {
        let flat = solid_frame(128);
        let sharp = checkerboard_frame();
        assert!(sharpness_of(&sharp) > sharpness_of(&flat));
    }
}
