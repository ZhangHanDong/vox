use makepad_widgets::*;
use makepad_widgets::makepad_draw::audio::{AudioDeviceId, AudioDevicesEvent};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Target sample rate for ASR (ominix-api expects 16kHz)
const TARGET_SAMPLE_RATE: f64 = 16_000.0;

/// Audio capture state shared between audio callback thread and main thread.
pub struct AudioCapture {
    /// Current RMS level, encoded as f32 bits in u64.
    /// Audio thread writes, UI thread reads.
    pub rms: Arc<AtomicU64>,

    /// Accumulated PCM samples at device sample rate.
    /// Audio thread writes (try_lock), main thread reads on stop.
    pcm_buffer: Arc<Mutex<Vec<f32>>>,

    /// Device sample rate captured from audio callback.
    device_sample_rate: Arc<AtomicU64>,

    /// Whether capture is active.
    active: Arc<AtomicBool>,

    /// Whether audio callback has been installed.
    callback_installed: bool,

    /// Default input device ID, obtained from AudioDevicesEvent.
    default_input: Option<AudioDeviceId>,
}

impl Default for AudioCapture {
    fn default() -> Self {
        Self {
            rms: Arc::new(AtomicU64::new(0)),
            pcm_buffer: Arc::new(Mutex::new(Vec::new())),
            device_sample_rate: Arc::new(AtomicU64::new(
                (44100.0f64).to_bits(),
            )),
            active: Arc::new(AtomicBool::new(false)),
            callback_installed: false,
            default_input: None,
        }
    }
}

impl AudioCapture {
    /// Install the audio input callback. Only call once.
    pub fn ensure_callback(&mut self, cx: &mut Cx) {
        if self.callback_installed {
            return;
        }
        self.callback_installed = true;

        let rms = self.rms.clone();
        let pcm = self.pcm_buffer.clone();
        let active = self.active.clone();
        let device_sr = self.device_sample_rate.clone();

        cx.audio_input(0, move |info, buffer| {
            // Store device sample rate
            device_sr.store(info.sample_rate.to_bits(), Ordering::Relaxed);

            if !active.load(Ordering::Relaxed) {
                return;
            }

            // Compute RMS from first channel
            let frame_count = buffer.frame_count();
            if frame_count == 0 {
                return;
            }

            let channel = buffer.channel(0);
            let mut sum = 0.0f32;
            for &s in channel {
                sum += s * s;
            }
            let rms_val = (sum / frame_count as f32).sqrt();
            rms.store((rms_val).to_bits() as u64, Ordering::Relaxed);

            // Accumulate PCM (non-blocking)
            if let Ok(mut buf) = pcm.try_lock() {
                buf.extend_from_slice(channel);
            }
            // If lock fails, we drop this chunk — acceptable for audio
        });
    }

    /// Handle audio device enumeration — save default input device.
    pub fn handle_audio_devices(&mut self, devices: &AudioDevicesEvent) {
        self.default_input = devices.default_input().into_iter().next();
    }

    /// Start recording.
    pub fn start(&mut self, cx: &mut Cx) {
        self.ensure_callback(cx);

        // Clear buffer
        if let Ok(mut buf) = self.pcm_buffer.lock() {
            buf.clear();
        }
        self.rms.store(0, Ordering::Relaxed);
        self.active.store(true, Ordering::Relaxed);

        // Activate default audio input device
        if let Some(device_id) = self.default_input {
            cx.use_audio_inputs(&[device_id]);
        }
    }

    /// Stop recording and return PCM samples resampled to 16kHz mono.
    pub fn stop(&mut self, cx: &mut Cx) -> Vec<f32> {
        self.active.store(false, Ordering::Relaxed);
        cx.use_audio_inputs(&[]); // deactivate

        let device_sr =
            f64::from_bits(self.device_sample_rate.load(Ordering::Relaxed));
        let samples = if let Ok(mut buf) = self.pcm_buffer.lock() {
            std::mem::take(&mut *buf)
        } else {
            Vec::new()
        };

        if samples.is_empty() {
            return Vec::new();
        }

        // Resample to TARGET_SAMPLE_RATE if needed
        if (device_sr - TARGET_SAMPLE_RATE).abs() < 1.0 {
            return samples;
        }

        resample(&samples, device_sr, TARGET_SAMPLE_RATE)
    }

    /// Read current smoothed RMS (call from UI thread).
    pub fn read_rms(&self) -> f32 {
        f32::from_bits(self.rms.load(Ordering::Relaxed) as u32)
    }
}

/// Simple linear interpolation resampler.
fn resample(input: &[f32], from_rate: f64, to_rate: f64) -> Vec<f32> {
    let ratio = to_rate / from_rate;
    let new_len = ((input.len() as f64) * ratio).round() as usize;
    if new_len == 0 {
        return Vec::new();
    }

    let mut output = Vec::with_capacity(new_len);
    for i in 0..new_len {
        let src_pos = i as f64 / ratio;
        let src_idx = src_pos as usize;
        let frac = (src_pos - src_idx as f64) as f32;
        let s0 = input.get(src_idx).copied().unwrap_or(0.0);
        let s1 = input.get(src_idx + 1).copied().unwrap_or(s0);
        output.push(s0 + (s1 - s0) * frac);
    }
    output
}

/// Encode PCM samples as 16-bit WAV (mono, 16kHz).
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let num_channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * u32::from(num_channels) * u32::from(bits_per_sample) / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let data_size = samples.len() as u32 * u32::from(block_align);
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(44 + data_size as usize);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_val = (clamped * 32767.0) as i16;
        buf.extend_from_slice(&i16_val.to_le_bytes());
    }

    buf
}

/// Raw mono float32 little-endian PCM at 16 kHz (no header).
/// Qwen3-ASR / vLLM streaming servers decode `application/octet-stream` chunks as `float32`
/// (see multimodal pipeline); **s16le is misinterpreted** and often becomes all-zero `float32`.
pub fn pcm_f32_le_from_f32(samples: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(samples.len() * 4);
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        buf.extend_from_slice(&clamped.to_le_bytes());
    }
    buf
}
