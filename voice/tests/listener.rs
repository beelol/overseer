//! The listener end to end, on the stream's own clock (AC-164, AC-173, AC-177): a made-up voice or
//! `say` speech in, events out. Words come from a script, so no model is needed.

use overseer_listener::listener::{self, Input, Options, Pace};
use overseer_listener::pcm;
use overseer_listener::protocol::{Command, Event, Source};
use overseer_listener::recognize::{ScriptLine, Scripted};
use overseer_listener::synth;
use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Overseer's voice in tests: a made-up voice, one second a phrase.
fn test_voice(_text: &str) -> Vec<f32> {
    synth::speechlike(1.0, 0.4)
}

struct Run {
    events: Vec<Event>,
}

impl Run {
    fn count(&self, f: impl Fn(&Event) -> bool) -> usize {
        self.events.iter().filter(|e| f(e)).count()
    }
    fn utterances(&self) -> Vec<(String, bool)> {
        self.events
            .iter()
            .filter_map(|e| {
                if let Event::Utterance { text, complete, .. } = e {
                    Some((text.clone(), *complete))
                } else {
                    None
                }
            })
            .collect()
    }
    fn spoke(&self) -> Vec<String> {
        self.events
            .iter()
            .filter_map(|e| {
                if let Event::Spoke { event, .. } = e {
                    Some(event.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

fn listen(audio: &[f32], script: Vec<ScriptLine>, timed: Vec<(u64, Command)>, echo: f32) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("in.wav");
    std::fs::write(&path, pcm::wav_bytes(audio)).unwrap();
    let sink = Sink::default();
    let opts = Options {
        input: Input::File(path),
        pace: Pace::Fast,
        control: false,
        timed,
        echo,
        synth: Some(test_voice),
        ..Default::default()
    };
    listener::run(
        opts,
        Some(Box::new(Scripted { lines: script })),
        sink.clone(),
    )
    .unwrap();
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    let events = text
        .lines()
        .map(|l| serde_json::from_str::<Event>(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect();
    Run { events }
}

fn line(start_ms: u64, end_ms: u64, text: &str) -> ScriptLine {
    ScriptLine {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

fn voice_at(total: f32, parts: &[(f32, f32)]) -> Vec<f32> {
    let mut s = synth::room(total, 0.002, 1);
    for &(at, dur) in parts {
        synth::mix(&mut s, &synth::speechlike(dur, 0.5), synth::secs(at));
    }
    s
}

#[test]
fn a_sentence_becomes_one_complete_utterance_with_levels_only_while_speaking() {
    let audio = voice_at(5.0, &[(1.0, 2.0)]);
    let r = listen(
        &audio,
        vec![line(
            1000,
            3000,
            "Tell Continuity to use the new wire format.",
        )],
        vec![],
        0.0,
    );
    assert!(matches!(r.events.first(), Some(Event::Ready { .. })));
    assert_eq!(
        r.utterances(),
        vec![(
            "Tell Continuity to use the new wire format.".to_string(),
            true
        )]
    );
    assert!(
        r.count(|e| matches!(e, Event::Words { .. })) >= 1,
        "words while speaking"
    );
    let levels: Vec<u64> = r
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Level {
                source: Source::Owner,
                t_ms,
                ..
            } = e
            {
                Some(*t_ms)
            } else {
                None
            }
        })
        .collect();
    assert!(!levels.is_empty());
    assert!(
        levels.iter().all(|t| (1000..=3400).contains(t)),
        "the owner's level only while the gate is open: {levels:?}"
    );
    let per_second = levels.len() as f32 / 2.4;
    assert!(per_second <= 30.0, "{per_second} levels a second");
    assert!(matches!(r.events.last(), Some(Event::End { .. })));
}

#[test]
fn noise_alone_gives_no_gate_no_level_and_no_words() {
    let mut audio = Vec::new();
    for (i, kind) in synth::NOISES.iter().enumerate() {
        audio.extend(synth::noise(kind, i as u64, 1.0).unwrap());
    }
    let r = listen(
        &audio,
        vec![line(0, 60_000, "this would be words if the gate opened")],
        vec![],
        0.0,
    );
    assert_eq!(r.count(|e| matches!(e, Event::Gate { .. })), 0);
    assert_eq!(r.count(|e| matches!(e, Event::Level { .. })), 0);
    assert_eq!(
        r.count(|e| matches!(e, Event::Utterance { .. } | Event::Words { .. })),
        0
    );
}

#[test]
fn a_pause_in_mid_thought_stays_one_utterance() {
    // "Tell Continuity and" ... 1.2 s ... "the phone agent to stop."
    let audio = voice_at(7.0, &[(0.5, 1.5), (3.2, 1.5)]);
    let r = listen(
        &audio,
        vec![
            line(500, 2000, "Tell Continuity and"),
            line(3200, 4700, "the phone agent to stop."),
        ],
        vec![],
        0.0,
    );
    assert_eq!(
        r.utterances(),
        vec![(
            "Tell Continuity and the phone agent to stop.".to_string(),
            true
        )]
    );
}

#[test]
fn talking_over_overseer_lowers_its_voice_and_it_comes_back() {
    // Overseer speaks a three-phrase line from 0.2 s; the owner says four words at 1.2 s.
    let audio = voice_at(7.0, &[(1.2, 1.2)]);
    let r = listen(
        &audio,
        vec![line(1200, 2400, "no just the phone")],
        vec![(200, Command::Speak { line: 1, text: "Telling Phone to switch, and Continuity to wait, and starting one agent, for the migration note, right now.".into() })],
        0.0,
    );
    let spoke = r.spoke();
    assert_eq!(spoke.first().map(String::as_str), Some("start"));
    assert!(spoke.contains(&"lowered".to_string()), "{spoke:?}");
    let lowered = spoke.iter().position(|e| e == "lowered").unwrap();
    assert_eq!(
        spoke.get(lowered + 1).map(String::as_str),
        Some("restored"),
        "nobody said stop, so it comes back: {spoke:?}"
    );
    assert_eq!(spoke.last().map(String::as_str), Some("done"));
    assert_eq!(
        r.count(|e| matches!(
            e,
            Event::Barge {
                stop_word: false,
                ..
            }
        )),
        1
    );
    assert_eq!(
        r.utterances().len(),
        1,
        "the owner's words still reach the daemon"
    );
}

#[test]
fn stop_stops_overseer_at_once() {
    let audio = voice_at(6.0, &[(1.2, 0.6)]);
    let r = listen(
        &audio,
        vec![line(1200, 1800, "stop")],
        vec![(
            200,
            Command::Speak {
                line: 4,
                text: "A long line, with many phrases, that goes on, and on, and on.".into(),
            },
        )],
        0.0,
    );
    let stopped = r.events.iter().find_map(|e| {
        if let Event::Spoke { event, t_ms, .. } = e {
            (event == "stopped").then_some(*t_ms)
        } else {
            None
        }
    });
    let t = stopped.expect("stopped");
    assert!(
        t <= 1800 + 300,
        "stopped at {t} ms: \"stop\" ended at 1800 ms"
    );
    assert_eq!(
        r.count(|e| matches!(
            e,
            Event::Barge {
                stop_word: true,
                ..
            }
        )),
        1
    );
    assert!(!r.spoke().contains(&"done".to_string()));
}

#[test]
fn overseer_s_own_voice_coming_back_is_not_the_owner() {
    // No echo cancellation: Overseer's voice comes back at 60% and opens the gate, and the
    // recognizer hears Overseer's own words. They are dropped as echo, and nothing is lowered.
    let audio = synth::room(6.0, 0.002, 3);
    let spoken = "Telling Phone and Continuity to use the new wire format.";
    let r = listen(
        &audio,
        vec![line(200, 2400, "telling phone and continuity to use")],
        vec![(
            200,
            Command::Speak {
                line: 2,
                text: spoken.into(),
            },
        )],
        0.6,
    );
    assert!(
        r.count(|e| matches!(e, Event::Gate { open: true, .. })) >= 1,
        "the echo is loud enough to open the gate"
    );
    assert_eq!(r.count(|e| matches!(e, Event::Barge { .. })), 0);
    assert!(!r.spoke().contains(&"lowered".to_string()));
    assert_eq!(r.utterances().len(), 0);
    assert!(r.count(|e| matches!(e, Event::Dropped { reason, .. } if reason == "echo")) >= 1);
}

#[test]
fn typing_and_backchannels_while_overseer_speaks_change_nothing() {
    let mut audio = synth::room(8.0, 0.002, 4);
    synth::mix(
        &mut audio,
        &synth::clicks(3.0, 8.0, 0.3, 5),
        synth::secs(0.5),
    );
    synth::mix(&mut audio, &synth::speechlike(0.7, 0.5), synth::secs(4.0));
    let r = listen(
        &audio,
        vec![line(4000, 4700, "mm-hm yeah")],
        vec![(
            200,
            Command::Speak {
                line: 3,
                text: "One phrase here, two phrases here, three phrases here, four, and five."
                    .into(),
            },
        )],
        0.0,
    );
    let spoke = r.spoke();
    assert!(!spoke.contains(&"lowered".to_string()), "{spoke:?}");
    assert!(!spoke.contains(&"stopped".to_string()));
    assert_eq!(spoke.last().map(String::as_str), Some("done"));
}

#[test]
fn a_suppressed_moment_is_not_heard() {
    // A cue plays at 1 s: the daemon asks the listener to ignore 1.5 s of input.
    let audio = voice_at(5.0, &[(1.1, 1.0)]);
    let r = listen(
        &audio,
        vec![line(1100, 2100, "this is the cue")],
        vec![(1000, Command::Suppress { ms: 1500 })],
        0.0,
    );
    assert_eq!(
        r.count(|e| matches!(e, Event::Gate { .. } | Event::Utterance { .. })),
        0
    );
}

#[test]
fn real_speech_from_say_is_heard_as_one_utterance() {
    let Ok(speech) = overseer_listener::speak::say("Stop the phone agent, and tell Continuity to wait.", Some("Samantha"), None) else {
        eprintln!("skipped: no `say`");
        return;
    };
    let mut audio = synth::room(0.8, 0.002, 6);
    let dur = speech.len() as u64 * 1000 / 16_000;
    audio.extend(speech);
    audio.extend(synth::room(3.0, 0.002, 7));
    let r = listen(
        &audio,
        vec![line(
            800,
            800 + dur,
            "Stop the phone agent, and tell Continuity to wait.",
        )],
        vec![],
        0.0,
    );
    assert_eq!(
        r.utterances(),
        vec![(
            "Stop the phone agent, and tell Continuity to wait.".to_string(),
            true
        )]
    );
}
