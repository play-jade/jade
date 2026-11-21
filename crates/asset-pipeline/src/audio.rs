//! Deterministic physical modal-resonance percussion audio synthesis (`44.1 kHz`, 16-bit mono WAV).
//!
//! Synthesizes realistic, tactile short board-piece impacts (`75–125 ms`) for:
//! - `jade` (dense crystalline nephrite tap on wooden board)
//! - `boxwood` (warm damped hardwood knock)
//! - `ivory-stone` (mellow matte stone clack)
//!
//! Each impact combines a half-sine Hertzian contact pulse, a short band-limited
//! micro-roughness transient, coupled wooden-board cavity resonance, and
//! inharmonic circular-plate modes with frequency-dependent viscoelastic damping.

use std::fs;
use std::path::{Path, PathBuf};

use crate::PipelineError;
use crate::noise::splitmix64;

pub const SAMPLE_RATE: u32 = 44_100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFamily {
    Jade,
    Boxwood,
    IvoryStone,
}

impl AudioFamily {
    pub const ALL: [AudioFamily; 3] = [
        AudioFamily::Jade,
        AudioFamily::Boxwood,
        AudioFamily::IvoryStone,
    ];

    pub fn dir_name(self) -> &'static str {
        match self {
            AudioFamily::Jade => "jade",
            AudioFamily::Boxwood => "boxwood",
            AudioFamily::IvoryStone => "ivory-stone",
        }
    }

    pub fn material_family(self) -> &'static str {
        match self {
            AudioFamily::Jade => "pale-jade",
            AudioFamily::Boxwood => "boxwood",
            AudioFamily::IvoryStone => "ivory-stone",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEvent {
    Place1,
    Place2,
    Capture,
    Check,
}

impl SoundEvent {
    pub const ALL: [SoundEvent; 4] = [
        SoundEvent::Place1,
        SoundEvent::Place2,
        SoundEvent::Capture,
        SoundEvent::Check,
    ];

    pub fn file_name(self) -> &'static str {
        match self {
            SoundEvent::Place1 => "place-1.wav",
            SoundEvent::Place2 => "place-2.wav",
            SoundEvent::Capture => "capture.wav",
            SoundEvent::Check => "check.wav",
        }
    }

    pub fn id_slug(self) -> &'static str {
        match self {
            SoundEvent::Place1 => "place-1",
            SoundEvent::Place2 => "place-2",
            SoundEvent::Capture => "capture",
            SoundEvent::Check => "check",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedAudio {
    pub family: AudioFamily,
    pub event: SoundEvent,
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub sample_rate: u32,
    pub sample_count: usize,
    pub duration_ms: f64,
    pub peak_amplitude: f64,
}

#[derive(Clone, Copy)]
struct Mode {
    freq_hz: f64,
    amp: f64,
    decay_s: f64,
    phase: f64,
}

struct StrikeSpec {
    onset_s: f64,
    gain: f64,
    contact_s: f64,
    contact_weight: f64,
    roughness_weight: f64,
    roughness_cutoff_hz: f64,
    pitch_scale: f64,
    seed: u64,
}

fn family_modes(family: AudioFamily) -> &'static [Mode] {
    match family {
        // Dense nephrite stone disc coupled to warm wooden Xiangqi board
        AudioFamily::Jade => &[
            Mode {
                freq_hz: 186.0,
                amp: 0.42,
                decay_s: 0.0150,
                phase: 0.05,
            },
            Mode {
                freq_hz: 342.0,
                amp: 0.22,
                decay_s: 0.0115,
                phase: 0.20,
            },
            Mode {
                freq_hz: 1185.0,
                amp: 0.34,
                decay_s: 0.0095,
                phase: 0.00,
            },
            Mode {
                freq_hz: 1910.0,
                amp: 0.26,
                decay_s: 0.0072,
                phase: 0.35,
            },
            Mode {
                freq_hz: 2790.0,
                amp: 0.19,
                decay_s: 0.0054,
                phase: 0.70,
            },
            Mode {
                freq_hz: 3860.0,
                amp: 0.13,
                decay_s: 0.0040,
                phase: 1.10,
            },
            Mode {
                freq_hz: 5180.0,
                amp: 0.07,
                decay_s: 0.0028,
                phase: 0.50,
            },
        ],
        // Warm damped honey boxwood disc on wooden board
        AudioFamily::Boxwood => &[
            Mode {
                freq_hz: 168.0,
                amp: 0.46,
                decay_s: 0.0145,
                phase: 0.05,
            },
            Mode {
                freq_hz: 315.0,
                amp: 0.28,
                decay_s: 0.0110,
                phase: 0.15,
            },
            Mode {
                freq_hz: 560.0,
                amp: 0.38,
                decay_s: 0.0095,
                phase: 0.00,
            },
            Mode {
                freq_hz: 895.0,
                amp: 0.30,
                decay_s: 0.0074,
                phase: 0.40,
            },
            Mode {
                freq_hz: 1365.0,
                amp: 0.20,
                decay_s: 0.0055,
                phase: 0.85,
            },
            Mode {
                freq_hz: 1980.0,
                amp: 0.11,
                decay_s: 0.0040,
                phase: 1.20,
            },
            Mode {
                freq_hz: 2740.0,
                amp: 0.05,
                decay_s: 0.0028,
                phase: 0.60,
            },
        ],
        // Quiet matte limestone / ivory-stone clack on wooden board
        AudioFamily::IvoryStone => &[
            Mode {
                freq_hz: 176.0,
                amp: 0.44,
                decay_s: 0.0150,
                phase: 0.05,
            },
            Mode {
                freq_hz: 328.0,
                amp: 0.24,
                decay_s: 0.0112,
                phase: 0.18,
            },
            Mode {
                freq_hz: 840.0,
                amp: 0.35,
                decay_s: 0.0092,
                phase: 0.00,
            },
            Mode {
                freq_hz: 1380.0,
                amp: 0.27,
                decay_s: 0.0070,
                phase: 0.32,
            },
            Mode {
                freq_hz: 2090.0,
                amp: 0.18,
                decay_s: 0.0052,
                phase: 0.75,
            },
            Mode {
                freq_hz: 2980.0,
                amp: 0.11,
                decay_s: 0.0038,
                phase: 1.05,
            },
            Mode {
                freq_hz: 4020.0,
                amp: 0.05,
                decay_s: 0.0026,
                phase: 0.45,
            },
        ],
    }
}

fn base_contact_time(family: AudioFamily) -> f64 {
    match family {
        AudioFamily::Jade => 0.00058,
        AudioFamily::Boxwood => 0.00118,
        AudioFamily::IvoryStone => 0.00086,
    }
}

fn base_roughness_cutoff(family: AudioFamily) -> f64 {
    match family {
        AudioFamily::Jade => 4600.0,
        AudioFamily::Boxwood => 2400.0,
        AudioFamily::IvoryStone => 3400.0,
    }
}

fn synthesize_strike(buf: &mut [f64], modes: &[Mode], spec: &StrikeSpec) {
    let dt = 1.0 / (SAMPLE_RATE as f64);
    let start_idx = ((spec.onset_s * (SAMPLE_RATE as f64)).round() as usize).min(buf.len());

    // Deterministic one-pole low-pass filtered surface-roughness burst
    let rc = 1.0 / (std::f64::consts::TAU * spec.roughness_cutoff_hz);
    let alpha = dt / (rc + dt);
    let mut lp_state = 0.0;

    for (k, sample) in buf[start_idx..].iter_mut().enumerate() {
        let t = (k as f64) * dt;

        // 1. Hertzian half-sine contact force pulse
        let hertzian = if t < spec.contact_s {
            let u = (std::f64::consts::PI * t / spec.contact_s).sin();
            u.powf(1.5) * spec.contact_weight
        } else {
            0.0
        };

        // 2. Microscopic surface-contact roughness burst (decays within ~3 ms)
        let n_bits = splitmix64(spec.seed.wrapping_add(k as u64));
        let white = (((n_bits >> 11) as f64) / ((1u64 << 53) as f64)) * 2.0 - 1.0;
        lp_state += alpha * (white - lp_state);
        let rough_env = (-t / 0.0014).exp() * (1.0 - (-t / 0.00018).exp());
        let roughness = lp_state * rough_env * spec.roughness_weight;

        // 3. Coupled board + circular-plate damped modal sum with smooth attack ramp
        let attack = 1.0 - (-t / (spec.contact_s * 0.45)).exp();
        let mut modal_sum = 0.0;
        for m in modes {
            let f = m.freq_hz * spec.pitch_scale;
            if f >= (SAMPLE_RATE as f64) * 0.45 {
                continue;
            }
            let env = (-t / m.decay_s).exp();
            modal_sum += m.amp * env * (std::f64::consts::TAU * f * t + m.phase).sin();
        }

        *sample += spec.gain * (hertzian + roughness + attack * modal_sum);
    }
}

pub fn synthesize_samples(family: AudioFamily, event: SoundEvent) -> (Vec<i16>, f64) {
    let modes = family_modes(family);
    let tc = base_contact_time(family);
    let cutoff = base_roughness_cutoff(family);

    let (duration_ms, strikes, target_peak): (f64, Vec<StrikeSpec>, f64) = match event {
        SoundEvent::Place1 => (
            82.0,
            vec![StrikeSpec {
                onset_s: 0.0005,
                gain: 1.0,
                contact_s: tc,
                contact_weight: 0.34,
                roughness_weight: 0.24,
                roughness_cutoff_hz: cutoff,
                pitch_scale: 1.000,
                seed: 0xA101_0001 ^ (family as u64 * 0x10001),
            }],
            0.74,
        ),
        SoundEvent::Place2 => (
            85.0,
            vec![StrikeSpec {
                onset_s: 0.0005,
                gain: 0.96,
                contact_s: tc * 1.06,
                contact_weight: 0.31,
                roughness_weight: 0.22,
                roughness_cutoff_hz: cutoff * 0.94,
                pitch_scale: 1.028,
                seed: 0xA102_0002 ^ (family as u64 * 0x20003),
            }],
            0.71,
        ),
        SoundEvent::Capture => {
            // Piece-on-piece lateral clack at 0.5 ms followed by firm board settle at 19.5 ms
            let clack_modes_pitch = 1.34;
            (
                112.0,
                vec![
                    StrikeSpec {
                        onset_s: 0.0005,
                        gain: 0.62,
                        contact_s: tc * 0.78,
                        contact_weight: 0.26,
                        roughness_weight: 0.30,
                        roughness_cutoff_hz: cutoff * 1.22,
                        pitch_scale: clack_modes_pitch,
                        seed: 0xA103_0003 ^ (family as u64 * 0x30005),
                    },
                    StrikeSpec {
                        onset_s: 0.0195,
                        gain: 1.05,
                        contact_s: tc * 0.95,
                        contact_weight: 0.36,
                        roughness_weight: 0.25,
                        roughness_cutoff_hz: cutoff,
                        pitch_scale: 0.985,
                        seed: 0xA103_0004 ^ (family as u64 * 0x30007),
                    },
                ],
                0.84,
            )
        }
        SoundEvent::Check => (
            120.0,
            vec![
                StrikeSpec {
                    onset_s: 0.0005,
                    gain: 1.12,
                    contact_s: tc * 0.84,
                    contact_weight: 0.40,
                    roughness_weight: 0.28,
                    roughness_cutoff_hz: cutoff * 1.12,
                    pitch_scale: 1.045,
                    seed: 0xA104_0005 ^ (family as u64 * 0x40009),
                },
                // Subtle coupled board rim rebound at 6.5 ms for authoritative crispness
                StrikeSpec {
                    onset_s: 0.0070,
                    gain: 0.34,
                    contact_s: tc * 0.90,
                    contact_weight: 0.14,
                    roughness_weight: 0.10,
                    roughness_cutoff_hz: cutoff * 1.05,
                    pitch_scale: 1.18,
                    seed: 0xA104_0006 ^ (family as u64 * 0x4000B),
                },
            ],
            0.88,
        ),
    };

    let n_samples = ((duration_ms * 0.001) * (SAMPLE_RATE as f64)).round() as usize;
    let mut raw = vec![0.0_f64; n_samples];

    for strike in &strikes {
        synthesize_strike(&mut raw, modes, strike);
    }

    // Remove any DC offset via a gentle 25 Hz one-pole high-pass filter
    let dt = 1.0 / (SAMPLE_RATE as f64);
    let hp_rc = 1.0 / (std::f64::consts::TAU * 25.0);
    let hp_alpha = hp_rc / (hp_rc + dt);
    let mut prev_x = 0.0;
    let mut prev_y = 0.0;
    for s in &mut raw {
        let x = *s;
        let y = hp_alpha * (prev_y + x - prev_x);
        prev_x = x;
        prev_y = y;
        *s = y;
    }

    // Apply smooth 12 ms raised-cosine tail fade so the sample terminates cleanly at zero
    let fade_samples = ((0.012 * (SAMPLE_RATE as f64)).round() as usize).min(n_samples / 3);
    let fade_start = n_samples.saturating_sub(fade_samples);
    for (i, s) in raw[fade_start..].iter_mut().enumerate() {
        let frac = (i as f64) / (fade_samples.max(1) as f64);
        let window = 0.5 * (1.0 + (std::f64::consts::PI * frac).cos());
        *s *= window;
    }
    if let Some(last) = raw.last_mut() {
        *last = 0.0;
    }

    // Normalize deterministically to `target_peak` (strictly < 0.90 full scale, zero clipping)
    let raw_peak = raw
        .iter()
        .fold(0.0_f64, |acc, &v| acc.max(v.abs()))
        .max(1e-9);
    let scale = target_peak / raw_peak;

    let mut pcm = Vec::with_capacity(n_samples);
    let mut max_pcm_norm = 0.0_f64;
    for &v in &raw {
        let norm = (v * scale).clamp(-0.95, 0.95);
        max_pcm_norm = max_pcm_norm.max(norm.abs());
        let q = (norm * 32767.0).round() as i16;
        pcm.push(q);
    }

    (pcm, max_pcm_norm)
}

pub fn encode_wav_mono_16bit(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_bytes = (samples.len() * 2) as u32;
    let riff_chunk_size = 36u32 + data_bytes;
    let byte_rate = sample_rate * 2;
    let block_align = 2u16;

    let mut out = Vec::with_capacity((44 + data_bytes) as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_chunk_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat = 1 (PCM)
    out.extend_from_slice(&1u16.to_le_bytes()); // NumChannels = 1 (mono)
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample = 16
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn generate_all_audio(repo_root: &Path) -> Result<Vec<GeneratedAudio>, PipelineError> {
    let mut generated = Vec::new();
    for family in AudioFamily::ALL {
        let dir = repo_root.join("assets/audio").join(family.dir_name());
        fs::create_dir_all(&dir)?;
        for event in SoundEvent::ALL {
            let (pcm, peak) = synthesize_samples(family, event);
            let wav_bytes = encode_wav_mono_16bit(&pcm, SAMPLE_RATE);
            let abs_path = dir.join(event.file_name());
            fs::write(&abs_path, &wav_bytes)?;
            let rel_path = format!("assets/audio/{}/{}", family.dir_name(), event.file_name());
            let duration_ms = (pcm.len() as f64) * 1000.0 / (SAMPLE_RATE as f64);
            generated.push(GeneratedAudio {
                family,
                event,
                rel_path,
                abs_path,
                sample_rate: SAMPLE_RATE,
                sample_count: pcm.len(),
                duration_ms,
                peak_amplitude: peak,
            });
        }
    }
    Ok(generated)
}

/// Validates a committed 16-bit mono 44.1 kHz WAV file for `--verify`.
pub fn verify_wav_bytes(
    bytes: &[u8],
    path_label: &str,
) -> Result<(u32, usize, f64), PipelineError> {
    if bytes.len() < 44 {
        return Err(PipelineError::Validation(format!(
            "{path_label}: WAV file shorter than 44-byte header"
        )));
    }
    if &bytes[0..4] != b"RIFF"
        || &bytes[8..12] != b"WAVE"
        || &bytes[12..16] != b"fmt "
        || &bytes[36..40] != b"data"
    {
        return Err(PipelineError::Validation(format!(
            "{path_label}: invalid canonical PCM WAV header chunks"
        )));
    }
    let audio_format = u16::from_le_bytes([bytes[20], bytes[21]]);
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
    let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let bits = u16::from_le_bytes([bytes[34], bytes[35]]);
    let data_len = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;

    if audio_format != 1 || channels != 1 || sample_rate != SAMPLE_RATE || bits != 16 {
        return Err(PipelineError::Validation(format!(
            "{path_label}: expected PCM mono {SAMPLE_RATE} Hz 16-bit, got format={audio_format} ch={channels} sr={sample_rate} bits={bits}"
        )));
    }
    if bytes.len() != 44 + data_len || !data_len.is_multiple_of(2) {
        return Err(PipelineError::Validation(format!(
            "{path_label}: data chunk length mismatch ({data_len} vs total {})",
            bytes.len()
        )));
    }

    let sample_count = data_len / 2;
    let duration_ms = (sample_count as f64) * 1000.0 / (SAMPLE_RATE as f64);
    if !(65.0..=140.0).contains(&duration_ms) {
        return Err(PipelineError::Validation(format!(
            "{path_label}: duration {duration_ms:.1} ms outside [65, 140] ms acoustic window"
        )));
    }

    let mut max_abs: i32 = 0;
    for &chunk in bytes[44..].as_chunks::<2>().0 {
        let s = i16::from_le_bytes(chunk) as i32;
        max_abs = max_abs.max(s.abs());
    }
    if max_abs >= 32700 {
        return Err(PipelineError::Validation(format!(
            "{path_label}: audio clips or lacks headroom (max_abs={max_abs})"
        )));
    }
    if max_abs < 4000 {
        return Err(PipelineError::Validation(format!(
            "{path_label}: audio sample too quiet (max_abs={max_abs})"
        )));
    }
    let last_sample = i16::from_le_bytes([bytes[bytes.len() - 2], bytes[bytes.len() - 1]]);
    if last_sample.abs() > 2 {
        return Err(PipelineError::Validation(format!(
            "{path_label}: non-zero tail sample ({last_sample}) may click on playback"
        )));
    }

    Ok((sample_rate, sample_count, (max_abs as f64) / 32767.0))
}
