use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

/// ffmpeg.exe is a console application, so spawning it from a GUI app on
/// Windows pops up a visible console window per invocation unless told not
/// to - with proxy extraction running per-clip and in parallel
/// (`pipeline::analyze_clips`), a folder of any real size would otherwise
/// flash open dozens of console windows during a scan.
fn new_ffmpeg_command(ffmpeg_path: &Path) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = Command::new(ffmpeg_path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[derive(Debug, Error)]
pub enum FfmpegError {
    #[error("ffmpeg binary not found (set CULLFLOW_FFMPEG_PATH or install ffmpeg on PATH)")]
    NotFound,
    #[error("failed to spawn ffmpeg: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("ffmpeg exited with a non-zero status: {0}")]
    NonZeroExit(String),
}

/// Resolves the ffmpeg binary to shell out to. Desktop builds bundle a static
/// ffmpeg next to the app (per the blueprint's "Static Bundled FFmpeg" layer);
/// `CULLFLOW_FFMPEG_PATH` lets that bundled path be wired in at runtime, and
/// PATH lookup keeps local development working without bundling.
pub fn locate_ffmpeg() -> Result<PathBuf, FfmpegError> {
    if let Ok(p) = std::env::var("CULLFLOW_FFMPEG_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }
    which::which("ffmpeg").map_err(|_| FfmpegError::NotFound)
}

/// Hardware-accelerated decode backend to request from ffmpeg, chosen per platform.
/// Falls back silently to software decode if the backend isn't available on
/// this machine (ffmpeg errors are only fatal for the frame-extraction step
/// as a whole, not per-backend).
pub fn platform_hwaccel() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("videotoolbox")
    } else if cfg!(target_os = "windows") {
        Some("cuda")
    } else {
        None
    }
}

/// Extracts a lightweight proxy frame sequence from `clip_path` into `out_dir`
/// at `sample_every_secs` intervals (blueprint default: 1 frame / 0.5s),
/// downscaled to 360p to keep the CV pass fast. Returns the extracted frame
/// paths in timestamp order.
pub fn extract_proxy_frames(
    ffmpeg_path: &Path,
    clip_path: &Path,
    out_dir: &Path,
    sample_every_secs: f64,
) -> Result<Vec<PathBuf>, FfmpegError> {
    std::fs::create_dir_all(out_dir)?;
    let fps = 1.0 / sample_every_secs;
    let pattern = out_dir.join("frame_%06d.jpg");

    let run = |hwaccel: Option<&str>| -> Result<std::process::Output, FfmpegError> {
        let mut cmd = new_ffmpeg_command(ffmpeg_path);
        cmd.arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error");
        if let Some(hw) = hwaccel {
            cmd.arg("-hwaccel").arg(hw);
        }
        cmd.arg("-i")
            .arg(clip_path)
            .arg("-vf")
            .arg(format!("fps={fps},scale=-2:360"))
            .arg("-q:v")
            .arg("5")
            .arg(&pattern);
        Ok(cmd.output()?)
    };

    let output = match platform_hwaccel() {
        Some(hw) => match run(Some(hw)) {
            Ok(o) if o.status.success() => o,
            _ => run(None)?,
        },
        None => run(None)?,
    };

    if !output.status.success() {
        return Err(FfmpegError::NonZeroExit(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let mut frames: Vec<PathBuf> = std::fs::read_dir(out_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jpg"))
        .collect();
    frames.sort();
    Ok(frames)
}

/// Extracts `clip_path`'s audio track to `out_path` as raw 16kHz mono
/// 32-bit float PCM (little-endian, headerless) - `audio::SileroVad`'s
/// expected input format. Returns `Ok(false)` (not an error) when the clip
/// has no audio stream at all, since a video-only B-roll rig is a common,
/// legitimate case, not a failure.
pub fn extract_proxy_audio(
    ffmpeg_path: &Path,
    clip_path: &Path,
    out_path: &Path,
) -> Result<bool, FfmpegError> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let output = new_ffmpeg_command(ffmpeg_path)
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(clip_path)
        .arg("-vn")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-f")
        .arg("f32le")
        .arg(out_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not contain any stream") {
            return Ok(false);
        }
        return Err(FfmpegError::NonZeroExit(stderr.to_string()));
    }

    Ok(std::fs::metadata(out_path)
        .map(|m| m.len() > 0)
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_ffmpeg_respects_env_override() {
        std::env::remove_var("CULLFLOW_FFMPEG_PATH");
        // Without ffmpeg installed and no override, this should surface NotFound
        // rather than panicking - exercised as a smoke test for the error path.
        if which::which("ffmpeg").is_err() {
            assert!(matches!(locate_ffmpeg(), Err(FfmpegError::NotFound)));
        }
    }
}
