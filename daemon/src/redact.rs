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
            // Consume escaped quotes/backslashes as part of the value.
            r#"(?i)"(access_token|refresh_token|id_token|api_key|apikey|password|secret)"\s*:\s*"(?:[^"\\]|\\.)*""#,
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
        assert_eq!(
            redact("key sk-abcdefghijklmnopqrstuv end"),
            "key [redacted] end"
        );
        assert_eq!(
            redact(r#"{"access_token": "abc.def"}"#),
            r#"{"access_token": "[redacted]"}"#
        );
        assert!(redact("Authorization: Bearer abcdefghijklmnopqrstuvwxyz").contains("[redacted]"));
        assert_eq!(redact("hello world"), "hello world");
        // Ids that merely contain the letters are left alone; keys after punctuation are not.
        assert_eq!(
            redact("answer ask-44219c6abbc47e11"),
            "answer ask-44219c6abbc47e11"
        );
        assert_eq!(redact("KEY=sk-abcdefghijklmnopqrstuv"), "KEY=[redacted]");
        assert_eq!(
            redact("\"sk-ant-api03-abcdefghijklmnop\""),
            "\"[redacted]\""
        );
    }

    #[test]
    fn quoted_secret_values_with_json_escapes_are_redacted_completely() {
        for text in [
            r#"before {"password":"synthetic\"tail"} after"#,
            r#"before {"password":"synthetic\\\"tail"} after"#,
            r#"before {"password":"synthetic\\tail"} after"#,
            r#"before {"password":"synthetic\u0022tail"} after"#,
            r#"before {"password":"synthetic\ntail"} after"#,
        ] {
            assert_eq!(
                redact(text),
                r#"before {"password": "[redacted]"} after"#,
                "{text}"
            );
        }
        assert_eq!(
            redact(r#"{"message":"a \"quoted\" phrase", "count":7}"#),
            r#"{"message":"a \"quoted\" phrase", "count":7}"#
        );
    }

    #[test]
    fn structured_credentials_are_removed_without_changing_safe_action_fields() {
        use serde_json::json;
        let clean = crate::daemon::redact_value(json!({
            "action":"message", "agent":"r-safe", "enabled":false, "count":7,
            "password":"synthetic\"tail",
            "context":[{"API_KEY":"synthetic", "text":r#"Keep {"secret":"synthetic\"tail"} useful."#}, null],
            "text":"Keep the useful context"
        }));
        assert_eq!(
            clean,
            json!({
                "action":"message", "agent":"r-safe", "enabled":false, "count":7,
                "password":"[redacted]",
                "context":[{"API_KEY":"[redacted]", "text":r#"Keep {"secret": "[redacted]"} useful."#}, null],
                "text":"Keep the useful context"
            })
        );
    }

    #[test]
    fn credential_shaped_keys_are_redacted_with_stable_collision_order() {
        use serde_json::json;
        let clean = crate::daemon::redact_value(json!({
            "action":"message", "agent":"r-safe", "token":"synthetic secret",
            "context":{"sk-proj-syntheticKEY0000000000000001":"first safe value", "sk-proj-syntheticKEY0000000000000002":"last safe value"}
        }));
        assert_eq!(
            clean,
            json!({"action":"message", "agent":"r-safe", "token":"[redacted]",
            "context":{"[redacted]":"last safe value"}})
        );
    }
}
