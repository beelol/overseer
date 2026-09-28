//! Small judgements about words that need no model: backchannels, stop words, and whether heard
//! words are Overseer's own voice coming back through the microphone.

/// Lowercase words without punctuation.
pub fn normalize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['-', '’'], " ")
        .replace('\'', "")
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// The sounds of listening (AC-164): never a request, never a barge.
pub const BACKCHANNELS: &[&str] = &[
    "mm", "mmm", "mhm", "hmm", "hm", "uh", "huh", "um", "ah", "oh", "yeah", "yep", "okay", "ok",
    "right", "sure", "alright", "uhhuh",
];

/// Words that are not backchannels.
pub fn content_words(text: &str) -> Vec<String> {
    normalize(text)
        .into_iter()
        .filter(|w| !BACKCHANNELS.contains(&w.as_str()))
        .collect()
}

/// Whether the words are only a backchannel ("mm-hm", "yeah okay", "got it").
pub fn is_backchannel(text: &str) -> bool {
    let w = normalize(text);
    !w.is_empty() && (content_words(text).is_empty() || w.join(" ") == "got it")
}

/// "Stop", "wait" and "hold on" stop Overseer's voice at once (AC-164), when they lead.
pub fn leads_with_stop_word(text: &str) -> bool {
    let w = normalize(text);
    match w.as_slice() {
        [first, ..] if first == "stop" || first == "wait" => true,
        [first, second, ..] if first == "hold" && second == "on" => true,
        _ => false,
    }
}

/// Whether heard words are Overseer's own voice: two or more words, of which at least 60% are
/// among the words it is saying or just said.
pub fn is_echo(heard: &str, spoken: &[String]) -> bool {
    let h = normalize(heard);
    if h.len() < 2 || spoken.is_empty() {
        return false;
    }
    let inside = h.iter().filter(|w| spoken.contains(w)).count();
    inside as f32 / h.len() as f32 >= 0.6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backchannels() {
        assert!(is_backchannel("Mm-hm."));
        assert!(is_backchannel("yeah, okay"));
        assert!(is_backchannel("Got it."));
        assert_eq!(content_words("yes, allow it"), vec!["yes", "allow", "it"]);
        assert!(!is_backchannel("yeah tell phone"));
        assert!(!is_backchannel(""));
    }

    #[test]
    fn stop_words_lead() {
        assert!(leads_with_stop_word("Stop."));
        assert!(leads_with_stop_word("wait, no"));
        assert!(leads_with_stop_word("Hold on a second"));
        assert!(!leads_with_stop_word("don't stop"));
        assert!(!leads_with_stop_word("hold the phone agent"));
    }

    #[test]
    fn echo_is_mostly_overseer_s_own_words() {
        let spoken = normalize("On it: telling Phone and Continuity to use the new wire format.");
        assert!(is_echo("telling phone and continuity", &spoken));
        assert!(!is_echo("no, just the phone agent please", &spoken));
        assert!(!is_echo("phone", &spoken), "one word decides nothing");
    }
}
