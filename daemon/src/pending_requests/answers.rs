//! Private typed answer claims. Public inputs select stored authority; they do
//! not supply native identity, generation, offered scope or caller provenance.
use super::*;
use crate::daemon::Daemon;
use crate::server::{NativeAuthority, ProtoError};
use crate::shim::LaunchFile;
use std::sync::{Mutex, MutexGuard, TryLockError};

/// Registration precedes claim publication. Registry guards never survive
/// entry into a process gate/Store scope, including this guard's destructor.
struct ActiveClaim<'a> {
    daemon: &'a Daemon,
    token: String,
}
impl<'a> ActiveClaim<'a> {
    fn register(daemon: &'a Daemon, token: &str) -> Self {
        daemon
            .native_active_claims
            .lock()
            .unwrap()
            .insert(token.to_string());
        Self {
            daemon,
            token: token.to_string(),
        }
    }
}
impl Drop for ActiveClaim<'_> {
    fn drop(&mut self) {
        self.daemon
            .native_active_claims
            .lock()
            .unwrap()
            .remove(&self.token);
    }
}

fn gate_until<T>(lock: &Mutex<T>, deadline: Instant) -> Result<MutexGuard<'_, T>> {
    loop {
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => return Err(refuse("native_unqualified")),
            Err(TryLockError::WouldBlock) => {}
        }
        if Instant::now() >= deadline {
            return Err(refuse("native_busy"));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub(super) fn refuse(code: &'static str) -> anyhow::Error {
    ProtoError::new(
        code,
        match code {
            "revoked" => "This phone is no longer paired",
            "watch_only" => "This phone can watch but cannot answer",
            "stale_generation" => "The request belongs to an earlier process",
            "request_resolved" => "The native request is no longer waiting",
            "already_answered" => "An answer already claimed this request",
            "stale_request" => "Read the current request before answering",
            "scope_enlarged" => "The answer exceeds the native offered scope",
            "native_veto" => "The native request forbids persistent approval",
            "native_unqualified" => "This native answer is not qualified",
            "invalid_params" => {
                "Only the stored request selector, revision and typed answer are accepted"
            }
            "invalid_answer" => "The answer does not match this native request",
            _ => "Native answer could not be accepted",
        },
    )
    .into()
}
struct Item {
    key: String,
    owner: String,
    display: String,
    generation: i64,
    protocol: String,
    envelope: String,
    offer_digest: String,
    revision: i64,
    lifecycle: String,
    token: Option<String>,
    digest: Option<String>,
}
fn item(store: &Store, key: &str) -> Result<Item> {
    store.conn.query_row("SELECT key,process_run_id,display_run_id,generation,protocol,envelope,offer_digest,revision,lifecycle,delivery_token,answer_digest FROM native_pending_requests WHERE key=?1",[key],|r|Ok(Item {
        key:r.get(0)?,owner:r.get(1)?,display:r.get(2)?,generation:r.get(3)?,protocol:r.get(4)?,
        envelope:r.get(5)?,offer_digest:r.get(6)?,revision:r.get(7)?,lifecycle:r.get(8)?,token:r.get(9)?,digest:r.get(10)?,
    })).optional()?.ok_or_else(||refuse("stale_request"))
}
pub(super) fn migrate(conn: &Connection) -> Result<()> {
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(native_pending_requests)")?
        .query_map([], |r| r.get(1))?
        .collect::<rusqlite::Result<_>>()?;
    for name in ["delivery_token", "answer_digest"] {
        if !columns.iter().any(|c| c == name) {
            conn.execute_batch(&format!(
                "ALTER TABLE native_pending_requests ADD COLUMN {name} TEXT"
            ))?;
        }
    }
    conn.execute_batch("CREATE TABLE IF NOT EXISTS native_answer_attempts(
        delivery_token TEXT PRIMARY KEY, request_key TEXT NOT NULL REFERENCES native_pending_requests(key),
        generation INTEGER NOT NULL, answer_digest TEXT NOT NULL, actor TEXT NOT NULL,
        created_ms INTEGER NOT NULL, result TEXT);")?;
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(native_answer_attempts)")?
        .query_map([], |r| r.get(1))?
        .collect::<rusqlite::Result<_>>()?;
    for name in ["denied_tool", "denied_detail"] {
        if !columns.iter().any(|c| c == name) {
            conn.execute_batch(&format!(
                "ALTER TABLE native_answer_attempts ADD COLUMN {name} TEXT"
            ))?;
        }
    }
    let added_delivery = !columns.iter().any(|c| c == "receipt_delivery");
    if added_delivery {
        conn.execute_batch("ALTER TABLE native_answer_attempts ADD COLUMN receipt_delivery TEXT")?;
    }
    if !columns.iter().any(|c| c == "recovery_cursor") {
        conn.execute_batch("ALTER TABLE native_answer_attempts ADD COLUMN recovery_cursor INTEGER NOT NULL DEFAULT 0")?;
    }
    conn.execute_batch("CREATE TABLE IF NOT EXISTS native_receipt_recovery_cursor(
        id INTEGER PRIMARY KEY CHECK(id=1), value INTEGER NOT NULL);
        INSERT OR IGNORE INTO native_receipt_recovery_cursor(id,value) VALUES(1,0);
        CREATE INDEX IF NOT EXISTS native_attempt_recovery ON native_answer_attempts(receipt_delivery,recovery_cursor,created_ms,delivery_token);")?;
    if added_delivery {
        let previous: Vec<(String, String)> = conn
            .prepare(
                "SELECT delivery_token,result FROM native_answer_attempts WHERE result IS NOT NULL",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (token, result) in previous {
            let result: Value = serde_json::from_str(&result).unwrap_or(Value::Null);
            if let Some(delivery @ ("written" | "not_written")) = result["delivery"].as_str() {
                conn.execute(
                    "UPDATE native_answer_attempts SET receipt_delivery=?2 WHERE delivery_token=?1",
                    params![token, delivery],
                )?;
            }
        }
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS native_denial_receipts(
        delivery_token TEXT PRIMARY KEY REFERENCES native_answer_attempts(delivery_token),
        denied_rowid INTEGER NOT NULL UNIQUE);",
    )?;
    Ok(())
}

/// Canonical operation comes from the saved native offer, after its submitted
/// answer passed the codec. Never use public title/preview or offered roots as
/// an invented file operation. Private fallback input is never shown in errors.
fn denial_descriptor(row: &Item, answer: &Value) -> Result<Option<(String, String)>> {
    let envelope: Value = serde_json::from_str(&row.envelope)?;
    let p = if row.protocol == "claude-2.1.288" {
        &envelope["request"]
    } else {
        &envelope["params"]
    };
    if answer["kind"] == "tool" && answer["allow"] == false {
        let input = &p["input"];
        let detail = input["command"]
            .as_str()
            .or(input["file_path"].as_str())
            .or(input["path"].as_str())
            .map(str::to_string)
            .unwrap_or_else(|| input.to_string().chars().take(200).collect());
        return Ok(Some((
            p["tool_name"].as_str().unwrap_or("native tool").to_string(),
            detail,
        )));
    }
    if answer["kind"] == "decision"
        && matches!(answer["decision"].as_str(), Some("decline" | "abort"))
    {
        if let Some(command) = p["command"].as_str() {
            return Ok(Some(("command".to_string(), command.to_string())));
        }
        // File approval descriptors need exact item changes; grantRoot alone is
        // only offered scope. This family remains explicitly unqualified here.
    }
    Ok(None)
}

fn record_written_denial(store: &Store, token: &str) -> Result<()> {
    let row: Option<(String,String,String)> = store.conn.query_row(
        "SELECT p.display_run_id,a.denied_tool,a.denied_detail FROM native_answer_attempts a JOIN native_pending_requests p ON p.key=a.request_key
         WHERE a.delivery_token=?1 AND a.denied_tool IS NOT NULL AND a.denied_detail IS NOT NULL AND NOT EXISTS(SELECT 1 FROM native_denial_receipts n WHERE n.delivery_token=a.delivery_token)",
        [token],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    if let Some((run, tool, detail)) = row {
        store.conn.execute(
            "INSERT INTO denied_permissions(run_id,tool,detail,ts) VALUES(?1,?2,?3,?4)",
            params![run, tool, detail, crate::daemon::now()],
        )?;
        store.conn.execute(
            "INSERT INTO native_denial_receipts(delivery_token,denied_rowid) VALUES(?1,?2)",
            params![token, store.conn.last_insert_rowid()],
        )?;
    }
    Ok(())
}
fn authority(store: &Store, a: &NativeAuthority, key: &str, revision: i64) -> Result<Value> {
    Ok(match a {
        NativeAuthority::LocalOwner => json!({"origin":"local_owner"}),
        NativeAuthority::Device(id) => {
            let dev = store
                .device(id)?
                .filter(|d| d.revoked_ms.is_none())
                .ok_or_else(|| refuse("revoked"))?;
            if dev.scope != "full" {
                return Err(refuse("watch_only"));
            }
            json!({"origin":"device","device_id":id})
        }
        NativeAuthority::ConfirmedVoice {
            request_key,
            revision: confirmed,
        } => {
            if request_key != key || *confirmed != revision {
                return Err(refuse("stale_request"));
            }
            json!({"origin":"confirmed_voice"})
        }
        NativeAuthority::ConfirmedOverseer {
            request_key,
            revision: confirmed,
            proposal,
            device,
        } => {
            if request_key != key || *confirmed != revision {
                return Err(refuse("stale_request"));
            }
            if let Some(id) = device {
                authority(store, &NativeAuthority::Device(id.clone()), key, revision)?;
            }
            let mut actor = json!({"origin":"confirmed_overseer","proposal":proposal});
            if let Some(id) = device {
                actor["device_id"] = json!(id);
            }
            actor
        }
    })
}
fn live(store: &Store, row: &Item, run_id: &str) -> Result<Run> {
    if row.display != run_id && row.owner != run_id {
        return Err(refuse("stale_request"));
    }
    let run = store
        .run(&row.owner)?
        .ok_or_else(|| refuse("stale_generation"))?;
    if run.process_generation != row.generation {
        return Err(refuse("stale_generation"));
    }
    if matches!(
        row.lifecycle.as_str(),
        "native_resolved" | "cancelled" | "process_ended" | "process_unknown"
    ) {
        return Err(refuse("request_resolved"));
    }
    if !crate::daemon::ACTIVE.contains(&run.status.as_str()) {
        return Err(refuse("request_resolved"));
    }
    let (dir, _, _) = store
        .run_process(&row.owner)?
        .ok_or_else(|| refuse("native_unqualified"))?;
    if Path::new(&dir).join("interrupt.requested").exists() {
        return Err(refuse("request_resolved"));
    }
    let protocol = match row.protocol.as_str() {
        "codex-0.158" => Protocol::Codex0158,
        "claude-2.1.288" => Protocol::Claude21288,
        _ => return Err(refuse("native_unqualified")),
    };
    let envelope: Value =
        serde_json::from_str(&row.envelope).map_err(|_| refuse("native_unqualified"))?;
    let NativeMessage::Owner(request) =
        native_requests::decode(protocol, &envelope).map_err(|_| refuse("native_unqualified"))?
    else {
        return Err(refuse("native_unqualified"));
    };
    super::current_context(store, &run, &request)?;
    Ok(run)
}
fn response(store: &Store, row: &Item, answer: &Value) -> Result<String> {
    let protocol = match row.protocol.as_str() {
        "codex-0.158" => Protocol::Codex0158,
        "claude-2.1.288" => Protocol::Claude21288,
        _ => return Err(refuse("native_unqualified")),
    };
    if format!("{:x}", Sha256::digest(row.envelope.as_bytes())) != row.offer_digest {
        return Err(refuse("native_unqualified"));
    }
    let envelope: Value =
        serde_json::from_str(&row.envelope).map_err(|_| refuse("native_unqualified"))?;
    let NativeMessage::Owner(request) =
        native_requests::decode(protocol, &envelope).map_err(|_| refuse("native_unqualified"))?
    else {
        return Err(refuse("native_unqualified"));
    };
    let run = store
        .run(&row.owner)?
        .ok_or_else(|| refuse("stale_generation"))?;
    let current_context = super::current_context(store, &run, &request)?;
    let mut answer: native_requests::Answer =
        serde_json::from_value(answer.clone()).map_err(|_| refuse("invalid_answer"))?;
    if let native_requests::Answer::Questions { answers } = &mut answer {
        let q = if protocol == Protocol::Claude21288 {
            &envelope["request"]["input"]["questions"]
        } else {
            &envelope["params"]["questions"]
        };
        let q = q.as_array().ok_or_else(|| refuse("native_unqualified"))?;
        if answers.len() != q.len() {
            return Err(refuse("invalid_answer"));
        }
        let mut native = std::collections::BTreeMap::new();
        for (n, question) in q.iter().enumerate() {
            let values = answers
                .get(&format!("field-{n}"))
                .ok_or_else(|| refuse("invalid_answer"))?;
            let key = question[if protocol == Protocol::Claude21288 {
                "question"
            } else {
                "id"
            }]
            .as_str()
            .ok_or_else(|| refuse("native_unqualified"))?;
            native.insert(key.to_string(), values.clone());
        }
        *answers = native;
    }
    let value = native_requests::encode(&request, &current_context, &answer).map_err(|e| {
        refuse(match e {
            native_requests::CodecError::Enlarged => "scope_enlarged",
            native_requests::CodecError::NativeVeto => "native_veto",
            native_requests::CodecError::Unqualified => "native_unqualified",
            _ => "invalid_answer",
        })
    })?;
    let data = format!("{value}\n");
    if data.len() > crate::shim::MAX_LINE_BYTES {
        return Err(refuse("invalid_answer"));
    }
    Ok(data)
}
fn update(
    store: &Store,
    row: &Item,
    lifecycle: &str,
    reason: &str,
    actor: &Value,
) -> Result<Event> {
    let mut public: Value = serde_json::from_str(&store.conn.query_row(
        "SELECT projection FROM native_pending_requests WHERE key=?1",
        [&row.key],
        |r| r.get::<_, String>(0),
    )?)?;
    public["revision"] = json!(row.revision + 1);
    public["lifecycle"] = json!(lifecycle);
    public["reason_code"] = json!(reason);
    public["actor"] = actor.clone();
    let run = store
        .run(&row.owner)?
        .ok_or_else(|| refuse("stale_generation"))?;
    let event = store.insert_event(
        crate::daemon::now(),
        Some(&run.task_id),
        Some(&row.display),
        "pending_request_changed",
        "user",
        "exact",
        &public,
    )?;
    store.conn.execute("UPDATE native_pending_requests SET revision=?2,lifecycle=?3,projection=?4,changed_seq=?5 WHERE key=?1",
        params![row.key,row.revision+1,lifecycle,public.to_string(),event.seq])?;
    super::refresh_attention(store, &run)?;
    if row.owner != row.display {
        super::refresh_display_attention(store, &row.display)?;
    }
    Ok(event)
}
fn frozen_socket(store: &Store, row: &Item) -> Result<std::path::PathBuf> {
    let (dir, _, _) = store
        .run_process(&row.owner)?
        .ok_or_else(|| refuse("native_unqualified"))?;
    let launch: LaunchFile = serde_json::from_slice(
        &std::fs::read(Path::new(&dir).join("launch.json"))
            .map_err(|_| refuse("native_unqualified"))?,
    )
    .map_err(|_| refuse("native_unqualified"))?;
    if launch.native_reply_generation != Some(row.generation) {
        return Err(refuse("native_unqualified"));
    }
    Ok(launch.control_socket.into())
}

/// Startup-only synthetic seam. Caller enters only after all locks are dropped.
/// Timeout never approves or sends; the final recheck still decides authority.
pub(crate) fn hold(phase: &str, key: &str) -> Result<()> {
    if std::env::var("OVERSEER_TEST_NET").as_deref() != Ok("1")
        || std::env::var("FIXTURE_MODE").as_deref() != Ok("native-pending")
    {
        return Ok(());
    }
    let Some(dir) = std::env::var_os("OVERSEER_TEST_NATIVE_ANSWER_GATE")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        return Ok(());
    };
    let config: Value = std::fs::read(dir.join("config.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    if config["phase"] != phase || config["request_key"] != key || dir.join("release").exists() {
        return Ok(());
    }
    std::fs::write(
        dir.join("reached.json"),
        json!({"phase":phase,"request_key":key}).to_string(),
    )
    .map_err(|_| refuse("native_unqualified"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !dir.join("release").exists() {
        if Instant::now() >= deadline {
            return Err(refuse("native_unqualified"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

/// No control client has been constructed at this point. Seal that attempt's
/// first refusal even if final gate acquisition failed; never leave a hidden
/// NULL-result claim that can be mistaken for a retriable automatic send.
fn seal_no_contact(
    d: &Daemon,
    key: &str,
    id: &str,
    token: &str,
    actor: &Value,
    error: &anyhow::Error,
) -> Result<()> {
    let store = d.store.lock().unwrap();
    let row = item(&store, key)?;
    let result = json!({"error":crate::server::error_value(error)});
    let tx = store.conn.unchecked_transaction()?;
    store.conn.execute(
        "UPDATE native_answer_attempts SET result=?2 WHERE delivery_token=?1 AND result IS NULL",
        params![token, result.to_string()],
    )?;
    save_receipt_delivery(&store, token, "not_written")?;
    // Store serialization decides whether same-generation restoration still
    // applies. A concurrently published replacement/resolution is never reset.
    let event = if row.lifecycle == "claimed"
        && row.token.as_deref() == Some(token)
        && live(&store, &row, id).is_ok()
    {
        Some(update(&store, &row, "pending", "definitely_unsent", actor)?)
    } else {
        None
    };
    tx.commit()?;
    if let Some(event) = event {
        let _ = d.events.send(event);
    }
    Ok(())
}

pub(crate) fn answer(d: &Daemon, p: &Value) -> Result<Value> {
    if !p.as_object().is_some_and(|o| {
        o.len() == 4
            && o.keys()
                .all(|k| ["run_id", "request_key", "revision", "answer"].contains(&k.as_str()))
    }) {
        return Err(refuse("invalid_params"));
    }
    let id = p["run_id"]
        .as_str()
        .ok_or_else(|| refuse("invalid_params"))?;
    let key = p["request_key"]
        .as_str()
        .ok_or_else(|| refuse("invalid_params"))?;
    let revision = p["revision"]
        .as_i64()
        .filter(|r| *r > 0)
        .ok_or_else(|| refuse("invalid_params"))?;
    let trusted = crate::server::native_authority().ok_or_else(|| refuse("native_unqualified"))?;
    let owner = item(&d.store.lock().unwrap(), key)?.owner;
    let device = match &trusted {
        NativeAuthority::Device(id) => Some(d.native_device_gate(id)),
        NativeAuthority::ConfirmedOverseer {
            device: Some(id), ..
        } => Some(d.native_device_gate(id)),
        _ => None,
    };
    let process = d.native_process_gate(&owner);
    // Preliminary pure validation. No claim or private reply survives this scope.
    {
        let deadline = Instant::now() + Duration::from_secs(2);
        let _device = device
            .as_ref()
            .map(|g| gate_until(g, deadline))
            .transpose()?;
        let _process = gate_until(&process, deadline)?;
        let store = d.store.lock().unwrap();
        let row = item(&store, key)?;
        authority(&store, &trusted, key, revision)?;
        live(&store, &row, id)?;
        if row.lifecycle != "pending" {
            return Err(refuse("already_answered"));
        }
        if row.revision != revision {
            return Err(refuse("stale_request"));
        }
        response(&store, &row, &p["answer"])?;
    }
    hold("validated_before_claim", key)?;
    let token = format!("delivery-{}", uuid::Uuid::new_v4().simple());
    let _active_claim = ActiveClaim::register(d, &token);
    let (token, digest, data, socket, generation, claimed_revision, actor) = {
        let deadline = Instant::now() + Duration::from_secs(2);
        let _device = device
            .as_ref()
            .map(|g| gate_until(g, deadline))
            .transpose()?;
        let _process = gate_until(&process, deadline)?;
        let store = d.store.lock().unwrap();
        let row = item(&store, key)?;
        let actor = authority(&store, &trusted, key, revision)?;
        live(&store, &row, id)?;
        if row.lifecycle != "pending" {
            return Err(refuse("already_answered"));
        }
        if row.revision != revision {
            return Err(refuse("stale_request"));
        }
        let data = response(&store, &row, &p["answer"])?;
        let denial = denial_descriptor(&row, &p["answer"])?;
        let digest = format!("{:x}", Sha256::digest(data.as_bytes()));
        let socket = frozen_socket(&store, &row)?;
        let tx = store.conn.unchecked_transaction()?;
        store.conn.execute("INSERT INTO native_answer_attempts(delivery_token,request_key,generation,answer_digest,actor,created_ms,denied_tool,denied_detail) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![token,key,row.generation,digest,actor.to_string(),crate::daemon::now(),denial.as_ref().map(|d|d.0.as_str()),denial.as_ref().map(|d|d.1.as_str())])?;
        store.conn.execute(
            "UPDATE native_pending_requests SET delivery_token=?2,answer_digest=?3 WHERE key=?1",
            params![key, token, digest],
        )?;
        let event = update(&store, &row, "claimed", "answer_claimed", &actor)?;
        tx.commit()?;
        let _ = d.events.send(event);
        (
            token,
            digest,
            data,
            socket,
            row.generation,
            row.revision + 1,
            actor,
        )
    };
    let held = hold("claimed_before_send", key);
    let deadline = Instant::now() + Duration::from_secs(2);
    let device_result = device.as_ref().map(|g| gate_until(g, deadline)).transpose();
    let _device = match device_result {
        Ok(guard) => guard,
        Err(error) => {
            seal_no_contact(d, key, id, &token, &actor, &error)?;
            return Err(error);
        }
    };
    let _process = match gate_until(&process, deadline) {
        Ok(guard) => guard,
        Err(error) => {
            drop(_device);
            seal_no_contact(d, key, id, &token, &actor, &error)?;
            return Err(error);
        }
    };
    let final_check = (|| -> Result<()> {
        held?;
        let store = d.store.lock().unwrap();
        let row = item(&store, key)?;
        authority(&store, &trusted, key, revision)?;
        live(&store, &row, id)?;
        if row.lifecycle != "claimed"
            || row.revision != claimed_revision
            || row.token.as_deref() != Some(token.as_str())
            || row.digest.as_deref() != Some(digest.as_str())
        {
            return Err(refuse("already_answered"));
        }
        if frozen_socket(&store, &row)? != socket {
            return Err(refuse("stale_generation"));
        }
        Ok(())
    })();
    if let Err(error) = final_check {
        seal_no_contact(d, key, id, &token, &actor, &error)?;
        return Err(error);
    }
    // No Store lock during I/O. Device/process guards protect final authority;
    // the connector and pipe writer both have aggregate bounded deadlines.
    let receipt = crate::shim::native_reply::control(
        &socket,
        &json!({"op":"request_reply","generation":generation,
        "delivery_token":token,"answer_digest":digest,"data":data}),
    );
    let delivery = match receipt
        .as_ref()
        .ok()
        .filter(|r| {
            r["ok"] == true
                && r["generation"] == generation
                && r["delivery_token"] == token
                && r["answer_digest"] == digest
        })
        .and_then(|r| r["state"].as_str())
    {
        Some("written") => "written",
        Some("not_written") => "not_written",
        _ => "uncertain",
    };
    let lifecycle = match delivery {
        "written" => "answered_awaiting_native",
        "not_written" => "pending",
        _ => "uncertain",
    };
    let store = d.store.lock().unwrap();
    let row = item(&store, key)?;
    let result = json!({"request_key":key,"revision":row.revision+1,"delivery":delivery,"lifecycle":lifecycle});
    let tx = store.conn.unchecked_transaction()?;
    store.conn.execute(
        "UPDATE native_answer_attempts SET result=?2 WHERE delivery_token=?1 AND result IS NULL",
        params![token, result.to_string()],
    )?;
    save_receipt_delivery(&store, &token, delivery)?;
    if delivery == "written" {
        record_written_denial(&store, &token)?;
    }
    let event = update(
        &store,
        &row,
        lifecycle,
        if delivery == "not_written" {
            "definitely_unsent"
        } else {
            delivery
        },
        &actor,
    )?;
    tx.commit()?;
    let _ = d.events.send(event);
    Ok(result)
}

/// Boolean compatibility is deliberately limited to a checked stored offer.
/// The public selector/revision is preserved through the same typed claim path.
pub(crate) fn bool_answer(
    d: &Daemon,
    id: &str,
    key: &str,
    revision: Option<i64>,
    allow: bool,
    message: &str,
    always: bool,
) -> Result<Option<Value>> {
    let value = {
        let store = d.store.lock().unwrap();
        let projection: Option<String> = store
            .conn
            .query_row(
                "SELECT projection FROM native_pending_requests WHERE key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        let Some(projection) = projection else {
            return Ok(None);
        };
        let row = item(&store, key)?;
        if row.display != id && row.owner != id {
            return Err(refuse("stale_request"));
        }
        let projection: Value = serde_json::from_str(&projection)?;
        if projection["bool_compatible"] != true {
            return Err(refuse("invalid_answer"));
        }
        let revision = revision.ok_or_else(|| refuse("invalid_params"))?;
        let protocol = match row.protocol.as_str() {
            "codex-0.158" => Protocol::Codex0158,
            "claude-2.1.288" => Protocol::Claude21288,
            _ => return Err(refuse("native_unqualified")),
        };
        let envelope: Value = serde_json::from_str(&row.envelope)?;
        let NativeMessage::Owner(request) = native_requests::decode(protocol, &envelope)
            .map_err(|_| refuse("native_unqualified"))?
        else {
            return Err(refuse("native_unqualified"));
        };
        if always {
            return Err(refuse(if request.suppress_always() {
                "native_veto"
            } else {
                "native_unqualified"
            }));
        }
        let answer = match request.family() {
            Family::Tool => json!({"kind":"tool","allow":allow,"message":message}),
            Family::LegacyCommand | Family::LegacyFile => {
                json!({"kind":"decision","decision":if allow {"approved"} else {"abort"}})
            }
            Family::Command | Family::File => {
                json!({"kind":"decision","decision":if allow {"accept"} else {"decline"}})
            }
            _ => return Err(refuse("invalid_answer")),
        };
        json!({"run_id":id,"request_key":key,"revision":revision,"answer":answer})
    };
    answer(d, &value).map(Some)
}

/// The inspection cursor is private: reserving a page does not change any
/// request, public cursor, first answer result or native grant. Advance even
/// skipped/uncertain candidates so they cannot starve a later orphan receipt.
struct RecoveryAttempt {
    key: String,
    owner: String,
    token: String,
    digest: String,
    generation: i64,
    actor: String,
}
fn recovery_page(store: &Store, run_id: Option<&str>) -> Result<Vec<RecoveryAttempt>> {
    reserve_recovery_page(&store.conn, run_id)
}
fn reserve_recovery_page(conn: &Connection, run_id: Option<&str>) -> Result<Vec<RecoveryAttempt>> {
    let tx = conn.unchecked_transaction()?;
    let rows = {
        let mut query = conn.prepare("SELECT p.key,p.process_run_id,a.delivery_token,a.answer_digest,a.generation,a.actor
            FROM native_answer_attempts a JOIN native_pending_requests p ON p.key=a.request_key
            JOIN runs r ON r.id=p.process_run_id
            WHERE a.receipt_delivery IS NULL AND a.generation=p.generation AND a.generation=r.process_generation
            AND (?1 IS NULL OR p.process_run_id=?1 OR p.display_run_id=?1)
            ORDER BY a.recovery_cursor,a.created_ms,a.delivery_token LIMIT 16")?;
        let rows = query
            .query_map([run_id], |r| {
                Ok(RecoveryAttempt {
                    key: r.get(0)?,
                    owner: r.get(1)?,
                    token: r.get(2)?,
                    digest: r.get(3)?,
                    generation: r.get(4)?,
                    actor: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    if !rows.is_empty() {
        let cursor: i64 = conn.query_row(
            "SELECT value FROM native_receipt_recovery_cursor WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let next = cursor
            .checked_add(1)
            .ok_or_else(|| refuse("native_unqualified"))?;
        conn.execute(
            "UPDATE native_receipt_recovery_cursor SET value=?1 WHERE id=1",
            [next],
        )?;
        for row in &rows {
            conn.execute(
                "UPDATE native_answer_attempts SET recovery_cursor=?2 WHERE delivery_token=?1",
                params![row.token, next],
            )?;
        }
    }
    tx.commit()?;
    Ok(rows)
}
fn save_receipt_delivery(store: &Store, token: &str, delivery: &str) -> Result<()> {
    if matches!(delivery, "written" | "not_written") {
        store.conn.execute("UPDATE native_answer_attempts SET receipt_delivery=?2 WHERE delivery_token=?1 AND receipt_delivery IS NULL",params![token,delivery])?;
    }
    Ok(())
}
/// Status evidence needs the surviving qualified transport, not an active turn.
/// It never grants permission to send, reopen a request or use a replacement.
fn historical_socket(
    store: &Store,
    row: &Item,
    attempt: &RecoveryAttempt,
) -> Result<std::path::PathBuf> {
    let run = store
        .run(&attempt.owner)?
        .ok_or_else(|| refuse("stale_generation"))?;
    if row.owner != attempt.owner
        || row.generation != attempt.generation
        || run.process_generation != attempt.generation
    {
        return Err(refuse("stale_generation"));
    }
    frozen_socket(store, row)
}
fn current_orphan(row: &Item, attempt: &RecoveryAttempt, revision: i64) -> bool {
    row.revision == revision
        && row.token.as_deref() == Some(attempt.token.as_str())
        && row.digest.as_deref() == Some(attempt.digest.as_str())
        && matches!(row.lifecycle.as_str(), "claimed" | "uncertain")
}

/// Query receipts only. Historical written policy is independent of live answer
/// eligibility; terminal/superseded requests retain their exact public state.
pub(crate) fn reconcile(d: &Daemon, run_id: Option<&str>) -> Result<()> {
    let rows = recovery_page(&d.store.lock().unwrap(), run_id)?;
    for attempt in rows {
        let process = d.native_process_gate(&attempt.owner);
        let _process = match gate_until(&process, Instant::now() + Duration::from_secs(2)) {
            Ok(g) => g,
            Err(_) => continue,
        };
        // Registration precedes claim publication under this process gate.
        // Release registry before Store: neither queries nor tombstones may
        // interfere with a daemon-owned answer that has not contacted the shim.
        if d.native_active_claims
            .lock()
            .unwrap()
            .contains(&attempt.token)
        {
            continue;
        }
        let (socket, revision) = {
            let store = d.store.lock().unwrap();
            let row = item(&store, &attempt.key)?;
            let socket = match historical_socket(&store, &row, &attempt) {
                Ok(s) => s,
                Err(_) => continue,
            };
            (socket, row.revision)
        };
        // Strictly no `data`, answer decode/encode, launch or native reply.
        let receipt = crate::shim::native_reply::control(
            &socket,
            &json!({
            "op":"request_reply_status","generation":attempt.generation,
            "delivery_token":attempt.token,"answer_digest":attempt.digest}),
        );
        let delivery = match receipt
            .as_ref()
            .ok()
            .filter(|r| {
                r["ok"] == true
                    && r["generation"] == attempt.generation
                    && r["delivery_token"] == attempt.token
                    && r["answer_digest"] == attempt.digest
            })
            .and_then(|r| r["state"].as_str())
        {
            Some("written") => "written",
            Some("not_written") => "not_written",
            _ => "uncertain",
        };
        let lifecycle = match delivery {
            "written" => "answered_awaiting_native",
            "not_written" => "pending",
            _ => "uncertain",
        };
        let reason = if delivery == "not_written" {
            "definitely_unsent"
        } else {
            delivery
        };
        let event = {
            let store = d.store.lock().unwrap();
            let row = item(&store, &attempt.key)?;
            if historical_socket(&store, &row, &attempt).ok().as_ref() != Some(&socket) {
                continue;
            }
            let saved: Option<(String,i64,Option<String>)> = store.conn.query_row(
                "SELECT answer_digest,generation,receipt_delivery FROM native_answer_attempts WHERE delivery_token=?1 AND request_key=?2",
                params![attempt.token,attempt.key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            if !saved.is_some_and(|(digest, generation, delivery)| {
                digest == attempt.digest && generation == attempt.generation && delivery.is_none()
            }) {
                continue;
            }
            let tx = store.conn.unchecked_transaction()?;
            save_receipt_delivery(&store, &attempt.token, delivery)?;
            if delivery == "written" {
                record_written_denial(&store, &attempt.token)?;
            }
            // A historical receipt has no execution/confirmation authority.
            // Only this exact still-live orphan may change public lifecycle.
            let event = if current_orphan(&row, &attempt, revision)
                && live(&store, &row, &row.display).is_ok()
            {
                let result = json!({"request_key":attempt.key,"revision":row.revision+1,"delivery":delivery,"lifecycle":lifecycle});
                store.conn.execute("UPDATE native_answer_attempts SET result=?2 WHERE delivery_token=?1 AND result IS NULL",params![attempt.token,result.to_string()])?;
                let public: String = store.conn.query_row(
                    "SELECT projection FROM native_pending_requests WHERE key=?1",
                    [&attempt.key],
                    |r| r.get(0),
                )?;
                let public: Value = serde_json::from_str(&public)?;
                if row.lifecycle != lifecycle || public["reason_code"].as_str() != Some(reason) {
                    Some(update(
                        &store,
                        &row,
                        lifecycle,
                        reason,
                        &serde_json::from_str::<Value>(&attempt.actor)?,
                    )?)
                } else {
                    None
                }
            } else {
                None
            };
            tx.commit()?;
            event
        };
        if let Some(event) = event {
            let _ = d.events.send(event);
        }
    }
    Ok(())
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn unresolved_receipt_pages_advance_past_uncertain_rows_and_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recovery.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE runs(id TEXT PRIMARY KEY,process_generation INTEGER NOT NULL);
            INSERT INTO runs VALUES('owner',1);",
        )
        .unwrap();
        super::super::migrate(&conn).unwrap();
        for n in 0..17 {
            let key = format!("key-{n:02}");
            let token = format!("token-{n:02}");
            conn.execute("INSERT INTO native_pending_requests(key,process_run_id,display_run_id,generation,native_id,protocol,envelope,offer_digest,projection,revision,lifecycle,arrival_seq,changed_seq,created_ms)
                VALUES(?1,'owner','owner',1,?1,'codex-0.158','{}','offer','{}',3,'uncertain',?2,?2,?2)",params![key,n]).unwrap();
            conn.execute("INSERT INTO native_answer_attempts(delivery_token,request_key,generation,answer_digest,actor,created_ms,result)
                VALUES(?1,?2,1,'digest','{}',?3,'immutable-first-result')",params![token,key,n]).unwrap();
        }
        let first = reserve_recovery_page(&conn, Some("owner")).unwrap();
        assert_eq!(first.len(), 16);
        assert_eq!(first[0].token, "token-00");
        assert_eq!(first[15].token, "token-15");
        drop(conn);
        let conn = Connection::open(path).unwrap();
        let second = reserve_recovery_page(&conn, Some("owner")).unwrap();
        assert_eq!(second.len(), 16);
        assert_eq!(
            second[0].token, "token-16",
            "earlier uncertain attempts cannot permanently starve the later token"
        );
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM native_pending_requests WHERE lifecycle='uncertain' AND revision=3 AND projection='{}'",[],|r|r.get::<_,i64>(0)).unwrap(),17);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM native_answer_attempts WHERE result='immutable-first-result' AND receipt_delivery IS NULL",[],|r|r.get::<_,i64>(0)).unwrap(),17);
        assert_eq!(
            conn.query_row(
                "SELECT value FROM native_receipt_recovery_cursor WHERE id=1",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
    }

    #[test]
    fn historical_attempt_cannot_settle_a_new_token_or_terminal_request() {
        let attempt = RecoveryAttempt {
            key: "key".into(),
            owner: "owner".into(),
            token: "old".into(),
            digest: "digest".into(),
            generation: 1,
            actor: "{}".into(),
        };
        let mut row = Item {
            key: "key".into(),
            owner: "owner".into(),
            display: "owner".into(),
            generation: 1,
            protocol: "codex-0.158".into(),
            envelope: "{}".into(),
            offer_digest: "offer".into(),
            revision: 3,
            lifecycle: "claimed".into(),
            token: Some("new".into()),
            digest: Some("digest".into()),
        };
        assert!(
            !current_orphan(&row, &attempt, 3),
            "an older status receipt cannot settle a newer claim"
        );
        row.token = Some("old".into());
        assert!(
            current_orphan(&row, &attempt, 3),
            "the exact current orphan remains eligible for the separate live check"
        );
        assert!(
            !current_orphan(&row, &attempt, 2),
            "a changed revision remains frozen"
        );
        row.lifecycle = "native_resolved".into();
        assert!(
            !current_orphan(&row, &attempt, 3),
            "terminal historical bookkeeping must not reopen the request"
        );
    }
}
