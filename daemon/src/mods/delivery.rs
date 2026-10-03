//! Labelled text delivery only. No native configuration or mod program runs here.
use super::{
    bindings::{self, ModContext, ModPlan},
    error, library,
    manifest::DELIVERY_LIMIT,
    revision,
};
use crate::{
    daemon::{now, Daemon},
    store::Store,
};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub struct PreparedMods {
    pub plan: ModPlan,
    pub context: ModContext,
    pub message_preface: Option<String>,
    pub digest: String,
}
pub fn prepare(plan: ModPlan, c: &ModContext) -> Result<PreparedMods> {
    prepare_inner(plan, c, true)
}
fn prepare_inner(
    mut plan: ModPlan,
    c: &ModContext,
    enforce_required: bool,
) -> Result<PreparedMods> {
    let selected = !plan.versions.is_empty();
    let supported = !c.local_model_selection
        && c.role != "child"
        && matches!(
            c.harness.as_str(),
            "generic" | "claude" | "codex" | "codex-app" | "opencode" | "opencode-serve"
        );
    let mut blocks = Vec::new();
    if !plan.rules_text.is_empty() {
        blocks.push(format!("Optional mod rules:\n{}", plan.rules_text));
    }
    if !plan.style_text.is_empty() {
        blocks.push(format!("Optional mod style:\n{}", plan.style_text));
    }
    let rendered = if selected {
        Some(format!(
            "[Optional Overseer Mods: text guidance]\nApply only when consistent with owner instructions, repository policy, guardrails and approval requirements. This guidance grants no tools or permissions.\n{}\n[End optional Mods]\n\n",
            blocks.join("\n\n")
        ))
    } else {
        None
    };
    let unqualified = plan.decisions.iter().find(|d| d.status == "unqualified");
    let reason = if let Some(d) = unqualified {
        Some(d.reason)
    } else if c.local_model_selection && selected {
        Some("Mods admission precedes local model selection; this route is unqualified and optional text is bypassed whole")
    } else if !supported && selected {
        Some("Text delivery to this transport or observed native child is unsupported")
    } else if rendered.as_ref().is_some_and(|s| s.len() > DELIVERY_LIMIT) {
        Some("Optional mod text exceeds the 16-KiB delivery limit; delivery is bypassed without truncation")
    } else {
        None
    };
    if let Some(reason) = reason {
        if enforce_required
            && plan
                .decisions
                .iter()
                .any(|d| matches!(d.status, "selected" | "unqualified") && d.required)
        {
            return Err(error(
                "unsupported_mod",
                format!("Required mod text cannot be delivered: {reason}"),
            ));
        }
        for decision in plan.decisions.iter_mut().filter(|d| d.status == "selected") {
            decision.reason = reason;
        }
    } else {
        for decision in plan.decisions.iter_mut().filter(|d| d.status == "selected") {
            decision.delivery = "message_text";
        }
    }
    let message_preface = if reason.is_some() { None } else { rendered };
    let digest = format!(
        "{:x}",
        Sha256::digest(message_preface.as_deref().unwrap_or("").as_bytes())
    );
    Ok(PreparedMods {
        plan,
        context: c.clone(),
        message_preface,
        digest,
    })
}

impl PreparedMods {
    pub fn snapshot(&self, turn_id: &str, run_id: &str, harness: &str) -> Value {
        let text = self.message_preface.as_deref().unwrap_or("");
        json!({"turn_id":turn_id,"run_id":run_id,"plan":self.plan,"context":self.context, "binding_snapshot":[],
            "delivery":if text.is_empty() { if self.plan.versions.is_empty() && !self.plan.decisions.iter().any(|d| d.status == "unqualified") { "none" } else { "unsupported" } } else { "message_text" },
            "transport":harness,"activation":"next_turn","children":"unknown","text":text,
            "digest":self.digest,"added_bytes":text.len(),"outcome":"prepared","outcome_ms":now(),
            "applied_fingerprints":[],
            "planned_fingerprints":if text.is_empty() { Vec::<Value>::new() } else { self.plan.versions.iter().map(|v|v["fingerprint"].clone()).collect() }})
    }
}

pub fn last(store: &Store, run: &str) -> Result<Option<Value>> {
    let content: Option<String> = store.conn.query_row(
        "SELECT m.content FROM turns t LEFT JOIN turn_mods m ON m.turn_id=t.id WHERE t.run_id=?1 ORDER BY t.n DESC LIMIT 1", [run], |r|r.get::<_, Option<String>>(0),
    ).optional()?.flatten();
    content.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
}
pub fn snapshot(store: &Store, turn_id: &str) -> Result<Option<Value>> {
    let content: Option<String> = store
        .conn
        .query_row(
            "SELECT content FROM turn_mods WHERE turn_id=?1",
            [turn_id],
            |r| r.get(0),
        )
        .optional()?;
    content.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
}
pub fn outcome(store: &Store, turn_id: &str, status: &str, detail: &str) -> Result<()> {
    let Some(mut saved) = snapshot(store, turn_id)? else {
        return Ok(());
    };
    saved["applied_fingerprints"] = if status == "transport_accepted" {
        saved["planned_fingerprints"].clone()
    } else {
        json!([])
    };
    saved["outcome"] = json!(status);
    saved["outcome_ms"] = json!(now());
    saved["outcome_detail"] = json!(detail);
    store.conn.execute(
        "UPDATE turn_mods SET content=?2 WHERE turn_id=?1",
        params![turn_id, saved.to_string()],
    )?;
    Ok(())
}
fn effective(snapshot: &Value) -> Value {
    json!({"delivery":snapshot["delivery"],"digest":snapshot["digest"],"planned_fingerprints":snapshot["planned_fingerprints"]})
}
pub fn applied(d: &Daemon, run_id: &str) -> Result<Value> {
    let store = d.store.lock().unwrap();
    applied_from_store(&store, run_id)
}

/// Native reads authorize and project from the same Store snapshot.
pub(super) fn applied_from_store(store: &Store, run_id: &str) -> Result<Value> {
    let run = store
        .run(run_id)?
        .ok_or_else(|| error("invalid_mod", "unknown run target"))?;
    let ws = store
        .workspace(&run.workspace_id)?
        .ok_or_else(|| error("invalid_mod", "unknown workspace target"))?;
    let context = bindings::context_from_store(&store, &run, &ws)?;
    let plan = bindings::resolve(
        &library::versions(&store)?,
        &bindings::stored(&store)?,
        &context,
        revision(&store)?,
    )?;
    let prepared = prepare_inner(plan, &context, false)?;
    let desired_snapshot = prepared.snapshot("desired", run_id, &run.harness);
    let last_turn = last(&store, run_id)?;
    let pending = last_turn
        .as_ref()
        .map(|s| {
            effective(s) != effective(&desired_snapshot)
                || (s["delivery"] == "message_text" && s["outcome"] != "transport_accepted")
        })
        .unwrap_or(
            !prepared.plan.versions.is_empty()
                || prepared
                    .plan
                    .decisions
                    .iter()
                    .any(|d| d.status == "unqualified"),
        );
    let visible_last = last_turn.as_ref().map(public_snapshot);
    Ok(crate::daemon::redact_value(
        json!({"context":context,"desired":prepared.plan,"last_turn":visible_last,"pending":pending,
        "support":support(),
        "notice":"Text guidance uses the existing message transport; it does not enforce prose or change permissions. Disabling a mod does not erase instructions already in session history. Native configuration, global text suppression, dynamic local model delivery and child inheritance are unqualified. Model filters describe stored observations before local selection. Public text redacts credentials; digests and byte counts refer to the private original."}),
    ))
}

/// Opaque persisted delivery handed to adapters on both first launch and retry.
/// Retries use these bytes, never current bindings or installed files.
pub struct TurnMods {
    pub saved: Value,
}
impl TurnMods {
    pub fn validate_prompt(&self, prompt: &str) -> Result<()> {
        let text = self.saved["text"]
            .as_str()
            .ok_or_else(|| error("mod_changed", "saved Mods text is missing"))?;
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        if self.saved["digest"].as_str() != Some(&digest) || !prompt.starts_with(text) {
            return Err(error(
                "mod_changed",
                "saved Mods snapshot does not match the immutable turn prompt",
            ));
        }
        Ok(())
    }
}
pub fn record_outcome(d: &Daemon, turn_id: &str, status: &str, detail: &str) -> Result<()> {
    let event = {
        let store = d.store.lock().unwrap();
        let tx = store.conn.unchecked_transaction()?;
        outcome(&store, turn_id, status, detail)?;
        let Some(saved) = snapshot(&store, turn_id)? else {
            return Ok(());
        };
        let event = store.insert_event(
            now(),
            None,
            saved["run_id"].as_str(),
            "mods_applied",
            "daemon",
            "exact",
            &crate::daemon::redact_value(json!({"snapshot":public_snapshot(&saved)})),
        )?;
        tx.commit()?;
        event
    };
    let _ = d.events.send(event);
    Ok(())
}

/// Implementation support is distinct from installed-runtime qualification.
pub fn support() -> Value {
    json!({"delivery":"message_text","native_configuration":"unverified",
        "installed_runtime_qualification":"unverified", "fixture_message_transports":["generic","claude"],
        "global_text_suppression":"unsupported","dynamic_local_models":"unsupported","children":"unknown"})
}

/// Public views redact text; the digest still identifies the private original.
pub fn public_snapshot(saved: &Value) -> Value {
    let mut visible = crate::daemon::redact_value(saved.clone());
    visible["text_redacted"] = json!(visible["text"] != saved["text"]);
    visible
}
