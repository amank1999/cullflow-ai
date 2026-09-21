use thiserror::Error;

/// Sample rate the bundled VAD model and `ffmpeg::extract_proxy_audio` both
/// use - the model also supports 8kHz, but standardizing on one rate avoids
/// needing to plumb a second code path through for no benefit here.
pub const SAMPLE_RATE: u32 = 16000;
const NUM_SAMPLES: usize = 512;
const CONTEXT_SIZE: usize = 64;
#[cfg_attr(not(feature = "audio-vad"), allow(dead_code))]
const STATE_LEN: usize = 2 * 128; // [2, batch=1, 128]
pub const DEFAULT_SPEECH_THRESHOLD: f32 = 0.5;
/// Samples at or above this absolute amplitude count as clipped/over-driven
/// ("uncalibrated") audio - 0.98 leaves a small margin below full-scale
/// (1.0) for lossy-codec rounding.
pub const DEFAULT_CLIPPING_THRESHOLD: f32 = 0.98;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("failed to load VAD model: {0}")]
    ModelLoad(String),
    #[error("VAD inference failed: {0}")]
    Inference(String),
    #[error("VAD model returned an unexpected output")]
    UnexpectedOutput,
    #[error("audio VAD was not compiled into this build (missing the audio-vad feature)")]
    Disabled,
}

/// Parses raw little-endian 32-bit float PCM (as produced by
/// `ffmpeg::extract_proxy_audio`) into samples. Pure and independent of
/// ffmpeg or the ONNX runtime; a trailing partial sample (fewer than 4
/// bytes) is silently dropped rather than treated as an error.
pub fn read_pcm_f32le(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Peak absolute sample amplitude - 1.0 is full-scale. Pure DSP metric,
/// independent of any model.
pub fn peak_amplitude(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()))
}

/// Fraction of samples at or above `threshold` absolute amplitude - a proxy
/// for clipping/over-driven ("uncalibrated") audio, per the blueprint's
/// audio gate. Pure DSP metric, independent of any model.
pub fn clipping_ratio(samples: &[f32], threshold: f32) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let clipped = samples.iter().filter(|&&s| s.abs() >= threshold).count();
    clipped as f64 / samples.len() as f64
}

/// Concatenates the trailing `CONTEXT_SIZE` samples carried over from the
/// previous chunk (all zero for the first chunk) with the current chunk -
/// Silero VAD's own windowing scheme, which gives each conv layer a little
/// lookback across chunk boundaries. Pure translation of the reference
/// `OnnxWrapper.__call__` (see THIRD_PARTY_LICENSES.md) - independent of the
/// ONNX runtime, so it's testable without a model.
#[cfg_attr(not(feature = "audio-vad"), allow(dead_code))]
fn windowed_input(context: &[f32; CONTEXT_SIZE], chunk: &[f32; NUM_SAMPLES]) -> Vec<f32> {
    let mut input = Vec::with_capacity(CONTEXT_SIZE + NUM_SAMPLES);
    input.extend_from_slice(context);
    input.extend_from_slice(chunk);
    input
}

/// Splits raw samples into fixed `NUM_SAMPLES`-length (32ms at 16kHz)
/// chunks, zero-padding the final chunk if needed. Pure and independent of
/// the ONNX runtime.
#[cfg_attr(not(feature = "audio-vad"), allow(dead_code))]
fn chunk_samples(samples: &[f32]) -> Vec<[f32; NUM_SAMPLES]> {
    if samples.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::with_capacity(samples.len().div_ceil(NUM_SAMPLES));
    let mut i = 0;
    while i < samples.len() {
        let mut chunk = [0f32; NUM_SAMPLES];
        let end = (i + NUM_SAMPLES).min(samples.len());
        chunk[..end - i].copy_from_slice(&samples[i..end]);
        chunks.push(chunk);
        i += NUM_SAMPLES;
    }
    chunks
}

/// Wraps the bundled Silero VAD model (MIT licensed, see
/// THIRD_PARTY_LICENSES.md) to measure how much of a clip's audio track has
/// detected speech, for the blueprint's "dead mic take" signal
/// (`AnalyzedClip::speech_ratio`). Like face/blink detection, this is
/// informational only - see `models::AnalyzedClip::speech_ratio`.
pub struct SileroVad {
    #[cfg(feature = "audio-vad")]
    session: ort::session::Session,
    #[cfg(feature = "audio-vad")]
    state: [f32; STATE_LEN],
    #[cfg(feature = "audio-vad")]
    context: [f32; CONTEXT_SIZE],
}

impl SileroVad {
    #[cfg(feature = "audio-vad")]
    pub fn load() -> Result<Self, AudioError> {
        let bytes: &[u8] = include_bytes!("../assets/silero_vad.onnx");
        let session = ort::session::Session::builder()
            .map_err(|e| AudioError::ModelLoad(e.to_string()))?
            .commit_from_memory(bytes)
            .map_err(|e| AudioError::ModelLoad(e.to_string()))?;
        Ok(Self {
            session,
            state: [0.0; STATE_LEN],
            context: [0.0; CONTEXT_SIZE],
        })
    }

    #[cfg(not(feature = "audio-vad"))]
    pub fn load() -> Result<Self, AudioError> {
        Err(AudioError::Disabled)
    }

    #[cfg(feature = "audio-vad")]
    fn reset(&mut self) {
        self.state = [0.0; STATE_LEN];
        self.context = [0.0; CONTEXT_SIZE];
    }

    #[cfg(feature = "audio-vad")]
    fn process_chunk(&mut self, chunk: &[f32; NUM_SAMPLES]) -> Result<f32, AudioError> {
        use ort::value::Tensor;

        let input_data = windowed_input(&self.context, chunk);
        let input_len = input_data.len() as i64;
        let input = Tensor::from_array((vec![1i64, input_len], input_data))
            .map_err(|e| AudioError::Inference(e.to_string()))?;
        let state_tensor = Tensor::from_array((vec![2i64, 1, 128], self.state.to_vec()))
            .map_err(|e| AudioError::Inference(e.to_string()))?;
        let sr_tensor = Tensor::from_array(((), vec![SAMPLE_RATE as i64]))
            .map_err(|e| AudioError::Inference(e.to_string()))?;

        let outputs = self
            .session
            .run(ort::inputs! { "input" => input, "state" => state_tensor, "sr" => sr_tensor })
            .map_err(|e| AudioError::Inference(e.to_string()))?;

        let (_, prob) = outputs
            .get("output")
            .ok_or(AudioError::UnexpectedOutput)?
            .try_extract_tensor::<f32>()
            .map_err(|e| AudioError::Inference(e.to_string()))?;
        let (_, new_state) = outputs
            .get("stateN")
            .ok_or(AudioError::UnexpectedOutput)?
            .try_extract_tensor::<f32>()
            .map_err(|e| AudioError::Inference(e.to_string()))?;

        if new_state.len() != STATE_LEN || prob.is_empty() {
            return Err(AudioError::UnexpectedOutput);
        }
        self.state.copy_from_slice(new_state);
        self.context
            .copy_from_slice(&chunk[NUM_SAMPLES - CONTEXT_SIZE..]);

        Ok(prob[0])
    }

    /// Runs VAD over an entire clip's mono 16kHz samples and returns the
    /// fraction of ~32ms windows classified as speech (probability >=
    /// `threshold`) - the "how much of this clip has a live mic" signal
    /// behind `AnalyzedClip::speech_ratio`. Resets internal state first,
    /// since the model's recurrent state must not leak between unrelated
    /// clips.
    #[cfg(feature = "audio-vad")]
    pub fn speech_ratio(&mut self, samples: &[f32], threshold: f32) -> Result<f64, AudioError> {
        self.reset();
        let chunks = chunk_samples(samples);
        if chunks.is_empty() {
            return Ok(0.0);
        }
        let mut speech_count = 0usize;
        for chunk in &chunks {
            if self.process_chunk(chunk)? >= threshold {
                speech_count += 1;
            }
        }
        Ok(speech_count as f64 / chunks.len() as f64)
    }

    #[cfg(not(feature = "audio-vad"))]
    pub fn speech_ratio(&mut self, _samples: &[f32], _threshold: f32) -> Result<f64, AudioError> {
        unreachable!("SileroVad::load always fails without the audio-vad feature")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_pcm_f32le_parses_little_endian_floats() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&(-0.5f32).to_le_bytes());
        bytes.push(0xAA); // trailing partial sample, should be dropped
        let samples = read_pcm_f32le(&bytes);
        assert_eq!(samples, vec![1.0, -0.5]);
    }

    #[test]
    fn peak_amplitude_finds_largest_absolute_value() {
        assert_eq!(peak_amplitude(&[0.1, -0.9, 0.3]), 0.9);
        assert_eq!(peak_amplitude(&[]), 0.0);
    }

    #[test]
    fn clipping_ratio_counts_samples_past_threshold() {
        let samples = [0.1, 0.99, -1.0, 0.2, 1.0];
        let ratio = clipping_ratio(&samples, DEFAULT_CLIPPING_THRESHOLD);
        assert!((ratio - 0.6).abs() < 1e-9); // 3 of 5 samples >= 0.98
    }

    #[test]
    fn clipping_ratio_of_empty_audio_is_zero() {
        assert_eq!(clipping_ratio(&[], DEFAULT_CLIPPING_THRESHOLD), 0.0);
    }

    #[test]
    fn windowed_input_prepends_context_before_chunk() {
        let context = [1.0f32; CONTEXT_SIZE];
        let mut chunk = [0.0f32; NUM_SAMPLES];
        chunk[0] = 9.0;
        let windowed = windowed_input(&context, &chunk);
        assert_eq!(windowed.len(), CONTEXT_SIZE + NUM_SAMPLES);
        assert!(windowed[..CONTEXT_SIZE].iter().all(|&v| v == 1.0));
        assert_eq!(windowed[CONTEXT_SIZE], 9.0);
    }

    #[test]
    fn chunk_samples_pads_the_final_short_chunk_with_zeros() {
        let samples = vec![1.0f32; NUM_SAMPLES + 10];
        let chunks = chunk_samples(&samples);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].iter().all(|&v| v == 1.0));
        assert!(chunks[1][..10].iter().all(|&v| v == 1.0));
        assert!(chunks[1][10..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn chunk_samples_of_empty_audio_is_empty() {
        assert!(chunk_samples(&[]).is_empty());
    }
}
