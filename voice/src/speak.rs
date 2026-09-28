//! Overseer's voice. Lines are spoken with an installed macOS voice through `say`, one phrase at a
//! time, so Overseer can stop at the end of a phrase. The listener plays them itself: it then knows
//! exactly what the speakers play (for the echo guard and the voice-processing unit) and can lower
//! or stop its voice at once (AC-164).

use crate::pcm::{self, FRAME};
use anyhow::{bail, Result};
use std::collections::VecDeque;
use std::process::Command;

/// Overseer's voice while the owner speaks over it: about 10 dB lower.
pub const LOWERED: f32 = 0.3;

/// Speaks `text` into samples, in memory, with the system's voices: no file is written at any
/// point (AC-173). This is Overseer's own voice, never the owner's.
///
/// A short line takes 100 to 300 ms on an idle Mac and seconds on a busy one, so the lines
/// Overseer says most are made when the listener starts (`COMMON`) and play at once. The speech
/// service sometimes never finishes; the line is then tried once more.
pub fn synthesize(text: &str, voice: Option<&str>, rate: Option<u32>) -> Result<Vec<f32>> {
    match crate::memspeech::speak(text, voice, rate) {
        Ok(a) => Ok(a),
        Err(_) => crate::memspeech::speak(text, voice, rate),
    }
}

/// Runs `say` into a temporary WAV file and returns its samples: for tests and the spike, which
/// make speech at test time in a temporary folder (AC-164). macOS's speech service sometimes
/// never answers, so `say` gets a time limit that grows with the text (20 s and 100 ms a
/// character: generous, because a busy Mac makes it slow without making it stuck), is killed when
/// it passes it, and is tried once more.
pub fn say(text: &str, voice: Option<&str>, rate: Option<u32>) -> Result<Vec<f32>> {
    let dir = tempfile::Builder::new().prefix("ovs-voice-").tempdir()?;
    let path = dir.path().join("line.wav");
    let limit = std::time::Duration::from_millis(20_000 + 100 * text.chars().count() as u64);
    for _ in 0..2 {
        let mut cmd = Command::new("say");
        if let Some(v) = voice {
            cmd.args(["-v", v]);
        }
        if let Some(r) = rate {
            cmd.args(["-r", &r.to_string()]);
        }
        cmd.arg("-o")
            .arg(&path)
            .arg("--data-format=LEI16@16000")
            .arg("--")
            .arg(text);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn()?;
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    bail!("say could not speak the line");
                }
                return pcm::read_wav(&std::fs::read(&path)?);
            }
            if started.elapsed() > limit {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    bail!("say did not finish twice in a row")
}

/// Lines Overseer says often, made in advance.
pub const COMMON: &[&str] = &[
    "On it.",
    "Working on it.",
    "Sent.",
    "Cancelled.",
    "Not sent.",
    "Allowed.",
    "Denied.",
    "Stopped.",
    "Go on.",
    "Muted.",
    "Listening.",
];

/// Splits a line into phrases at sentence and clause ends; a fragment of one or two words joins
/// the phrase before it.
pub fn phrases(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let boundary = matches!(c, '.' | '?' | '!' | ';' | ':' | ',' | '—')
            && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        if boundary {
            let p = cur.trim().to_string();
            if !p.is_empty() {
                if p.split_whitespace().count() <= 2
                    && !out.is_empty()
                    && !p.ends_with(['.', '?', '!'])
                {
                    let last = out.last_mut().unwrap();
                    last.push(' ');
                    last.push_str(&p);
                } else {
                    out.push(p);
                }
            }
            cur.clear();
        }
    }
    let rest = cur.trim();
    if !rest.is_empty() {
        if rest.split_whitespace().count() <= 2 && !out.is_empty() {
            let last = out.last_mut().unwrap();
            last.push(' ');
            last.push_str(rest);
        } else {
            out.push(rest.to_string());
        }
    }
    out
}

/// What the speaker reports.
#[derive(Clone, Debug, PartialEq)]
pub enum SpeakEvent {
    Start {
        line: u64,
    },
    Lowered {
        line: u64,
        phrase: usize,
    },
    Restored {
        line: u64,
        phrase: usize,
    },
    Stopped {
        line: u64,
        phrase: usize,
    },
    /// A phrase ended and the next one begins: a moment a cue may play (AC-172).
    Phrase {
        line: u64,
        phrase: usize,
    },
    Done {
        line: u64,
    },
}

struct Line {
    id: u64,
    phrases: Vec<String>,
    audio: Vec<Option<Vec<f32>>>,
}

struct Playing {
    line: Line,
    phrase: usize,
    pos: usize,
    started: bool,
}

/// Plays queued lines phrase by phrase. Synthesis happens elsewhere (`synthesize`, on a worker or
/// inline); the speaker asks for it through `jobs` and receives it through `synthesized`.
#[derive(Default)]
pub struct Speaker {
    queue: VecDeque<Line>,
    playing: Option<Playing>,
    lowered: bool,
    stop_after_phrase: bool,
    /// The words of what was said last, and how many samples ago it ended (for the echo guard).
    recent: Vec<String>,
    since_done: usize,
    peak_db: f32,
}

impl Speaker {
    pub fn new() -> Self {
        Self {
            peak_db: -100.0,
            since_done: usize::MAX / 2,
            ..Default::default()
        }
    }

    /// Queues a line; returns the phrases to synthesize, as (line, phrase, text).
    pub fn queue(&mut self, id: u64, text: &str) -> Vec<(u64, usize, String)> {
        let ph = phrases(text);
        let jobs = ph
            .iter()
            .enumerate()
            .map(|(i, p)| (id, i, p.clone()))
            .collect();
        self.queue.push_back(Line {
            id,
            audio: vec![None; ph.len()],
            phrases: ph,
        });
        jobs
    }

    pub fn synthesized(&mut self, line: u64, phrase: usize, audio: Vec<f32>) {
        let slot = self
            .playing
            .as_mut()
            .filter(|p| p.line.id == line)
            .map(|p| &mut p.line)
            .or_else(|| self.queue.iter_mut().find(|l| l.id == line));
        if let Some(l) = slot {
            if let Some(a) = l.audio.get_mut(phrase) {
                *a = Some(audio);
            }
        }
    }

    pub fn speaking(&self) -> bool {
        self.playing.as_ref().is_some_and(|p| p.started)
    }

    pub fn is_lowered(&self) -> bool {
        self.lowered
    }

    /// The line and phrase playing now.
    pub fn current(&self) -> Option<(u64, usize)> {
        self.playing
            .as_ref()
            .filter(|p| p.started)
            .map(|p| (p.line.id, p.phrase))
    }

    /// Words Overseer is saying or said in the last 1.5 s: whatever the recognizer hears of these
    /// is Overseer's own voice coming back, not the owner.
    pub fn recent_words(&self) -> Vec<String> {
        let mut words = if self.since_done < 24_000 {
            self.recent.clone()
        } else {
            Vec::new()
        };
        if let Some(p) = &self.playing {
            for ph in &p.line.phrases {
                words.extend(crate::words::normalize(ph));
            }
        }
        words
    }

    pub fn lower(&mut self) -> Option<SpeakEvent> {
        let (line, phrase) = self.current()?;
        if self.lowered {
            return None;
        }
        self.lowered = true;
        Some(SpeakEvent::Lowered { line, phrase })
    }

    pub fn restore(&mut self) -> Option<SpeakEvent> {
        if !self.lowered {
            return None;
        }
        self.lowered = false;
        self.stop_after_phrase = false;
        self.current()
            .map(|(line, phrase)| SpeakEvent::Restored { line, phrase })
    }

    /// Stops now: the rest of the line and everything queued after it are dropped.
    pub fn stop_now(&mut self, line: Option<u64>) -> Vec<SpeakEvent> {
        let mut out = Vec::new();
        if let Some(p) = self.playing.take() {
            if line.is_none_or(|l| l == p.line.id) {
                if p.started {
                    self.remember(&p.line);
                    out.push(SpeakEvent::Stopped {
                        line: p.line.id,
                        phrase: p.phrase,
                    });
                }
                self.queue.clear();
            } else {
                self.playing = Some(p);
                self.queue.retain(|l| Some(l.id) != line);
            }
        } else {
            self.queue.retain(|l| line.is_some_and(|x| x != l.id));
        }
        self.lowered = false;
        self.stop_after_phrase = false;
        out
    }

    /// Stops at the end of the phrase playing now.
    pub fn stop_at_phrase(&mut self) {
        if self.playing.is_some() {
            self.stop_after_phrase = true;
        }
    }

    fn remember(&mut self, line: &Line) {
        self.recent = line
            .phrases
            .iter()
            .flat_map(|p| crate::words::normalize(p))
            .collect();
        self.since_done = 0;
    }

    /// The next `n` samples of Overseer's voice (silence when nothing plays), what happened, and
    /// the level of what played (0 to 1), or None when nothing played.
    pub fn next(&mut self, n: usize) -> (Vec<f32>, Vec<SpeakEvent>, Option<f32>) {
        let mut out = vec![0.0f32; n];
        let mut events = Vec::new();
        let mut filled = 0;
        self.since_done = self.since_done.saturating_add(n);
        while filled < n {
            if self.playing.is_none() {
                match self.queue.pop_front() {
                    Some(line) => {
                        self.playing = Some(Playing {
                            line,
                            phrase: 0,
                            pos: 0,
                            started: false,
                        })
                    }
                    None => break,
                }
            }
            let p = self.playing.as_mut().unwrap();
            let Some(audio) = p.line.audio.get(p.phrase).and_then(|a| a.as_ref()) else {
                break; // the phrase is not synthesized yet
            };
            if !p.started {
                p.started = true;
                events.push(SpeakEvent::Start { line: p.line.id });
            }
            let take = (audio.len() - p.pos).min(n - filled);
            let gain = if self.lowered { LOWERED } else { 1.0 };
            for i in 0..take {
                out[filled + i] = audio[p.pos + i] * gain;
            }
            filled += take;
            p.pos += take;
            if p.pos >= audio.len() {
                let last = p.phrase + 1 >= p.line.phrases.len();
                if last || self.stop_after_phrase {
                    let p = self.playing.take().unwrap();
                    self.remember(&p.line);
                    if last {
                        events.push(SpeakEvent::Done { line: p.line.id });
                    } else {
                        events.push(SpeakEvent::Stopped {
                            line: p.line.id,
                            phrase: p.phrase,
                        });
                        self.queue.clear();
                    }
                    self.lowered = false;
                    self.stop_after_phrase = false;
                } else {
                    events.push(SpeakEvent::Phrase {
                        line: p.line.id,
                        phrase: p.phrase,
                    });
                    p.phrase += 1;
                    p.pos = 0;
                }
            }
        }
        let level = if filled > 0 {
            let rms = (out.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
            let db = 20.0 * (rms + 1e-9).log10();
            self.peak_db = self.peak_db.max(db) - 6.0 / 50.0 * (n as f32 / FRAME as f32);
            self.peak_db = self.peak_db.max(db);
            Some(
                ((db - (self.peak_db - 30.0)) / 30.0)
                    .clamp(0.0, 1.0)
                    .powf(2.2),
            )
        } else {
            None
        };
        (out, events, level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrases_split_at_clause_ends() {
        assert_eq!(
            phrases("On it: telling Phone and Continuity, and starting one agent for the note."),
            vec![
                "On it:",
                "telling Phone and Continuity,",
                "and starting one agent for the note."
            ]
        );
        assert_eq!(phrases("Sent."), vec!["Sent."]);
        assert_eq!(
            phrases("Stopped. Phone picked it up, Continuity gets it after this turn."),
            vec![
                "Stopped.",
                "Phone picked it up,",
                "Continuity gets it after this turn."
            ]
        );
    }

    fn tone(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.1).sin() * 0.5).collect()
    }

    #[test]
    fn a_line_plays_phrase_by_phrase_and_stops_at_the_phrase_end() {
        let mut s = Speaker::new();
        let jobs = s.queue(1, "First part, second part here. Third part now.");
        assert_eq!(jobs.len(), 3);
        for (l, p, _) in &jobs {
            s.synthesized(*l, *p, tone(FRAME * 3));
        }
        let (_, ev, level) = s.next(FRAME);
        assert_eq!(ev, vec![SpeakEvent::Start { line: 1 }]);
        assert!(level.unwrap() > 0.0);
        s.stop_at_phrase();
        let mut all = Vec::new();
        for _ in 0..10 {
            all.extend(s.next(FRAME).1);
        }
        assert_eq!(all, vec![SpeakEvent::Stopped { line: 1, phrase: 0 }]);
        assert!(!s.speaking());
    }

    #[test]
    fn lowering_scales_the_voice_and_restoring_brings_it_back() {
        let mut s = Speaker::new();
        s.queue(7, "A long enough line to lower.");
        s.synthesized(7, 0, tone(FRAME * 20));
        let (full, _, _) = s.next(FRAME);
        assert!(matches!(
            s.lower(),
            Some(SpeakEvent::Lowered { line: 7, .. })
        ));
        let (low, _, _) = s.next(FRAME);
        let peak = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak(&low) < peak(&full) * 0.35);
        assert!(matches!(
            s.restore(),
            Some(SpeakEvent::Restored { line: 7, .. })
        ));
        let (back, _, _) = s.next(FRAME);
        assert!(peak(&back) > peak(&full) * 0.9);
    }

    #[test]
    fn stopping_now_drops_the_rest_and_the_queue() {
        let mut s = Speaker::new();
        s.queue(1, "One line.");
        s.queue(2, "Another line.");
        s.synthesized(1, 0, tone(FRAME * 10));
        s.synthesized(2, 0, tone(FRAME * 10));
        s.next(FRAME);
        assert_eq!(
            s.stop_now(None),
            vec![SpeakEvent::Stopped { line: 1, phrase: 0 }]
        );
        assert!(s.next(FRAME).1.is_empty());
        assert!(!s.speaking());
        assert!(
            s.recent_words().contains(&"line".to_string()),
            "remembered for the echo guard"
        );
    }
}
