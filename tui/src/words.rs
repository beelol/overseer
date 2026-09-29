//! Plain words in the terminal (AC-245), as the extension's `media/plain-words.js` says them: no
//! internal id, snake_case state, lowercase harness id or raw error text; and Overseer's Markdown
//! drawn as a terminal can (bullets, bold, headings, code without its marks).

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

/// An agent's state words in Overseer's replies (it reads the daemon's roster): "waiting for you".
pub fn states(text: &str) -> String {
    let mut t = text.to_string();
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
    fn markdown_as_a_terminal_draws_it() {
        assert_eq!(markdown_line("- **Write the docs**: running"), ("• Write the docs: running".to_string(), false));
        assert_eq!(markdown_line("## Status"), ("Status".to_string(), true));
        assert_eq!(markdown_line("Use `npm test`."), ("Use npm test.".to_string(), false));
    }
}
