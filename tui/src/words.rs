//! Plain words in the terminal (AC-245), as the extension's `media/plain-words.js` says them: no
//! internal id, snake_case state, lowercase harness id or raw error text; and Overseer's Markdown
//! drawn as a terminal can (bullets, bold, headings, code without its marks).

/// What the Mac's own login is called, as VS Code calls it (AC-235).
pub const DEFAULT_LOGIN: &str = "Mac's default login";

/// An account as VS Code names it: the Mac's own login is "Mac's default login", not "claude (existing login)".
pub fn account(name: &str) -> String {
    if name.ends_with(" (existing login)") { DEFAULT_LOGIN.into() } else { name.to_string() }
}

/// A harness by name: "Claude Code", never "claude".
pub fn harness(id: &str) -> String {
    match id {
        "claude" => "Claude Code".into(),
        "codex" | "codex-app" => "Codex".into(),
        "opencode" => "OpenCode".into(),
        "opencode-serve" => "Local model".into(),
        "generic" => "Program".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

fn is_snake(w: &str) -> bool {
    w.contains('_') && w.split('_').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase()))
}

fn is_id(w: &str) -> bool {
    let Some((pre, rest)) = w.split_once('-') else { return false };
    matches!(pre, "r" | "p" | "sh" | "w") && rest.len() >= 8 && rest.chars().all(|c| c.is_ascii_hexdigit())
}

/// Raw text (a failure's reason, a card's words) in plain words.
pub fn plain(text: &str) -> String {
    let t = text.trim();
    let lower = t.to_lowercase();
    if lower.contains("[rate_limit]") || lower.contains("rate limit") || lower.contains("rate_limit") || lower.contains("usage limit") || lower.contains("(429)") || lower.contains("quota exceeded") {
        return "hit its usage limit".into();
    }
    let mut t = t.to_string();
    for noise in ["Connection Failed: Connect error: ", "turn reported failure; last error ", "turn reported failure; ", "turn reported failure", "API Error: ", "Error: ", "error: ", "fatal: ", "Connection Failed: ", "Connect error: "] {
        t = t.replace(noise, "");
    }
    while let Some(at) = t.find(" (os error ") {
        let end = t[at..].find(')').map(|e| at + e + 1).unwrap_or(t.len());
        t.replace_range(at..end, "");
    }
    t = t.replace("Connection refused", "could not connect");
    let words: Vec<String> = t
        .split(' ')
        .filter_map(|w| {
            let core = w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'));
            if is_id(core) {
                return None;
            }
            if is_snake(core) {
                return Some(w.replace(core, &core.replace('_', " ")));
            }
            if matches!(core, "claude" | "codex" | "opencode" | "codex-app") {
                return Some(w.replace(core, &harness(core)));
            }
            Some(w.to_string())
        })
        .collect();
    let out = words.join(" ").replace("()", "").replace("  ", " ");
    out.trim().trim_start_matches([':', ';', ',', '.']).trim().to_string()
}

fn is_request_id(w: &str) -> bool {
    w.strip_prefix("V-").is_some_and(|n| n.len() >= 3 && n.chars().all(|c| c.is_ascii_digit()))
}

/// A spoken request in Overseer's conversation reaches Overseer as its notes, then
/// "Request V-0001: <the words>"; the owner reads the words alone, never the id (AC-219).
pub fn spoken(text: &str) -> Option<&str> {
    let at = text.rfind("Request V-")?;
    if at > 0 && !text[..at].ends_with('\n') {
        return None;
    }
    let rest = &text[at + "Request ".len()..];
    let (id, words) = rest.split_once(": ")?;
    is_request_id(id).then(|| words.trim())
}

/// A voice request's id in Overseer's own words reads "the request" (AC-219).
fn requests(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for w in text.split(' ') {
        let core = w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '-'));
        if !is_request_id(core) {
            out.push(w.to_string());
            continue;
        }
        let tail = &w[w.find(core).unwrap() + core.len()..];
        if out.last().is_some_and(|p| p == "(request") {
            out.pop();
            continue;
        }
        let capital = out.last().map_or(true, |p| p.ends_with(['.', '!', '?']));
        match out.last().map(String::as_str) {
            Some("request" | "Request") => {
                let p = out.pop().unwrap();
                if out.last().is_some_and(|q| matches!(q.as_str(), "your" | "the" | "Your" | "The")) {
                    out.push(format!("{p}{tail}"));
                } else {
                    let capital = p == "Request" || out.last().map_or(true, |q| q.ends_with(['.', '!', '?']));
                    out.push(format!("{} request{tail}", if capital { "The" } else { "the" }));
                }
            }
            Some("your" | "the" | "Your" | "The") => out.push(format!("request{tail}")),
            _ => out.push(format!("{} request{tail}", if capital { "The" } else { "the" })),
        }
    }
    out.join(" ")
}

/// An agent's state words in Overseer's replies (it reads the daemon's roster): "waiting for you".
pub fn states(text: &str) -> String {
    // A proposal's or an agent's id echoed into the reply ("(proposal p-…)") is left out.
    let mut t = text.split(' ').filter(|w| {
        let core = w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '-'));
        !is_id(core) && *w != "(proposal"
    }).collect::<Vec<_>>().join(" ");
    t = requests(&t);
    for (raw, word) in [("waiting_for_user", "waiting for you"), ("waiting_for_connection", "waiting for a connection"), ("waiting_for_memory", "waiting for memory"), ("handed_off", "handed off"), ("cancel_requested", "stopping")] {
        t = t.replace(raw, word);
    }
    t
}

/// One line of Overseer's Markdown as the terminal draws it: its text without the marks, and
/// whether it is a heading (drawn bold). "- **x**: y" becomes "• x: y".
pub fn markdown_line(line: &str) -> (String, bool) {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let (body, heading) = if let Some(h) = trimmed.strip_prefix("### ").or_else(|| trimmed.strip_prefix("## ")).or_else(|| trimmed.strip_prefix("# ")) {
        (h.to_string(), true)
    } else if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("+ ")) {
        (format!("{indent}• {item}"), false)
    } else {
        (line.to_string(), false)
    };
    let body = body.replace("**", "").replace("__", "").replace('`', "");
    (if body.trim() == "```" { String::new() } else { body }, heading)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_reasons_in_plain_words() {
        assert_eq!(plain("turn reported failure; last error [rate_limit]: API Error: Request rejected (429)"), "hit its usage limit");
        assert_eq!(plain("start failed: the codex harness is not installed"), "start failed: the Codex harness is not installed");
        assert_eq!(plain("waiting_for_user since r-198adab9b2b0"), "waiting for user since");
        assert_eq!(plain("Ollama: Connection Failed: Connect error: Connection refused (os error 61)"), "Ollama: could not connect");
    }

    #[test]
    fn no_voice_request_ids() {
        assert_eq!(spoken("(Spoken aloud to you in Voice Mode.)\nRequest V-0001: Tell Phone to rebase."), Some("Tell Phone to rebase."));
        assert_eq!(spoken("Request V-0012: hello"), Some("hello"));
        assert_eq!(spoken("Tell the Request V-0001: team"), None);
        assert_eq!(spoken("rebase onto main"), None);
        assert_eq!(states("Request V-0001 went to Phone. V-0002 is waiting_for_user."), "The request went to Phone. The request is waiting for you.");
        assert_eq!(states("I sent your request V-0003 to Phone."), "I sent your request to Phone.");
    }

    #[test]
    fn markdown_as_a_terminal_draws_it() {
        assert_eq!(markdown_line("- **Write the docs**: running"), ("• Write the docs: running".to_string(), false));
        assert_eq!(markdown_line("## Status"), ("Status".to_string(), true));
        assert_eq!(markdown_line("Use `npm test`."), ("Use npm test.".to_string(), false));
    }
}
