use image::DynamicImage;
use thiserror::Error;

#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
const INPUT_WIDTH: usize = 320;
#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
const INPUT_HEIGHT: usize = 240;
pub const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.7;
pub const DEFAULT_IOU_THRESHOLD: f32 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceBox {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub confidence: f32,
}

#[derive(Debug, Error)]
pub enum FaceError {
    #[error("failed to load face detection model: {0}")]
    ModelLoad(String),
    #[error("face detection inference failed: {0}")]
    Inference(String),
    #[error("face detection model returned an unexpected output")]
    UnexpectedOutput,
    #[error(
        "face detection was not compiled into this build (missing the face-detection feature)"
    )]
    Disabled,
}

/// Preprocesses an image into the flat, row-major CHW `f32` buffer the
/// bundled detector expects: resized to 320x240, RGB, normalized to
/// `(pixel - 127) / 128` per the model's own reference implementation (see
/// `crates/cullflow-core/assets/THIRD_PARTY_LICENSES.md`). Pure and
/// independent of the ONNX runtime, so it's testable without a model.
#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
fn preprocess(image: &DynamicImage) -> Vec<f32> {
    let resized = image
        .resize_exact(
            INPUT_WIDTH as u32,
            INPUT_HEIGHT as u32,
            image::imageops::FilterType::Triangle,
        )
        .to_rgb8();

    let mut data = vec![0f32; 3 * INPUT_HEIGHT * INPUT_WIDTH];
    for (x, y, pixel) in resized.enumerate_pixels() {
        for c in 0..3 {
            let idx = c * INPUT_HEIGHT * INPUT_WIDTH + (y as usize) * INPUT_WIDTH + (x as usize);
            data[idx] = (pixel[c] as f32 - 127.0) / 128.0;
        }
    }
    data
}

/// Turns raw model output into face boxes in the original image's pixel
/// coordinates: threshold on the face-class confidence, then greedy
/// IoU-based non-max suppression - a direct translation of the model's own
/// reference `predict()`/`hard_nms()` (see THIRD_PARTY_LICENSES.md). Pure
/// and independent of the ONNX runtime, so it's testable against synthetic
/// tensors without a model.
///
/// `confidences` is the flat `[num_priors * 2]` (background, face) output;
/// `boxes` is the flat `[num_priors * 4]` corner-form output normalized to
/// `[0, 1]`.
#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
fn postprocess(
    confidences: &[f32],
    boxes: &[f32],
    orig_width: f32,
    orig_height: f32,
    confidence_threshold: f32,
    iou_threshold: f32,
) -> Vec<FaceBox> {
    let num_priors = confidences.len() / 2;
    let mut candidates: Vec<FaceBox> = (0..num_priors)
        .filter_map(|i| {
            let face_confidence = confidences[i * 2 + 1];
            if face_confidence > confidence_threshold {
                Some(FaceBox {
                    x1: boxes[i * 4] * orig_width,
                    y1: boxes[i * 4 + 1] * orig_height,
                    x2: boxes[i * 4 + 2] * orig_width,
                    y2: boxes[i * 4 + 3] * orig_height,
                    confidence: face_confidence,
                })
            } else {
                None
            }
        })
        .collect();

    candidates.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

    let mut picked: Vec<FaceBox> = Vec::new();
    'candidates: for candidate in candidates {
        for kept in &picked {
            if iou(&candidate, kept) > iou_threshold {
                continue 'candidates;
            }
        }
        picked.push(candidate);
    }
    picked
}

#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
fn iou(a: &FaceBox, b: &FaceBox) -> f32 {
    let inter_x1 = a.x1.max(b.x1);
    let inter_y1 = a.y1.max(b.y1);
    let inter_x2 = a.x2.min(b.x2);
    let inter_y2 = a.y2.min(b.y2);
    let inter_area = (inter_x2 - inter_x1).max(0.0) * (inter_y2 - inter_y1).max(0.0);

    let area_a = (a.x2 - a.x1).max(0.0) * (a.y2 - a.y1).max(0.0);
    let area_b = (b.x2 - b.x1).max(0.0) * (b.y2 - b.y1).max(0.0);
    let union = area_a + area_b - inter_area;

    if union <= 0.0 {
        0.0
    } else {
        inter_area / union
    }
}

/// Wraps the bundled face detector (Ultra-Light-Fast-Generic-Face-Detector-1MB,
/// RFB-320 variant - MIT licensed, see THIRD_PARTY_LICENSES.md). Detects
/// whether *a* face is present, for the informational "contains a face"
/// signal in the UI; it does not yet locate eye landmarks, so it cannot
/// drive the blueprint's blink/expression gate (see README's "Not yet
/// built" - that needs a landmark model this project hasn't been able to
/// source and verify yet).
pub struct FaceDetector {
    #[cfg(feature = "face-detection")]
    session: ort::session::Session,
}

impl FaceDetector {
    #[cfg(feature = "face-detection")]
    pub fn load() -> Result<Self, FaceError> {
        let bytes: &[u8] = include_bytes!("../assets/face_detector_rfb320.onnx");
        let session = ort::session::Session::builder()
            .map_err(|e| FaceError::ModelLoad(e.to_string()))?
            .commit_from_memory(bytes)
            .map_err(|e| FaceError::ModelLoad(e.to_string()))?;
        Ok(Self { session })
    }

    #[cfg(not(feature = "face-detection"))]
    pub fn load() -> Result<Self, FaceError> {
        Err(FaceError::Disabled)
    }

    #[cfg(feature = "face-detection")]
    pub fn detect(&mut self, image: &DynamicImage) -> Result<Vec<FaceBox>, FaceError> {
        use ort::value::Tensor;

        let (orig_width, orig_height) = (image.width() as f32, image.height() as f32);
        let data = preprocess(image);
        let input =
            Tensor::from_array((vec![1i64, 3, INPUT_HEIGHT as i64, INPUT_WIDTH as i64], data))
                .map_err(|e| FaceError::Inference(e.to_string()))?;

        let outputs = self
            .session
            .run(ort::inputs! { "input" => input })
            .map_err(|e| FaceError::Inference(e.to_string()))?;

        let (_, confidences) = outputs
            .get("scores")
            .ok_or(FaceError::UnexpectedOutput)?
            .try_extract_tensor::<f32>()
            .map_err(|e| FaceError::Inference(e.to_string()))?;
        let (_, boxes) = outputs
            .get("boxes")
            .ok_or(FaceError::UnexpectedOutput)?
            .try_extract_tensor::<f32>()
            .map_err(|e| FaceError::Inference(e.to_string()))?;

        Ok(postprocess(
            confidences,
            boxes,
            orig_width,
            orig_height,
            DEFAULT_CONFIDENCE_THRESHOLD,
            DEFAULT_IOU_THRESHOLD,
        ))
    }

    #[cfg(not(feature = "face-detection"))]
    pub fn detect(&mut self, _image: &DynamicImage) -> Result<Vec<FaceBox>, FaceError> {
        unreachable!("FaceDetector::load always fails without the face-detection feature")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    #[test]
    fn preprocess_produces_expected_shape_and_normalization() {
        let image =
            DynamicImage::ImageRgb8(ImageBuffer::from_pixel(640, 480, Rgb([127, 127, 127])));
        let data = preprocess(&image);
        assert_eq!(data.len(), 3 * INPUT_HEIGHT * INPUT_WIDTH);
        // 127 is the model's own mean, so a flat mid-gray frame should
        // normalize to ~0.0 everywhere.
        assert!(data.iter().all(|v| v.abs() < 0.01));
    }

    fn synthetic_output(
        num_priors: usize,
        face_at: &[(usize, f32, [f32; 4])],
    ) -> (Vec<f32>, Vec<f32>) {
        let mut confidences = vec![0.0f32; num_priors * 2];
        let mut boxes = vec![0.0f32; num_priors * 4];
        for i in 0..num_priors {
            confidences[i * 2] = 1.0; // background by default
        }
        for &(i, conf, bbox) in face_at {
            confidences[i * 2] = 1.0 - conf;
            confidences[i * 2 + 1] = conf;
            boxes[i * 4..i * 4 + 4].copy_from_slice(&bbox);
        }
        (confidences, boxes)
    }

    #[test]
    fn postprocess_filters_by_confidence_threshold() {
        let (confidences, boxes) = synthetic_output(
            4,
            &[
                (0, 0.9, [0.1, 0.1, 0.3, 0.3]),
                (1, 0.2, [0.5, 0.5, 0.7, 0.7]),
            ],
        );
        let result = postprocess(&confidences, &boxes, 100.0, 100.0, 0.5, 0.3);
        assert_eq!(result.len(), 1);
        assert!(result[0].confidence > 0.5);
    }

    #[test]
    fn postprocess_scales_boxes_to_original_image_size() {
        let (confidences, boxes) = synthetic_output(1, &[(0, 0.9, [0.1, 0.2, 0.5, 0.6])]);
        let result = postprocess(&confidences, &boxes, 200.0, 100.0, 0.5, 0.3);
        assert_eq!(result.len(), 1);
        let b = result[0];
        assert!((b.x1 - 20.0).abs() < 1e-4);
        assert!((b.y1 - 20.0).abs() < 1e-4);
        assert!((b.x2 - 100.0).abs() < 1e-4);
        assert!((b.y2 - 60.0).abs() < 1e-4);
    }

    #[test]
    fn postprocess_suppresses_overlapping_lower_confidence_boxes() {
        // Two near-identical boxes for the same face - NMS should keep only the higher-confidence one.
        let (confidences, boxes) = synthetic_output(
            2,
            &[
                (0, 0.95, [0.1, 0.1, 0.5, 0.5]),
                (1, 0.8, [0.11, 0.11, 0.5, 0.5]),
            ],
        );
        let result = postprocess(&confidences, &boxes, 100.0, 100.0, 0.5, 0.3);
        assert_eq!(result.len(), 1);
        assert!(result[0].confidence > 0.9);
    }

    #[test]
    fn postprocess_keeps_distinct_non_overlapping_faces() {
        let (confidences, boxes) = synthetic_output(
            2,
            &[
                (0, 0.9, [0.0, 0.0, 0.2, 0.2]),
                (1, 0.8, [0.6, 0.6, 0.9, 0.9]),
            ],
        );
        let result = postprocess(&confidences, &boxes, 100.0, 100.0, 0.5, 0.3);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn empty_scene_yields_no_faces() {
        let (confidences, boxes) = synthetic_output(10, &[]);
        let result = postprocess(&confidences, &boxes, 100.0, 100.0, 0.5, 0.3);
        assert!(result.is_empty());
    }
}
