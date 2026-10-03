//! Opt-in daemon-owned Audio Mode: canonical semantics, current pack, one worker.
pub(crate) mod lines;
pub(crate) mod semantics;
pub(crate) mod decode;
mod pack;
mod source;
pub(super) mod player;
use crate::daemon::Daemon;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;
use tokio::sync::mpsc;
use lines::Line;
use semantics::Ticket;

const MAX_COALESCED: usize = 32;
const WINDOW: Duration = Duration::from_millis(800);

enum Kind { Semantic(Vec<Ticket>), Preview(Line), Heard }
struct Cue { kind: Kind, epoch: u64 }
struct Runtime {
    enabled: AtomicBool,
    epoch: AtomicU64,
    daemon: Weak<Daemon>,
    routine: mpsc::Sender<Cue>,
    urgent: mpsc::Sender<Cue>,
    feedback: mpsc::Sender<Cue>,
}
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn test_sink() -> bool { std::env::var_os("OVERSEER_TEST_AUDIO_LOG").is_some() }
fn player(path: &str) -> bool {
    std::env::var_os("OVERSEER_TEST_AUDIO_UNAVAILABLE").is_none()
        && cfg!(target_os="macos") && Path::new(path).exists()
}

impl Runtime {
    fn enqueue(&self, kind: Kind, urgent: bool) -> Result<()> {
        let cue=Cue{kind,epoch:self.epoch.load(Ordering::SeqCst)};
        let lane=if matches!(&cue.kind,Kind::Heard) {&self.feedback}
            else if urgent {&self.urgent} else {&self.routine};
        lane.try_send(cue).map_err(|_|anyhow!("Audio is busy; try again."))
    }
    fn current(&self, cue: &Cue) -> bool {
        cue.epoch==self.epoch.load(Ordering::SeqCst)
            && (matches!(&cue.kind,Kind::Preview(_)) || self.enabled.load(Ordering::SeqCst))
    }
}

/// Called after a committed enabled transition under source's admission guard.
/// Disable invalidates all pending work and stops the owned active player.
pub(super) fn enablement_changed(enabled: bool) {
    if let Some(runtime)=RUNTIME.get() {
        let previous=runtime.enabled.swap(enabled,Ordering::SeqCst);
        if previous!=enabled {runtime.epoch.fetch_add(1,Ordering::SeqCst);}
        if !enabled {
            if let Some(d)=runtime.daemon.upgrade() {player::cancel(&d);}
        }
    }
}

pub(super) fn enqueue_preview(line: Line) -> Result<()> {
    RUNTIME.get().ok_or_else(||anyhow!("Audio is unavailable."))?
        .enqueue(Kind::Preview(line),line.urgent())
}

/// Dedicated nonspoken Voice feedback, not a Line or pack key.
pub(crate) fn heard(_d: &Arc<Daemon>) {
    if let Some(runtime)=RUNTIME.get().filter(|r|r.enabled.load(Ordering::SeqCst)) {
        let _=runtime.enqueue(Kind::Heard,false);
    }
}

pub fn get(d: &Arc<Daemon>) -> Result<Value> { source::get(d) }

pub fn source_set(d: &Arc<Daemon>, p: &Value) -> Result<Value> { source::select(d, p) }

pub fn set(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    source::set_enabled(d, p, |enabled, revision| {
        // Interim bridge: canonical runtime integration replaces only this hook
        // with enablement_changed(enabled). Source owns commit/runtime ordering.
        if let Some(runtime) = RUNTIME.get() { runtime.enabled.store(enabled, Ordering::SeqCst); }
        if !enabled { player::cancel(d); }
        test_transition_applied(revision);
    })
}

// Environment-only synthetic observation gates; never expose an RPC or authority.
// Bounded waits and release-on-Drop test guards prevent an abandoned fixture hang.
pub(super) fn test_hold(point: &str, payload: &Value) -> Result<()> {
    let Some(root) = std::env::var_os(format!("OVERSEER_TEST_AUDIO_{point}_HOLD")) else { return Ok(()); };
    if let Some(key) = std::env::var_os(format!("OVERSEER_TEST_AUDIO_{point}_KEY")) {
        if payload["key"].as_str() != key.to_str() { return Ok(()); }
    }
    let dir = PathBuf::from(root);
    match std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join("claimed")) {
        Ok(_) => {},
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(_) => return Err(anyhow!("Cannot create the synthetic audio gate.")),
    }
    std::fs::write(dir.join("ready.json"), payload.to_string()).map_err(|_| anyhow!("Cannot publish the synthetic audio gate."))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !dir.join("release").exists() {
        if Instant::now() >= deadline { return Err(anyhow!("The synthetic audio gate exceeded its deadline.")); }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
fn test_transition_applied(revision: i64) {
    use std::io::Write;
    if let (Some(path), Some(runtime)) = (std::env::var_os("OVERSEER_TEST_AUDIO_TRANSITION_LOG"), RUNTIME.get()) {
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{}", json!({"revision":revision,"enabled":runtime.enabled.load(Ordering::SeqCst)}));
        }
    }
}

pub fn import_commander(_d: &Arc<Daemon>, _p: &Value) -> Result<Value> {
    Err(anyhow!("Use From folder with a complete twelve-line audio-pack.json manifest and the displayed revision."))
}

fn installed_voices() -> Result<Vec<(String, String)>> {
    if !player("/usr/bin/say") {
        return Err(anyhow!("macOS system speech is unavailable"));
    }
    let output = std::process::Command::new("/usr/bin/say")
        .args(["-v", "?"])
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("could not list system voices"));
    }
    let mut voices = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<&str> = line
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if fields.len() < 2 {
            continue;
        }
        voices.push((
            fields[..fields.len() - 1].join(" "),
            fields[fields.len() - 1].to_string(),
        ));
    }
    Ok(voices)
}

pub fn voices() -> Result<Value> {
    Ok(json!(installed_voices()?
        .into_iter()
        .map(|(name, locale)| json!({"name":name,"locale":locale}))
        .collect::<Vec<_>>()))
}

pub fn preview(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let line=source::validate_preview(d,p)?;
    enqueue_preview(line)?;
    Ok(json!({"queued":true,"key":line.key()}))
}

/// Voice's conversational voice selection remains independent of Audio packs.
pub fn voice_installed(name: &str) -> Result<bool> {
    Ok(test_sink() || installed_voices()?.iter().any(|(n,_)|n==name))
}

fn semantic_line(d: &Arc<Daemon>, tickets: &[Ticket]) -> Option<Line> {
    semantics::fresh(&d.store.lock().unwrap(),tickets).ok().flatten()
}

fn play_cue(d: &Arc<Daemon>, cue: Cue) -> Result<()> {
    let Some(runtime)=RUNTIME.get().filter(|r|r.current(&cue)) else {return Ok(())};
    let (line,preview)=match &cue.kind {
        Kind::Preview(line)=>(*line,true),
        Kind::Semantic(tickets)=>{
            let Some(line)=semantic_line(d,tickets) else {return Ok(())};
            (line,false)
        },
        Kind::Heard=>{
            // Separate feedback has no pack selection, public key or speech.
            if crate::voice::before_cue("voice_heard_feedback",350) && runtime.current(&cue) {
                player::heard_feedback(d)?;
            }
            return Ok(());
        },
    };
    if !preview && !crate::voice::before_cue(line.key(),15000) {return Ok(())};
    if !runtime.current(&cue) {return Ok(())};
    // Cardinality/specificity may change during the arbiter wait.
    let line=match &cue.kind {
        Kind::Semantic(tickets)=>match semantic_line(d,tickets) {Some(line)=>line,None=>return Ok(())},
        _=>line,
    };
    // The final player admission callback records only the live identities
    // actually represented by this clip. A resolved member of a plural batch
    // must not acquire a playback receipt from the remaining member's clip.
    let admitted=std::sync::Mutex::new(Vec::<Ticket>::new());
    let written=player::play_checked(d,line,preview,||{
        if !runtime.current(&cue) {return false;}
        match &cue.kind {
            Kind::Semantic(tickets)=>{
                let selection=semantics::selection(&d.store.lock().unwrap(),tickets).ok().flatten();
                let Some(selection)=selection.filter(|selection|selection.line==line) else {return false;};
                *admitted.lock().unwrap()=selection.tickets;
                true
            },
            _=>true,
        }
    })?;
    if written {
        let tickets=admitted.into_inner().unwrap();
        if !tickets.is_empty() {semantics::played(&d.store.lock().unwrap(),&tickets)?;}
    }
    Ok(())
}

/// Subscribe only after reconciliation; historical tail producers separately
/// retain startup provenance so asynchronously replayed exits cannot announce.
pub fn start(d: Arc<Daemon>) -> Result<()> {
    let enabled=source::snapshot(&d)?.enabled;
    let mut events=d.events.subscribe();
    let (routine,mut routine_rx)=mpsc::channel(4);
    let (urgent,mut urgent_rx)=mpsc::channel(2);
    let (feedback,mut feedback_rx)=mpsc::channel(1);
    RUNTIME.set(Runtime{enabled:AtomicBool::new(enabled),epoch:AtomicU64::new(0),
        daemon:Arc::downgrade(&d),routine,urgent,feedback})
        .map_err(|_|anyhow!("Audio worker already started."))?;
    let worker=d.clone();
    tokio::spawn(async move {
        loop {
            let cue=tokio::select! {biased;
                Some(cue)=feedback_rx.recv()=>cue,
                Some(cue)=urgent_rx.recv()=>cue,
                Some(cue)=routine_rx.recv()=>cue,
                else=>break,
            };
            let d=worker.clone();
            match tokio::task::spawn_blocking(move||play_cue(&d,cue)).await {
                Ok(Ok(()))=>{},
                _=>crate::log("audio playback failed; selected source was not changed"),
            }
        }
        player::cancel(&worker);
    });
    tokio::spawn(async move {
        let mut pending=Vec::<Ticket>::new();
        let mut deadline:Option<tokio::time::Instant>=None;
        let mut epoch=0;
        loop {
            let timeout=deadline.unwrap_or_else(||tokio::time::Instant::now()+Duration::from_secs(86400));
            tokio::select! {
                event=events.recv()=>{
                    let event=match event {
                        Ok(event)=>event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>continue,
                        Err(_)=>break,
                    };
                    let Some(runtime)=RUNTIME.get() else {break};
                    let now_epoch=runtime.epoch.load(Ordering::SeqCst);
                    if epoch!=now_epoch {pending.clear();deadline=None;epoch=now_epoch;}
                    if !runtime.enabled.load(Ordering::SeqCst) {continue;}
                    let Some(ticket)=semantics::ticket(&event) else {continue};
                    let Some(line)=semantic_line(&d,std::slice::from_ref(&ticket)) else {continue};
                    if line.urgent() {
                        if let Some(old)=pending.iter_mut().find(|old|old.subject==ticket.subject) {*old=ticket;}
                        else if pending.len()<MAX_COALESCED {pending.push(ticket);}
                        else {crate::log("audio coalescing capacity reached");}
                        if deadline.is_none() {deadline=Some(tokio::time::Instant::now()+WINDOW);}
                    } else if runtime.enqueue(Kind::Semantic(vec![ticket]),false).is_err() {
                        crate::log("audio routine queue full");
                    }
                },
                _=tokio::time::sleep_until(timeout),if deadline.is_some()=>{
                    deadline=None;
                    let tickets=std::mem::take(&mut pending);
                    let Some(runtime)=RUNTIME.get() else {break};
                    if epoch!=runtime.epoch.load(Ordering::SeqCst) || !runtime.enabled.load(Ordering::SeqCst) {continue;}
                    if semantic_line(&d,&tickets).is_some()
                        && runtime.enqueue(Kind::Semantic(tickets),true).is_err() {crate::log("audio urgent queue full");}
                },
            }
        }
    });
    Ok(())
}
