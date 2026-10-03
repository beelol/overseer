//! Private native requests. Public callers receive only daemon-issued selectors
//! and allowlisted projections, never native IDs, offers or response envelopes.
//! Slice1 deliberately cannot answer: generation-bound claims/receipts are Slice2.
use crate::adapters::{
    self,
    native_requests::{self, Family, NativeMessage, NativeRequest, Protocol},
};
use crate::store::{Event, Run, Store};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS native_pending_requests(
        key TEXT PRIMARY KEY, process_run_id TEXT NOT NULL REFERENCES runs(id),
        display_run_id TEXT NOT NULL REFERENCES runs(id), generation INTEGER NOT NULL,
        native_id TEXT NOT NULL, protocol TEXT NOT NULL, envelope TEXT NOT NULL,
        offer_digest TEXT NOT NULL, projection TEXT NOT NULL, revision INTEGER NOT NULL,
        lifecycle TEXT NOT NULL, arrival_seq INTEGER NOT NULL, changed_seq INTEGER NOT NULL,
        created_ms INTEGER NOT NULL,
        UNIQUE(process_run_id,generation,native_id));
        CREATE INDEX IF NOT EXISTS native_pending_owner ON native_pending_requests(process_run_id,generation,arrival_seq);")?;
    Ok(())
}

/// Probe the executable actually selected for this launch, outside the store lock.
/// Stdout is read through a nonblocking pipe with a 4096-byte cap and two-second
/// deadline; no output file can grow. The owned process group is killed/reaped
/// before return. This invokes --version only and publishes no arbitrary output.
pub fn launch_qualification(
    program: &Path,
    env: &std::collections::BTreeMap<String, String>,
    harness: &str,
) -> Value {
    if !matches!(harness, "codex-app" | "claude") {
        return Value::Null;
    }
    let probe = || -> Option<String> {
        let mut child = Command::new(program)
            .arg("--version")
            .current_dir(adapters::neutral_dir())
            .env_clear()
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .ok()?;
        let group = child.id() as i32;
        let result = (|| -> Option<String> {
            let mut output = child.stdout.take()?;
            let fd = output.as_raw_fd();
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return None;
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut bytes = Vec::new();
            let mut exited = None;
            let mut eof = false;
            loop {
                let mut chunk = [0u8; 4097];
                match output.read(&mut chunk[..4097 - bytes.len()]) {
                    Ok(0) => eof = true,
                    Ok(n) => {
                        bytes.extend_from_slice(&chunk[..n]);
                        if bytes.len() > 4096 {
                            return None;
                        }
                    }
                    Err(e)
                        if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                    Err(_) => return None,
                }
                if exited.is_none() {
                    exited = child.try_wait().ok()?;
                }
                if let Some(status) = exited.as_ref().filter(|_| eof) {
                    return status
                        .success()
                        .then(|| String::from_utf8(bytes).ok())
                        .flatten()
                        .map(|s| s.trim().to_string());
                }
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })();
        // Only this newly spawned probe group is addressed; never a harness run,
        // owner daemon or caller-supplied process. Close pipes before cleanup.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
        let _ = child.wait();
        result
    };
    let version = probe().unwrap_or_default();
    // Explicit installed schemas and previously qualified boolean transports.
    // Synthetic defaults are qualified only by their exact fixture version text.
    match (harness, version.as_str()) {
        ("codex-app", "codex-cli 0.158.0") => {
            json!({"qualification":"codec","protocol":"codex-0.158"})
        }
        ("claude", "2.1.288 (Claude Code)") => {
            json!({"qualification":"codec","protocol":"claude-2.1.288"})
        }
        ("codex-app", "codex-cli 0.155.0" | "codex-app-fixture 0.0.0 (synthetic)") => {
            json!({"qualification":"legacy","protocol":"codex-bool"})
        }
        ("claude", "2.1.246 (Claude Code)" | "claude-fixture 0.0.0 (synthetic)") => {
            json!({"qualification":"legacy","protocol":"claude-bool"})
        }
        _ => json!({"qualification":"unqualified"}),
    }
}

fn qualified_protocol(store: &Store, run: &Run) -> Result<(Option<Protocol>, bool)> {
    let launch: Option<String> =
        store
            .conn
            .query_row("SELECT launch FROM runs WHERE id=?1", [&run.id], |r| {
                r.get(0)
            })?;
    let metadata: Value = launch
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null);
    let q = &metadata["native_protocol"];
    Ok((
        match q["protocol"].as_str() {
            Some("codex-0.158") => Some(Protocol::Codex0158),
            Some("claude-2.1.288") => Some(Protocol::Claude21288),
            _ => None,
        },
        q["qualification"] == "legacy",
    ))
}

fn control(harness: &str, frame: &Value) -> bool {
    match harness {
        "codex-app" => {
            (frame.get("id").is_some() && frame.get("method").is_some())
                || frame["method"] == "serverRequest/resolved"
        }
        "claude" => frame["type"]
            .as_str()
            .is_some_and(|s| s.starts_with("control_")),
        _ => false,
    }
}

/// Ordinary known output follows existing adapters. Unknown native JSON never
/// becomes public raw text, even if version qualification failed.
fn known_output(harness: &str, frame: &Value) -> bool {
    match harness {
        "codex-app" => {
            if frame.get("id").is_some() && frame.get("method").is_none() {
                return matches!(
                    frame["id"].as_str(),
                    Some(
                        "ovs-init"
                            | "ovs-account"
                            | "ovs-auto-quota"
                            | "ovs-auto-tools"
                            | "ovs-thread"
                            | "ovs-turn"
                            | "ovs-interrupt"
                    )
                );
            }
            matches!(
                frame["method"].as_str(),
                Some(
                    "thread/started"
                        | "turn/started"
                        | "turn/completed"
                        | "thread/tokenUsage/updated"
                        | "account/rateLimits/updated"
                        | "error"
                        | "item/started"
                        | "item/completed"
                )
            )
        }
        "claude" => match frame["type"].as_str() {
            Some("system") => matches!(
                frame["subtype"].as_str(),
                Some(
                    "init"
                        | "background_tasks_changed"
                        | "task_started"
                        | "task_progress"
                        | "task_notification"
                )
            ),
            Some("assistant" | "user" | "result" | "rate_limit_event") => true,
            _ => false,
        },
        _ => true,
    }
}

/// Established adapters deliberately ignore these streaming fragments/replies.
/// Keep their bytes private without adding one diagnostic for every token.
fn ignored_frame(harness: &str, frame: &Value) -> bool {
    match harness {
        "claude" => matches!(
            frame["type"].as_str(),
            Some("stream_event" | "control_response")
        ),
        "codex-app" => {
            frame.get("id").is_none()
                && matches!(
                    frame["method"].as_str(),
                    Some(
                        "item/agentMessage/delta"
                            | "item/plan/delta"
                            | "command/exec/outputDelta"
                            | "process/outputDelta"
                            | "item/commandExecution/outputDelta"
                            | "item/fileChange/outputDelta"
                            | "item/reasoning/summaryTextDelta"
                            | "item/reasoning/textDelta"
                            | "thread/realtime/item/transcript/delta"
                            | "thread/realtime/transcript/delta"
                            | "thread/realtime/outputAudio/delta"
                    )
                )
        }
        _ => false,
    }
}

/// This guard is independent of the decoder/version. Native request/reply and
/// unknown JSON bodies stay in 0600 shim segments, never raw-output RPC.
pub fn public_raw(harness: &str, stream: &str, data: &str) -> String {
    if matches!(harness, "codex-app" | "claude")
        && data.lines().any(|line| {
            match serde_json::from_str::<Value>(line) {
                Ok(frame) => {
                    stream == "i"
                        || stream == "e"
                        || (harness == "codex-app"
                            && frame.get("id").is_some()
                            && frame.get("method").is_none())
                        || control(harness, &frame)
                        || !known_output(harness, &frame)
                }
                // Malformed native JSON could contain private future controls.
                Err(_) => line.trim_start().starts_with(['{', '[']),
            }
        })
    {
        "[private native protocol frame]".to_string()
    } else {
        crate::redact::redact(data)
    }
}

fn diagnostic(store: &Store, run: &Run, reason: &str, out: &mut Vec<Event>) -> Result<()> {
    out.push(store.insert_event(
        crate::daemon::now(),
        Some(&run.task_id),
        Some(&run.id),
        "native_request_diagnostic",
        "harness",
        "exact",
        &json!({"reason_code":reason,
            "capability":"native request","action":"A qualified native capability is required"}),
    )?);
    Ok(())
}

/// Called under the existing tail/store transaction before any legacy parsing.
/// `true` consumes the private frame. No shim I/O occurs under this lock.
pub fn intercept(
    store: &Store,
    run: &Run,
    stream: &str,
    data: &str,
    out: &mut Vec<Event>,
    sends: &mut Vec<String>,
) -> Result<bool> {
    if !matches!(run.harness.as_str(), "codex-app" | "claude") || matches!(stream, "i" | "x") {
        return Ok(false);
    }
    let frame: Value = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(_) if data.trim_start().starts_with(['{', '[']) => {
            diagnostic(store, run, "malformed_native_frame", out)?;
            return Ok(true);
        }
        Err(_) => return Ok(false),
    };
    if stream != "o" {
        // Native JSON printed on stderr is not an authenticated protocol request
        // and must not be echoed as an ordinary stderr body either.
        diagnostic(store, run, "native_stderr_frame", out)?;
        return Ok(true);
    }
    if ignored_frame(&run.harness, &frame) {
        return Ok(true);
    }
    if !control(&run.harness, &frame) {
        if !known_output(&run.harness, &frame) {
            diagnostic(store, run, "unsupported_native_kind", out)?;
            return Ok(true);
        }
        return Ok(false);
    }
    let current = store
        .run(&run.id)?
        .ok_or_else(|| anyhow!("native request process is unavailable"))?;
    if current.process_generation != run.process_generation {
        // Never associate an old tail frame with a replacement process.
        diagnostic(store, run, "stale_native_generation", out)?;
        return Ok(true);
    }
    let (protocol, legacy) = qualified_protocol(store, &current)?;
    if legacy {
        let bool_shape = match run.harness.as_str() {
            "codex-app" => matches!(
                frame["method"].as_str(),
                Some(
                    "execCommandApproval"
                        | "applyPatchApproval"
                        | "item/commandExecution/requestApproval"
                        | "item/fileChange/requestApproval"
                )
            ),
            "claude" => {
                frame["type"] == "control_request"
                    && frame["request"]["subtype"] == "can_use_tool"
                    && frame["request"]["tool_name"] != "AskUserQuestion"
            }
            _ => false,
        };
        let valid_identity = if run.harness == "codex-app" {
            (frame["id"].is_string() || frame["id"].as_i64().is_some())
                && frame["params"].is_object()
        } else {
            frame["request_id"].is_string()
                && frame["request"]["tool_name"].is_string()
                && frame["request"]["input"].is_object()
        };
        if bool_shape && valid_identity {
            return Ok(false);
        }
        diagnostic(store, run, "unsupported_native_kind", out)?;
        unsupported_codex_reply(&frame, &run.harness, sends);
        return Ok(true);
    }
    let Some(protocol) = protocol else {
        diagnostic(store, run, "unqualified_native_version", out)?;
        unsupported_codex_reply(&frame, &run.harness, sends);
        return Ok(true);
    };
    match native_requests::decode(protocol, &frame) {
        Ok(NativeMessage::Owner(request)) => insert(store, &current, protocol, &request, out)?,
        Ok(NativeMessage::Resolved { id, context }) => {
            let native_id = tagged(&id.value());
            let row: Option<(String, String)> = store.conn.query_row(
                "SELECT key,envelope FROM native_pending_requests WHERE process_run_id=?1 AND generation=?2 AND native_id=?3",
                params![run.id,run.process_generation,native_id], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
            if let Some((key, envelope)) = row {
                let original: Value = serde_json::from_str(&envelope)?;
                // Codex resolved frames must match the exact thread. Claude's
                // cancellation is scoped by owning pipe/generation and raw ID.
                if protocol == Protocol::Claude21288
                    || context.thread.as_deref() == original["params"]["threadId"].as_str()
                {
                    change(
                        store,
                        &key,
                        if protocol == Protocol::Claude21288 {
                            "cancelled"
                        } else {
                            "native_resolved"
                        },
                        out,
                    )?;
                    refresh_attention(store, &current)?;
                } else {
                    diagnostic(store, run, "native_context_conflict", out)?;
                }
            }
        }
        Ok(NativeMessage::Machine { .. }) => {
            diagnostic(store, run, "machine_capability_unavailable", out)?;
            unsupported_codex_reply(&frame, &run.harness, sends);
        }
        Ok(NativeMessage::Unsupported(_)) => {
            diagnostic(store, run, "unsupported_native_kind", out)?;
            unsupported_codex_reply(&frame, &run.harness, sends);
        }
        Err(_) => {
            diagnostic(store, run, "invalid_native_request", out)?;
        }
    }
    Ok(true)
}

fn unsupported_codex_reply(frame: &Value, harness: &str, sends: &mut Vec<String>) {
    if harness == "codex-app"
        && frame.get("method").is_some()
        && (frame["id"].is_string() || frame["id"].as_i64().is_some())
    {
        // This is a machine unsupported error, not owner consent or cancellation.
        sends.push(format!("{}\n", json!({"id":frame["id"],"error":{"code":-32601,"message":"Native capability unavailable in Overseer"}})));
    }
}

fn tagged(id: &Value) -> String {
    format!(
        "{}:{}",
        if id.is_string() { "string" } else { "integer" },
        id
    )
}
fn family(f: Family) -> &'static str {
    match f {
        Family::LegacyCommand | Family::Command => "command",
        Family::LegacyFile | Family::File => "file",
        Family::Permissions => "permissions",
        Family::Questions => "questions",
        Family::Tool => "tool",
        Family::Form => "form",
        Family::ExternalForm => "external_form",
        Family::Url => "url",
        Family::Verification => "verification",
    }
}
fn bool_choices(request: &NativeRequest) -> Vec<String> {
    let choices: &[&str] = match request.family() {
        Family::LegacyCommand | Family::LegacyFile => {
            &["approved", "approved_for_session", "abort"]
        }
        Family::Command | Family::File => &["accept", "acceptForSession", "decline", "cancel"],
        _ => &[],
    };
    choices
        .iter()
        .filter(|s| {
            native_requests::encode(
                request,
                request.context(),
                &native_requests::Answer::Decision { decision: json!(s) },
            )
            .is_ok()
        })
        .map(|s| (*s).to_string())
        .collect()
}
fn compatible(request: &NativeRequest) -> bool {
    if request.family() == Family::Tool {
        return true;
    }
    let choices = bool_choices(request);
    let allow = choices
        .iter()
        .any(|s| matches!(s.as_str(), "accept" | "approved"));
    let deny = choices
        .iter()
        .any(|s| matches!(s.as_str(), "decline" | "abort"));
    allow && deny
}

fn projection(request: &NativeRequest, run: &Run, display: &str, key: &str) -> Value {
    let envelope = request.envelope();
    let p = if run.harness == "claude" {
        &envelope["request"]
    } else {
        &envelope["params"]
    };
    let mut public = json!({"key":key,"revision":1,"run_id":display,"process_run_id":run.id,
        "family":family(request.family()),"lifecycle":"pending","default_to_no":request.default_to_no(),
        "suppress_always":request.suppress_always(),"reason_code":"typed_delivery_unavailable",
        "bool_compatible":compatible(request)});
    // Only user-facing fields; never raw identity, arbitrary tool input,
    // native grant descriptors, schema defaults, answers or opaque payloads.
    if matches!(request.family(), Family::LegacyCommand | Family::Command) {
        let command = p["command"]
            .as_str()
            .map(|s| s.chars().take(2000).collect::<String>());
        public["target"] = json!(command);
        public["choices"] = json!(bool_choices(request));
    }
    if request.family() == Family::Questions {
        let q = if run.harness == "claude" {
            &p["input"]["questions"]
        } else {
            &p["questions"]
        };
        let questions: Vec<Value> = q.as_array().into_iter().flatten().enumerate().map(|(n,q)| {
            let secret = q["isSecret"] == true;
            let options: Vec<Value> = q["options"].as_array().into_iter().flatten().map(|o|
                json!({"label":o["label"].as_str().unwrap_or(""),"description":o["description"].as_str().unwrap_or("")})).collect();
            json!({"key":format!("field-{n}"),"header":q["header"].as_str().unwrap_or(""),
                "question":q["question"].as_str().unwrap_or(""),"secret":secret,"options":options})
        }).collect();
        public["questions"] = json!(questions);
    }
    if matches!(
        request.family(),
        Family::Url | Family::ExternalForm | Family::Verification
    ) {
        public["reason_code"] = json!("external_authority_unqualified");
        // URL opening and external completion are not qualified in this slice.
        // Do not expose credentials, query, opaque native IDs or invented offers.
    }
    crate::daemon::redact_value(public)
}

fn insert(
    store: &Store,
    run: &Run,
    protocol: Protocol,
    request: &NativeRequest,
    out: &mut Vec<Event>,
) -> Result<()> {
    let native_id = tagged(&request.id().value());
    // serde_json's map ordering is deterministic; descriptor remains private.
    let envelope = serde_json::to_string(request.envelope())?;
    let digest = format!("{:x}", Sha256::digest(envelope.as_bytes()));
    let old: Option<String> = store.conn.query_row("SELECT offer_digest FROM native_pending_requests WHERE process_run_id=?1 AND generation=?2 AND native_id=?3",
        params![run.id,run.process_generation,native_id], |r| r.get(0)).optional()?;
    if let Some(old) = old {
        if old != digest {
            diagnostic(store, run, "native_identity_conflict", out)?;
        }
        return Ok(()); // Same raw identity never acquires new authority in one generation.
    }
    let display = if run.harness == "codex-app" {
        if let Some(thread) = request
            .context()
            .thread
            .as_deref()
            .filter(|t| run.native_id.as_deref() != Some(*t))
        {
            let child: Option<String> = store.conn.query_row(
                "WITH RECURSIVE tree(id) AS (SELECT id FROM runs WHERE id=?1 UNION SELECT r.id FROM runs r JOIN tree t ON r.parent_run_id=t.id) SELECT r.id FROM runs r JOIN tree t ON r.id=t.id WHERE r.native_id=?2 LIMIT 1",
                params![run.id,thread], |r| r.get(0)).optional()?;
            let Some(child) = child else {
                diagnostic(store, run, "unknown_native_owner", out)?;
                return Ok(());
            };
            child
        } else {
            run.id.clone()
        }
    } else {
        run.id.clone()
    };
    let key = format!("req-{}", uuid::Uuid::new_v4().simple());
    let public = projection(request, run, &display, &key);
    let created = crate::daemon::now();
    let event = store.insert_event(
        created,
        Some(&run.task_id),
        Some(&display),
        "pending_request",
        "harness",
        "exact",
        &public,
    )?;
    store.conn.execute("INSERT INTO native_pending_requests(key,process_run_id,display_run_id,generation,native_id,protocol,envelope,offer_digest,projection,revision,lifecycle,arrival_seq,changed_seq,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,'pending',?10,?10,?11)",
        params![key,run.id,display,run.process_generation,native_id,match protocol {Protocol::Codex0158=>"codex-0.158",Protocol::Claude21288=>"claude-2.1.288"},envelope,digest,public.to_string(),event.seq,created])?;
    out.push(event);
    refresh_attention(store, run)?;
    if display != run.id {
        refresh_display_attention(store, &display)?;
    }
    out.push(store.insert_event(
        created,
        Some(&run.task_id),
        Some(&run.id),
        "status",
        "harness",
        "exact",
        &json!({"status":"waiting_for_user"}),
    )?);
    Ok(())
}

fn change(store: &Store, key: &str, lifecycle: &str, out: &mut Vec<Event>) -> Result<()> {
    let (owner,display,projection,revision,old): (String,String,String,i64,String) = store.conn.query_row(
        "SELECT process_run_id,display_run_id,projection,revision,lifecycle FROM native_pending_requests WHERE key=?1", [key], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    if old != "pending" {
        return Ok(());
    }
    let mut public: Value = serde_json::from_str(&projection)?;
    public["revision"] = json!(revision + 1);
    public["lifecycle"] = json!(lifecycle);
    public["reason_code"] = json!(lifecycle);
    let run = store
        .run(&owner)?
        .ok_or_else(|| anyhow!("native request owner is unavailable"))?;
    let event = store.insert_event(
        crate::daemon::now(),
        Some(&run.task_id),
        Some(&display),
        "pending_request_changed",
        "harness",
        "exact",
        &public,
    )?;
    store.conn.execute("UPDATE native_pending_requests SET revision=?2,lifecycle=?3,projection=?4,changed_seq=?5 WHERE key=?1", params![key,revision+1,lifecycle,public.to_string(),event.seq])?;
    out.push(event);
    if display != owner {
        refresh_display_attention(store, &display)?;
    }
    Ok(())
}

/// Owning generation retirement is explicit. Lost supervisor ≠ native success.
pub fn retire(store: &Store, run: &Run, confirmed_exit: bool, out: &mut Vec<Event>) -> Result<()> {
    let mut stmt = store.conn.prepare("SELECT key FROM native_pending_requests WHERE process_run_id=?1 AND generation=?2 AND lifecycle='pending'")?;
    let keys: Vec<String> = stmt
        .query_map(params![run.id, run.process_generation], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    for key in keys {
        change(
            store,
            &key,
            if confirmed_exit {
                "process_ended"
            } else {
                "process_unknown"
            },
            out,
        )?;
    }
    Ok(())
}

fn attention(public: &Value) -> Value {
    json!({"kind":if public["bool_compatible"] == true {"permission"} else {"native_request"},
        "request_id":public["key"],"typed_native":true,"revision":public["revision"],
        "tool":format!("Native {} request",public["family"].as_str().unwrap_or("capability")),
        "input":{},"reason_code":public["reason_code"]})
}
fn refresh_display_attention(store: &Store, display: &str) -> Result<()> {
    let row: Option<String> = store.conn.query_row("SELECT p.projection FROM native_pending_requests p JOIN runs r ON r.id=p.process_run_id WHERE p.display_run_id=?1 AND p.generation=r.process_generation AND p.lifecycle='pending' ORDER BY p.arrival_seq LIMIT 1",[display],|r|r.get(0)).optional()?;
    if let Some(row) = row {
        let public: Value = serde_json::from_str(&row)?;
        store.set_run_attention(display, Some(&attention(&public)))?;
        store.update_run_status(display, "waiting_for_user", None, None)?;
    } else if store
        .run(display)?
        .and_then(|r| r.attention)
        .is_some_and(|a| a["typed_native"] == true)
    {
        store.set_run_attention(display, None)?;
        if store
            .run(display)?
            .is_some_and(|r| r.status == "waiting_for_user")
        {
            store.update_run_status(display, "running", None, None)?;
        }
    }
    Ok(())
}
pub fn refresh_attention(store: &Store, run: &Run) -> Result<()> {
    if !store
        .run(&run.id)?
        .is_some_and(|r| r.process_generation == run.process_generation)
    {
        return Ok(());
    }
    let rows = collection(store, Some(&run.id), true)?;
    let rows = rows["requests"].as_array().expect("collection array");
    let pending: Vec<&Value> = rows
        .iter()
        .filter(|p| p["lifecycle"] == "pending")
        .collect();
    if let Some(first) = pending
        .iter()
        .find(|p| p["bool_compatible"] == true)
        .copied()
        .or_else(|| pending.first().copied())
    {
        store.set_run_attention(&run.id, Some(&attention(first)))?;
        store.update_run_status(&run.id, "waiting_for_user", None, None)?;
    } else if store
        .run(&run.id)?
        .and_then(|r| r.attention)
        .is_some_and(|a| a["typed_native"] == true)
    {
        store.set_run_attention(&run.id, None)?;
        if store
            .run(&run.id)?
            .is_some_and(|r| r.status == "waiting_for_user")
        {
            store.update_run_status(&run.id, "running", None, None)?;
        }
    }
    Ok(())
}

/// Current process generations only. The cursor is durable collection change,
/// not global event activity; it includes past generation retirement changes.
pub fn collection(store: &Store, run: Option<&str>, include_hidden: bool) -> Result<Value> {
    if let Some(id) = run {
        if store.run(id)?.is_none() {
            return Err(anyhow!("unknown run"));
        }
    }
    let mut stmt = store.conn.prepare("SELECT p.projection FROM native_pending_requests p JOIN runs r ON r.id=p.process_run_id
        WHERE p.generation=r.process_generation AND (?1 IS NULL OR p.process_run_id=?1 OR p.display_run_id=?1)
        AND (?2 OR NOT EXISTS(SELECT 1 FROM run_roles rr WHERE rr.run_id=p.process_run_id AND rr.role='overseer')) ORDER BY p.arrival_seq")?;
    let raw: Vec<String> = stmt
        .query_map(params![run, include_hidden], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let requests: Vec<Value> = raw
        .iter()
        .map(|s| serde_json::from_str(s))
        .collect::<serde_json::Result<_>>()?;
    let cursor: i64 = store.conn.query_row("SELECT COALESCE(MAX(p.changed_seq),0) FROM native_pending_requests p WHERE (?1 IS NULL OR p.process_run_id=?1 OR p.display_run_id=?1)
        AND (?2 OR NOT EXISTS(SELECT 1 FROM run_roles rr WHERE rr.run_id=p.process_run_id AND rr.role='overseer'))",params![run,include_hidden],|r|r.get(0))?;
    Ok(json!({"requests":requests,"cursor":cursor}))
}
