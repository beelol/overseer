extern crate regex;
extern crate serde_json;
#[path = "redact-baseline.rs"]
mod redact;
use serde_json::{json, Value};

fn check(label: &str, action: Value) {
    let serialized = action.to_string();
    let redacted = redact::redact(&serialized);
    let parsed = serde_json::from_str::<Value>(&redacted);
    println!("{label}\nserialized={serialized}\nredacted={redacted}\nparse_ok={}", parsed.is_ok());
    if let Err(e) = &parsed { println!("parse_error={e}"); }
    let stored = parsed.unwrap_or(action);
    println!("fallback_stored={stored}\n");
}

fn main() {
    let marker = "sk-proj-syntheticSYNTHETICabcdefghijkl012345";
    check("ordinary token baseline", json!({"action":"start", "repo":"/private/tmp/synthetic-repo", "prompt":format!("Write notes using {marker}")}));
    check("escaped password poisons whole action", json!({"action":"start", "repo":"/private/tmp/synthetic-repo", "prompt":format!("Write notes using {marker}"), "password":"synthetic\"secret"}));
    check("backslash plus quote", json!({"action":"start", "repo":"/private/tmp/synthetic-repo", "prompt":"Write notes", "password":"synthetic\\\"secret"}));
    let text = r#"Record {"password":"synthetic-secret-value"} accurately"#;
    check("credential JSON in required prompt", json!({"action":"start", "repo":"/private/tmp/synthetic-repo", "prompt":text}));
    println!("plain_prompt_redaction={}", redact::redact(text));
    let escaped_text = r#"Record {"password":"synthetic\"secret"} accurately"#;
    println!("plain_escaped_prompt_redaction={}", redact::redact(escaped_text));
}
