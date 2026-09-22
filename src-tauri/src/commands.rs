use crate::license_store::{self, LicenseStatus};
use crate::sidecar::resolve_ffmpeg;
use crate::state::AppState;
use cullflow_core::models::ProjectSummary;
use cullflow_core::pipeline::{analyze_clips, PipelineConfig};
use cullflow_core::{ingest, scoring, xml_export, AnalyzedClip, ClipInfo, Tolerance};
use serde::Serialize;
use std::path::Path;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct AnalysisResult {
    pub clips: Vec<AnalyzedClip>,
    pub summary: ProjectSummary,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn scan_folder(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<ClipInfo>, String> {
    // Walking a real wedding-shoot folder (500GB+, per the blueprint) can
    // take a while - run it off the UI thread so the window doesn't appear
    // to hang while it's still walking.
    let clips = tauri::async_runtime::spawn_blocking(move || ingest::scan_folder(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    *state.scanned_clips.lock().unwrap() = clips.clone();
    Ok(clips)
}

#[tauri::command]
pub async fn analyze_project(
    tolerance: String,
    state: State<'_, AppState>,
) -> Result<AnalysisResult, String> {
    let clips = state.scanned_clips.lock().unwrap().clone();
    if clips.is_empty() {
        return Err("No folder scanned yet - run scan_folder first.".to_string());
    }

    let ffmpeg_path = resolve_ffmpeg()?;
    let tolerance = Tolerance::from_str_or_default(&tolerance);
    *state.tolerance.lock().unwrap() = tolerance;

    let config = PipelineConfig {
        sample_every_secs: 0.5,
        tolerance,
        proxy_root: state.proxy_root.clone(),
    };

    // analyze_clips shells out to ffmpeg per clip (in parallel via rayon)
    // and can run for minutes on a real folder - run it off the UI thread
    // so the window stays responsive instead of going "Not Responding"
    // while it works.
    let results = tauri::async_runtime::spawn_blocking(move || {
        analyze_clips(&ffmpeg_path, &clips, &config)
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut analyzed = Vec::new();
    let mut errors = Vec::new();
    for r in results {
        match r {
            Ok(a) => analyzed.push(a),
            Err(e) => errors.push(e),
        }
    }

    let summary = ProjectSummary::from_clips(&analyzed);
    *state.analyzed_clips.lock().unwrap() = analyzed.clone();

    Ok(AnalysisResult {
        clips: analyzed,
        summary,
        errors,
    })
}

/// Backs the UI's live sensitivity slider: reclassifies already-scored clips
/// against a new tolerance without touching ffmpeg or re-scoring frames.
#[tauri::command]
pub fn set_tolerance(tolerance: String, state: State<AppState>) -> Result<AnalysisResult, String> {
    let tolerance = Tolerance::from_str_or_default(&tolerance);
    *state.tolerance.lock().unwrap() = tolerance;

    let current = state.analyzed_clips.lock().unwrap().clone();
    if current.is_empty() {
        return Err("No analyzed clips yet - run analyze_project first.".to_string());
    }

    let reclassified = scoring::reclassify(current, tolerance);
    let summary = ProjectSummary::from_clips(&reclassified);
    *state.analyzed_clips.lock().unwrap() = reclassified.clone();

    Ok(AnalysisResult {
        clips: reclassified,
        summary,
        errors: vec![],
    })
}

/// Exporting the timeline is the paid deliverable: scanning and scoring
/// stay free so an editor can see CullFlow actually work on their footage
/// before buying, but generating the NLE sequence requires an active
/// license (see `license_store` for what "active" checks).
#[tauri::command]
pub fn export_xml(output_path: String, state: State<AppState>) -> Result<String, String> {
    if !license_store::is_active(&state.config_dir) {
        return Err(
            "Activate a CullFlow AI license to export your project - enter your license key in the app's License panel.".to_string(),
        );
    }

    let clips = state.analyzed_clips.lock().unwrap().clone();
    if clips.is_empty() {
        return Err("No analyzed clips yet - run analyze_project first.".to_string());
    }

    let xml = xml_export::generate_fcpxml(&clips, 0.5);
    std::fs::write(&output_path, xml).map_err(|e| e.to_string())?;
    Ok(output_path)
}

#[tauri::command]
pub fn get_license_status(state: State<AppState>) -> LicenseStatus {
    license_store::current_status(&state.config_dir)
}

#[tauri::command]
pub fn activate_license(key: String, state: State<AppState>) -> Result<LicenseStatus, String> {
    license_store::activate(&state.config_dir, &key)
}
