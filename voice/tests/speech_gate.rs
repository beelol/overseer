//! AC-164: noise never opens the speech gate; speech does, quickly.
//!
//! Speech is synthesized with `say` into a temporary folder that is deleted at the end (macOS); the
//! noises are generated. Nothing is recorded and nothing is kept.

use overseer_listener::gate::{GateConfig, SpeechGate};
use overseer_listener::pcm::{self, FRAME, RATE};
use overseer_listener::synth::{self, NOISES};
use std::process::Command;

fn say(text: &str, voice: &str) -> Option<Vec<f32>> {
    overseer_listener::speak::say(text, Some(voice), None).ok()
}

#[test]
fn every_noise_100_times_never_opens_the_gate_or_moves_the_mark() {
    for kind in NOISES {
        for i in 0..100u64 {
            let scale = [0.5, 1.0, 1.6][(i % 3) as usize];
            let mut s = synth::room(0.5, 0.002, 1000 + i);
            s.extend(synth::noise(kind, i, scale).unwrap());
            let mut g = SpeechGate::new(GateConfig::default());
            let (opens, _, level) = synth::run(&mut g, &s);
            assert_eq!(opens, 0, "{kind} #{i} at {scale}x opened the gate");
            assert_eq!(level, 0.0, "{kind} #{i} moved the mark");
        }
    }
}

#[test]
fn noise_while_nobody_speaks_then_speech_still_opens() {
    // Ten seconds of the whole catalogue back to back, then someone speaks.
    let mut s = Vec::new();
    for (i, kind) in NOISES.iter().enumerate() {
        s.extend(synth::noise(kind, i as u64, 1.0).unwrap());
    }
    let mut g = SpeechGate::new(GateConfig::default());
    assert_eq!(synth::run(&mut g, &s).0, 0);
    let mut voice = synth::room(0.4, 0.002, 77);
    voice.extend(synth::speechlike(1.5, 0.5));
    assert_eq!(synth::run(&mut g, &voice).0, 1, "speech after the noise");
}

/// The mark starts to move when the gate opens (AC-177). Measured from the first sound of the first
/// word, which for words like "Stop" is a consonant with no voice in it yet.
#[test]
fn spoken_sentences_open_within_300_ms_for_nine_in_ten_and_450_at_most() {
    if Command::new("say").arg("-v").arg("?").output().is_err() {
        eprintln!("skipped: no `say` on this system");
        return;
    }
    let sentences = [
        "Tell Continuity to use the new wire format.",
        "Stop the phone agent.",
        "What is everyone doing?",
        "Back to Overseer.",
        "Yes, allow it.",
        "Phone and Continuity should both use the new wire format, and someone should write the migration note.",
    ];
    let mut times = Vec::new();
    let mut levels = Vec::new();
    for voice in [
        "Daniel",
        "Eddy (English (US))",
        "Flo (English (UK))",
        "Samantha",
        "Fred",
    ] {
        for text in sentences {
            let Some(speech) = say(text, voice) else {
                panic!("say failed for {voice}")
            };
            let mut s = synth::room(0.6, 0.002, 5);
            let lead = s.len();
            s.extend(&speech);
            s.extend(synth::room(0.8, 0.002, 6));
            let onset = lead + speech.iter().position(|v| v.abs() > 0.02).unwrap_or(0);
            let mut g = SpeechGate::new(GateConfig::default());
            let mut first = None;
            let mut open_levels = Vec::new();
            for (i, f) in s.chunks_exact(FRAME).enumerate() {
                g.push(f);
                if g.is_open() {
                    first.get_or_insert(i);
                    open_levels.push(g.level());
                }
            }
            let Some(first) = first else {
                panic!("{voice}: '{text}' never opened the gate")
            };
            let ms = ((first + 1) * FRAME) as f32 * 1000.0 / RATE as f32
                - onset as f32 * 1000.0 / RATE as f32;
            times.push(ms);
            levels.push(open_levels.iter().sum::<f32>() / open_levels.len() as f32);
            assert!(!g.is_open(), "{voice}: '{text}' left the gate open");
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p90 = times[times.len() * 9 / 10];
    let max = times[times.len() - 1];
    let level = levels.iter().sum::<f32>() / levels.len() as f32;
    eprintln!("opening after the first word: median {:.0} ms, p90 {p90:.0} ms, max {max:.0} ms; mean level while open {level:.2}", times[times.len() / 2]);
    assert!(p90 <= 300.0, "p90 {p90} ms");
    assert!(max <= 450.0, "max {max} ms");
    assert!(
        (0.25..=0.7).contains(&level),
        "the level should move with syllables, not sit at the top: {level}"
    );
}
