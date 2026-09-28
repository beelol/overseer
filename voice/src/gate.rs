//! The speech gate (AC-164): decides from the sound alone whether someone is speaking.
//!
//! The owner's complaint about other voice modes is that they stop what they are doing at a tiny
//! noise when nobody is talking. So the gate opens only for sound with the shape of speech:
//! voiced (periodic at a speaking pitch), in the speech band, well above the room's noise floor,
//! for long enough, and with a pitch that moves the way speech does. Taps, clicks, typing, a door,
//! a cough, a breathy laugh, a fan and sustained musical notes do not open it.
//!
//! The gate also gives the level that moves the mark (AC-177): one number, 0 to 1, and only while
//! the gate is open.

use crate::pcm::{FRAME, RATE};

/// The gate's numbers. The spike (AC-162) sets them on the owner's Mac; a change is recorded in
/// docs/rfcs/voice-mode.md.
#[derive(Clone, Debug)]
pub struct GateConfig {
    /// A frame counts as voiced when it is at least this far above the noise floor (dB)...
    pub above_floor_db: f32,
    /// ...and at least this loud in absolute terms (dB relative to full scale).
    pub min_level_db: f32,
    /// Normalized autocorrelation needed for a frame to count as periodic.
    pub voicing: f32,
    /// Share of the frame's energy that must be in the speech band: a voice carries most of its
    /// energy above 200 Hz, a door's thud or a hum below it.
    pub band_share: f32,
    /// The window looked at to open the gate, in frames (20 ms each).
    pub window: usize,
    /// Voiced frames needed in that window: 9 of 16 is 180 ms of voice in 320 ms.
    pub voiced_needed: usize,
    /// Two voiced frames whose periodicity has this similar a shape sound the same.
    pub same_shape: f32,
    /// Above this share of same-sounding steps in the window, the sound is a ring, a chime or a
    /// held note, not speech: speech changes its shape all the time.
    pub steady_share: f32,
    /// The fast way in, for a strong and changing voice: this many voiced frames of the last
    /// `fast_window` (7 of 8 is 140 ms in 160 ms), each at least `fast_above_db` over the floor.
    pub fast_needed: usize,
    pub fast_window: usize,
    pub fast_above_db: f32,
    /// Frames without voice before the gate closes again (300 ms).
    pub hangover: usize,
    /// How fast the noise floor may rise, in dB per second (it falls at once).
    pub floor_rise_db_per_s: f32,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            above_floor_db: 12.0,
            min_level_db: -60.0,
            voicing: 0.45,
            band_share: 0.3,
            window: 16,
            voiced_needed: 9,
            same_shape: 0.97,
            steady_share: 0.6,
            fast_needed: 7,
            fast_window: 8,
            fast_above_db: 18.0,
            hangover: 15,
            floor_rise_db_per_s: 1.5,
        }
    }
}

/// A second-order section (RBJ cookbook), for the speech band.
#[derive(Clone, Debug)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn new(kind: &str, hz: f32) -> Self {
        let w = 2.0 * std::f32::consts::PI * hz / RATE as f32;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let (b, a0, a) = match kind {
            "high" => (
                [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0],
                1.0 + alpha,
                [-2.0 * c, 1.0 - alpha],
            ),
            _ => (
                [(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0],
                1.0 + alpha,
                [-2.0 * c, 1.0 - alpha],
            ),
        };
        Self {
            b: [b[0] / a0, b[1] / a0, b[2] / a0],
            a: [a[0] / a0, a[1] / a0],
            z: [0.0; 2],
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// What one 20 ms frame looked like.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    /// Loudness in the speech band, dB relative to full scale.
    pub level_db: f32,
    /// The noise floor after this frame.
    pub floor_db: f32,
    /// Periodicity, 0 to 1.
    pub voicing: f32,
    /// The pitch period in samples, when periodic.
    pub period: Option<usize>,
    pub voiced: bool,
    /// Voiced, and sounding the same as the voiced frame before it.
    pub steady: bool,
}

/// A change the gate reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GateEvent {
    /// Speech started. `lead` is how many samples before this frame it began (the window's
    /// first voiced frame), so the utterance can include its first syllable.
    Open { lead: usize },
    /// Speech stopped (after the hangover).
    Close,
}

pub struct SpeechGate {
    cfg: GateConfig,
    hp: Biquad,
    lp: Biquad,
    /// Removes only the DC offset, for the frame's whole energy.
    dc: Biquad,
    /// The last two frames after the band-pass, for a 40 ms voicing window.
    band: Vec<f32>,
    floor_db: f32,
    frames_seen: u64,
    recent: std::collections::VecDeque<Frame>,
    /// The last voiced frame's periodicity shape.
    last_shape: Option<Vec<f32>>,
    open: bool,
    quiet: usize,
    level: f32,
    /// The voice's recent peak, so the mark follows syllables whatever the microphone's gain.
    peak_db: f32,
}

impl SpeechGate {
    pub fn new(cfg: GateConfig) -> Self {
        Self {
            cfg,
            hp: Biquad::new("high", 200.0),
            lp: Biquad::new("low", 3800.0),
            dc: Biquad::new("high", 20.0),
            band: vec![0.0; 2 * FRAME],
            floor_db: -70.0,
            frames_seen: 0,
            recent: Default::default(),
            last_shape: None,
            open: false,
            quiet: 0,
            level: 0.0,
            peak_db: -100.0,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The level for the mark: 0 to 1 while the gate is open, 0 while it is closed.
    pub fn level(&self) -> f32 {
        if self.open {
            self.level
        } else {
            0.0
        }
    }

    pub fn floor_db(&self) -> f32 {
        self.floor_db
    }

    /// Feeds one frame of `FRAME` samples; returns the frame's analysis and any change.
    pub fn push(&mut self, samples: &[f32]) -> (Frame, Option<GateEvent>) {
        debug_assert_eq!(samples.len(), FRAME);
        self.band.drain(..FRAME);
        let mut whole = 0.0f32;
        for &x in samples {
            let y = self.lp.run(self.hp.run(x));
            self.band.push(y);
            let w = self.dc.run(x);
            whole += w * w;
        }
        let cur = &self.band[FRAME..];
        let energy = cur.iter().map(|v| v * v).sum::<f32>() / FRAME as f32;
        let in_band = energy * FRAME as f32 / whole.max(1e-12);
        let level_db = 10.0 * (energy + 1e-12).log10() + 3.01; // a full-scale sine reads 0 dB
                                                               // The floor falls at once and rises slowly, so a steady fan or hum becomes the floor while
                                                               // speech, which comes and goes, stays above it. It starts from the first frames heard.
        self.frames_seen += 1;
        if self.frames_seen <= 5 || level_db < self.floor_db {
            self.floor_db = if self.frames_seen == 1 {
                level_db
            } else {
                self.floor_db.min(level_db).max(-100.0)
            };
        } else {
            self.floor_db += self.cfg.floor_rise_db_per_s / 50.0;
            self.floor_db = self.floor_db.min(level_db);
        }
        // Periodicity is only worth measuring for a frame loud enough to count.
        let loud = level_db >= self.cfg.min_level_db
            && level_db - self.floor_db >= self.cfg.above_floor_db
            && in_band >= self.cfg.band_share;
        let (voicing, period, shape) = if loud {
            periodicity(&self.band)
        } else {
            (0.0, None, Vec::new())
        };
        let voiced = loud && voicing >= self.cfg.voicing;
        let steady = voiced
            && self
                .last_shape
                .as_ref()
                .is_some_and(|prev| similarity(prev, &shape) >= self.cfg.same_shape);
        if voiced {
            self.last_shape = Some(shape);
        } else {
            self.last_shape = None;
        }
        let frame = Frame {
            level_db,
            floor_db: self.floor_db,
            voicing,
            period: if voiced { period } else { None },
            voiced,
            steady,
        };
        self.recent.push_back(frame);
        if self.recent.len() > self.cfg.window {
            self.recent.pop_front();
        }
        // The mark's level follows the syllables: a voiced frame at the voice's recent peak is 1,
        // 30 dB under it is 0, on a curve that leaves room between syllables. The peak jumps up at
        // once and falls back 6 dB a second.
        if voiced {
            self.peak_db = self.peak_db.max(level_db);
        } else {
            self.peak_db = (self.peak_db - 6.0 / 50.0).max(self.floor_db + self.cfg.above_floor_db);
        }
        let target = ((level_db - (self.peak_db - 30.0)) / 30.0)
            .clamp(0.0, 1.0)
            .powf(2.2);
        self.level = if voiced {
            target
        } else {
            self.level.min(target)
        };
        let mut event = None;
        if !self.open {
            let voiced_n = self.recent.iter().filter(|f| f.voiced).count();
            let fast = self
                .recent
                .iter()
                .rev()
                .take(self.cfg.fast_window)
                .filter(|f| f.voiced && f.level_db - f.floor_db >= self.cfg.fast_above_db)
                .count();
            let enough = (self.recent.len() == self.cfg.window
                && voiced_n >= self.cfg.voiced_needed)
                || fast >= self.cfg.fast_needed;
            if enough && !self.held_note() {
                self.open = true;
                self.quiet = 0;
                let first = self.recent.iter().position(|f| f.voiced).unwrap_or(0);
                event = Some(GateEvent::Open {
                    lead: (self.cfg.window - first) * FRAME,
                });
            }
        } else if voiced {
            self.quiet = 0;
        } else {
            self.quiet += 1;
            if self.quiet >= self.cfg.hangover {
                self.open = false;
                self.level = 0.0;
                self.peak_db = -100.0;
                event = Some(GateEvent::Close);
            }
        }
        (frame, event)
    }

    /// A ring, a chime or a held note sounds the same from frame to frame; speech never does.
    fn held_note(&self) -> bool {
        let voiced = self.recent.iter().filter(|f| f.voiced).count();
        let steady = self.recent.iter().filter(|f| f.steady).count();
        voiced >= 3 && steady as f32 / (voiced - 1) as f32 > self.cfg.steady_share
    }
}

/// Normalized autocorrelation over a speaking pitch range (60 to 400 Hz): the strongest value, its
/// period in 16 kHz samples, and the whole curve (the frame's "shape"). It runs on every other
/// sample: the band ends at 3.8 kHz, under 8 kHz's limit.
fn periodicity(x: &[f32]) -> (f32, Option<usize>, Vec<f32>) {
    let d: Vec<f32> = x.iter().step_by(2).copied().collect();
    let n = d.len();
    let half = RATE / 2;
    let (min_lag, max_lag) = ((half / 400) as usize, ((half / 60) as usize).min(n / 2));
    let mut best = (0.0f32, None);
    let mut shape = Vec::with_capacity(max_lag + 1);
    for lag in 1..=max_lag {
        let (mut r, mut e0, mut e1) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..n - lag {
            r += d[i] * d[i + lag];
            e0 += d[i] * d[i];
            e1 += d[i + lag] * d[i + lag];
        }
        let c = r / (e0 * e1).sqrt().max(1e-12);
        shape.push(c);
        if lag >= min_lag && c > best.0 {
            best = (c, Some(lag * 2));
        }
    }
    (best.0, best.1, shape)
}

/// Cosine similarity of two periodicity curves.
fn similarity(a: &[f32], b: &[f32]) -> f32 {
    let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    ab / (aa * bb).sqrt().max(1e-12)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub use crate::synth::*;

    #[test]
    fn silence_and_room_tone_never_open() {
        for amp in [0.0, 0.001, 0.01, 0.05] {
            let mut g = SpeechGate::new(GateConfig::default());
            let (opens, _, level) = run(&mut g, &room(5.0, amp, 7));
            assert_eq!(opens, 0, "room tone at {amp}");
            assert_eq!(level, 0.0);
        }
    }

    #[test]
    fn speech_opens_within_300_ms_and_moves_the_level() {
        let mut s = room(1.0, 0.002, 3);
        s.extend(speechlike(2.0, 0.3));
        s.extend(room(1.0, 0.002, 4));
        let mut g = SpeechGate::new(GateConfig::default());
        let (opens, first, level) = run(&mut g, &s);
        assert_eq!(opens, 1);
        let open_ms = first.unwrap() as f32 * 20.0 - 1000.0;
        assert!(
            open_ms <= 300.0,
            "opened {open_ms} ms after the speech began"
        );
        assert!(level > 0.3, "level {level}");
        assert!(!g.is_open(), "closes after the speech ends");
    }

    #[test]
    fn typing_taps_and_clicks_never_open() {
        for (per_s, amp) in [(8.0, 0.3), (2.0, 0.8), (15.0, 0.1)] {
            let mut g = SpeechGate::new(GateConfig::default());
            let (opens, _, level) = run(&mut g, &clicks(6.0, per_s, amp, 11));
            assert_eq!(opens, 0, "{per_s} clicks a second at {amp}");
            assert_eq!(level, 0.0);
        }
    }

    fn over_fan(voice_amp: f32) -> usize {
        // A loud, steady fan (RMS about 0.046): alone it never opens the gate and becomes the floor.
        let mut g = SpeechGate::new(GateConfig::default());
        assert_eq!(run(&mut g, &room(4.0, 0.08, 5)).0, 0, "the fan alone");
        let voice = speechlike(1.5, voice_amp);
        let mut s = room(1.5, 0.08, 6);
        mix(&mut s, &voice, 0);
        run(&mut g, &s).0
    }

    #[test]
    fn a_steady_fan_becomes_the_floor_and_speech_over_it_opens() {
        assert_eq!(over_fan(1.1), 1, "speech about 15 dB over the fan");
    }

    #[test]
    fn faint_speech_over_a_fan_does_not_open() {
        // Talk from across the room or a video playing quietly: about 3 dB over the fan.
        assert_eq!(over_fan(0.4), 0);
    }

    #[test]
    fn a_held_note_is_not_speech() {
        // Music: chords of steady harmonic notes, changing every 400 ms.
        let notes = [220.0f32, 261.6, 329.6, 392.0, 293.7, 246.9];
        let mut s = Vec::new();
        for (k, f) in notes.iter().cycle().take(15).enumerate() {
            s.extend((0..secs(0.4)).map(|i| {
                let t = i as f32 / RATE as f32;
                let env = (t * 30.0).min(1.0) * (1.0 - t / 0.45);
                0.2 * env
                    * (1..=6)
                        .map(|h| {
                            (2.0 * std::f32::consts::PI * f * h as f32 * t + k as f32).sin()
                                / h as f32
                        })
                        .sum::<f32>()
            }));
        }
        let mut g = SpeechGate::new(GateConfig::default());
        assert_eq!(run(&mut g, &s).0, 0);
    }

    #[test]
    fn short_voiced_sounds_do_not_open() {
        // A cup set down: a bright ring that dies in 150 ms. A grunt: 100 ms of voice.
        let mut s = room(0.5, 0.002, 8);
        let ring: Vec<f32> = (0..secs(0.15))
            .map(|i| {
                0.5 * (2.0 * std::f32::consts::PI * 1800.0 * i as f32 / RATE as f32).sin()
                    * (-(i as f32) / 600.0).exp()
            })
            .collect();
        mix(&mut s, &ring, secs(0.2));
        s.extend(room(0.5, 0.002, 9));
        s.extend(speechlike(0.10, 0.4));
        s.extend(room(0.8, 0.002, 10));
        let mut g = SpeechGate::new(GateConfig::default());
        assert_eq!(run(&mut g, &s).0, 0);
    }
}
