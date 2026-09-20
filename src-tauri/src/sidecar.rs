use std::path::PathBuf;

/// Resolves the ffmpeg binary CullFlow should shell out to, in priority order:
/// 1. `CULLFLOW_FFMPEG_PATH` env var (explicit override, mainly for testing).
/// 2. The bundled sidecar binary shipped next to the app executable - this is
///    where Tauri's `externalBin` (see tauri.conf.json) places it at build
///    time, so a packaged release never needs the user to have ffmpeg
///    installed at all.
/// 3. `ffmpeg` on `PATH` - dev-mode fallback so `npm run tauri dev` works
///    without a bundled binary present (see `scripts/fetch-ffmpeg.*` to build
///    one locally, or just have ffmpeg installed while developing).
pub fn resolve_ffmpeg() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("CULLFLOW_FFMPEG_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }

    if let Some(bundled) = bundled_sidecar_path() {
        if bundled.is_file() {
            return Ok(bundled);
        }
    }

    cullflow_core::ffmpeg::locate_ffmpeg().map_err(|e| e.to_string())
}

/// Sidecar binaries are placed by Tauri in the same directory as the app's
/// own executable, named after the `externalBin` entry with the
/// target-triple suffix stripped (plus `.exe` on Windows) - this mirrors
/// `tauri-plugin-shell`'s `relative_command_path` resolution so we get the
/// same behavior without depending on the shell plugin's async Command API.
fn bundled_sidecar_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    Some(dir.join(name))
}
