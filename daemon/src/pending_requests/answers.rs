//! Private typed answer claims. Public inputs select stored authority; they do
//! not supply native identity, generation, offered scope or caller provenance.
use super::*;
use crate::daemon::Daemon;
use crate::server::{NativeAuthority, ProtoError};
use crate::shim::LaunchFile;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

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
fn refuse(code: &'static str) -> anyhow::Error {
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
        } => {
            if request_key != key || *confirmed != revision {
                return Err(refuse("stale_request"));
            }
            json!({"origin":"confirmed_overseer","proposal":proposal})
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
    Ok(run)
}
fn response(row: &Item, answer: &Value) -> Result<String> {
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
    let value = native_requests::encode(&request, request.context(), &answer).map_err(|e| {
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
fn hold(phase: &str, key: &str) -> Result<()> {
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

pub(crate) fn answer(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
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
        response(&row, &p["answer"])?;
    }
    hold("validated_before_claim", key)?;
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
        let data = response(&row, &p["answer"])?;
        let digest = format!("{:x}", Sha256::digest(data.as_bytes()));
        let token = format!("delivery-{}", uuid::Uuid::new_v4().simple());
        let socket = frozen_socket(&store, &row)?;
        let tx = store.conn.unchecked_transaction()?;
        store.conn.execute("INSERT INTO native_answer_attempts(delivery_token,request_key,generation,answer_digest,actor,created_ms) VALUES(?1,?2,?3,?4,?5,?6)",
            params![token,key,row.generation,digest,actor.to_string(),crate::daemon::now()])?;
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
