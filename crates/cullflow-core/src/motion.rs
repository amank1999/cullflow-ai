use image::GrayImage;

const BLOCK_SIZE: u32 = 8;
const SEARCH_RADIUS: i32 = 4;

#[derive(Debug, Clone, Copy)]
struct MotionVector {
    dx: f64,
    dy: f64,
}

/// How incoherently a frame's blocks moved relative to each other between
/// `prev` and `cur` (both expected to be the same small downsampled
/// thumbnail size - see `scoring::thumbnail`). This is the key number: it's
/// near zero for both a static frame *and* a deliberate, uniform camera pan
/// (every block moves the same way, so there's nothing to disagree about),
/// but rises sharply for handheld shake, whip-pans, or a dropped camera,
/// where blocks disagree with each other about which way the frame moved.
/// That single property is what lets one metric stand in for the
/// blueprint's "differentiate intentional pans from errant handheld
/// movement" goal without needing a full optical-flow implementation.
///
/// This is block-matching motion estimation (search each block's best match
/// in a small neighborhood, like classic video-codec motion estimation) -
/// coarser than dense per-pixel optical flow (e.g. Farneback), but far
/// cheaper and dependency-free (no OpenCV), which matters for an app whose
/// whole pitch is running fast on the editor's own laptop.
pub fn motion_incoherence(prev: &GrayImage, cur: &GrayImage) -> f64 {
    let (width, height) = prev.dimensions();
    if cur.dimensions() != (width, height) {
        return 0.0;
    }

    let vectors = block_vectors(prev, cur, width, height);
    if vectors.is_empty() {
        return 0.0;
    }

    let n = vectors.len() as f64;
    let mean_dx = vectors.iter().map(|v| v.dx).sum::<f64>() / n;
    let mean_dy = vectors.iter().map(|v| v.dy).sum::<f64>() / n;

    let variance = vectors
        .iter()
        .map(|v| (v.dx - mean_dx).powi(2) + (v.dy - mean_dy).powi(2))
        .sum::<f64>()
        / n;

    variance.sqrt()
}

fn block_vectors(prev: &GrayImage, cur: &GrayImage, width: u32, height: u32) -> Vec<MotionVector> {
    let mut vectors = Vec::new();
    let mut by = 0;
    while by + BLOCK_SIZE <= height {
        let mut bx = 0;
        while bx + BLOCK_SIZE <= width {
            vectors.push(best_match(prev, cur, bx, by, width, height));
            bx += BLOCK_SIZE;
        }
        by += BLOCK_SIZE;
    }
    vectors
}

/// Finds the (dx, dy) offset within `SEARCH_RADIUS` pixels that best matches
/// the block at (bx, by) in `prev` against `cur`, by minimizing sum of
/// absolute differences (SAD) - the same core operation MPEG-style video
/// encoders use for motion estimation.
fn best_match(
    prev: &GrayImage,
    cur: &GrayImage,
    bx: u32,
    by: u32,
    width: u32,
    height: u32,
) -> MotionVector {
    let mut best_sad = u64::MAX;
    let mut best = (0i32, 0i32);

    for dy in -SEARCH_RADIUS..=SEARCH_RADIUS {
        for dx in -SEARCH_RADIUS..=SEARCH_RADIUS {
            let nx = bx as i32 + dx;
            let ny = by as i32 + dy;
            if nx < 0 || ny < 0 || nx as u32 + BLOCK_SIZE > width || ny as u32 + BLOCK_SIZE > height
            {
                continue;
            }

            let sad = block_sad(prev, cur, bx, by, nx as u32, ny as u32);
            if sad < best_sad {
                best_sad = sad;
                best = (dx, dy);
            }
        }
    }

    MotionVector {
        dx: best.0 as f64,
        dy: best.1 as f64,
    }
}

fn block_sad(prev: &GrayImage, cur: &GrayImage, px: u32, py: u32, cx: u32, cy: u32) -> u64 {
    let mut sad = 0u64;
    for y in 0..BLOCK_SIZE {
        for x in 0..BLOCK_SIZE {
            let p = prev.get_pixel(px + x, py + y).0[0] as i32;
            let c = cur.get_pixel(cx + x, cy + y).0[0] as i32;
            sad += (p - c).unsigned_abs() as u64;
        }
    }
    sad
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Luma};

    fn checkerboard(offset_x: u32) -> GrayImage {
        ImageBuffer::from_fn(64, 32, |x, y| {
            let x = (x + offset_x) % 64;
            if (x / 8 + y / 8) % 2 == 0 {
                Luma([240])
            } else {
                Luma([15])
            }
        })
    }

    fn random_frame(seed: u64) -> GrayImage {
        // True per-pixel randomness (not a smooth hash pattern, which can
        // accidentally have locally-matchable structure): each block ends up
        // with no coherent best-match direction relative to another
        // independently-random frame, which is what simulates shake/a
        // dropped camera rather than a deliberate pan.
        use rand::{Rng, SeedableRng};
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        ImageBuffer::from_fn(64, 32, |_, _| Luma([rng.gen::<u8>()]))
    }

    #[test]
    fn identical_frames_have_zero_incoherence() {
        let frame = checkerboard(0);
        assert_eq!(motion_incoherence(&frame, &frame), 0.0);
    }

    #[test]
    fn uniform_pan_has_low_incoherence() {
        // Every block shifts by the same amount - a deliberate pan, not shake.
        let a = checkerboard(0);
        let b = checkerboard(2);
        assert!(motion_incoherence(&a, &b) < 1.0);
    }

    #[test]
    fn erratic_motion_has_higher_incoherence_than_a_uniform_pan() {
        let pan_a = checkerboard(0);
        let pan_b = checkerboard(2);
        let pan_incoherence = motion_incoherence(&pan_a, &pan_b);

        let shake_a = random_frame(1);
        let shake_b = random_frame(2);
        let shake_incoherence = motion_incoherence(&shake_a, &shake_b);

        assert!(shake_incoherence > pan_incoherence);
    }
}
