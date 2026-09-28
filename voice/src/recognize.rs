//! Speech to words, on the Mac. The spike (AC-162) chose whisper.cpp's small English model with
//! Overseer's vocabulary as the initial prompt: 4.5% word errors on synthesized speech at about
//! 150 ms a sentence, in about 700 MiB. The scripted recognizer stands in for it in tests and in the
//! simulated voice, where the words are known in advance.

use anyhow::Result;

/// Words Overseer always expects, whatever agents are running.
pub const VOCABULARY: &str = "Overseer, Continuity, Codex, Claude, OpenCode, worktree, redirect, merge back, pull request, swarm, grid, hold, release, allow, deny, cancel.";

pub trait Recognizer: Send {
    /// The words in `samples` (16 kHz mono), which begin `start_ms` into the stream. `hint` lists
    /// the words to expect (agent names and the vocabulary).
    fn words(&mut self, samples: &[f32], start_ms: u64, hint: &str) -> Result<String>;
    fn name(&self) -> String;
}

pub struct Whisper {
    state: whisper_rs::WhisperState,
    name: String,
    threads: i32,
}

impl Whisper {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        whisper_rs::install_logging_hooks();
        let ctx = whisper_rs::WhisperContext::new_with_params(
            path,
            whisper_rs::WhisperContextParameters::default(),
        )?;
        let state = ctx.create_state()?;
        let name = path
            .file_name()
            .map(|n| {
                n.to_string_lossy()
                    .trim_start_matches("ggml-")
                    .trim_end_matches(".bin")
                    .to_string()
            })
            .unwrap_or_default();
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(4) as i32)
            .unwrap_or(4);
        Ok(Self {
            state,
            name,
            threads,
        })
    }
}

impl Recognizer for Whisper {
    fn words(&mut self, samples: &[f32], _start_ms: u64, hint: &str) -> Result<String> {
        // Whisper wants at least a second; pad short utterances with silence.
        let mut padded;
        let samples = if samples.len() < 16_000 {
            padded = samples.to_vec();
            padded.resize(16_000, 0.0);
            &padded[..]
        } else {
            samples
        };
        let mut p =
            whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some("en"));
        p.set_n_threads(self.threads);
        p.set_no_context(true);
        p.set_single_segment(true);
        p.set_suppress_blank(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        p.set_print_special(false);
        if !hint.is_empty() {
            p.set_initial_prompt(hint);
        }
        self.state.full(p, samples)?;
        let mut text = String::new();
        for seg in self.state.as_iter() {
            if seg.no_speech_probability() > 0.6 {
                continue;
            }
            text.push_str(
                &seg.to_str_lossy()
                    .map(|c| c.into_owned())
                    .unwrap_or_default(),
            );
        }
        Ok(clean(&text))
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

/// Whisper marks sounds it heard but could not read, like "[BLANK_AUDIO]" or "(typing)": they are
/// not words.
pub fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A line of the script: these words are spoken from `start_ms` to `end_ms` in the stream.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ScriptLine {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Stands in for the recognizer when the words are known: returns the script's words that fall in
/// the audio's time, and for a line only partly covered, the share of its words said by then.
pub struct Scripted {
    pub lines: Vec<ScriptLine>,
    /// Lines added while the listener runs (the daemon's simulated voice says what it feeds).
    pub live: Option<std::sync::Arc<std::sync::Mutex<Vec<ScriptLine>>>>,
}

impl Recognizer for Scripted {
    fn words(&mut self, samples: &[f32], start_ms: u64, _hint: &str) -> Result<String> {
        if let Some(live) = &self.live {
            self.lines.append(&mut live.lock().unwrap());
        }
        let end_ms = start_ms + samples.len() as u64 * 1000 / 16_000;
        let mut out = Vec::new();
        for l in &self.lines {
            if l.end_ms <= start_ms || l.start_ms >= end_ms {
                continue;
            }
            let words: Vec<&str> = l.text.split_whitespace().collect();
            let said = if end_ms >= l.end_ms {
                words.len()
            } else {
                ((end_ms - l.start_ms) as f32 / (l.end_ms - l.start_ms).max(1) as f32
                    * words.len() as f32) as usize
            };
            out.extend(words.into_iter().take(said));
        }
        // A test hook: a scripted line can make the recognizer fail (AC-175).
        if out.iter().any(|w| *w == "<recognizer-fails>") {
            anyhow::bail!("the recognizer failed (a scripted failure)");
        }
        Ok(out.join(" "))
    }

    fn name(&self) -> String {
        "scripted".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_of_sound_are_not_words() {
        assert_eq!(clean(" [BLANK_AUDIO] "), "");
        assert_eq!(
            clean(" (typing) Stop the  phone agent."),
            "Stop the phone agent."
        );
    }

    #[test]
    fn the_script_says_its_words_as_time_passes() {
        let mut s = Scripted {
            live: None,
            lines: vec![ScriptLine {
                start_ms: 1000,
                end_ms: 3000,
                text: "tell continuity to use the new format".into(),
            }],
        };
        assert_eq!(s.words(&vec![0.0; 16_000], 0, "").unwrap(), "");
        assert_eq!(
            s.words(&vec![0.0; 32_000], 0, "").unwrap(),
            "tell continuity to"
        );
        assert_eq!(
            s.words(&vec![0.0; 64_000], 0, "").unwrap(),
            "tell continuity to use the new format"
        );
    }
}
