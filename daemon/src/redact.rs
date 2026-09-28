//! Best-effort secret redaction for stored/displayed output.

use regex::Regex;
use std::sync::OnceLock;

fn patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            // At a word's start: `ask-3f…` (a question's id) is not a key.
            r"\bsk-[A-Za-z0-9_\-]{16,}",
            r"\bsk-ant-[A-Za-z0-9_\-]{16,}",
            r"eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
            r"(?i)bearer\s+[A-Za-z0-9._\-]{16,}",
            r#"(?i)"(access_token|refresh_token|id_token|api_key|apikey|password|secret)"\s*:\s*"[^"]*""#,
            r"gh[pousr]_[A-Za-z0-9]{20,}",
            r"xox[abprs]-[A-Za-z0-9\-]{10,}",
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    })
}

pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    for re in patterns() {
        if re.is_match(&out) {
            out = re
                .replace_all(&out, |caps: &regex::Captures| {
                    if let Some(key) = caps.get(1) {
                        format!("\"{}\": \"[redacted]\"", key.as_str())
                    } else {
                        "[redacted]".to_string()
                    }
                })
                .to_string();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redacts_common_secrets() {
        assert_eq!(redact("key sk-abcdefghijklmnopqrstuv end"), "key [redacted] end");
        assert_eq!(redact(r#"{"access_token": "abc.def"}"#), r#"{"access_token": "[redacted]"}"#);
        assert!(redact("Authorization: Bearer abcdefghijklmnopqrstuvwxyz").contains("[redacted]"));
        assert_eq!(redact("hello world"), "hello world");
        // Ids that merely contain the letters are left alone; keys after punctuation are not.
        assert_eq!(redact("answer ask-44219c6abbc47e11"), "answer ask-44219c6abbc47e11");
        assert_eq!(redact("KEY=sk-abcdefghijklmnopqrstuv"), "KEY=[redacted]");
        assert_eq!(redact("\"sk-ant-api03-abcdefghijklmnop\""), "\"[redacted]\"");
    }
}
