//! Audio in the listener's one format: 16 kHz mono samples in -1.0..1.0.
//!
//! Everything that reaches the speech gate and the recognizer is converted here first: WAV files
//! (tests and the simulated voice), raw 16-bit PCM (standard input, the daemon's feed) and the
//! microphone. Nothing here writes audio anywhere; `write_wav` exists for tests only.

use anyhow::{bail, Context, Result};

/// The listener's sample rate.
pub const RATE: u32 = 16_000;
/// One frame of the speech gate: 20 ms.
pub const FRAME: usize = (RATE / 50) as usize;

/// Little-endian signed 16-bit samples to floats.
pub fn from_s16le(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
}

/// Floats to little-endian signed 16-bit samples, clipped.
pub fn to_s16le(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Reads a WAV file's samples as 16 kHz mono: 8, 16, 24 or 32-bit integer PCM or 32-bit float,
/// any number of channels (mixed down) and any sample rate (resampled).
pub fn read_wav(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("not a WAV file");
    }
    let mut pos = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None; // (format, channels, rate, bits)
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = bytes.get(pos + 8..(pos + 8 + len).min(bytes.len())).context("truncated WAV chunk")?;
        match id {
            b"fmt " => {
                if body.len() < 16 {
                    bail!("short fmt chunk");
                }
                let mut tag = u16::from_le_bytes([body[0], body[1]]);
                let channels = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                let bits = u16::from_le_bytes([body[14], body[15]]);
                if tag == 0xFFFE && body.len() >= 26 {
                    tag = u16::from_le_bytes([body[24], body[25]]); // WAVE_FORMAT_EXTENSIBLE: the sub-format's first two bytes
                }
                format = Some((tag, channels, rate, bits));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    let (tag, channels, rate, bits) = format.context("WAV file has no fmt chunk")?;
    let data = data.context("WAV file has no data chunk")?;
    if channels == 0 {
        bail!("WAV file has no channels");
    }
    let width = (bits as usize).div_ceil(8);
    let frame = width * channels as usize;
    let sample = |b: &[u8]| -> Result<f32> {
        Ok(match (tag, bits) {
            (1, 8) => (b[0] as f32 - 128.0) / 128.0,
            (1, 16) => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
            (1, 24) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0,
            (1, 32) => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
            (3, 32) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            _ => bail!("unsupported WAV sample format {tag}/{bits} bits"),
        })
    };
    let mut mono = Vec::with_capacity(data.len() / frame.max(1));
    for f in data.chunks_exact(frame) {
        let mut sum = 0.0;
        for c in 0..channels as usize {
            sum += sample(&f[c * width..(c + 1) * width])?;
        }
        mono.push(sum / channels as f32);
    }
    Ok(resample(&mono, rate, RATE))
}

/// Resamples by linear interpolation, after a moving-average low-pass when the rate falls, which
/// is enough for speech on its way to a 16 kHz recognizer.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let filtered: Vec<f32> = if from > to {
        let width = (from as f32 / to as f32).round().max(1.0) as usize;
        let mut acc = 0.0;
        let mut out = Vec::with_capacity(input.len());
        for i in 0..input.len() {
            acc += input[i];
            if i >= width {
                acc -= input[i - width];
            }
            out.push(acc / width.min(i + 1) as f32);
        }
        out
    } else {
        input.to_vec()
    };
    let step = from as f64 / to as f64;
    let n = ((input.len() as f64) / step).floor() as usize;
    (0..n)
        .map(|i| {
            let x = i as f64 * step;
            let a = x.floor() as usize;
            let b = (a + 1).min(filtered.len() - 1);
            let t = (x - a as f64) as f32;
            filtered[a] * (1.0 - t) + filtered[b] * t
        })
        .collect()
}

/// Writes 16 kHz mono 16-bit WAV bytes. For tests and the simulated voice only: the listener
/// never writes what it hears.
pub fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data = to_s16le(samples);
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s16le_round_trips() {
        let s = vec![0.0, 0.5, -0.5, 0.999, -1.0];
        let back = from_s16le(&to_s16le(&s));
        for (a, b) in s.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-3, "{a} {b}");
        }
    }

    #[test]
    fn wav_round_trips_at_16k() {
        let s: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let back = read_wav(&wav_bytes(&s)).unwrap();
        assert_eq!(back.len(), s.len());
        assert!(s.iter().zip(back.iter()).all(|(a, b)| (a - b).abs() < 1e-3));
    }

    #[test]
    fn resampling_keeps_duration_and_tone() {
        // One second of 440 Hz at 48 kHz becomes one second at 16 kHz with the same zero crossings.
        let s: Vec<f32> = (0..48_000).map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin()).collect();
        let r = resample(&s, 48_000, RATE);
        assert!((r.len() as i64 - 16_000).abs() <= 1);
        let crossings = r.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((870..=890).contains(&crossings), "{crossings}");
    }

    #[test]
    fn stereo_is_mixed_down() {
        // A 16 kHz stereo file with the left channel at 0.5 and the right at -0.5 is silence.
        let mut body = Vec::new();
        for _ in 0..160 {
            body.extend_from_slice(&16384i16.to_le_bytes());
            body.extend_from_slice(&(-16384i16).to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + body.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&(RATE * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(body.len() as u32).to_le_bytes());
        wav.extend_from_slice(&body);
        let s = read_wav(&wav).unwrap();
        assert_eq!(s.len(), 160);
        assert!(s.iter().all(|v| v.abs() < 1e-3));
    }

    #[test]
    fn not_a_wav_is_refused() {
        assert!(read_wav(b"hello").is_err());
    }
}
