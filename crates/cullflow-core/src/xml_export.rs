use crate::models::{AnalyzedClip, Classification};

/// Nominal timeline rate used for the exported sequence. Proxy scoring only
/// gives us frame timestamps at the sampling interval (default 0.5s), not the
/// source clip's true frame rate, so clip durations here are an editor-usable
/// approximation, not frame-accurate - the export's job is to land in the
/// right place on the timeline with the right marker, not to replace a
/// conform pass.
const TIMELINE_FPS: u32 = 30;

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Percent-encodes everything outside a small safe set (keeping `/` and `:`
/// unescaped so drive letters and path separators stay readable) - real
/// footage paths routinely contain spaces, and an un-encoded space in a
/// `file://` URL is invalid enough that some importers fail to resolve the
/// media even when the rest of the XML parses fine.
fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Builds a `file://localhost/...` path URL the way Final Cut Pro 7 XML
/// (and Premiere Pro/DaVinci Resolve's importers for it) expect: an absolute
/// path with forward slashes, a leading slash before a Windows drive letter,
/// and percent-encoded special characters.
fn file_pathurl(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let absolute = if normalized.starts_with('/') {
        normalized
    } else {
        format!("/{normalized}")
    };
    format!("file://localhost{}", percent_encode_path(&absolute))
}

fn frames(secs: f64) -> u64 {
    (secs * TIMELINE_FPS as f64).round().max(1.0) as u64
}

fn clip_duration_secs(clip: &AnalyzedClip, sample_every_secs: f64) -> f64 {
    let n = clip.frames.len() as f64;
    (n * sample_every_secs).max(sample_every_secs)
}

fn marker_for(classification: Classification, score: f64, flags: &[String]) -> String {
    let label = match classification {
        Classification::BestTake => "Best Take",
        Classification::UsableBRoll => "Usable B-Roll",
        Classification::DiscardTake => "Discard Take",
    };
    let flag_suffix = if flags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", flags.join(", "))
    };
    xml_escape(&format!("{label} (score {:.0}){flag_suffix}", score))
}

fn rate_block(indent: &str) -> String {
    format!("{indent}<rate>\n{indent}  <timebase>{TIMELINE_FPS}</timebase>\n{indent}  <ntsc>FALSE</ntsc>\n{indent}</rate>\n")
}

/// Builds a "Final Cut Pro 7 XML Interchange Format" (XMEML v5) sequence -
/// despite the name, this is the format Adobe Premiere Pro's File > Import
/// actually understands, and DaVinci Resolve accepts it too. It is a
/// completely different, older schema from modern FCPXML (what Final Cut
/// Pro X itself uses, `<fcpxml version="...">`) - the two are easily
/// confused since both get casually called "Final Cut Pro XML", but
/// Premiere Pro's importer does not recognize modern FCPXML at all (it
/// rejects it immediately as an unsupported file type, before parsing any
/// content).
///
/// One clipitem per source clip in folder order, a point marker carrying
/// the classification + score on each, and discard takes disabled
/// (`<enabled>FALSE</enabled>`) rather than removed, so an editor can review
/// and restore them - matching the blueprint's "muted secondary track for
/// editor safety" intent without needing multi-lane math.
pub fn generate_premiere_xml(clips: &[AnalyzedClip], sample_every_secs: f64) -> String {
    let mut clipitems = String::new();
    let mut start_frame: u64 = 0;

    for (i, clip) in clips.iter().enumerate() {
        let n = i + 1;
        let duration_secs = clip_duration_secs(clip, sample_every_secs);
        let duration_frames = frames(duration_secs);
        let end_frame = start_frame + duration_frames;
        let name = xml_escape(&clip.clip.file_name);
        let pathurl = xml_escape(&file_pathurl(&clip.clip.path));
        let enabled = if clip.classification == Classification::DiscardTake {
            "FALSE"
        } else {
            "TRUE"
        };
        let marker_value = marker_for(clip.classification, clip.score, &clip.flags);

        clipitems.push_str(&format!("          <clipitem id=\"clipitem-{n}\">\n"));
        clipitems.push_str(&format!("            <name>{name}</name>\n"));
        clipitems.push_str(&format!("            <enabled>{enabled}</enabled>\n"));
        clipitems.push_str(&format!(
            "            <duration>{duration_frames}</duration>\n"
        ));
        clipitems.push_str(&rate_block("            "));
        clipitems.push_str(&format!("            <start>{start_frame}</start>\n"));
        clipitems.push_str(&format!("            <end>{end_frame}</end>\n"));
        clipitems.push_str("            <in>0</in>\n");
        clipitems.push_str(&format!("            <out>{duration_frames}</out>\n"));
        clipitems.push_str(&format!("            <file id=\"file-{n}\">\n"));
        clipitems.push_str(&format!("              <name>{name}</name>\n"));
        clipitems.push_str(&format!("              <pathurl>{pathurl}</pathurl>\n"));
        clipitems.push_str(&rate_block("              "));
        clipitems.push_str(&format!(
            "              <duration>{duration_frames}</duration>\n"
        ));
        clipitems.push_str("              <media>\n");
        clipitems.push_str("                <video>\n");
        clipitems.push_str("                  <samplecharacteristics>\n");
        clipitems.push_str("                    <width>1920</width>\n");
        clipitems.push_str("                    <height>1080</height>\n");
        clipitems.push_str("                  </samplecharacteristics>\n");
        clipitems.push_str("                </video>\n");
        if clip.has_audio {
            clipitems.push_str("                <audio>\n");
            clipitems.push_str("                  <samplecharacteristics>\n");
            clipitems.push_str("                    <depth>16</depth>\n");
            clipitems.push_str("                    <samplerate>48000</samplerate>\n");
            clipitems.push_str("                  </samplecharacteristics>\n");
            clipitems.push_str("                  <channelcount>2</channelcount>\n");
            clipitems.push_str("                </audio>\n");
        }
        clipitems.push_str("              </media>\n");
        clipitems.push_str("            </file>\n");
        clipitems.push_str("            <marker>\n");
        clipitems.push_str(&format!("              <name>{marker_value}</name>\n"));
        clipitems.push_str("              <in>0</in>\n");
        clipitems.push_str("              <out>-1</out>\n");
        clipitems.push_str("              <comment></comment>\n");
        clipitems.push_str("            </marker>\n");
        clipitems.push_str("          </clipitem>\n");

        start_frame = end_frame;
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xmeml>
<xmeml version="5">
  <sequence>
    <name>CullFlow Cull Pass</name>
    <duration>{start_frame}</duration>
{seq_rate}    <media>
      <video>
        <format>
          <samplecharacteristics>
            <width>1920</width>
            <height>1080</height>
          </samplecharacteristics>
        </format>
        <track>
{clipitems}        </track>
      </video>
    </media>
  </sequence>
</xmeml>
"#,
        seq_rate = rate_block("    "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ClipInfo;

    fn clip(name: &str, classification: Classification, score: f64) -> AnalyzedClip {
        AnalyzedClip {
            clip: ClipInfo {
                id: name.into(),
                path: format!("/footage/{name}"),
                file_name: name.into(),
                size_bytes: 0,
            },
            frames: vec![],
            min_sharpness: 0.0,
            max_motion_incoherence: 0.0,
            min_luminance: 0.0,
            contains_face: false,
            contains_blink: false,
            has_audio: false,
            speech_ratio: None,
            audio_clipping_ratio: 0.0,
            score,
            classification,
            flags: vec![],
        }
    }

    #[test]
    fn discard_clips_are_disabled_in_the_timeline() {
        let xml = generate_premiere_xml(&[clip("bad.mp4", Classification::DiscardTake, 10.0)], 0.5);
        assert!(xml.contains("<enabled>FALSE</enabled>"));
    }

    #[test]
    fn enabled_clips_are_marked_true() {
        let xml = generate_premiere_xml(&[clip("good.mp4", Classification::BestTake, 90.0)], 0.5);
        assert!(xml.contains("<enabled>TRUE</enabled>"));
    }

    #[test]
    fn output_is_xmeml_not_modern_fcpxml() {
        // Modern FCPXML (<fcpxml version="...">) is Final Cut Pro X's own
        // format - Premiere Pro's importer doesn't recognize it at all and
        // rejects the file outright. XMEML (<xmeml version="5">) is the
        // legacy "Final Cut Pro 7 XML" format Premiere's File > Import
        // actually understands, and DaVinci Resolve accepts it too.
        let xml = generate_premiere_xml(&[clip("a.mp4", Classification::BestTake, 90.0)], 0.5);
        assert!(xml.contains("<!DOCTYPE xmeml>"));
        assert!(xml.contains("<xmeml version=\"5\">"));
        assert!(!xml.contains("fcpxml"));
    }

    #[test]
    fn output_is_well_formed_enough_to_parse_as_xml() {
        let xml = generate_premiere_xml(
            &[
                clip("a.mp4", Classification::BestTake, 90.0),
                clip("b.mp4", Classification::UsableBRoll, 50.0),
                clip("c.mp4", Classification::DiscardTake, 5.0),
            ],
            0.5,
        );
        // Cheap structural sanity check without pulling in a full XML parser dependency.
        assert_eq!(xml.matches("<clipitem ").count(), 3);
        assert_eq!(xml.matches("</clipitem>").count(), 3);
        assert!(xml.starts_with("<?xml"));
    }

    #[test]
    fn clip_timeline_positions_are_contiguous() {
        let xml = generate_premiere_xml(
            &[
                clip("a.mp4", Classification::BestTake, 90.0),
                clip("b.mp4", Classification::UsableBRoll, 50.0),
            ],
            0.5,
        );
        // Each clip has 0 sampled frames in this fixture, so
        // clip_duration_secs falls back to sample_every_secs (0.5s = 15
        // frames at 30fps): clip 1 spans [0,15), clip 2 spans [15,30).
        assert!(xml.contains("<start>0</start>"));
        assert!(xml.contains("<end>15</end>"));
        assert!(xml.contains("<start>15</start>"));
        assert!(xml.contains("<end>30</end>"));
        assert!(xml.contains("<duration>30</duration>")); // sequence total
    }

    #[test]
    fn path_with_spaces_is_percent_encoded_in_the_pathurl() {
        let mut c = clip("My Clip.mp4", Classification::BestTake, 90.0);
        c.clip.path = "E:\\Wedding Shoot\\raw\\My Clip.mp4".to_string();
        let xml = generate_premiere_xml(&[c], 0.5);
        assert!(xml.contains("file://localhost/E:/Wedding%20Shoot/raw/My%20Clip.mp4"));
        assert!(!xml.contains("localhost/E:/Wedding Shoot"));
    }

    #[test]
    fn audio_samplecharacteristics_only_present_when_clip_has_audio() {
        let mut with_audio = clip("a.mp4", Classification::BestTake, 90.0);
        with_audio.has_audio = true;
        let xml = generate_premiere_xml(&[with_audio], 0.5);
        assert!(xml.contains("<audio>"));

        let without_audio = clip("b.mp4", Classification::BestTake, 90.0);
        let xml = generate_premiere_xml(&[without_audio], 0.5);
        assert!(!xml.contains("<audio>"));
    }
}
