//! AC-162, the recognizer part of the spike: how fast and how well whisper.cpp turns Overseer's
//! kind of sentences into words on this Mac, and what it costs in memory.
//!
//!     cargo run --release -p overseer-listener --example spike_recognizer -- <model.bin> [hint]
//!
//! Speech is synthesized with `say` into a temporary folder and deleted at the end; nothing is
//! recorded. Run once per model, so each run's peak memory is that model's. With a second
//! argument `hint`, the recognizer is given Overseer's vocabulary as its initial prompt.

use overseer_listener::pcm;
use std::path::Path;
use std::time::Instant;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const SENTENCES: &[&str] = &[
    "Tell Continuity to use the new wire format.",
    "Phone and Continuity should both use the new wire format, and someone should write the migration note.",
    "Stop the phone agent.",
    "What is everyone doing?",
    "Redirect Codex to fix the failing test in the worktree.",
    "Whoever has AC 116 should add tests for the gateway.",
    "Yes, allow it.",
    "Cancel that.",
    "Talk to Continuity.",
    "Back to Overseer.",
    "Merge back the review branch when the tests pass.",
    "Overseer, hold the swarm until the grid is green.",
];
const VOICES: &[&str] = &["Daniel", "Eddy (English (US))", "Flo (English (UK))"];
pub const HINT: &str = "Overseer, Continuity, Codex, Claude, OpenCode, worktree, AC-116, redirect, merge back, swarm, grid, gateway.";

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .replace('-', " ")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn word_errors(expected: &[String], got: &[String]) -> usize {
    let mut row: Vec<usize> = (0..=got.len()).collect();
    for (i, e) in expected.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, g) in got.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if e == g {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j + 1])
            };
            prev = cur;
        }
    }
    row[got.len()]
}

fn peak_rss_mib() -> f64 {
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    u.ru_maxrss as f64 / (1024.0 * 1024.0) // bytes on macOS
}

fn main() -> anyhow::Result<()> {
    let model = std::env::args()
        .nth(1)
        .expect("usage: spike_recognizer <model.bin> [hint]");
    let hint = std::env::args().nth(2).is_some();
    whisper_rs::install_logging_hooks();
    let mut clips = Vec::new();
    for (vi, voice) in VOICES.iter().enumerate() {
        for (si, text) in SENTENCES.iter().enumerate() {
            let _ = (vi, si);
            clips.push((
                voice.to_string(),
                text.to_string(),
                overseer_listener::speak::say(text, Some(voice), None)?,
            ));
        }
    }
    let before = peak_rss_mib();
    let t = Instant::now();
    let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())?;
    let load_ms = t.elapsed().as_millis();
    let mut state = ctx.create_state()?;
    let run =
        |state: &mut whisper_rs::WhisperState, samples: &[f32]| -> anyhow::Result<(String, u128)> {
            let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            p.set_language(Some("en"));
            p.set_n_threads(4);
            p.set_no_context(true);
            p.set_single_segment(true);
            p.set_print_progress(false);
            p.set_print_realtime(false);
            p.set_print_timestamps(false);
            p.set_print_special(false);
            if hint {
                p.set_initial_prompt(HINT);
            }
            let t = Instant::now();
            state.full(p, samples)?;
            let ms = t.elapsed().as_millis();
            let text: String = state
                .as_iter()
                .map(|s| s.to_str_lossy().map(|c| c.into_owned()).unwrap_or_default())
                .collect();
            Ok((text.trim().to_string(), ms))
        };
    run(&mut state, &clips[0].2)?; // warm up (Metal compiles its kernels on the first run)
    let (mut errors, mut total, mut times, mut per_second) = (0, 0, Vec::new(), Vec::new());
    println!("| Voice | Said | Heard | ms | audio s |");
    println!("| --- | --- | --- | --- | --- |");
    for (voice, text, samples) in &clips {
        let (heard, ms) = run(&mut state, samples)?;
        let e = word_errors(&words(text), &words(&heard));
        errors += e;
        total += words(text).len();
        times.push(ms);
        let secs = samples.len() as f64 / pcm::RATE as f64;
        per_second.push(ms as f64 / secs);
        println!(
            "| {voice} | {text} | {heard}{} | {ms} | {secs:.1} |",
            if e > 0 {
                format!(" ({e} wrong)")
            } else {
                String::new()
            }
        );
    }
    // Partial words while the owner is still speaking: the cost of a 1 s and a 2 s window.
    let long = &clips[1].2;
    let (_, one) = run(&mut state, &long[..pcm::RATE as usize])?;
    let (_, two) = run(&mut state, &long[..2 * pcm::RATE as usize])?;
    times.sort();
    let name = Path::new(&model)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    println!();
    println!(
        "{name}{}: load {load_ms} ms; per utterance median {} ms, p95 {} ms, max {} ms; {:.0} ms per second of speech; partial 1 s {one} ms, 2 s {two} ms; word error rate {:.1}% ({errors}/{total}); peak memory {:.0} MiB (before the model {:.0} MiB)",
        if hint { " with the hint" } else { "" },
        times[times.len() / 2],
        times[(times.len() * 95 / 100).min(times.len() - 1)],
        times[times.len() - 1],
        per_second.iter().sum::<f64>() / per_second.len() as f64,
        100.0 * errors as f64 / total as f64,
        peak_rss_mib(),
        before
    );
    Ok(())
}
