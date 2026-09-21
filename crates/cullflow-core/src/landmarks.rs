use crate::face::FaceBox;
use image::DynamicImage;
use thiserror::Error;

const NUM_LMS: usize = 68;
#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
const NUM_NB: usize = 10;
const INPUT_SIZE: usize = 256;
#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
const NET_STRIDE: usize = 32;
pub const DEFAULT_EAR_THRESHOLD: f32 = 0.20;

#[derive(Debug, Error)]
pub enum LandmarkError {
    #[error("failed to load landmark model: {0}")]
    ModelLoad(String),
    #[error("landmark inference failed: {0}")]
    Inference(String),
    #[error("landmark model returned an unexpected output")]
    UnexpectedOutput,
    #[error(
        "landmark detection was not compiled into this build (missing the face-detection feature)"
    )]
    Disabled,
}

/// 300W-layout mean-face, 68 landmarks (136 floats), vendored from upstream
/// PIPNet (MIT): https://github.com/jhb86253817/PIPNet/tree/master/data -
/// see `assets/THIRD_PARTY_LICENSES.md`. Standard iBUG/300W point order:
/// indices 36-41 and 42-47 are the two eyes, used by `eye_aspect_ratio`.
#[rustfmt::skip]
const MEANFACE_300W_68: [f32; 136] = [
    0.05558998895410058, 0.23848280098218655, 0.05894856684324656, 0.3590187767402909,
    0.0736574254414371, 0.4792196439871159, 0.09980016420365162, 0.5959029676167197,
    0.14678670154995865, 0.7035615597409001, 0.21847188218752928, 0.7971705893013413,
    0.30554692814599393, 0.8750572978073209, 0.4018434142644611, 0.9365018059444535,
    0.5100536090382116, 0.9521295666029498, 0.6162039414413925, 0.9309467340899419,
    0.7094522484942942, 0.8669275031738761, 0.7940993502957612, 0.7879369615524398,
    0.8627063649669019, 0.6933756633633967, 0.9072386130534111, 0.5836975017700834,
    0.9298874997796132, 0.4657004930314701, 0.9405202670724796, 0.346063993805527,
    0.9425419553088846, 0.22558131891345742, 0.13304298285530403, 0.14853071838028062,
    0.18873587368440375, 0.09596491613770254, 0.2673231915839219, 0.08084218279128136,
    0.34878638553224905, 0.09253591849498964, 0.4226713753717798, 0.12466063383809506,
    0.5618513152452376, 0.11839668911898667, 0.6394952560845826, 0.08480191391770678,
    0.7204375851516752, 0.07249669092117161, 0.7988615904537885, 0.08766933146893043,
    0.8534884939460948, 0.1380096813348583, 0.49610677423740546, 0.21516740699375395,
    0.49709661403980665, 0.2928875699060973, 0.4982292618461611, 0.3699985379939941,
    0.49982965173254235, 0.4494119144493957, 0.406772397599095, 0.5032397294041786,
    0.45231994786363067, 0.5197953144002292, 0.49969685987914064, 0.5332489262413073,
    0.5470074224053442, 0.518413595827126, 0.5892261151542287, 0.5023530079850803,
    0.22414578747180394, 0.22835847349949062, 0.27262947128194215, 0.19915251892241678,
    0.3306759252861797, 0.20026034220607236, 0.38044435864341913, 0.23839196034290633,
    0.32884072789429913, 0.24902443794896897, 0.2707409300714473, 0.24950886025380967,
    0.6086826011068529, 0.23465048639345917, 0.660397116846103, 0.1937087938594717,
    0.7177815187666494, 0.19317079039835858, 0.7652328176062365, 0.22088822845258235,
    0.722727677909097, 0.24195514178450958, 0.6658378927310327, 0.2441554205021945,
    0.32894370935769124, 0.6496589505331646, 0.39347179739100613, 0.6216899667490776,
    0.4571976492475472, 0.60794251109236, 0.4990484623797022, 0.6190124015360254,
    0.5465555522325872, 0.6071477960565326, 0.6116127327356168, 0.6205387097430033,
    0.6742318496058836, 0.6437466364395467, 0.6144773141699744, 0.7077526646009754,
    0.5526442055374252, 0.7363350735898412, 0.5018120662554302, 0.7424476622366345,
    0.4554458875556401, 0.7382303858617719, 0.3923750731597415, 0.7118887028663435,
    0.35530766372404593, 0.6524479416354049, 0.457111071610868, 0.6467108367268608,
    0.49974082228815025, 0.6508406774477011, 0.5477027224368399, 0.6451242819422733,
    0.6478392760505715, 0.647852382880368, 0.5488474760115958, 0.6779061893042735,
    0.5001073351044452, 0.6845280260362221, 0.4564831746654594, 0.6799300301441035,
];

#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
fn meanface_points() -> [(f32, f32); NUM_LMS] {
    let mut points = [(0f32, 0f32); NUM_LMS];
    for (i, p) in points.iter_mut().enumerate() {
        *p = (MEANFACE_300W_68[i * 2], MEANFACE_300W_68[i * 2 + 1]);
    }
    points
}

/// Precomputed reverse-index tables for PIPNet's "pixel-in-pixel" decoding:
/// for each landmark, which (other landmark, neighbor-slot) pairs predict
/// its position, padded/cycled to a common length. Pure translation of the
/// reference `get_meanface_info`/`_build_neighbor_indices` (see
/// THIRD_PARTY_LICENSES.md) - independent of the ONNX runtime, so it's
/// testable without a model.
#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
fn build_neighbor_indices(meanface: &[(f32, f32)], num_nb: usize) -> (Vec<i64>, Vec<i64>, usize) {
    let num_lms = meanface.len();

    let mut neighbor_lists: Vec<Vec<usize>> = Vec::with_capacity(num_lms);
    for i in 0..num_lms {
        let (px, py) = meanface[i];
        let mut dists: Vec<(usize, f32)> = (0..num_lms)
            .map(|j| {
                let (qx, qy) = meanface[j];
                (j, (px - qx).powi(2) + (py - qy).powi(2))
            })
            .collect();
        dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        neighbor_lists.push(dists[1..1 + num_nb].iter().map(|&(j, _)| j).collect());
    }

    let mut reversed: Vec<(Vec<usize>, Vec<usize>)> = vec![(Vec::new(), Vec::new()); num_lms];
    for (i, neighbors) in neighbor_lists.iter().enumerate() {
        for (j, &neighbor) in neighbors.iter().enumerate() {
            reversed[neighbor].0.push(i);
            reversed[neighbor].1.push(j);
        }
    }

    let max_len = reversed
        .iter()
        .map(|(idx1, _)| idx1.len())
        .max()
        .unwrap_or(0);

    let mut reverse_index1 = Vec::with_capacity(num_lms * max_len);
    let mut reverse_index2 = Vec::with_capacity(num_lms * max_len);
    for (idx1, idx2) in &reversed {
        reverse_index1.extend(idx1.iter().map(|&v| v as i64));
        reverse_index2.extend(idx2.iter().map(|&v| v as i64));
        let need = max_len - idx1.len();
        // Upstream pads by cycling each landmark's own reference list back
        // from the start, so every landmark ends up with the same slot count.
        if !idx1.is_empty() {
            for k in 0..need {
                reverse_index1.push(idx1[k % idx1.len()] as i64);
                reverse_index2.push(idx2[k % idx2.len()] as i64);
            }
        }
    }

    (reverse_index1, reverse_index2, max_len)
}

/// PIPNet's pixel-in-pixel decode: each landmark's own peak location
/// predicts its own sub-pixel offset *and* the offsets of its `num_nb`
/// nearest neighbors; final position is the average of a landmark's own
/// prediction with every neighbor-prediction that targets it. Returns
/// points normalized to `[0, 1]` within the model's input canvas. Pure
/// translation of the reference `PIPNetONNX._decode` (see
/// THIRD_PARTY_LICENSES.md) - independent of the ONNX runtime, so it's
/// testable against synthetic tensors without a model.
#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
fn decode(
    cls_map: &[f32],
    offset_x: &[f32],
    offset_y: &[f32],
    nb_x: &[f32],
    nb_y: &[f32],
    reverse_index1: &[i64],
    reverse_index2: &[i64],
    max_len: usize,
    num_lms: usize,
    num_nb: usize,
    feat_h: usize,
    feat_w: usize,
    net_stride: usize,
    input_w: usize,
    input_h: usize,
) -> Vec<(f32, f32)> {
    let hw = feat_h * feat_w;
    let scale_x = input_w as f32 / net_stride as f32;
    let scale_y = input_h as f32 / net_stride as f32;

    let mut cols = vec![0f32; num_lms];
    let mut rows = vec![0f32; num_lms];
    let mut max_ids = vec![0usize; num_lms];
    let mut pred_x = vec![0f32; num_lms];
    let mut pred_y = vec![0f32; num_lms];

    for i in 0..num_lms {
        let slice = &cls_map[i * hw..(i + 1) * hw];
        let mut max_idx = 0usize;
        let mut max_val = f32::NEG_INFINITY;
        for (idx, &v) in slice.iter().enumerate() {
            if v > max_val {
                max_val = v;
                max_idx = idx;
            }
        }
        max_ids[i] = max_idx;
        cols[i] = (max_idx % feat_w) as f32;
        rows[i] = (max_idx / feat_w) as f32;

        let own_x = offset_x[i * hw + max_idx];
        let own_y = offset_y[i * hw + max_idx];
        pred_x[i] = (cols[i] + own_x) / scale_x;
        pred_y[i] = (rows[i] + own_y) / scale_y;
    }

    let mut nb_pred_x = vec![0f32; num_lms * num_nb];
    let mut nb_pred_y = vec![0f32; num_lms * num_nb];
    for i in 0..num_lms {
        let max_idx = max_ids[i];
        for j in 0..num_nb {
            let base = (i * num_nb + j) * hw;
            nb_pred_x[i * num_nb + j] = (cols[i] + nb_x[base + max_idx]) / scale_x;
            nb_pred_y[i * num_nb + j] = (rows[i] + nb_y[base + max_idx]) / scale_y;
        }
    }

    let mut result = Vec::with_capacity(num_lms);
    for k in 0..num_lms {
        let mut sum_x = pred_x[k];
        let mut sum_y = pred_y[k];
        let mut count = 1usize;
        for slot in 0..max_len {
            let flat = k * max_len + slot;
            let i = reverse_index1[flat] as usize;
            let j = reverse_index2[flat] as usize;
            sum_x += nb_pred_x[i * num_nb + j];
            sum_y += nb_pred_y[i * num_nb + j];
            count += 1;
        }
        result.push((sum_x / count as f32, sum_y / count as f32));
    }
    result
}

/// Upstream's asymmetric bbox padding (+10% left/right/bottom, -10% top,
/// clamped to the image bounds) before resizing to the model's input size.
/// Returns `(x1, y1, crop_w, crop_h)` in original-image pixel coordinates.
/// Pure and independent of the ONNX runtime.
#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
fn crop_bounds(img_w: i64, img_h: i64, bbox: &FaceBox) -> (i64, i64, i64, i64) {
    let (mut x1, mut y1, mut x2, mut y2) = (
        bbox.x1 as f64,
        bbox.y1 as f64,
        bbox.x2 as f64,
        bbox.y2 as f64,
    );
    let det_w = x2 - x1 + 1.0;
    let det_h = y2 - y1 + 1.0;
    let pad = 0.1;
    x1 -= (det_w * pad).trunc();
    y1 += (det_h * pad).trunc();
    x2 += (det_w * pad).trunc();
    y2 += (det_h * pad).trunc();

    let x1 = (x1.trunc() as i64).max(0);
    let y1 = (y1.trunc() as i64).max(0);
    let x2 = (x2.trunc() as i64).min(img_w - 1);
    let y2 = (y2.trunc() as i64).min(img_h - 1);
    (x1, y1, x2 - x1 + 1, y2 - y1 + 1)
}

/// Resizes a face crop to the model's 256x256 input and applies ImageNet
/// normalization (the crop is already RGB, unlike the upstream OpenCV/BGR
/// reference, so no channel swap is needed). Pure and independent of the
/// ONNX runtime.
#[cfg_attr(not(feature = "face-detection"), allow(dead_code))]
fn preprocess(crop: &DynamicImage) -> Vec<f32> {
    const MEAN: [f32; 3] = [0.485 * 255.0, 0.456 * 255.0, 0.406 * 255.0];
    const STD: [f32; 3] = [0.229 * 255.0, 0.224 * 255.0, 0.225 * 255.0];

    let resized = crop
        .resize_exact(
            INPUT_SIZE as u32,
            INPUT_SIZE as u32,
            image::imageops::FilterType::Triangle,
        )
        .to_rgb8();

    let mut data = vec![0f32; 3 * INPUT_SIZE * INPUT_SIZE];
    for (x, y, pixel) in resized.enumerate_pixels() {
        for c in 0..3 {
            let idx = c * INPUT_SIZE * INPUT_SIZE + (y as usize) * INPUT_SIZE + (x as usize);
            data[idx] = (pixel[c] as f32 - MEAN[c]) / STD[c];
        }
    }
    data
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Eye Aspect Ratio (Soukupova & Cech, 2016) for both eyes from a 68-point
/// iBUG/300W landmark set (indices 36-41 and 42-47). `None` if `points`
/// isn't a 68-point set. Pure and independent of the ONNX runtime.
pub fn eye_aspect_ratio(points: &[(f32, f32)]) -> Option<(f32, f32)> {
    if points.len() != NUM_LMS {
        return None;
    }
    let ear = |corner_a: usize,
               top1: usize,
               top2: usize,
               corner_b: usize,
               bot1: usize,
               bot2: usize|
     -> f32 {
        let horiz = dist(points[corner_a], points[corner_b]);
        if horiz <= 0.0 {
            return 0.0;
        }
        (dist(points[top1], points[bot2]) + dist(points[top2], points[bot1])) / (2.0 * horiz)
    };
    Some((ear(36, 37, 38, 39, 40, 41), ear(42, 43, 44, 45, 46, 47)))
}

/// True only when both eyes' EAR falls below `threshold` - per the
/// blueprint spec, `DEFAULT_EAR_THRESHOLD` (0.20) marks a closed eye.
/// Requiring both eyes avoids a false positive from a single strong-angle
/// landmark misfire, and `None` (fewer than 68 points, or degenerate
/// geometry) reads as "not closed" rather than an error.
pub fn eyes_closed(points: &[(f32, f32)], threshold: f32) -> bool {
    match eye_aspect_ratio(points) {
        Some((left, right)) => left < threshold && right < threshold,
        None => false,
    }
}

/// Wraps the bundled PIPNet landmark model (68-point, 300W+CelebA GSSL
/// checkpoint - MIT licensed, see THIRD_PARTY_LICENSES.md) to locate eye
/// landmarks inside an already-detected face box, for the blink signal
/// (`eyes_closed`). Like `face::FaceDetector`, this is informational only
/// in the scoring pipeline - see `models::FrameMetrics::eyes_closed`.
pub struct LandmarkDetector {
    #[cfg(feature = "face-detection")]
    session: ort::session::Session,
    #[cfg(feature = "face-detection")]
    reverse_index1: Vec<i64>,
    #[cfg(feature = "face-detection")]
    reverse_index2: Vec<i64>,
    #[cfg(feature = "face-detection")]
    max_len: usize,
}

impl LandmarkDetector {
    #[cfg(feature = "face-detection")]
    pub fn load() -> Result<Self, LandmarkError> {
        let bytes: &[u8] = include_bytes!("../assets/pipnet_landmarks_300w_68.onnx");
        let session = ort::session::Session::builder()
            .map_err(|e| LandmarkError::ModelLoad(e.to_string()))?
            .commit_from_memory(bytes)
            .map_err(|e| LandmarkError::ModelLoad(e.to_string()))?;
        let (reverse_index1, reverse_index2, max_len) =
            build_neighbor_indices(&meanface_points(), NUM_NB);
        Ok(Self {
            session,
            reverse_index1,
            reverse_index2,
            max_len,
        })
    }

    #[cfg(not(feature = "face-detection"))]
    pub fn load() -> Result<Self, LandmarkError> {
        Err(LandmarkError::Disabled)
    }

    #[cfg(feature = "face-detection")]
    pub fn get_landmarks(
        &mut self,
        image: &DynamicImage,
        bbox: &FaceBox,
    ) -> Result<Vec<(f32, f32)>, LandmarkError> {
        use ort::value::Tensor;

        let (x1, y1, crop_w, crop_h) =
            crop_bounds(image.width() as i64, image.height() as i64, bbox);
        if crop_w <= 0 || crop_h <= 0 {
            return Err(LandmarkError::Inference("degenerate face crop".into()));
        }
        let crop = image.crop_imm(x1 as u32, y1 as u32, crop_w as u32, crop_h as u32);
        let data = preprocess(&crop);
        let input = Tensor::from_array((vec![1i64, 3, INPUT_SIZE as i64, INPUT_SIZE as i64], data))
            .map_err(|e| LandmarkError::Inference(e.to_string()))?;

        let outputs = self
            .session
            .run(ort::inputs! { "input" => input })
            .map_err(|e| LandmarkError::Inference(e.to_string()))?;

        let (_, cls_map) = outputs
            .get("cls_map")
            .ok_or(LandmarkError::UnexpectedOutput)?
            .try_extract_tensor::<f32>()
            .map_err(|e| LandmarkError::Inference(e.to_string()))?;
        let (_, offset_x) = outputs
            .get("offset_x")
            .ok_or(LandmarkError::UnexpectedOutput)?
            .try_extract_tensor::<f32>()
            .map_err(|e| LandmarkError::Inference(e.to_string()))?;
        let (_, offset_y) = outputs
            .get("offset_y")
            .ok_or(LandmarkError::UnexpectedOutput)?
            .try_extract_tensor::<f32>()
            .map_err(|e| LandmarkError::Inference(e.to_string()))?;
        let (_, nb_x) = outputs
            .get("nb_x")
            .ok_or(LandmarkError::UnexpectedOutput)?
            .try_extract_tensor::<f32>()
            .map_err(|e| LandmarkError::Inference(e.to_string()))?;
        let (_, nb_y) = outputs
            .get("nb_y")
            .ok_or(LandmarkError::UnexpectedOutput)?
            .try_extract_tensor::<f32>()
            .map_err(|e| LandmarkError::Inference(e.to_string()))?;

        let feat_h = INPUT_SIZE / NET_STRIDE;
        let feat_w = INPUT_SIZE / NET_STRIDE;
        let normalized = decode(
            cls_map,
            offset_x,
            offset_y,
            nb_x,
            nb_y,
            &self.reverse_index1,
            &self.reverse_index2,
            self.max_len,
            NUM_LMS,
            NUM_NB,
            feat_h,
            feat_w,
            NET_STRIDE,
            INPUT_SIZE,
            INPUT_SIZE,
        );

        Ok(normalized
            .into_iter()
            .map(|(nx, ny)| {
                (
                    nx * crop_w as f32 + x1 as f32,
                    ny * crop_h as f32 + y1 as f32,
                )
            })
            .collect())
    }

    #[cfg(not(feature = "face-detection"))]
    pub fn get_landmarks(
        &mut self,
        _image: &DynamicImage,
        _bbox: &FaceBox,
    ) -> Result<Vec<(f32, f32)>, LandmarkError> {
        unreachable!("LandmarkDetector::load always fails without the face-detection feature")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meanface_has_68_points_in_unit_square() {
        let points = meanface_points();
        assert_eq!(points.len(), 68);
        for (x, y) in points {
            assert!((0.0..=1.0).contains(&x));
            assert!((0.0..=1.0).contains(&y));
        }
    }

    #[test]
    fn build_neighbor_indices_covers_every_landmark() {
        let points = meanface_points();
        let (idx1, idx2, max_len) = build_neighbor_indices(&points, NUM_NB);
        assert!(max_len > 0);
        assert_eq!(idx1.len(), points.len() * max_len);
        assert_eq!(idx2.len(), points.len() * max_len);
        assert!(idx1.iter().all(|&v| (0..points.len() as i64).contains(&v)));
        assert!(idx2.iter().all(|&v| (0..NUM_NB as i64).contains(&v)));
    }

    #[test]
    fn preprocess_produces_expected_shape_and_normalization() {
        use image::{DynamicImage, ImageBuffer, Rgb};
        // ImageNet mean pixel (roughly [124, 117, 104]) should normalize to ~0.
        let image =
            DynamicImage::ImageRgb8(ImageBuffer::from_pixel(200, 200, Rgb([124, 117, 104])));
        let data = preprocess(&image);
        assert_eq!(data.len(), 3 * INPUT_SIZE * INPUT_SIZE);
        assert!(data.iter().all(|v| v.abs() < 0.05));
    }

    #[test]
    fn crop_bounds_pads_asymmetrically_and_clamps_to_image() {
        // A 100x100 box comfortably inside a 1000x1000 image: no clamping,
        // so the padding arithmetic can be checked exactly.
        let bbox = FaceBox {
            x1: 100.0,
            y1: 100.0,
            x2: 199.0,
            y2: 199.0,
            confidence: 1.0,
        };
        let (x1, y1, w, h) = crop_bounds(1000, 1000, &bbox);
        // det_w = det_h = 100, pad = 10.
        assert_eq!(x1, 90); // 100 - 10
        assert_eq!(y1, 110); // 100 + 10
        assert_eq!(w, 120); // (199+10) - 90 + 1
        assert_eq!(h, 100); // (199+10) - 110 + 1
    }

    #[test]
    fn crop_bounds_clamps_to_image_edges() {
        let bbox = FaceBox {
            x1: 0.0,
            y1: 0.0,
            x2: 19.0,
            y2: 19.0,
            confidence: 1.0,
        };
        let (x1, y1, w, h) = crop_bounds(50, 50, &bbox);
        assert_eq!(x1, 0);
        assert_eq!(y1, 2); // 0 + int(20*0.1)=2
        assert!(w > 0 && h > 0);
    }

    fn synthetic_decode_inputs() -> (Vec<i64>, Vec<i64>, usize) {
        // 2 landmarks, 1 neighbor each, each pointing at the other -
        // small enough to hand-verify the reverse-gather math.
        let points = [(0.0, 0.0), (1.0, 0.0)];
        build_neighbor_indices(&points, 1)
    }

    #[test]
    fn decode_averages_own_prediction_with_neighbor_predictions() {
        let (reverse_index1, reverse_index2, max_len) = synthetic_decode_inputs();
        let num_lms = 2;
        let num_nb = 1;
        let feat_h = 2;
        let feat_w = 2;
        let net_stride = 1;
        let hw = feat_h * feat_w;

        // Landmark 0 peaks at flat index 0 (row 0, col 0), landmark 1 at
        // flat index 3 (row 1, col 1). Zero sub-pixel offsets everywhere
        // except landmark 0's own offset, to make the arithmetic obvious.
        let mut cls_map = vec![0f32; num_lms * hw];
        cls_map[0 * hw + 0] = 1.0;
        cls_map[1 * hw + 3] = 1.0;

        let mut offset_x = vec![0f32; num_lms * hw];
        let mut offset_y = vec![0f32; num_lms * hw];
        offset_x[0 * hw + 0] = 0.5;
        offset_y[0 * hw + 0] = 0.25;

        // nb_x/nb_y: landmark 0's single neighbor head predicts landmark
        // 1's location relative to landmark 0's own peak; landmark 1's
        // neighbor head predicts landmark 0's location relative to its peak.
        let mut nb_x = vec![0f32; num_lms * num_nb * hw];
        let mut nb_y = vec![0f32; num_lms * num_nb * hw];
        nb_x[(0 * num_nb + 0) * hw + 0] = 1.0; // from landmark 0's peak (col 0), predicts col 1
        nb_y[(0 * num_nb + 0) * hw + 0] = 0.0;
        nb_x[(1 * num_nb + 0) * hw + 3] = -1.0; // from landmark 1's peak (col 1), predicts col 0
        nb_y[(1 * num_nb + 0) * hw + 3] = 0.0;

        let result = decode(
            &cls_map,
            &offset_x,
            &offset_y,
            &nb_x,
            &nb_y,
            &reverse_index1,
            &reverse_index2,
            max_len,
            num_lms,
            num_nb,
            feat_h,
            feat_w,
            net_stride,
            feat_w * net_stride,
            feat_h * net_stride,
        );

        assert_eq!(result.len(), 2);
        // Landmark 0: own pred = ((0+0.5)/2, (0+0.25)/2) = (0.25, 0.125).
        // Landmark 1's neighbor head (pointing back at landmark 0) predicts
        // ((1-1)/2, (1+0)/2) = (0.0, 0.5). Average of the two: (0.125, 0.3125).
        assert!((result[0].0 - 0.125).abs() < 1e-5);
        assert!((result[0].1 - 0.3125).abs() < 1e-5);
        // Landmark 1: own pred = ((1+0)/2, (1+0)/2) = (0.5, 0.5). Landmark
        // 0's neighbor head predicts ((0+1)/2, (0+0)/2) = (0.5, 0.0).
        // Average: (0.5, 0.25).
        assert!((result[1].0 - 0.5).abs() < 1e-5);
        assert!((result[1].1 - 0.25).abs() < 1e-5);
    }

    #[test]
    fn eye_aspect_ratio_reports_wide_open_eye_as_high_ratio() {
        let mut points = [(0.0f32, 0.0f32); 68];
        // Wide-open eye: tall gap between lids relative to eye width.
        points[36] = (0.0, 0.5);
        points[37] = (0.3, 0.0);
        points[38] = (0.7, 0.0);
        points[39] = (1.0, 0.5);
        points[40] = (0.7, 1.0);
        points[41] = (0.3, 1.0);
        // Second eye identical for simplicity.
        for i in 0..6 {
            points[42 + i] = points[36 + i];
        }
        let (left, right) = eye_aspect_ratio(&points).unwrap();
        assert!(left > 0.8);
        assert!(right > 0.8);
        assert!(!eyes_closed(&points, DEFAULT_EAR_THRESHOLD));
    }

    #[test]
    fn eye_aspect_ratio_reports_closed_eye_as_low_ratio() {
        let mut points = [(0.0f32, 0.0f32); 68];
        // Closed eye: lids nearly touching (tiny vertical gap) relative to width.
        points[36] = (0.0, 0.5);
        points[37] = (0.3, 0.48);
        points[38] = (0.7, 0.48);
        points[39] = (1.0, 0.5);
        points[40] = (0.7, 0.52);
        points[41] = (0.3, 0.52);
        for i in 0..6 {
            points[42 + i] = points[36 + i];
        }
        let (left, right) = eye_aspect_ratio(&points).unwrap();
        assert!(left < DEFAULT_EAR_THRESHOLD);
        assert!(right < DEFAULT_EAR_THRESHOLD);
        assert!(eyes_closed(&points, DEFAULT_EAR_THRESHOLD));
    }

    #[test]
    fn eye_aspect_ratio_needs_68_points() {
        let points = vec![(0.0, 0.0); 10];
        assert!(eye_aspect_ratio(&points).is_none());
        assert!(!eyes_closed(&points, DEFAULT_EAR_THRESHOLD));
    }
}
