pub mod ffmpeg;
pub mod ingest;
pub mod models;
pub mod pipeline;
pub mod scoring;
pub mod xml_export;

pub use models::{AnalyzedClip, Classification, ClipInfo, ProjectSummary, Tolerance};
