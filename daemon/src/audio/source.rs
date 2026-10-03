//! Revisioned local selection. Persist only settings and validation metadata, never media.
use super::{
    lines::Line,
    pack::{Pack, ValidatedPack},
    player,
};
use crate::daemon::Daemon;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

// Serializes committed source identity/enablement with final player admission.
// Decode, availability inspection and response/event projection stay outside it.
static ADMISSION: Mutex<()> = Mutex::new(());
pub(super) fn admission_guard() -> MutexGuard<'static, ()> {
    ADMISSION.lock().unwrap()
}
pub(super) fn current_revision(d: &Arc<Daemon>) -> Result<i64> {
    revision(&d.store.lock().unwrap().conn)
}

#[derive(Clone, Serialize, Deserialize)]
struct Selection {
    kind: String,
    path: Option<PathBuf>,
    validated: Option<ValidatedPack>,
}
impl Selection {
    fn builtin() -> Self {
        Self {
            kind: "builtin".into(),
            path: None,
            validated: None,
        }
    }
}
pub(super) struct SourceSnapshot {
    selection: Selection,
    pub(super) enabled: bool,
    pub(super) revision: i64,
    pub(super) available: bool,
    pub(super) reason: Option<&'static str>,
}
fn meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}
fn revision(conn: &Connection) -> Result<i64> {
    Ok(meta(conn, "audio.source.revision")?
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n >= 0)
        .unwrap_or(0))
}
fn loaded(conn: &Connection) -> Result<(Selection, bool, i64)> {
    let selected = if let Some(raw) = meta(conn, "audio.source.selection")? {
        serde_json::from_str(&raw).map_err(|_| anyhow!("Stored audio settings need repair."))?
    } else if meta(conn, "audio.track")?.as_deref() == Some("commander") {
        // A legacy private choice is never replaced with Built-in, including a missing path.
        Selection {
            kind: "folder".into(),
            path: meta(conn, "audio.commander_dir")?.map(PathBuf::from),
            validated: None,
        }
    } else {
        Selection::builtin()
    };
    Ok((
        selected,
        meta(conn, "audio.reactor.enabled")?.as_deref() == Some("1"),
        revision(conn)?,
    ))
}
impl SourceSnapshot {
    fn inspect(selection: Selection, enabled: bool, revision: i64) -> Self {
        let reason = match selection.kind.as_str() {
            "builtin" => Some("The approved Built-in twelve-line assets are not yet available."),
            "folder" => match (&selection.path, &selection.validated) {
                (Some(path), Some(validated)) if Pack::from_validated(path, validated.clone()).is_ok() => {
                    if player::available() { None } else { Some("Opened-descriptor playback still needs qualification on this Mac.") }
                },
                _ => Some("The selected folder is missing, changed or needs a valid twelve-line manifest. Select it again."),
            },
            _ => Some("The saved audio source needs repair."),
        };
        Self {
            selection,
            enabled,
            revision,
            available: reason.is_none(),
            reason,
        }
    }
    pub(super) fn open_line(&self, line: Line) -> Result<super::pack::OpenedLine> {
        if !self.available {
            bail!("The selected audio source is unavailable. Select a valid folder or check its availability.");
        }
        let path = self
            .selection
            .path
            .as_ref()
            .ok_or_else(|| anyhow!("Built-in audio is unavailable."))?;
        let validated = self
            .selection
            .validated
            .clone()
            .ok_or_else(|| anyhow!("The selected folder needs validation."))?;
        Pack::from_validated(path, validated)?.open_line(line)
    }
    fn public(&self) -> Value {
        let label = if self.selection.kind == "folder" {
            self.selection
                .validated
                .as_ref()
                .map(|p| p.label.as_str())
                .unwrap_or("From folder")
        } else {
            "Built-in"
        };
        json!({"enabled":self.enabled,"revision":self.revision,"available":self.available,
            "source":{"kind":self.selection.kind,"label":label,"available":self.available,"reason":self.reason},
            "lines":Line::ALL.into_iter().map(|line| json!({"key":line.key(),"phrase":line.phrase()})).collect::<Vec<_>>()})
    }
}
pub(super) fn snapshot(d: &Arc<Daemon>) -> Result<SourceSnapshot> {
    let (selected, enabled, revision) = { loaded(&d.store.lock().unwrap().conn)? };
    // No filesystem/codec work under the Store lock. Inspection never decodes twelve files.
    Ok(SourceSnapshot::inspect(selected, enabled, revision))
}
pub(super) fn get(d: &Arc<Daemon>) -> Result<Value> {
    Ok(crate::daemon::redact_value(snapshot(d)?.public()))
}
fn object<'a>(p: &'a Value, fields: &[&str]) -> Result<&'a serde_json::Map<String, Value>> {
    let object = p
        .as_object()
        .ok_or_else(|| anyhow!("Audio settings must be an object."))?;
    if object.keys().any(|k| !fields.contains(&k.as_str())) {
        bail!("Unknown audio setting. Use the local source picker and separate on/off control.");
    }
    Ok(object)
}
fn expected(p: &Value) -> Result<i64> {
    p["expected_revision"]
        .as_i64()
        .filter(|n| *n >= 0)
        .ok_or_else(|| anyhow!("Provide the displayed audio revision, then refresh if it changes."))
}
fn put(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
    Ok(())
}
fn changed(d: &Arc<Daemon>, state: &Value) -> Result<()> {
    d.emit(
        None,
        None,
        "audio_changed",
        "audio",
        "certain",
        state.clone(),
    )?;
    Ok(())
}
pub(super) fn select(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    object(p, &["source", "path", "expected_revision"])?;
    let expected = expected(p)?;
    // Reject stale work before decoder launch, then revalidate under the commit lock.
    if snapshot(d)?.revision != expected {
        bail!("Audio revision changed. Refresh before choosing a source.");
    }
    let selected = match p["source"].as_str() {
        Some("builtin") if !p.as_object().unwrap().contains_key("path") => Selection::builtin(),
        Some("folder") => {
            let text = p["path"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control))
                .ok_or_else(|| anyhow!("Choose a valid local folder."))?;
            // Resolve Mac aliases such as /tmp once at explicit selection; every later
            // open walks this stored root with no-follow and checks its descriptor identity.
            let path = std::fs::canonicalize(text)
                .map_err(|_| anyhow!("The selected folder is unavailable."))?;
            let pack = Pack::open(&path)?;
            Selection {
                kind: "folder".into(),
                path: Some(path),
                validated: Some(pack.validated),
            }
        }
        _ => bail!("Choose Built-in or From folder; Built-in cannot include a folder path."),
    };
    {
        let _admission = admission_guard();
        let store = d.store.lock().unwrap();
        let tx = store.conn.unchecked_transaction()?;
        if revision(&tx)? != expected {
            bail!("Audio revision changed. Refresh before choosing a source.");
        }
        put(
            &tx,
            "audio.source.selection",
            &serde_json::to_string(&selected)?,
        )?;
        put(
            &tx,
            "audio.source.revision",
            &expected
                .checked_add(1)
                .ok_or_else(|| anyhow!("Audio revision exhausted."))?
                .to_string(),
        )?;
        tx.commit()?;
    }
    let state = get(d)?;
    changed(d, &state)?;
    Ok(state)
}
/// Commit and publish the runtime transition in the same settings order, without
/// retaining Store through the callback, filesystem work or public event emission.
pub(super) fn set_enabled(
    d: &Arc<Daemon>,
    p: &Value,
    apply_runtime: impl FnOnce(bool, i64),
) -> Result<Value> {
    object(p, &["enabled", "expected_revision"])?;
    let expected = expected(p)?;
    let enabled = p["enabled"]
        .as_bool()
        .ok_or_else(|| anyhow!("enabled must be a boolean."))?;
    let current = snapshot(d)?;
    if current.revision != expected {
        bail!("Audio revision changed. Refresh before changing Audio Mode.");
    }
    if enabled && !current.available {
        bail!("The selected audio source is unavailable; Audio Mode remains unchanged.");
    }
    let transition;
    let admission = admission_guard();
    {
        let store = d.store.lock().unwrap();
        let tx = store.conn.unchecked_transaction()?;
        let (_, previous, latest) = loaded(&tx)?;
        if latest != expected {
            bail!("Audio revision changed. Refresh before changing Audio Mode.");
        }
        transition = if previous != enabled {
            Some(enabled)
        } else {
            None
        };
        put(
            &tx,
            "audio.reactor.enabled",
            if enabled { "1" } else { "0" },
        )?;
        put(
            &tx,
            "audio.source.revision",
            &expected
                .checked_add(1)
                .ok_or_else(|| anyhow!("Audio revision exhausted."))?
                .to_string(),
        )?;
        tx.commit()?;
    }
    if let Some(enabled) = transition {
        super::test_hold(
            "TRANSITION",
            &json!({"revision":expected + 1,"enabled":enabled}),
        )?;
        apply_runtime(enabled, expected + 1);
    }
    drop(admission);
    let state = get(d)?;
    changed(d, &state)?;
    Ok(state)
}
pub(super) fn validate_preview(d: &Arc<Daemon>, p: &Value) -> Result<Line> {
    object(p, &["key"])?;
    let line = p["key"]
        .as_str()
        .and_then(Line::parse)
        .ok_or_else(|| anyhow!("Choose one of the twelve Audio Mode lines."))?;
    if !snapshot(d)?.available {
        bail!("The selected audio source is unavailable.");
    }
    Ok(line)
}
