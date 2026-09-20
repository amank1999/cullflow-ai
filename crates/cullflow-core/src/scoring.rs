use crate::models::{AnalyzedClip, Classification, ClipInfo, FrameMetrics, Tolerance};
use image::imageops::FilterType;
use image::{GrayImage, ImageBuffer, Luma};
use imageproc::filter::laplacian_filter;

const JITTER_THUMB_WIDTH: u32 = 64;
const JITTER_THUMB_HEIGHT: u32 = 36;

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

/// Mean absolute pixel-luminance delta between two downsampled frames, used
/// as a cheap stand-in for Farneback optical flow: large, uniform frame-wide
/// luminance shifts correlate with camera shake/whip-pans far better than
/// nothing, without requiring a full optical-flow implementation. Real dense
/// optical flow is a documented follow-up (see README "Known simplifications").
fn jitter_delta(
    prev_thumb: &ImageBuffer<Luma<u8>, Vec<u8>>,
    cur_thumb: &ImageBuffer<Luma<u8>, Vec<u8>>,
) -> f64 {
    let sum: u64 = prev_thumb
        .pixels()
        .zip(cur_thumb.pixels())
        .map(|(a, b)| (a.0[0] as i32 - b.0[0] as i32).unsigned_abs() as u64)
        .sum();
    sum as f64 / prev_thumb.len() as f64
}

fn thumbnail(gray: &GrayImage) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    image::imageops::resize(
        gray,
        JITTER_THUMB_WIDTH,
        JITTER_THUMB_HEIGHT,
        FilterType::Triangle,
    )
}

/// Scores an already-extracted proxy frame sequence for one clip. `frame_paths`
/// must be in timestamp order (as returned by `ffmpeg::extract_proxy_frames`).
pub fn score_frames(
    frame_paths: &[std::path::PathBuf],
    sample_every_secs: f64,
) -> image::ImageResult<Vec<FrameMetrics>> {
    let mut metrics = Vec::with_capacity(frame_paths.len());
    let mut prev_thumb: Option<ImageBuffer<Luma<u8>, Vec<u8>>> = None;

    for (i, path) in frame_paths.iter().enumerate() {
        let gray = image::open(path)?.to_luma8();
        let sharpness = sharpness_of(&gray);
        let mean_luminance = mean_luminance_of(&gray);
        let cur_thumb = thumbnail(&gray);

        let jitter = match &prev_thumb {
            Some(prev) => jitter_delta(prev, &cur_thumb),
            None => 0.0,
        };

        metrics.push(FrameMetrics {
            timestamp_secs: i as f64 * sample_every_secs,
            sharpness,
            mean_luminance,
            jitter_delta: jitter,
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

fn jitter_score(max_jitter: f64, threshold: f64) -> f64 {
    if threshold <= 0.0 {
        return 100.0;
    }
    (1.0 - (max_jitter / threshold).clamp(0.0, 1.0)) * 100.0
}

/// Combines per-frame metrics into a single clip-level verdict: a 0-100
/// composite score, a Green/Cyan/Red classification, and the flags that drove
/// it. Sharpness is weighted higher than jitter because an out-of-focus take
/// is unusable in a way that mild handheld motion often isn't.
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
            max_jitter: 0.0,
            min_luminance: 0.0,
            score: 0.0,
            classification: Classification::DiscardTake,
            flags: vec!["unreadable".to_string()],
        };
    }

    let min_sharpness = frames
        .iter()
        .map(|f| f.sharpness)
        .fold(f64::INFINITY, f64::min);
    let max_jitter = frames.iter().map(|f| f.jitter_delta).fold(0.0, f64::max);
    let min_luminance = frames
        .iter()
        .map(|f| f.mean_luminance)
        .fold(f64::INFINITY, f64::min);

    let mut flags = Vec::new();
    if min_sharpness < tolerance.sharpness_threshold() {
        flags.push("blurry".to_string());
    }
    if max_jitter > tolerance.jitter_threshold() {
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
            max_jitter,
            min_luminance,
            score: 0.0,
            classification: Classification::DiscardTake,
            flags,
        };
    }

    let score = 0.6 * sharpness_score(min_sharpness, tolerance.sharpness_threshold())
        + 0.4 * jitter_score(max_jitter, tolerance.jitter_threshold());

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
        max_jitter,
        min_luminance,
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
            jitter_delta: 0.0,
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
                jitter_delta: 1.0,
            },
            FrameMetrics {
                timestamp_secs: 0.5,
                sharpness: 420.0,
                mean_luminance: 130.0,
                jitter_delta: 2.0,
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
            jitter_delta: 0.0,
        }];
        let analyzed = classify_clip(make_clip(), frames, Tolerance::Conservative);
        assert!(analyzed.flags.contains(&"blurry".to_string()));
        assert_ne!(analyzed.classification, Classification::BestTake);
    }

    #[test]
    fn checkerboard_scores_sharper_than_flat_frame() {
        let flat = solid_frame(128);
        let sharp = checkerboard_frame();
        assert!(sharpness_of(&sharp) > sharpness_of(&flat));
    }

    #[test]
    fn identical_frames_have_zero_jitter() {
        let a = thumbnail(&solid_frame(100));
        let b = thumbnail(&solid_frame(100));
        assert_eq!(jitter_delta(&a, &b), 0.0);
    }

    #[test]
    fn very_different_frames_have_higher_jitter_than_identical_ones() {
        let same_a = thumbnail(&solid_frame(100));
        let same_b = thumbnail(&solid_frame(100));
        let different_a = thumbnail(&solid_frame(10));
        let different_b = thumbnail(&checkerboard_frame());
        assert!(jitter_delta(&different_a, &different_b) > jitter_delta(&same_a, &same_b));
    }
}
