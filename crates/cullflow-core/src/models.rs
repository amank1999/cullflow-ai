use serde::{Deserialize, Serialize};

pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mxf", "mkv", "avi", "braw", "r3d", "m4v"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipInfo {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
}

/// Aggressive flags more takes as discard/usable (tighter tolerances);
/// Conservative keeps more borderline takes as best/usable.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Tolerance {
    Aggressive,
    Conservative,
}

impl Tolerance {
    pub fn from_str_or_default(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "aggressive" => Tolerance::Aggressive,
            _ => Tolerance::Conservative,
        }
    }

    /// Laplacian variance below this is flagged as blurry.
    pub fn sharpness_threshold(self) -> f64 {
        match self {
            Tolerance::Aggressive => 120.0,
            Tolerance::Conservative => 60.0,
        }
    }

    /// Block-motion incoherence (see `motion::motion_incoherence`) above this
    /// is flagged as unstable/shaky. Scaled to that function's ~0-3.3 range
    /// (0 = every block agrees, ~3.3 = fully chaotic within its search
    /// radius) - a deliberate pan should land near 0 regardless of speed,
    /// since all its blocks move together.
    pub fn motion_incoherence_threshold(self) -> f64 {
        match self {
            Tolerance::Aggressive => 0.8,
            Tolerance::Conservative => 1.5,
        }
    }

    /// Mean luminance (0-255) below this is flagged as a blackout / lens-cap frame.
    pub fn blackout_threshold(self) -> f64 {
        12.75 // ~5% of 255, per blueprint spec
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Classification {
    /// Green marker: high sharpness, stable, not blacked out.
    BestTake,
    /// Cyan marker: usable but not best (decent stability/composition).
    UsableBRoll,
    /// Red / muted marker: out of focus, lens cap, floor drop, or severe shake.
    DiscardTake,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameMetrics {
    pub timestamp_secs: f64,
    pub sharpness: f64,
    pub mean_luminance: f64,
    /// See `motion::motion_incoherence` - near zero for both a static shot
    /// and a deliberate uniform pan, high for shake/whip-pans/drops.
    pub motion_incoherence: f64,
    /// Whether the bundled face detector (`face::FaceDetector`) found at
    /// least one face in this frame. Informational only - it doesn't affect
    /// score or classification, since a face-free frame is often legitimate
    /// B-roll, not a bad take. A real blink/expression gate needs an eye
    /// landmark model this project hasn't been able to source yet (see
    /// README).
    pub face_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedClip {
    pub clip: ClipInfo,
    pub frames: Vec<FrameMetrics>,
    /// Worst (lowest) per-frame sharpness observed - drives the blur flag.
    pub min_sharpness: f64,
    /// Worst (highest) per-frame motion incoherence observed - drives the shake flag.
    pub max_motion_incoherence: f64,
    /// Worst (lowest) per-frame mean luminance - drives the blackout flag.
    pub min_luminance: f64,
    /// Whether any frame in the clip had a detected face - informational
    /// only, see `FrameMetrics::face_detected`.
    pub contains_face: bool,
    /// Composite 0-100 quality score.
    pub score: f64,
    pub classification: Classification,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub total_clips: usize,
    pub best_take_count: usize,
    pub usable_broll_count: usize,
    pub discard_count: usize,
    pub blurry_flagged: usize,
    pub shaky_flagged: usize,
    pub blackout_flagged: usize,
}

impl ProjectSummary {
    pub fn from_clips(clips: &[AnalyzedClip]) -> Self {
        let mut s = ProjectSummary {
            total_clips: clips.len(),
            ..Default::default()
        };
        for c in clips {
            match c.classification {
                Classification::BestTake => s.best_take_count += 1,
                Classification::UsableBRoll => s.usable_broll_count += 1,
                Classification::DiscardTake => s.discard_count += 1,
            }
            if c.flags.iter().any(|f| f == "blurry") {
                s.blurry_flagged += 1;
            }
            if c.flags.iter().any(|f| f == "shaky") {
                s.shaky_flagged += 1;
            }
            if c.flags.iter().any(|f| f == "blackout") {
                s.blackout_flagged += 1;
            }
        }
        s
    }
}
