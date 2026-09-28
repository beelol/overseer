//! The end of a thought (AC-164). A pause is not the end: people stop to think in the middle of a
//! sentence. An utterance ends after 0.7 s of silence when its words read complete, after up to
//! 2.5 s when they read unfinished ("and", "so", "the", "um", a comma...), and at 90 s at most.
//! Silence here means the speech gate is closed, so noise in the pause does not keep it going.

use crate::gate::GateEvent;
use crate::pcm::RATE;
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct EndConfig {
    pub complete_ms: u64,
    pub unfinished_ms: u64,
    pub longest_ms: u64,
    /// When the silence reaches this, the words so far are needed to decide.
    pub check_ms: u64,
    /// Audio kept from before the gate opened, so the first syllable is in the utterance.
    pub preroll_ms: u64,
}

impl Default for EndConfig {
    fn default() -> Self {
        Self {
            complete_ms: 700,
            unfinished_ms: 2500,
            longest_ms: 90_000,
            check_ms: 450,
            preroll_ms: 300,
        }
    }
}

/// Words that leave a sentence open when they come last.
const OPEN_ENDINGS: &[&str] = &[
    "and", "or", "but", "so", "because", "the", "a", "an", "to", "of", "for", "with", "that", "if",
    "then", "um", "uh", "er", "like", "also", "when", "which", "who", "is", "are", "my", "your",
    "their", "our",
];

/// Whether the words read unfinished.
pub fn reads_unfinished(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if t.ends_with(',')
        || t.ends_with("...")
        || t.ends_with('…')
        || t.ends_with('-')
        || t.ends_with('—')
    {
        return true;
    }
    let last = t
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    OPEN_ENDINGS.contains(&last.as_str())
}

#[derive(Clone, Debug, PartialEq)]
pub enum EndAction {
    /// A new utterance began.
    Started { id: u64, start_ms: u64 },
    /// The words so far are needed now (the silence reached `check_ms`).
    NeedWords { id: u64 },
    /// The utterance ended, with its audio for the final words. `complete` says whether the words
    /// known so far read complete.
    Ended {
        id: u64,
        start_ms: u64,
        end_ms: u64,
        complete: bool,
        audio: Vec<f32>,
        /// Where `audio` starts: after the parts already handed off (a long utterance).
        audio_start_ms: u64,
    },
}

struct Utterance {
    id: u64,
    start: u64,
    audio: Vec<f32>,
    /// Samples since the gate closed; 0 while it is open.
    silence: u64,
    asked: bool,
    words: Option<String>,
    /// Samples at the front already handed off for words (AC-173: 30 s held at most).
    taken: u64,
}

pub struct Endpointer {
    cfg: EndConfig,
    preroll: VecDeque<f32>,
    current: Option<Utterance>,
    next_id: u64,
}

fn ms(samples: u64) -> u64 {
    samples * 1000 / RATE as u64
}

impl Endpointer {
    pub fn new(cfg: EndConfig) -> Self {
        Self {
            cfg,
            preroll: VecDeque::new(),
            current: None,
            next_id: 1,
        }
    }

    pub fn current(&self) -> Option<u64> {
        self.current.as_ref().map(|u| u.id)
    }

    /// The audio of the current utterance so far.
    pub fn audio(&self) -> &[f32] {
        self.current
            .as_ref()
            .map(|u| u.audio.as_slice())
            .unwrap_or(&[])
    }

    /// Hands off the first `n` samples of a long utterance, which the endpointer then forgets:
    /// the utterance's id, where they start, and the samples.
    pub fn take_front(&mut self, n: usize) -> Option<(u64, u64, Vec<f32>)> {
        let u = self.current.as_mut()?;
        let n = n.min(u.audio.len());
        let start = ms(u.start + u.taken);
        let front: Vec<f32> = u.audio.drain(..n).collect();
        u.taken += n as u64;
        Some((u.id, start, front))
    }

    /// The words recognized for the current utterance, for deciding whether they read complete.
    pub fn set_words(&mut self, id: u64, text: &str) {
        if let Some(u) = self.current.as_mut().filter(|u| u.id == id) {
            u.words = Some(text.to_string());
        }
    }

    /// Feeds one frame. `now` is the stream position (in samples) at the end of the frame.
    pub fn push(
        &mut self,
        frame: &[f32],
        gate: Option<GateEvent>,
        open: bool,
        now: u64,
    ) -> Vec<EndAction> {
        let mut out = Vec::new();
        match (&mut self.current, gate) {
            (None, Some(GateEvent::Open { lead })) => {
                let keep = (lead + (self.cfg.preroll_ms as usize * RATE as usize / 1000))
                    .min(self.preroll.len());
                let mut audio: Vec<f32> = self
                    .preroll
                    .iter()
                    .skip(self.preroll.len() - keep)
                    .copied()
                    .collect();
                audio.extend_from_slice(frame);
                let start = now.saturating_sub(audio.len() as u64);
                let id = self.next_id;
                self.next_id += 1;
                self.current = Some(Utterance {
                    id,
                    start,
                    audio,
                    silence: 0,
                    asked: false,
                    words: None,
                    taken: 0,
                });
                out.push(EndAction::Started {
                    id,
                    start_ms: ms(start),
                });
            }
            (Some(u), _) => {
                u.audio.extend_from_slice(frame);
                if open {
                    u.silence = 0;
                    u.asked = false;
                    u.words = None;
                } else {
                    u.silence += frame.len() as u64;
                }
                let silence = ms(u.silence);
                let length = ms(u.audio.len() as u64 + u.taken);
                if silence >= self.cfg.check_ms && !u.asked {
                    u.asked = true;
                    out.push(EndAction::NeedWords { id: u.id });
                }
                let complete = u.words.as_deref().map(|w| !reads_unfinished(w));
                let end =
                    (u.silence > 0 && silence >= self.cfg.complete_ms && complete == Some(true))
                        || (u.silence > 0 && silence >= self.cfg.unfinished_ms)
                        || length >= self.cfg.longest_ms;
                if end {
                    let u = self.current.take().unwrap();
                    // Keep 300 ms of the silence at the end, no more.
                    let extra = u.silence.saturating_sub(RATE as u64 * 3 / 10);
                    let mut audio = u.audio;
                    audio.truncate(audio.len().saturating_sub(extra as usize));
                    out.push(EndAction::Ended {
                        id: u.id,
                        start_ms: ms(u.start),
                        end_ms: ms(now - extra),
                        complete: complete.unwrap_or(true),
                        audio,
                        audio_start_ms: ms(u.start + u.taken),
                    });
                    if open && length >= self.cfg.longest_ms {
                        // Still speaking at 90 s: go on in a new utterance.
                        let id = self.next_id;
                        self.next_id += 1;
                        self.current = Some(Utterance {
                            id,
                            start: now,
                            audio: Vec::new(),
                            silence: 0,
                            asked: false,
                            words: None,
                            taken: 0,
                        });
                        out.push(EndAction::Started {
                            id,
                            start_ms: ms(now),
                        });
                    }
                }
            }
            _ => {}
        }
        let keep = self.cfg.preroll_ms as usize * RATE as usize / 1000 + RATE as usize / 2;
        self.preroll.extend(frame.iter().copied());
        while self.preroll.len() > keep {
            self.preroll.pop_front();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcm::FRAME;

    #[test]
    fn open_endings() {
        assert!(reads_unfinished("Tell Continuity and"));
        assert!(reads_unfinished("so the"));
        assert!(reads_unfinished("Phone should, um"));
        assert!(reads_unfinished("wait,"));
        assert!(!reads_unfinished(
            "Tell Continuity to use the new wire format."
        ));
        assert!(!reads_unfinished("Stop the phone agent"));
        assert!(!reads_unfinished(""));
    }

    /// Feeds `plan` as (gate open?, seconds) and returns the actions with their times.
    fn drive(
        e: &mut Endpointer,
        plan: &[(bool, f32)],
        words: Option<&str>,
    ) -> Vec<(u64, EndAction)> {
        let mut now = 0u64;
        let mut out = Vec::new();
        let mut was_open = false;
        let frame = vec![0.0f32; FRAME];
        for &(open, secs) in plan {
            for _ in 0..(secs * 50.0) as usize {
                now += FRAME as u64;
                let ev = match (was_open, open) {
                    (false, true) => Some(GateEvent::Open { lead: 0 }),
                    (true, false) => Some(GateEvent::Close),
                    _ => None,
                };
                was_open = open;
                for a in e.push(&frame, ev, open, now) {
                    if let (EndAction::NeedWords { id }, Some(w)) = (&a, words) {
                        e.set_words(*id, w);
                    }
                    out.push((ms(now), a));
                }
            }
        }
        out
    }

    fn ended(actions: &[(u64, EndAction)]) -> Vec<(u64, bool)> {
        actions
            .iter()
            .filter_map(|(t, a)| {
                if let EndAction::Ended { complete, .. } = a {
                    Some((*t, *complete))
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn complete_words_end_after_700_ms_of_silence() {
        let mut e = Endpointer::new(EndConfig::default());
        let a = drive(
            &mut e,
            &[(false, 0.5), (true, 2.0), (false, 1.5)],
            Some("Stop the phone agent."),
        );
        assert_eq!(ended(&a), vec![(3200, true)]);
    }

    #[test]
    fn unfinished_words_wait_up_to_2_5_s() {
        let mut e = Endpointer::new(EndConfig::default());
        let a = drive(
            &mut e,
            &[(false, 0.5), (true, 2.0), (false, 3.0)],
            Some("Tell Continuity and"),
        );
        assert_eq!(ended(&a), vec![(5000, false)]);
    }

    #[test]
    fn a_pause_in_mid_thought_is_one_utterance() {
        // "Tell Continuity and" ... 1.2 s ... "the phone agent to stop."
        let mut e = Endpointer::new(EndConfig::default());
        let a = drive(
            &mut e,
            &[(false, 0.5), (true, 1.5), (false, 1.2), (true, 1.5)],
            Some("Tell Continuity and"),
        );
        assert!(ended(&a).is_empty(), "still one utterance: {a:?}");
        let started = a
            .iter()
            .filter(|(_, a)| matches!(a, EndAction::Started { .. }))
            .count();
        assert_eq!(started, 1);
        let b = drive(
            &mut e,
            &[(false, 1.0)],
            Some("Tell Continuity and the phone agent to stop."),
        );
        assert_eq!(ended(&b).len(), 1);
    }

    #[test]
    fn words_not_known_yet_wait_for_them() {
        // The recognizer has not answered: not ended at 0.7 s, ended at the longest wait.
        let mut e = Endpointer::new(EndConfig::default());
        let a = drive(&mut e, &[(false, 0.5), (true, 1.0), (false, 3.0)], None);
        assert_eq!(ended(&a), vec![(4000, true)]);
    }

    #[test]
    fn ninety_seconds_is_the_longest() {
        let mut e = Endpointer::new(EndConfig::default());
        let a = drive(&mut e, &[(true, 95.0)], None);
        assert_eq!(ended(&a).len(), 1);
        assert_eq!(
            a.iter()
                .filter(|(_, a)| matches!(a, EndAction::Started { .. }))
                .count(),
            2,
            "goes on in a new one"
        );
    }
}
