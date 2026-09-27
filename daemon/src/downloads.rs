//! Model downloads (Continuity, AC-89). A model is pulled from the Ollama registry only when the
//! owner's setting allows it, only while there is a connection, only when the disk has room, and
//! the first pull ever is confirmed once with its size. A pull streams its progress as events,
//! can be cancelled, and picks up where it stopped when it is asked for again.
//!
//! Prefetch, when the owner has turned it on, keeps the best-fitting eligible model downloaded
//! while online, and never while a paid turn is running.

use crate::continuity::{self, Conn};
use crate::daemon::{now, Daemon, ACTIVE};
use crate::local;
use crate::sys::{self, gib};
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Serialize, Clone, Debug)]
pub struct Download {
    pub tag: String,
    /// `starting`, `downloading`, `done`, `cancelled` or `failed`.
    pub status: String,
    pub completed: u64,
    pub total: u64,
    pub percent: Option<u64>,
    /// Who asked: `user`, `prefetch` or `pick`.
    pub by: String,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub reason: Option<String>,
    #[serde(skip)]
    cancel: Arc<AtomicBool>,
}

static DOWNLOADS: OnceLock<Mutex<BTreeMap<String, Download>>> = OnceLock::new();

fn downloads() -> std::sync::MutexGuard<'static, BTreeMap<String, Download>> {
    DOWNLOADS.get_or_init(|| Mutex::new(BTreeMap::new())).lock().unwrap()
}

pub fn list() -> Vec<Download> {
    downloads().values().cloned().collect()
}

/// Is a download going on?
pub fn busy() -> bool {
    active().is_some()
}

fn active() -> Option<Download> {
    downloads().values().find(|d| d.status == "starting" || d.status == "downloading").cloned()
}

fn confirmed(d: &Daemon) -> bool {
    continuity::meta_get(d, "continuity.first_pull_confirmed").is_some()
}

/// The name asked of Ollama: the tag itself, or the tag at the owner's mirror.
pub fn registry_name(tag: &str, registry: &str) -> String {
    let host = registry.trim_start_matches("https://").trim_end_matches('/');
    if host.is_empty() {
        return tag.to_string();
    }
    if tag.contains('/') { format!("{host}/{tag}") } else { format!("{host}/library/{tag}") }
}

/// Sums what Ollama reports per layer into one progress.
#[derive(Default)]
pub struct Progress {
    layers: BTreeMap<String, (u64, u64)>,
}

impl Progress {
    /// One line of `/api/pull`; returns the status text.
    pub fn line(&mut self, v: &Value) -> String {
        if let (Some(digest), Some(total)) = (v["digest"].as_str(), v["total"].as_u64()) {
            self.layers.insert(digest.to_string(), (v["completed"].as_u64().unwrap_or(0), total));
        }
        v["status"].as_str().unwrap_or_default().to_string()
    }
    pub fn completed(&self) -> u64 {
        self.layers.values().map(|(c, _)| *c).sum()
    }
    pub fn total(&self) -> u64 {
        self.layers.values().map(|(_, t)| *t).sum()
    }
}

/// Everything that must hold before a pull starts. The error is the reason shown to the user.
pub fn may_pull(d: &Daemon, tag: &str, by: &str, confirm: bool) -> Result<Value> {
    let settings = continuity::settings();
    if !settings.allow_model_downloads {
        bail!("{tag} is not installed, and model downloads are off (overseer.continuity.allowModelDownloads)");
    }
    if by == "prefetch" && !settings.prefetch {
        bail!("prefetch is off");
    }
    match continuity::status() {
        Some(s) if s.state == Conn::Offline => bail!("downloads need a connection; Overseer is offline ({})", s.reason),
        _ => {}
    }
    let ollama = local::ollama_status();
    if !ollama.running {
        // Allowed to, Overseer starts Ollama (or installs it first); otherwise it says what is missing.
        bail!("{}; a model cannot be downloaded{}", ollama.detail, if settings.allow_ollama_install { " yet" } else { "" });
    }
    if local::installed_models()?.iter().any(|m| m.tag == tag) {
        bail!("{tag} is already installed");
    }
    let size = local::catalogue().iter().find(|e| e.tag == tag).map(|e| e.disk_bytes);
    let dir = local::models_dir();
    let free = sys::disk_free(&dir);
    if let (Some(size), Some(free)) = (size, free) {
        let needed = size + size / 10;
        if needed > free {
            bail!("{tag} needs {} GiB with room to spare and the disk has {} GiB free: {} GiB are missing", gib(needed), gib(free), gib(needed - free));
        }
    }
    if let Some(other) = active().filter(|a| a.tag != tag) {
        bail!("{} is being downloaded; one model at a time", other.tag);
    }
    if !confirmed(d) {
        if !confirm || by != "user" {
            // The first pull ever is confirmed once by the user, with the size in front of them.
            return Ok(json!({"needs_confirmation": true, "tag": tag, "bytes": size, "free": free}));
        }
        continuity::meta_set(d, "continuity.first_pull_confirmed", &now().to_string())?;
    }
    Ok(json!({"needs_confirmation": false, "tag": tag, "bytes": size, "free": free}))
}

/// Starts a pull and returns at once; progress comes as `local_download` events.
pub fn pull(d: &Arc<Daemon>, tag: &str, by: &str, confirm: bool) -> Result<Value> {
    if continuity::settings().allow_model_downloads && continuity::settings().allow_ollama_install && !local::ollama_status().running {
        crate::ollama_install::ensure_running(d)?;
    }
    if let Some(a) = active().filter(|a| a.tag == tag) {
        return Ok(json!({"download": a, "already": true}));
    }
    let check = may_pull(d, tag, by, confirm)?;
    if check["needs_confirmation"] == true {
        return Ok(check);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let dl = Download { tag: tag.to_string(), status: "starting".into(), completed: 0, total: check["bytes"].as_u64().unwrap_or(0), percent: None, by: by.to_string(), started_ms: now(), ended_ms: None, reason: None, cancel: cancel.clone() };
    downloads().insert(tag.to_string(), dl.clone());
    d.emit(None, None, "local_download", "daemon", "exact", json!({"download": dl}))?;
    let (d, tag, by) = (d.clone(), tag.to_string(), by.to_string());
    let name = registry_name(&tag, &continuity::settings().registry);
    std::thread::Builder::new().name("download".into()).spawn(move || {
        let outcome = stream(&d, &tag, &name, &cancel);
        let mut all = downloads();
        if let Some(dl) = all.get_mut(&tag) {
            dl.ended_ms = Some(now());
            match &outcome {
                Ok(true) => {
                    dl.status = "done".into();
                    dl.completed = dl.total.max(dl.completed);
                    dl.percent = Some(100);
                }
                Ok(false) => {
                    dl.status = "cancelled".into();
                    dl.reason = Some("cancelled; what was downloaded is kept, and the next download continues from it".into());
                }
                Err(e) => {
                    dl.status = "failed".into();
                    dl.reason = Some(e.to_string());
                }
            }
            let snapshot = dl.clone();
            drop(all);
            crate::log(&format!("continuity: download of {tag} ({by}): {}{}", snapshot.status, snapshot.reason.as_ref().map(|r| format!(": {r}")).unwrap_or_default()));
            let _ = d.emit(None, None, "local_download", "daemon", "exact", json!({"download": snapshot}));
        }
    })?;
    Ok(json!({"download": dl, "already": false}))
}

/// Follows `/api/pull` to its end. `Ok(false)` when it was cancelled.
fn stream(d: &Arc<Daemon>, tag: &str, name: &str, cancel: &Arc<AtomicBool>) -> Result<bool> {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(5)).timeout_read(Duration::from_secs(120)).build();
    let res = match agent.post(&format!("{}/api/pull", local::ollama_url()?)).set("content-type", "application/json").send_string(&json!({"model": name, "stream": true}).to_string()) {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => bail!("Ollama answered {code}: {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>()),
        Err(e) => bail!("Ollama is not answering: {e}"),
    };
    let mut progress = Progress::default();
    let mut last = Instant::now() - Duration::from_secs(10);
    let mut last_percent = None;
    let mut success = false;
    let every = Duration::from_millis(std::env::var("OVERSEER_TEST_DOWNLOAD_EVENT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1000));
    for line in BufReader::new(res.into_reader()).lines() {
        if cancel.load(Ordering::SeqCst) {
            // Dropping the connection stops the pull; Ollama keeps the layers it already has.
            return Ok(false);
        }
        let line = line.map_err(|e| anyhow!("the download stopped: {e}"))?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(e) = v["error"].as_str() {
            bail!("Ollama could not download {name}: {e}");
        }
        let status = progress.line(&v);
        if status == "success" {
            success = true;
        }
        let (completed, total) = (progress.completed(), progress.total());
        let percent = (total > 0).then(|| completed * 100 / total);
        // The disk is watched while it fills.
        if let Some(free) = sys::disk_free(&local::models_dir()) {
            if free < 2 * sys::GIB {
                bail!("the disk has {} GiB left; the download was stopped", gib(free));
            }
        }
        if last.elapsed() >= every && percent != last_percent {
            last = Instant::now();
            last_percent = percent;
            let snapshot = {
                let mut all = downloads();
                all.get_mut(tag).map(|dl| {
                    dl.status = "downloading".into();
                    dl.completed = completed;
                    dl.total = total.max(dl.total);
                    dl.percent = percent;
                    dl.clone()
                })
            };
            if let Some(dl) = snapshot {
                let _ = d.emit(None, None, "local_download", "daemon", "exact", json!({"download": dl, "step": status}));
            }
        }
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(false);
    }
    if !success {
        bail!("the download of {name} ended before Ollama reported success");
    }
    let registry = continuity::settings().registry;
    if !registry.is_empty() && name != tag {
        // From a mirror the model arrives under the mirror's name; it is given its own.
        local::post("/api/copy", &json!({"source": name, "destination": tag}), 60)?;
    }
    Ok(true)
}

pub fn cancel(tag: &str) -> Result<Value> {
    let all = downloads();
    let dl = all.get(tag).filter(|d| d.status == "starting" || d.status == "downloading").ok_or_else(|| anyhow!("{tag} is not being downloaded"))?;
    dl.cancel.store(true, Ordering::SeqCst);
    Ok(json!({"cancelling": tag}))
}

/// A paid turn is one on an online provider's account.
fn paid_turn_running(d: &Daemon) -> bool {
    d.store.lock().unwrap().runs().map(|runs| runs.iter().any(|r| r.parent_run_id.is_none() && ACTIVE.contains(&r.status.as_str()) && ["openai", "anthropic"].contains(&crate::accounts::provider_of(&r.harness)))).unwrap_or(false)
}

/// Called on every check of the connection: while online, with prefetch on and downloads
/// allowed and confirmed, the pick that still needs a download is fetched. Returns what it did.
pub fn prefetch(d: &Arc<Daemon>) -> Option<String> {
    let settings = continuity::settings();
    if !settings.prefetch || !settings.allow_model_downloads || !confirmed(d) {
        return None;
    }
    if continuity::status().is_none_or(|s| s.state != Conn::Online) || active().is_some() || paid_turn_running(d) {
        return None;
    }
    let pick = continuity::pick_value(d).ok()?;
    let chosen = &pick["pick"]["chosen"];
    if chosen["installed"] != false {
        return None;
    }
    let tag = chosen["tag"].as_str()?.to_string();
    // A download that failed or was cancelled is not retried until the settings change or the daemon restarts.
    if downloads().get(&tag).is_some_and(|d| d.status == "failed" || d.status == "cancelled") {
        return None;
    }
    pull(d, &tag, "prefetch", false).ok().map(|_| tag)
}

pub fn handles(method: &str) -> bool {
    matches!(method, "local.pull" | "local.pull_cancel" | "local.downloads")
}

pub fn dispatch(d: &Arc<Daemon>, method: &str, p: &Value) -> Result<Value> {
    let tag = || p["tag"].as_str().ok_or_else(|| anyhow!("missing string parameter tag"));
    Ok(match method {
        "local.pull" => pull(d, tag()?, "user", p["confirm"].as_bool().unwrap_or(false))?,
        "local.pull_cancel" => cancel(tag()?)?,
        "local.downloads" => json!({"downloads": list(), "first_pull_confirmed": confirmed(d)}),
        other => bail!("unknown method {other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_summed_over_layers() {
        let mut p = Progress::default();
        assert_eq!(p.line(&json!({"status": "pulling manifest"})), "pulling manifest");
        assert_eq!((p.completed(), p.total()), (0, 0));
        p.line(&json!({"status": "pulling aaa", "digest": "sha256:aaa", "total": 1000, "completed": 250}));
        p.line(&json!({"status": "pulling bbb", "digest": "sha256:bbb", "total": 200}));
        assert_eq!((p.completed(), p.total()), (250, 1200));
        p.line(&json!({"status": "pulling aaa", "digest": "sha256:aaa", "total": 1000, "completed": 1000}));
        p.line(&json!({"status": "pulling bbb", "digest": "sha256:bbb", "total": 200, "completed": 200}));
        assert_eq!((p.completed(), p.total()), (1200, 1200));
        assert_eq!(p.line(&json!({"status": "success"})), "success");
    }

    #[test]
    fn a_mirror_changes_the_name_asked_for() {
        assert_eq!(registry_name("qwen2.5-coder:7b", ""), "qwen2.5-coder:7b");
        assert_eq!(registry_name("qwen2.5-coder:7b", "https://mirror.example.invalid/"), "mirror.example.invalid/library/qwen2.5-coder:7b");
        assert_eq!(registry_name("someone/model:1b", "https://mirror.example.invalid"), "mirror.example.invalid/someone/model:1b");
    }
}
