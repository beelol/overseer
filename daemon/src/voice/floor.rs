//! Who has the floor (AC-164) and one speaker at a time (AC-172).
//!
//! The listener lowers Overseer's voice by itself when the owner speaks two or more words over it
//! and stops it for "stop", "wait" and "hold on". The daemon decides the rest: if the owner is still
//! speaking 0.7 s later and the words are meant for Overseer, Overseer stops at the end of its
//! phrase; otherwise it goes on, and the listener brings its voice back when the owner's words end.
//!
//! The arbiter keeps Audio Mode's cues and Overseer's voice from overlapping: a cue waits for the
//! end of Overseer's phrase; while the owner speaks a routine cue is dropped and an attention cue
//! waits for the end of the thought, 5 s at most. The listener ignores its microphone while a cue
//! plays, so a cue is never heard as the owner.

use super::Voice;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Barge {
    pub utterance: u64,
}

/// Words that open a command.
const COMMAND_VERBS: &[&str] = &[
    "tell", "ask", "stop", "start", "hold", "release", "redirect", "cancel", "pause", "resume",
    "mute", "unmute", "allow", "deny", "merge", "open", "pin", "show", "switch", "talk", "go",
    "have", "make", "let", "send", "give", "check", "run", "fix", "add", "write", "remove", "wait",
    "undo", "overseer",
];

pub fn is_command_verb(w: &str) -> bool {
    COMMAND_VERBS.contains(&w)
}

/// Whether words read as meant for Overseer, with no model (AC-164): they name Overseer or an
/// agent, lead with a stop word or a command, or answer a question Overseer asked.
pub fn addressed(text: &str, agent_names: &[String], awaiting_answer: bool) -> bool {
    let words: Vec<String> = text
        .to_lowercase()
        .replace(['-', '’', '\''], " ")
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return false;
    }
    if awaiting_answer {
        return true;
    }
    if words.iter().any(|w| w == "overseer") {
        return true;
    }
    if COMMAND_VERBS.contains(&words[0].as_str())
        || (words.len() > 1 && words[0] == "please" && COMMAND_VERBS.contains(&words[1].as_str()))
    {
        return true;
    }
    if words.len() > 1 && words[0] == "hold" && words[1] == "on" {
        return true;
    }
    // A job for someone new: "someone should write the migration note", "we need someone to…"
    // (AC-168). Overseer can still answer that it was not meant for it.
    if words.len() > 2
        && matches!(words[0].as_str(), "someone" | "somebody")
        && matches!(
            words[1].as_str(),
            "should" | "needs" | "has" | "must" | "could" | "can"
        )
    {
        return true;
    }
    // "Three agents should each write a note", "two new agents should…".
    const COUNT: &[&str] = &[
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "1", "2", "3", "4",
        "5", "6", "7", "8", "9",
    ];
    if words.len() > 3
        && COUNT.contains(&words[0].as_str())
        && (words[1] == "agents"
            || words[1] == "agent"
            || (words[1] == "new" && words[2].starts_with("agent")))
        && words.iter().any(|w| w == "should")
    {
        return true;
    }
    if words.len() > 3
        && words[0] == "we"
        && words[1] == "need"
        && words[2..]
            .iter()
            .any(|w| matches!(w.as_str(), "someone" | "somebody" | "agent" | "agents"))
    {
        return true;
    }
    // A question about the agents or their work.
    const QUESTION: &[&str] = &[
        "what", "whats", "who", "whos", "how", "hows", "is", "are", "did", "has", "have", "any",
        "where", "which", "why", "when",
    ];
    const ABOUT_WORK: &[&str] = &[
        "agent",
        "agents",
        "everyone",
        "everybody",
        "anyone",
        "anybody",
        "running",
        "working",
        "waiting",
        "stuck",
        "done",
        "finished",
        "build",
        "tests",
        "test",
        "branch",
        "review",
        "merge",
        "status",
        "progress",
    ];
    if QUESTION.contains(&words[0].as_str())
        && words.iter().any(|w| ABOUT_WORK.contains(&w.as_str()))
    {
        return true;
    }
    agent_names.iter().any(|name| {
        let n: Vec<String> = name
            .to_lowercase()
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() >= 3)
            .collect();
        !n.is_empty()
            && (n.iter().all(|x| words.contains(x))
                || n.first().is_some_and(|x| x.len() >= 5 && words.contains(x)))
    })
}

impl Voice {
    /// The owner spoke over Overseer (the listener lowered its voice). In 0.7 s: still speaking,
    /// and meant for Overseer, means stop at the end of the phrase.
    pub(super) fn on_barge(self: &Arc<Self>, utterance: u64, text: &str, stop_word: bool) {
        if stop_word {
            return; // the listener stopped at once
        }
        self.st.lock().unwrap().barge = Some(Barge { utterance });
        let v = self.clone();
        let first = text.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            let (still, heard, awaiting) = {
                let st = v.st.lock().unwrap();
                let still = st.gate && st.barge.as_ref().is_some_and(|b| b.utterance == utterance);
                (
                    still,
                    if st.heard.is_empty() {
                        first.clone()
                    } else {
                        st.heard.clone()
                    },
                    st.awaiting_answer,
                )
            };
            if still && addressed(&heard, &v.agent_names(), awaiting) {
                v.send(json!({"cmd": "stop", "at": "phrase"}));
                v.emit(json!({"kind": "floor", "event": "yield", "words": heard}));
            }
        });
    }

    /// Before a cue plays (AC-172): false drops it. Waits while Overseer finishes its phrase, and
    /// while the owner finishes a thought for an attention cue.
    pub fn before_cue(self: &Arc<Self>, key: &str, cue_ms: u64) -> bool {
        if !self.running() {
            return true;
        }
        let attention = key == "agent_needs_attention";
        let start = Instant::now();
        let seq = self.st.lock().unwrap().spoke_seq;
        loop {
            let (gate, speaking, now_seq) = {
                let st = self.st.lock().unwrap();
                (st.gate, st.speaking, st.spoke_seq)
            };
            if gate {
                if !attention {
                    self.emit(json!({"kind": "cue", "key": key, "result": "dropped", "why": "the owner is speaking"}));
                    return false;
                }
                if start.elapsed() >= Duration::from_secs(5) {
                    break;
                }
            } else if speaking && now_seq == seq {
                // Wait for the end of the phrase (or of the line).
                if start.elapsed() >= Duration::from_secs(10) {
                    break;
                }
            } else {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.send(json!({"cmd": "suppress", "ms": cue_ms + 150}));
        self.emit(json!({"kind": "cue", "key": key, "result": "played", "waited_ms": start.elapsed().as_millis() as u64}));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meant_for_overseer() {
        let names = vec!["Phone app".to_string(), "Continuity".to_string()];
        assert!(addressed(
            "Overseer, what is everyone doing?",
            &names,
            false
        ));
        assert!(addressed(
            "tell Continuity to use the new format",
            &names,
            false
        ));
        assert!(addressed("the phone app should wait", &names, false));
        assert!(addressed("continuity is done?", &names, false));
        assert!(addressed("hold on", &names, false));
        assert!(!addressed("I'll grab lunch at noon", &names, false));
        assert!(!addressed("yeah that meeting went long", &names, false));
        assert!(addressed("what is everyone doing", &names, false));
        assert!(addressed("is anyone stuck?", &names, false));
        assert!(!addressed("what time is it", &names, false));
        assert!(
            addressed("yes", &names, true),
            "an answer to Overseer's question"
        );
        assert!(!addressed("", &names, true));
        assert!(addressed(
            "someone should write the migration note",
            &names,
            false
        ));
        assert!(addressed("we need someone to fix the build", &names, false));
        assert!(!addressed("someone left the door open", &names, false));
        assert!(!addressed("we need milk", &names, false));
        assert!(addressed(
            "three agents should each write a release note",
            &names,
            false
        ));
        assert!(!addressed("three friends should come over", &names, false));
    }
}
