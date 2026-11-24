//! Bounded mono PCM capture, speech endpointing and RIFF/WAVE interchange.
use anyhow::{Context, Result, ensure};
use orbit_protocol::VoiceConfig;
use std::{collections::VecDeque, io::Read, path::Path};

pub const SAMPLE_RATE: u32 = 16_000;
pub const MAX_AUDIO_BYTES: usize = 10 * 1024 * 1024;
const MAX_SECONDS: usize = 120;
const FRAME_SAMPLES: usize = 320;

fn check_length(frames: usize, rate: u32) -> Result<()> {
    ensure!((8_000..=96_000).contains(&rate), "unsupported sample rate");
    ensure!(
        frames > 0 && frames <= rate as usize * MAX_SECONDS,
        "audio must be between one sample and 120 seconds"
    );
    Ok(())
}

pub fn read_audio_file(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    ensure!(file.metadata()?.is_file(), "expected a regular audio file");
    let mut bytes = Vec::new();
    file.take(MAX_AUDIO_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_AUDIO_BYTES, "audio exceeds 10 MiB");
    Ok(bytes)
}

pub fn decode_pcm(bytes: &[u8], rate: u32) -> Result<Vec<i16>> {
    ensure!(
        bytes.len() <= MAX_AUDIO_BYTES && bytes.len().is_multiple_of(2),
        "invalid PCM16 length"
    );
    check_length(bytes.len() / 2, rate)?;
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect())
}

/// Area-average source intervals when decimating. Interpolate adjacent samples when upsampling.
pub fn resample_to_16k(samples: &[i16], rate: u32) -> Result<Vec<i16>> {
    check_length(samples.len(), rate)?;
    ensure!(samples.len() <= MAX_AUDIO_BYTES / 2, "audio exceeds 10 MiB");
    if rate == SAMPLE_RATE {
        return Ok(samples.to_vec());
    }
    let target = SAMPLE_RATE as u64;
    let step = u64::from(rate);
    let end = samples.len() as u64 * target;
    let mut out = Vec::with_capacity(end.div_ceil(step) as usize);
    for start in (0..end).step_by(rate as usize) {
        let index = (start / target) as usize;
        let value = if rate < SAMPLE_RATE {
            let fraction = (start % target) as i64;
            let next = samples.get(index + 1).copied().unwrap_or(samples[index]);
            (i64::from(samples[index]) * (target as i64 - fraction) + i64::from(next) * fraction)
                / target as i64
        } else {
            let stop = (start + step).min(end);
            let mut pos = start;
            let mut sum = 0i64;
            while pos < stop {
                let boundary = ((pos / target + 1) * target).min(stop);
                sum += i64::from(samples[(pos / target) as usize]) * (boundary - pos) as i64;
                pos = boundary;
            }
            sum / (stop - start) as i64
        };
        out.push(value as i16);
    }
    Ok(out)
}

pub fn encode_wav(samples: &[i16], rate: u32) -> Result<Vec<u8>> {
    check_length(samples.len(), rate)?;
    ensure!(
        samples.len() <= (MAX_AUDIO_BYTES - 44) / 2,
        "WAV exceeds 10 MiB"
    );
    let len = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + len as usize);
    out.extend(b"RIFF");
    out.extend((36 + len).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(len.to_le_bytes());
    out.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
    Ok(out)
}

/// Read PCM16 WAV, including padded unknown chunks. Stereo capture is mixed to mono.
pub fn decode_wav(bytes: &[u8]) -> Result<(u32, Vec<i16>)> {
    decode_wav_output(bytes, false)
}

/// Streaming WAV writers can use u32::MAX for unknown RIFF and final data sizes.
/// The HTTP caller bounds the full body before decoding and re-encodes a seekable WAV.
pub(super) fn decode_wav_output(bytes: &[u8], streaming: bool) -> Result<(u32, Vec<i16>)> {
    ensure!(
        bytes.len() <= MAX_AUDIO_BYTES && bytes.len() >= 12,
        "invalid WAV length"
    );
    let word = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let short = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    ensure!(
        &bytes[..4] == b"RIFF"
            && &bytes[8..12] == b"WAVE"
            && (word(&bytes[4..8]) as usize == bytes.len() - 8
                || streaming && word(&bytes[4..8]) == u32::MAX),
        "invalid RIFF/WAVE header"
    );
    let mut format = None;
    let mut data = None;
    let mut chunks = &bytes[12..];
    while !chunks.is_empty() {
        ensure!(chunks.len() >= 8, "truncated WAV chunk");
        let declared = word(&chunks[4..8]);
        let size = if streaming && &chunks[..4] == b"data" && declared == u32::MAX {
            chunks.len() - 8
        } else {
            declared as usize
        };
        let body = chunks[8..].get(..size).context("truncated WAV data")?;
        match &chunks[..4] {
            b"fmt " => {
                ensure!(
                    format.is_none()
                        && body.len() >= 16
                        && short(body) == 1
                        && short(&body[14..]) == 16,
                    "expected PCM16 WAV format"
                );
                let channels = short(&body[2..]);
                let rate = word(&body[4..]);
                ensure!(
                    (1..=2).contains(&channels) && short(&body[12..]) == channels * 2,
                    "invalid WAV channel layout"
                );
                check_length(1, rate)?;
                ensure!(
                    word(&body[8..]) == rate * u32::from(channels) * 2,
                    "invalid WAV byte rate"
                );
                format = Some((rate, usize::from(channels)));
            }
            b"data" => ensure!(data.replace(body).is_none(), "duplicate WAV data"),
            _ => {}
        }
        chunks = chunks
            .get(8 + size + (size % 2)..)
            .context("truncated WAV padding")?;
    }
    let (rate, channels) = format.context("missing WAV format")?;
    let data = data.context("missing WAV data")?;
    ensure!(data.len().is_multiple_of(2 * channels), "partial PCM frame");
    check_length(data.len() / (2 * channels), rate)?;
    let mono = data
        .chunks_exact(channels * 2)
        .map(|frame| {
            let sum: i32 = frame
                .as_chunks::<2>()
                .0
                .iter()
                .map(|s| i32::from(i16::from_le_bytes(*s)))
                .sum();
            (sum / channels as i32) as i16
        })
        .collect();
    Ok((rate, mono))
}

#[derive(Debug)]
pub struct PreRollAudioBuffer {
    capacity: usize,
    samples: VecDeque<i16>,
}

impl Default for PreRollAudioBuffer {
    fn default() -> Self {
        Self::new(300)
    }
}

impl PreRollAudioBuffer {
    pub fn new(ms: u32) -> Self {
        let capacity = ms.min(2000) as usize * 16;
        Self {
            capacity,
            samples: VecDeque::with_capacity(capacity),
        }
    }
    pub fn push(&mut self, frame: &[i16]) {
        let tail = &frame[frame.len().saturating_sub(self.capacity)..];
        self.samples
            .drain(..(self.samples.len() + tail.len()).saturating_sub(self.capacity));
        self.samples.extend(tail);
    }
    pub fn drain(&mut self) -> Vec<i16> {
        self.samples.drain(..).collect()
    }
}

/// Energy gating followed by zero-crossing and low/high-band energy screening.
pub struct VoiceActivityDetector {
    threshold: f32,
    noise_floor: f32,
    hangover: usize,
    silent_samples: usize,
    active: bool,
}

impl VoiceActivityDetector {
    pub fn new(config: &VoiceConfig) -> Self {
        let threshold = if config.vad_threshold.is_finite() {
            config.vad_threshold.clamp(0.0, 1.0)
        } else {
            VoiceConfig::default().vad_threshold
        };
        Self {
            threshold,
            noise_floor: 0.001,
            hangover: config.silence_hangover_ms.min(5000) as usize * 16,
            silent_samples: 0,
            active: false,
        }
    }

    /// Process a 20 ms frame (or the final partial frame).
    pub fn observe(&mut self, frame: &[i16]) -> bool {
        if frame.is_empty() {
            return self.active;
        }
        let n = frame.len() as f32;
        let mean = frame.iter().map(|s| f32::from(*s)).sum::<f32>() / n;
        let mut power = 0.;
        let mut high_power = 0.;
        let mut crossings = 0;
        let mut prev = f32::from(frame[0]) - mean;
        for &s in frame {
            let x = f32::from(s) - mean;
            power += x * x;
            high_power += (x - prev) * (x - prev) * 0.25;
            crossings += usize::from(x.is_sign_negative() != prev.is_sign_negative());
            prev = x;
        }
        let rms = (power / n).sqrt() / 32768.;
        let zcr = crossings as f32 / n;
        let speech = rms >= self.threshold.max(self.noise_floor * 2.5)
            && (0.004..0.48).contains(&zcr)
            && high_power < 0.58 * power;
        if speech {
            self.active = true;
            self.silent_samples = 0;
        } else {
            if !self.active {
                self.noise_floor += 0.04 * (rms.min(self.threshold * 2.) - self.noise_floor);
            }
            self.silent_samples = self.silent_samples.saturating_add(frame.len());
            if self.silent_samples >= self.hangover {
                self.active = false;
            }
        }
        self.active
    }
}

/// Keep speech bursts and their configured pre-roll/hangover in capture order.
pub(super) fn speech_frames(samples: &[i16], config: &VoiceConfig) -> Vec<i16> {
    let mut pending = PreRollAudioBuffer::new(config.preroll_ms);
    let mut detector = VoiceActivityDetector::new(config);
    let mut speech = Vec::new();
    for frame in samples.chunks(FRAME_SAMPLES) {
        let remaining = if detector.active {
            detector.hangover.saturating_sub(detector.silent_samples)
        } else {
            0
        };
        if detector.observe(frame) {
            speech.extend(pending.samples.drain(..));
            speech.extend_from_slice(frame);
        } else {
            // The release boundary can land inside the frame (e.g. 750 ms / 20 ms).
            let (tail, idle) = frame.split_at(remaining.min(frame.len()));
            speech.extend_from_slice(tail);
            pending.push(idle);
        }
    }
    speech
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Vec<i16> {
        (0..FRAME_SAMPLES)
            .map(|n| ((n as f32 * std::f32::consts::TAU / 64.).sin() * 7000.) as i16)
            .collect()
    }

    #[test]
    fn preserves_exact_leading_300ms_and_onset() {
        let before: Vec<i16> = (0..6400).map(|n| (n % 27) as i16).collect();
        let mut ring = PreRollAudioBuffer::default();
        ring.push(&before);
        assert_eq!(ring.drain(), before[1600..]);
        let mut capture = before.clone();
        capture.extend(frame());
        assert_eq!(
            speech_frames(&capture, &VoiceConfig::default()),
            capture[1600..]
        );
    }

    #[test]
    fn bursts_release_after_configured_silence_and_reject_high_frequency_noise() {
        let mut detector = VoiceActivityDetector::new(&VoiceConfig {
            silence_hangover_ms: 60,
            ..Default::default()
        });
        let silence = [0; FRAME_SAMPLES];
        let high: Vec<_> = (0..FRAME_SAMPLES)
            .map(|i| if i % 2 == 0 { 10000 } else { -10000 })
            .collect();
        assert!(!detector.observe(&silence));
        assert!(!detector.observe(&high));
        assert!(detector.observe(&frame()));
        assert!(detector.observe(&silence));
        assert!(detector.observe(&silence));
        assert!(!detector.observe(&silence));
        assert!(detector.observe(&frame()));

        let mut capture = frame();
        capture.extend(vec![0; 16000]);
        assert_eq!(
            speech_frames(&capture, &VoiceConfig::default()).len(),
            FRAME_SAMPLES + 12000
        );
    }

    #[test]
    fn pcm_resampling_wav_roundtrip_and_input_bounds() -> Result<()> {
        let samples = [-32768, 0, 32767, 15, -22];
        let wav = encode_wav(&samples, SAMPLE_RATE)?;
        assert_eq!(decode_wav(&wav)?, (SAMPLE_RATE, samples.to_vec()));
        for rate in [8000, 44100, 48000] {
            let resampled = resample_to_16k(&vec![154; rate as usize], rate)?;
            assert_eq!(resampled, vec![154; 16000]);
        }
        assert_eq!(resample_to_16k(&[3, 6, 9, 12, 15, 18], 48000)?, [6, 15]);
        assert!(decode_wav(&wav[..wav.len() - 1]).is_err());
        let mut truncated_chunk = wav.clone();
        truncated_chunk[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_wav(&truncated_chunk).is_err());
        truncated_chunk[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode_wav_output(&truncated_chunk, true)?,
            (SAMPLE_RATE, samples.to_vec())
        );
        assert!(decode_wav(&truncated_chunk).is_err());
        let oversized = vec![0; MAX_AUDIO_BYTES + 1];
        assert!(decode_wav(&oversized).is_err());
        assert!(decode_pcm(&oversized, 48000).is_err());
        assert!(decode_pcm(&[0, 0, 2], 16000).is_err());
        assert!(decode_pcm(&[0, 0], 0).is_err());
        assert!(decode_pcm(&vec![0; 8000 * 121 * 2], 8000).is_err());
        Ok(())
    }
}
