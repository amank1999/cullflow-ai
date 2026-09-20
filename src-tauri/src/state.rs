use cullflow_core::{AnalyzedClip, ClipInfo, Tolerance};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub scanned_clips: Mutex<Vec<ClipInfo>>,
    pub analyzed_clips: Mutex<Vec<AnalyzedClip>>,
    pub tolerance: Mutex<Tolerance>,
    pub proxy_root: PathBuf,
}

impl AppState {
    pub fn new(proxy_root: PathBuf) -> Self {
        Self {
            scanned_clips: Mutex::new(Vec::new()),
            analyzed_clips: Mutex::new(Vec::new()),
            tolerance: Mutex::new(Tolerance::Conservative),
            proxy_root,
        }
    }
}
