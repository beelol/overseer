//! What the listener and the daemon say to each other: one JSON object per line. Events go out on
//! standard output, commands come in on standard input. Audio never goes out; it comes in only in
//! feed mode (the simulated voice), as 16-bit PCM in base64.

use serde::{Deserialize, Serialize};

/// Whose voice a level is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Owner,
    Overseer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Listening: the model is loaded and the input is open.
    Ready {
        input: String,
        model: String,
        t_ms: u64,
    },
    /// The speech gate opened or closed.
    Gate {
        open: bool,
        t_ms: u64,
    },
    /// One number for the mark: how loud, 0 to 1. The owner's only while the gate is open.
    Level {
        source: Source,
        value: f32,
        t_ms: u64,
    },
    /// Words heard so far in an utterance that is still going on.
    Words {
        id: u64,
        text: String,
        t_ms: u64,
    },
    /// The end of a thought: the words of one utterance. `complete` is false when it was cut at
    /// the longest wait (2.5 s) or the longest utterance (90 s) with the words reading unfinished.
    Utterance {
        id: u64,
        text: String,
        complete: bool,
        start_ms: u64,
        end_ms: u64,
        t_ms: u64,
    },
    /// Words dropped before they reached the daemon, and why ("echo": Overseer's own voice;
    /// "empty": sound without words).
    Dropped {
        id: u64,
        text: String,
        reason: String,
        t_ms: u64,
    },
    /// The owner spoke two or more words over Overseer: its voice was lowered (or stopped, for a
    /// stop word). The daemon decides whether it stops at the end of its phrase or goes on.
    Barge {
        id: u64,
        text: String,
        stop_word: bool,
        t_ms: u64,
    },
    /// Overseer's voice: "start", "phrase" (one phrase ended, the next begins), "lowered",
    /// "restored", "stopped", "done".
    Spoke {
        line: u64,
        event: String,
        phrase: usize,
        t_ms: u64,
    },
    Error {
        message: String,
        t_ms: u64,
    },
    /// The input ended (a file or a closed feed).
    End {
        t_ms: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Speak a line. Lines play one after another; each is split into phrases.
    Speak {
        line: u64,
        text: String,
    },
    /// Back to full voice after a barge that was not meant for Overseer.
    Restore,
    /// Stop speaking: "now", or at the end of the current phrase. Without `line`, whatever plays;
    /// queued lines are dropped too.
    Stop {
        line: Option<u64>,
        at: Option<String>,
    },
    /// Words the recognizer should expect: agent names, repositories, Overseer's vocabulary.
    Hint {
        text: String,
    },
    /// A cue is about to play through the speakers: ignore the input for this long.
    Suppress {
        ms: u64,
    },
    /// Feed mode: 16-bit little-endian mono PCM at 16 kHz, base64.
    Feed {
        pcm: String,
    },
    /// Feed mode: the simulated input has ended.
    FeedEnd,
    /// Simulated voice: the words that go with audio being fed, for a listener started with
    /// `--script-live`.
    Script {
        lines: Vec<crate::recognize::ScriptLine>,
    },
    Quit,
}
