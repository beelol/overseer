//! Every daemon method has a class for devices, read from `protocol/protocol.json`.
//! A method without a class is refused, and the test suite fails on one.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// The calling device's own session and settings.
    Own,
    Read,
    Control,
    MacOnly,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Full,
    Watch,
}

impl Scope {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "full" => Some(Scope::Full),
            "watch" => Some(Scope::Watch),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Full => "full",
            Scope::Watch => "watch",
        }
    }
}

const SOURCE: &str = include_str!("../../../protocol/protocol.json");

fn table() -> &'static HashMap<String, Class> {
    static T: OnceLock<HashMap<String, Class>> = OnceLock::new();
    T.get_or_init(|| {
        let doc: Value = serde_json::from_str(SOURCE).expect("protocol/protocol.json is valid JSON");
        doc["methods"]
            .as_object()
            .expect("methods")
            .iter()
            .filter(|(_, m)| m["planned"].as_bool() != Some(true))
            .map(|(name, m)| {
                let class = match m["class"].as_str() {
                    Some("self") => Class::Own,
                    Some("read") => Class::Read,
                    Some("control") => Class::Control,
                    Some("mac_only") => Class::MacOnly,
                    other => panic!("protocol.json: method {name} has class {other:?}"),
                };
                (name.clone(), class)
            })
            .collect()
    })
}

/// Methods the protocol describes that are not built yet.
#[allow(dead_code)]
pub fn planned() -> Vec<String> {
    let doc: Value = serde_json::from_str(SOURCE).expect("protocol/protocol.json is valid JSON");
    doc["methods"].as_object().expect("methods").iter().filter(|(_, m)| m["planned"].as_bool() == Some(true)).map(|(k, _)| k.clone()).collect()
}

pub fn class_of(method: &str) -> Option<Class> {
    table().get(method).copied()
}

#[allow(dead_code)]
pub fn methods() -> Vec<(String, Class)> {
    let mut all: Vec<(String, Class)> = table().iter().map(|(k, v)| (k.clone(), *v)).collect();
    all.sort();
    all
}

/// What a device of this scope may call. `Err` carries the refusal code.
pub fn allowed(method: &str, scope: Scope) -> Result<Class, &'static str> {
    match class_of(method) {
        None => Err("unknown_method"),
        Some(Class::MacOnly) => Err("mac_only"),
        Some(Class::Control) if scope == Scope::Watch => Err("watch_only"),
        Some(class) => Ok(class),
    }
}

impl PartialOrd for Class {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Class {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `"name" =>` arm of the daemon's dispatch tables, read from the source.
    fn dispatched() -> Vec<String> {
        let mut names = Vec::new();
        for file in ["src/server.rs", "src/gateway/local.rs", "src/gateway/remote.rs"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
            let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{file}"));
            let re = regex::Regex::new(r#"(?m)^\s*"([a-z_]+(?:\.[a-z_]+)*)"\s*(?:\|\s*"[a-z_.]+"\s*)*=>"#).unwrap();
            for cap in re.captures_iter(&text) {
                names.push(cap[1].to_string());
            }
            let alt = regex::Regex::new(r#"\|\s*"([a-z_]+(?:\.[a-z_]+)+)"\s*(?:=>|\|)"#).unwrap();
            for cap in alt.captures_iter(&text) {
                names.push(cap[1].to_string());
            }
        }
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn every_daemon_method_has_a_class_and_every_class_has_a_method() {
        let dispatched = dispatched();
        assert!(dispatched.len() > 40, "the dispatch tables were not found: {dispatched:?}");
        let missing: Vec<&String> = dispatched.iter().filter(|m| class_of(m).is_none()).collect();
        assert!(missing.is_empty(), "methods without a class in protocol/protocol.json: {missing:?}");
        let stale: Vec<String> = methods().into_iter().map(|(m, _)| m).filter(|m| !dispatched.contains(m) && !["events.subscribe", "ui.focus"].contains(&m.as_str())).collect();
        assert!(stale.is_empty(), "protocol/protocol.json lists methods the daemon does not have: {stale:?}");
        let built: Vec<String> = planned().into_iter().filter(|m| dispatched.contains(m)).collect();
        assert!(built.is_empty(), "these methods exist now; remove \"planned\" from them in protocol/protocol.json: {built:?}");
    }

    #[test]
    fn the_readme_lists_every_method_with_its_phone_status() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let out = std::process::Command::new("python3").arg(root.join("protocol/capabilities.py")).arg("--check").output().expect("python3");
        assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
        for (method, _) in methods() {
            assert!(readme.contains(&format!("| `{method}` |")), "{method} is missing from the README's table");
        }
    }

    #[test]
    fn scopes_follow_the_classes() {
        for (method, class) in methods() {
            let full = allowed(&method, Scope::Full);
            let watch = allowed(&method, Scope::Watch);
            match class {
                Class::Own | Class::Read => assert!(full.is_ok() && watch.is_ok(), "{method}"),
                Class::Control => assert!(full.is_ok() && watch == Err("watch_only"), "{method}"),
                Class::MacOnly => assert!(full == Err("mac_only") && watch == Err("mac_only"), "{method}"),
            }
        }
        assert_eq!(allowed("no.such_method", Scope::Full), Err("unknown_method"));
        for mac in ["daemon.shutdown", "gateway.pair_start", "gateway.device_revoke", "gateway.device_scope", "gateway.enable", "gateway.disable"] {
            assert_eq!(class_of(mac), Some(Class::MacOnly), "{mac} must stay on the Mac");
        }
    }
}
