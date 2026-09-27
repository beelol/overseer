//! Made-up sound for tests and for the simulated voice (`OVERSEER_VOICE_SIMULATE=1`): noises that
//! must never open the speech gate, and a speech-like sound that must. Nothing here is a recording
//! and nothing is written to disk. Real test speech comes from `say`, made at test time.

use crate::gate::{GateEvent, SpeechGate};
use crate::pcm::{FRAME, RATE};
use std::f32::consts::PI;

/// A small deterministic noise source, so tests never depend on luck.
pub struct Noise(u64);
impl Noise {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    }
}

pub fn secs(s: f32) -> usize {
    (s * RATE as f32) as usize
}

pub fn mix(a: &mut [f32], b: &[f32], at: usize) {
    for (i, v) in b.iter().enumerate() {
        if let Some(x) = a.get_mut(at + i) {
            *x += v;
        }
    }
}

/// Feeds sound through a gate; returns how often it opened, the frame of the first opening and the
/// highest level the mark would have shown.
pub fn run(gate: &mut SpeechGate, s: &[f32]) -> (usize, Option<usize>, f32) {
    let mut opens = 0;
    let mut first_open = None;
    let mut max_level: f32 = 0.0;
    for (i, f) in s.chunks_exact(FRAME).enumerate() {
        let (_, ev) = gate.push(f);
        if let Some(GateEvent::Open { .. }) = ev {
            opens += 1;
            first_open.get_or_insert(i);
        }
        max_level = max_level.max(gate.level());
    }
    (opens, first_open, max_level)
}

/// Room tone: quiet broadband noise.
pub fn room(dur: f32, amp: f32, seed: u64) -> Vec<f32> {
    let mut n = Noise::new(seed);
    (0..secs(dur)).map(|_| amp * n.next()).collect()
}

/// A vowel-like sound: a gliding pitch with harmonics and a formant, in syllables of 250 ms.
pub fn speechlike(dur: f32, amp: f32) -> Vec<f32> {
    let mut phase = 0.0f32;
    (0..secs(dur))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let pitch = 140.0 + 30.0 * (2.0 * PI * 1.3 * t).sin() + 10.0 * (2.0 * PI * 3.1 * t).sin();
            phase += 2.0 * PI * pitch / RATE as f32;
            let syllable = (0.55 + 0.45 * (2.0 * PI * 4.0 * t).sin()).max(0.1);
            let v: f32 = (1..=12).map(|h| (h as f32 * phase).sin() * (1.0 / h as f32) * if (500.0..1500.0).contains(&(h as f32 * pitch)) { 2.0 } else { 1.0 }).sum();
            amp * syllable * v / 3.0
        })
        .collect()
}

/// Short broadband clicks, `per_s` a second: typing is about 8, taps on a desk about 2.
pub fn clicks(dur: f32, per_s: f32, amp: f32, seed: u64) -> Vec<f32> {
    let mut n = Noise::new(seed);
    let mut out = room(dur, 0.0005, seed + 1);
    let every = (RATE as f32 / per_s) as usize;
    let mut at = every / 3;
    while at + 400 < out.len() {
        let burst: Vec<f32> = (0..240).map(|i| amp * n.next() * (-(i as f32) / 40.0).exp()).collect();
        mix(&mut out, &burst, at);
        at += every + (n.next().abs() * every as f32 * 0.5) as usize;
    }
    out
}

/// A chair scraping: rough, band-limited noise for most of a second.
pub fn chair(amp: f32, seed: u64) -> Vec<f32> {
    let mut n = Noise::new(seed);
    let mut lp = 0.0f32;
    let mut out = room(0.3, 0.0005, seed);
    out.extend((0..secs(0.8)).map(|i| {
        let t = i as f32 / RATE as f32;
        lp += 0.2 * (n.next() - lp);
        amp * lp * (0.5 + 0.5 * (2.0 * PI * 23.0 * t).sin().abs()) * (t * 8.0).min(1.0) * (1.0 - t / 0.8)
    }));
    out.extend(room(0.4, 0.0005, seed + 1));
    out
}

/// A door closing: a low thud with a short rattle.
pub fn door(amp: f32, seed: u64) -> Vec<f32> {
    let mut n = Noise::new(seed);
    let mut out = room(0.3, 0.0005, seed);
    out.extend((0..secs(0.5)).map(|i| {
        let t = i as f32 / RATE as f32;
        amp * ((2.0 * PI * 70.0 * t).sin() * (-t * 12.0).exp() + 0.3 * n.next() * (-t * 30.0).exp())
    }));
    out.extend(room(0.4, 0.0005, seed + 1));
    out
}

/// A cup set down: a bright ring that dies in about 150 ms.
pub fn cup(amp: f32) -> Vec<f32> {
    let mut out = room(0.3, 0.0005, 3);
    out.extend((0..secs(0.4)).map(|i| amp * (2.0 * PI * 1800.0 * i as f32 / RATE as f32).sin() * (-(i as f32) / 600.0).exp()));
    out.extend(room(0.3, 0.0005, 4));
    out
}

/// A cough: two bursts of rough noise, 350 ms together, with no steady pitch.
pub fn cough(amp: f32, seed: u64) -> Vec<f32> {
    let mut n = Noise::new(seed);
    let mut out = room(0.3, 0.0005, seed);
    for len in [0.18f32, 0.17] {
        let mut lp = 0.0f32;
        out.extend((0..secs(len)).map(|i| {
            let t = i as f32 / RATE as f32;
            lp += 0.5 * (n.next() - lp);
            amp * lp * (t * 60.0).min(1.0) * (-t * 10.0).exp()
        }));
        out.extend(room(0.06, 0.0005, seed + 2));
    }
    out.extend(room(0.3, 0.0005, seed + 1));
    out
}

/// A breathy laugh: four short bursts of breath.
pub fn laugh(amp: f32, seed: u64) -> Vec<f32> {
    let mut n = Noise::new(seed);
    let mut out = room(0.2, 0.0005, seed);
    for _ in 0..4 {
        let mut lp = 0.0f32;
        out.extend((0..secs(0.12)).map(|i| {
            let t = i as f32 / RATE as f32;
            lp += 0.35 * (n.next() - lp);
            amp * lp * (t * 50.0).min(1.0) * (1.0 - t / 0.12)
        }));
        out.extend(room(0.1, 0.0005, seed + 3));
    }
    out.extend(room(0.3, 0.0005, seed + 1));
    out
}

/// A fan: steady broadband noise.
pub fn fan(dur: f32, amp: f32, seed: u64) -> Vec<f32> {
    room(dur, amp, seed)
}

/// Music: steady harmonic notes, a new one every 400 ms.
pub fn music(dur: f32, amp: f32) -> Vec<f32> {
    let notes = [220.0f32, 261.6, 329.6, 392.0, 293.7, 246.9];
    let mut s = Vec::new();
    for (k, f) in notes.iter().cycle().take((dur / 0.4).ceil() as usize).enumerate() {
        s.extend((0..secs(0.4)).map(|i| {
            let t = i as f32 / RATE as f32;
            let env = (t * 30.0).min(1.0) * (1.0 - t / 0.45);
            amp * env * (1..=6).map(|h| (2.0 * PI * f * h as f32 * t + k as f32).sin() / h as f32).sum::<f32>()
        }));
    }
    s
}

/// Every noise the gate must reject (AC-164), by name, at a given seed and loudness scale.
pub fn noise(kind: &str, seed: u64, scale: f32) -> Option<Vec<f32>> {
    Some(match kind {
        "taps" => clicks(4.0, 2.0, 0.8 * scale, seed),
        "clicks" => clicks(4.0, 5.0, 0.5 * scale, seed),
        "typing" => clicks(4.0, 8.0, 0.3 * scale, seed),
        "chair" => chair(0.5 * scale, seed),
        "door" => door(0.8 * scale, seed),
        "cup" => cup(0.5 * scale),
        "cough" => cough(0.8 * scale, seed),
        "laugh" => laugh(0.6 * scale, seed),
        "fan" => fan(4.0, 0.08 * scale, seed),
        "music" => music(4.0, 0.2 * scale),
        _ => return None,
    })
}

pub const NOISES: &[&str] = &["taps", "clicks", "typing", "chair", "door", "cup", "cough", "laugh", "fan", "music"];
