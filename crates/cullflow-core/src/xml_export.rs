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

fn file_uri(path: &str) -> String {
    if path.starts_with('/') {
        format!("file://{}", xml_escape(path))
    } else {
        // Windows-style paths (C:\...): normalize to file:///C:/...
        let normalized = path.replace('\\', "/");
        format!("file:///{}", xml_escape(&normalized))
    }
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

/// Builds a standard FCPXML 1.10 sequence (imports cleanly into both DaVinci
/// Resolve and Premiere Pro) with one asset-clip per source clip in folder
/// order, a marker carrying the classification + score on each, and discard
/// takes disabled in the timeline (`enabled="0"`) rather than deleted, so an
/// editor can review and restore them - matching the blueprint's "muted
/// secondary track for editor safety" intent without needing multi-lane math.
///
/// Marker type doubles as the color cue FCPXML itself doesn't carry: Best
/// Take is a completed to-do marker (renders as a green checkmark), Discard
/// is an incomplete to-do marker (renders red/orange), and Usable B-Roll is
/// a standard marker.
pub fn generate_fcpxml(clips: &[AnalyzedClip], sample_every_secs: f64) -> String {
    let mut resources = String::new();
    let mut spine = String::new();
    let mut offset_frames: u64 = 0;

    resources.push_str(&format!(
        "    <format id=\"r0\" name=\"FFVideoFormat1080p30\" frameDuration=\"1/{TIMELINE_FPS}s\" width=\"1920\" height=\"1080\"/>\n"
    ));

    for (i, clip) in clips.iter().enumerate() {
        let asset_id = format!("a{i}");
        let duration_secs = clip_duration_secs(clip, sample_every_secs);
        let duration_frames = frames(duration_secs);
        let uri = file_uri(&clip.clip.path);
        let name = xml_escape(&clip.clip.file_name);

        resources.push_str(&format!(
            "    <asset id=\"{asset_id}\" name=\"{name}\" src=\"{uri}\" start=\"0/{TIMELINE_FPS}s\" duration=\"{duration_frames}/{TIMELINE_FPS}s\" hasVideo=\"1\" format=\"r0\"/>\n"
        ));

        let enabled = if clip.classification == Classification::DiscardTake {
            "0"
        } else {
            "1"
        };
        let marker_value = marker_for(clip.classification, clip.score, &clip.flags);
        let marker_completed = match clip.classification {
            Classification::BestTake => "1",
            Classification::DiscardTake => "0",
            Classification::UsableBRoll => "0",
        };
        let marker_tag = if clip.classification == Classification::UsableBRoll {
            format!("<marker start=\"0/{TIMELINE_FPS}s\" duration=\"1/{TIMELINE_FPS}s\" value=\"{marker_value}\"/>")
        } else {
            format!("<marker-to-do start=\"0/{TIMELINE_FPS}s\" duration=\"1/{TIMELINE_FPS}s\" value=\"{marker_value}\" completed=\"{marker_completed}\"/>")
        };

        spine.push_str(&format!(
            "        <asset-clip ref=\"{asset_id}\" name=\"{name}\" offset=\"{offset_frames}/{TIMELINE_FPS}s\" duration=\"{duration_frames}/{TIMELINE_FPS}s\" start=\"0/{TIMELINE_FPS}s\" enabled=\"{enabled}\">\n"
        ));
        spine.push_str(&format!("          {marker_tag}\n"));
        spine.push_str("        </asset-clip>\n");

        offset_frames += duration_frames;
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE fcpxml>
<fcpxml version="1.10">
  <resources>
{resources}  </resources>
  <library>
    <event name="CullFlow AI Export">
      <project name="CullFlow Cull Pass">
        <sequence format="r0">
          <spine>
{spine}          </spine>
        </sequence>
      </project>
    </event>
  </library>
</fcpxml>
"#
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
        let xml = generate_fcpxml(&[clip("bad.mp4", Classification::DiscardTake, 10.0)], 0.5);
        assert!(xml.contains("enabled=\"0\""));
    }

    #[test]
    fn best_take_uses_completed_to_do_marker() {
        let xml = generate_fcpxml(&[clip("good.mp4", Classification::BestTake, 90.0)], 0.5);
        assert!(xml.contains("marker-to-do"));
        assert!(xml.contains("completed=\"1\""));
    }

    #[test]
    fn output_is_well_formed_enough_to_parse_as_xml() {
        let xml = generate_fcpxml(
            &[
                clip("a.mp4", Classification::BestTake, 90.0),
                clip("b.mp4", Classification::UsableBRoll, 50.0),
                clip("c.mp4", Classification::DiscardTake, 5.0),
            ],
            0.5,
        );
        // Cheap structural sanity check without pulling in a full XML parser dependency.
        assert_eq!(xml.matches("<asset-clip").count(), 3);
        assert_eq!(xml.matches("</asset-clip>").count(), 3);
        assert!(xml.starts_with("<?xml"));
    }
}
