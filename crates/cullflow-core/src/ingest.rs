use crate::models::{ClipInfo, VIDEO_EXTENSIONS};
use std::path::Path;
use uuid::Uuid;
use walkdir::WalkDir;

/// Recursively scans `root` for video files by extension, returning stable
/// per-clip ids used to key proxy frames and analysis results.
pub fn scan_folder(root: &Path) -> std::io::Result<Vec<ClipInfo>> {
    let mut clips = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();

        if !VIDEO_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        let metadata = entry.metadata()?;
        clips.push(ClipInfo {
            id: Uuid::new_v4().to_string(),
            path: path.to_string_lossy().to_string(),
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            size_bytes: metadata.len(),
        });
    }

    clips.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(clips)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn finds_video_files_and_skips_others() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("clip1.mp4"), b"fake").unwrap();
        fs::write(dir.path().join("clip2.MOV"), b"fake").unwrap();
        fs::write(dir.path().join("notes.txt"), b"fake").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("clip3.mkv"), b"fake").unwrap();

        let clips = scan_folder(dir.path()).unwrap();
        assert_eq!(clips.len(), 3);
        assert!(clips.iter().any(|c| c.file_name == "clip1.mp4"));
        assert!(clips.iter().any(|c| c.file_name == "clip2.MOV"));
        assert!(clips.iter().any(|c| c.file_name == "clip3.mkv"));
    }
}
