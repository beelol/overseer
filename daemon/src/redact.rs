//! Best-effort secret redaction for stored/displayed output.

use regex::Regex;
use std::sync::OnceLock;

fn patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            r"sk-[A-Za-z0-9_\-]{16,}",
            r"sk-ant-[A-Za-z0-9_\-]{16,}",
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
    }
}
