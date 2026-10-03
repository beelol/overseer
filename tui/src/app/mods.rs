//! Local-owner controls ask the daemon; this view never resolves scopes or grants permissions.
use super::{App, Confirm, Mode, Pending};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(super) enum Purpose {
    List,
    Why,
    TargetState,
    Preview,
    Mutation,
}
#[derive(Debug, Clone)]
pub(super) struct Request {
    pub generation: u64,
    pub epoch: u64,
    pub target: Option<String>,
    pub purpose: Purpose,
}
#[derive(Debug, Clone)]
pub enum Form {
    Source {
        source: String,
        operation: String,
    },
    Binding {
        value: Value,
        field: usize,
    },
    Filter {
        value: Value,
        field: usize,
        cursor: usize,
        input: Option<String>,
    },
    Target {
        cursor: usize,
    },
}
#[derive(Debug, Clone)]
pub struct Review {
    pub method: String,
    pub params: Value,
    pub text: String,
    pub generation: u64,
    pub epoch: u64,
}
#[derive(Debug, Clone)]
pub struct ModsView {
    pub back: Option<Mode>,
    pub target: Option<String>,
    pub target_name: String,
    pub own: bool,
    pub library: bool,
    pub data: Option<Value>,
    pub why: Option<Value>,
    pub preview: Option<Value>,
    pub form: Option<Form>,
    pub review: Option<Review>,
    pub error: Option<String>,
    pub stale: bool,
    pub busy: bool,
    pub cursor: usize,
    pub scroll: u16,
    pub help: bool,
    pub(super) epoch: u64,
    reads: usize,
    due: Option<Instant>,
    wanted: bool,
    own_due: Option<Instant>,
    target_read: bool,
}
impl Default for ModsView {
    fn default() -> Self {
        Self {
            back: None,
            target: None,
            target_name: "No selected run".into(),
            own: false,
            library: true,
            data: None,
            why: None,
            preview: None,
            form: None,
            review: None,
            error: None,
            stale: true,
            busy: false,
            cursor: 0,
            scroll: 0,
            help: false,
            epoch: 0,
            reads: 0,
            due: None,
            wanted: false,
            own_due: None,
            target_read: false,
        }
    }
}
/// Terminal controls must never execute. Request identifiers remain the original daemon bytes.
pub fn display(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect()
}
fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}
fn array(v: &Value, key: &str) -> Vec<Value> {
    v[key].as_array().cloned().unwrap_or_default()
}
fn label(raw: &str) -> String {
    match raw {
        "transport_accepted" => "Accepted by transport",
        "prepared" => "Prepared; not sent",
        "failed_before_effect" => "Failed before delivery",
        "uncertain_after_effect" => "Delivery uncertain",
        "message_text" => "Text in turn message",
        "native_instructions" => "Native instructions (unqualified)",
        "none" => "No text delivery",
        "unknown" => "Unqualified",
        "next_turn" => "Next turn",
        "next_thread" => "Next native thread",
        "all_agents" => "All agents (excludes Overseer)",
        "overseer" => "Overseer",
        "watchers" => "Active watchers",
        "not_in_scope" => "Outside this scope",
        "agent" => "Agent",
        "repository" => "Repository",
        _ => return raw.replace('_', " "),
    }
    .into()
}
const SCOPES: [&str; 5] = ["all_agents", "repository", "watchers", "agent", "overseer"];
const FIELDS: [&str; 8] = [
    "Scope",
    "Enabled",
    "Required",
    "Locked",
    "Harnesses",
    "Accounts",
    "Models",
    "Target",
];
fn filter_key(field: usize) -> &'static str {
    ["harnesses", "accounts", "models"][field - 4]
}
impl ModsView {
    pub fn coherent(&self) -> bool {
        self.data.is_some()
            && !self.stale
            && self.reads == 0
            && self.error.is_none()
            && (self.target.is_none()
                || self.why.as_ref().is_some_and(|w| {
                    w["desired"]["revision"] == self.data.as_ref().unwrap()["revision"]
                }))
    }
    fn rows(&self) -> Vec<(bool, Value)> {
        let Some(v) = &self.data else { return vec![] };
        array(v, "installed")
            .into_iter()
            .map(|v| (false, v))
            .chain(array(v, "bindings").into_iter().map(|v| (true, v)))
            .collect()
    }
    fn selected(&self) -> Option<(bool, Value)> {
        self.rows().get(self.cursor).cloned()
    }
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "{} | {}",
            if self.library { "Library" } else { "Applied" },
            if self.stale {
                "Offline / stale — writes disabled"
            } else if self.busy {
                "Request pending"
            } else if self.reads > 0 {
                "Reading daemon…"
            } else {
                "Daemon facts"
            }
        )];
        if let Some(e) = &self.error {
            lines.push(format!("Error: {e}"));
        }
        if self.data.is_some()
            && !self.stale
            && !self.coherent()
            && self.reads == 0
            && self.target.is_some()
        {
            lines.push("Mods changed between reads. Refresh before managing bindings.".into());
        }
        if self.help {
            lines.extend(
                [
                    "Tab / Shift-Tab: Applied / Library; Esc: return to previous view",
                    "r: refresh; a: choose agent; O: actual Overseer run",
                    "Library: i install source; u update source; b new binding; e edit binding",
                    "d remove installed version; x remove override; j/k select; PgUp/PgDn scroll",
                    "Forms: Tab field; arrows choose scope; Space toggle; Enter review",
                    "Filter fields: Enter edits exact IDs; a add; x remove; Esc back",
                    "Confirmation: only y confirms; n / Esc cancels. Install never enables.",
                    "No measured token savings or guaranteed prose quality.",
                ]
                .map(str::to_string),
            );
            return lines;
        }
        if let Some(form) = &self.form {
            match form {
                Form::Source { source, operation } => lines.extend([
                    format!(
                        "{} source",
                        if operation == "update" {
                            "Update"
                        } else {
                            "Install"
                        }
                    ),
                    format!("Folder or bundled source: {source}▌"),
                    "Enter previews; Esc cancels. Preview never enables.".into(),
                ]),
                Form::Binding { value, field } => {
                    lines.push(format!(
                        "Binding: {} · {}",
                        text(value, "mod_id"),
                        text(value, "fingerprint")
                    ));
                    for (i, name) in FIELDS.iter().enumerate() {
                        let detail = match i {
                            0 => value["scope"].to_string(),
                            1 => value["enabled"].to_string(),
                            2 => value["required"].to_string(),
                            3 => value["locked"].to_string(),
                            4..=6 => value["filters"][filter_key(i)].to_string(),
                            _ => value["scope"].to_string(),
                        };
                        lines.push(format!(
                            "{} {name}: {detail}",
                            if *field == i { ">" } else { " " }
                        ));
                    }
                    lines.push(
                        "All agents excludes Overseer. Disabled override differs from removing it."
                            .into(),
                    );
                    lines.push(
                        "Enter reviews; Tab field; arrows scope/target; Space toggle.".into(),
                    );
                }
                Form::Filter {
                    value,
                    field,
                    cursor,
                    input,
                } => {
                    lines.push(format!("Exact {} identifiers", FIELDS[*field]));
                    for (i, v) in array(&value["filters"], filter_key(*field))
                        .iter()
                        .enumerate()
                    {
                        lines.push(format!(
                            "{} {}",
                            if i == *cursor { ">" } else { " " },
                            v.as_str().unwrap_or_default()
                        ));
                    }
                    lines.push(
                        input
                            .as_ref()
                            .map(|s| format!("Identifier: {s}▌ (Enter adds, Esc cancels)"))
                            .unwrap_or(
                                "a add; x remove; j/k select; Esc returns to binding".into(),
                            ),
                    );
                }
                Form::Target { .. } => {
                    lines.push("Choose agent: j/k select, Enter inspect, Esc cancel".into())
                }
            }
            return lines;
        }
        if let Some(p) = &self.preview {
            lines.extend([
                format!(
                    "Preview {}: {} · {}",
                    text(p, "operation"),
                    text(&p["version"], "id"),
                    text(p, "fingerprint")
                ),
                format!("Source: {}", text(&p["version"], "source")),
                text(p, "notice"),
                format!(
                    "Permissions: {} | Unsupported: {}",
                    p["permissions"], p["unsupported"]
                ),
            ]);
            if p["contents_redacted"] == true {
                lines.push(
                    "Public preview text is redacted; digest and bytes refer to private original."
                        .into(),
                );
            }
            for f in array(p, "files") {
                lines.push(format!(
                    "{} · {} bytes · {}",
                    text(&f, "path"),
                    f["bytes"],
                    text(&f, "sha256")
                ));
            }
            if let Some(contents) = p["contents"].as_object() {
                for (path, content) in contents {
                    lines.push(format!("--- {path}"));
                    lines.extend(
                        content
                            .as_str()
                            .unwrap_or_default()
                            .lines()
                            .map(str::to_string),
                    );
                }
            }
            lines.push(
                "Enter reviews installation; Esc closes preview. Installation never enables."
                    .into(),
            );
            return lines;
        }
        if self.library {
            if let Some(v) = &self.data {
                lines.push(format!(
                    "Library revision {} · Installed versions and bindings",
                    v["revision"]
                ));
                if self.rows().is_empty() {
                    lines.push("No installed versions or bindings.".into());
                }
                for (i, (binding, v)) in self.rows().iter().enumerate() {
                    lines.push(format!(
                        "{} {} {} · {} · {}",
                        if self.cursor == i { ">" } else { " " },
                        if *binding { "Binding" } else { "Installed" },
                        text(v, if *binding { "mod_id" } else { "id" }),
                        text(v, "version"),
                        text(v, "fingerprint")
                    ));
                    if *binding {
                        lines.push(format!(
                            "  {} · enabled={} required={} locked={} · filters={}",
                            v["scope"], v["enabled"], v["required"], v["locked"], v["filters"]
                        ));
                    }
                }
                for v in array(v, "available_bundled") {
                    lines.push(format!(
                        "Bundled: {} · {}",
                        text(&v["manifest"], "name"),
                        text(&v, "source")
                    ));
                }
                for v in array(v, "unavailable") {
                    lines.push(format!(
                        "{}: {} (inactive)",
                        text(&v, "name"),
                        text(&v, "reason")
                    ));
                }
                lines.push(format!("Support: {}", v["support"]));
            }
        } else {
            lines.push(format!("Target: {}", self.target_name));
            if let Some(w) = &self.why {
                lines.push(format!(
                    "Run ID: {}",
                    self.target.as_deref().unwrap_or_default()
                ));
                lines.push(format!(
                    "Role: {} · Harness: {} · Model: {}",
                    label(&text(&w["context"], "role")),
                    crate::words::harness(&text(&w["context"], "harness")),
                    w["context"]["model"].as_str().unwrap_or("Not reported")
                ));
                lines.push(format!(
                    "Desired revision {} · pending={}",
                    w["desired"]["revision"], w["pending"]
                ));
                for d in array(&w["desired"], "decisions") {
                    lines.push(format!(
                        "{}: {} — {} · required={} · delivery={} · activation={} · children={}",
                        text(&d, "mod_id"),
                        label(&text(&d, "status")),
                        text(&d, "reason"),
                        d["required"],
                        label(&text(&d, "delivery")),
                        label(&text(&d, "activation")),
                        label(&text(&d, "children"))
                    ));
                }
                if w["last_turn"].is_null() {
                    lines.push("No recorded turn delivery".into());
                } else {
                    let s = &w["last_turn"];
                    lines.push(format!(
                        "Last recorded turn: {} · {} · {}",
                        text(s, "turn_id"),
                        label(&text(s, "delivery")),
                        label(&text(s, "outcome"))
                    ));
                    lines.push(format!(
                        "Digest: {} · Added bytes: {}",
                        text(s, "digest"),
                        s["added_bytes"]
                    ));
                    if s["text_redacted"] == true {
                        lines.push(
                            "Public text redacted; private original digest/bytes retained.".into(),
                        );
                    }
                    if s["outcome"] != "transport_accepted" {
                        lines.push("This outcome does not confirm successful delivery.".into());
                    }
                }
                lines.push(format!("Support: {}", w["support"]));
                lines.push(text(w, "notice"));
            }
        }
        lines.into_iter().map(|s| display(&s)).collect()
    }
}
impl App {
    fn mods_shown(&self) -> bool {
        matches!(
            self.mode,
            Mode::Mods { .. } | Mode::Confirm(Confirm::Mods { .. })
        )
    }
    pub(super) fn open_mods(&mut self) {
        let back = self.mode.clone();
        let target = self.focused().map(|r| r.id.clone());
        let target_name = self
            .focused()
            .map(|r| {
                if r.title.trim().is_empty() {
                    "Untitled agent".into()
                } else {
                    r.title.clone()
                }
            })
            .unwrap_or("No selected run".into());
        let epoch = self.mods.epoch + 1;
        self.mods = ModsView {
            back: Some(back),
            target: target.clone(),
            target_name,
            library: target.is_none(),
            epoch,
            ..Default::default()
        };
        self.mode = Mode::Mods { run_id: target };
        self.mods_refresh();
    }
    fn mods_request(&mut self, method: &str, params: Value, purpose: Purpose) {
        self.request(
            method,
            params,
            Pending::Mods(Request {
                generation: self.connect_generation,
                epoch: self.mods.epoch,
                target: self.mods.target.clone(),
                purpose,
            }),
        );
    }
    fn mods_writable(&mut self) -> bool {
        if !self.connected || !self.client.connected() || self.mods.stale {
            self.mods.error = Some("Reconnect and refresh before managing Mods.".into());
            return false;
        }
        if self.mods.busy || self.mods.reads > 0 {
            return false;
        }
        if !self.mods.coherent() {
            self.mods.error =
                Some("Mods changed between reads. Refresh before managing bindings.".into());
            return false;
        }
        true
    }
    fn mods_refresh(&mut self) {
        if !self.connected || !self.mods_shown() {
            return;
        }
        if self.mods.busy || self.mods.reads > 0 {
            self.mods.wanted = true;
            return;
        }
        // Draft fields belong to the snapshot from which they were opened. Never adopt
        // a newer revision underneath them, which would overwrite a concurrent owner edit.
        if matches!(
            self.mods.form,
            Some(Form::Binding { .. } | Form::Filter { .. })
        ) {
            self.mods.form = None;
            self.say("Binding edit cancelled because Mods refreshed. Review current bindings before editing again.", true);
        }
        self.mods.review = None;
        if matches!(self.mode, Mode::Confirm(Confirm::Mods { .. })) {
            self.mode = Mode::Mods {
                run_id: self.mods.target.clone(),
            };
        }
        self.mods.epoch += 1;
        self.mods.error = None;
        self.mods.stale = false;
        self.mods.due = None;
        self.mods.wanted = false;
        self.mods.reads = 1 + usize::from(self.mods.target.is_some());
        self.mods.why = None;
        self.mods_request("mods.list", json!({}), Purpose::List);
        if let Some(run) = self.mods.target.clone() {
            self.mods_request("mods.why", json!({"run_id":run}), Purpose::Why);
        }
    }
    pub(super) fn mods_tick(&mut self, now: Instant) -> bool {
        if self.mods.own_due.is_some_and(|d| now >= d)
            && !self.mods.target_read
            && !self.mods.busy
            && self.connected
            && self.mods_shown()
        {
            self.mods.own_due = None;
            self.mods_target_state();
            return true;
        }
        if self.mods.due.is_some_and(|d| now >= d) && self.connected && self.mods_shown() {
            self.mods.due = None;
            self.mods_refresh();
            return true;
        }
        false
    }
    pub(super) fn mods_state_changed(&mut self) {
        if !self.mods.own {
            if let Some(run) = self
                .mods
                .target
                .as_deref()
                .and_then(|id| self.state.run(id))
            {
                self.mods.target_name = if run.title.trim().is_empty() {
                    "Untitled agent".into()
                } else {
                    run.title.clone()
                };
            }
        }
        if self.mods_shown()
            && self.mods.own
            && self.mods.target.as_deref() != self.state.overseer["run_id"].as_str()
        {
            // A sent operation keeps its original identity until its acknowledgement.
            // Mark old reads stale immediately, then switch/refresh atomically after reply;
            // invalidating its epoch here would strand busy or encourage a write replay.
            if self.mods.busy {
                self.mods.stale = true;
                self.mods.wanted = true;
                self.mods.review = None;
                self.mods.preview = None;
                self.mods.form = None;
                return;
            }
            self.mods.epoch += 1;
            self.mods.reads = 0;
            self.mods.target_read = false;
            self.mods.own_due = None;
            self.mods.review = None;
            self.mods.preview = None;
            self.mods.form = None;
            self.mods.target = self.state.overseer["run_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            self.mods.target_name = if self.mods.target.is_some() {
                "Overseer".into()
            } else {
                "No current Overseer run".into()
            };
            self.mode = Mode::Mods {
                run_id: self.mods.target.clone(),
            };
            self.mods_refresh();
        }
    }
    pub(super) fn mods_event(&mut self, kind: &str, run: &str) {
        if self.mods_shown()
            && (kind == "mods_changed"
                || kind == "mods_applied" && self.mods.target.as_deref() == Some(run))
        {
            self.mods.due = Some(Instant::now() + Duration::from_millis(120));
        }
        if self.mods_shown() && self.mods.own && kind.starts_with("overseer_") {
            self.mods.own_due = Some(Instant::now() + Duration::from_millis(120));
        }
    }
    pub(super) fn mods_disconnect(&mut self) {
        self.mods.epoch += 1;
        self.mods.stale = true;
        self.mods.busy = false;
        self.mods.reads = 0;
        self.mods.preview = None;
        self.mods.review = None;
        self.mods.form = None;
        self.mods.due = None;
        self.mods.wanted = false;
        self.mods.own_due = None;
        self.mods.target_read = false;
        if matches!(self.mode, Mode::Confirm(Confirm::Mods { .. })) {
            self.mode = Mode::Mods {
                run_id: self.mods.target.clone(),
            };
        }
    }
    pub(super) fn mods_reconnect(&mut self) {
        if self.mods_shown() {
            if self.mods.own {
                self.mods_target_state();
            } else {
                self.mods_refresh();
            }
        }
    }
    fn mods_target_state(&mut self) {
        if !self.connected || self.mods.busy || self.mods.target_read {
            return;
        }
        self.mods.epoch += 1;
        self.mods.reads = 1;
        self.mods.preview = None;
        self.mods.review = None;
        self.mods.why = None;
        self.mods.form = None;
        self.mods.target_read = true;
        self.mods_request("state", json!({}), Purpose::TargetState);
    }
    pub(super) fn mods_reply(&mut self, r: Request, result: Result<Value, String>) {
        if !self.mods_shown()
            || r.generation != self.connect_generation
            || r.epoch != self.mods.epoch
            || r.target != self.mods.target
        {
            return;
        }
        match r.purpose {
            Purpose::List | Purpose::Why => {
                self.mods.reads = self.mods.reads.saturating_sub(1);
                match result {
                    Ok(v) => match r.purpose {
                        Purpose::List => self.mods.data = Some(v),
                        _ => self.mods.why = Some(v),
                    },
                    Err(e) => self.mods.error = Some(e),
                }
            }
            Purpose::TargetState => {
                self.mods.target_read = false;
                self.mods.reads = 0;
                match result {
                    Ok(v) => {
                        self.state.overseer = v["overseer"].clone();
                        self.mods.target = v["overseer"]["run_id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_string);
                        self.mods.target_name = if self.mods.target.is_some() {
                            "Overseer".into()
                        } else {
                            "No current Overseer run".into()
                        };
                        self.mode = Mode::Mods {
                            run_id: self.mods.target.clone(),
                        };
                        self.mods_refresh();
                    }
                    Err(e) => self.mods.error = Some(e),
                }
            }
            Purpose::Preview => {
                self.mods.busy = false;
                match result {
                    Ok(v) => {
                        self.mods.preview = Some(v);
                        self.mods.form = None;
                    }
                    Err(e) => self.mods.error = Some(e),
                }
                self.mods_state_changed();
            }
            Purpose::Mutation => {
                self.mods.busy = false;
                self.mods.preview = None;
                self.mods.form = None;
                let error = result.err();
                self.mods_state_changed();
                if self.mods.reads == 0 {
                    self.mods_refresh();
                }
                if let Some(e) = error {
                    self.mods.error = Some(e);
                }
            }
        }
        if self.mods.reads == 0 && self.mods.wanted && !self.mods.busy {
            self.mods_refresh();
        }
    }
    fn mods_review(&mut self, method: &str, params: Value, text: String) {
        self.mods.review = Some(Review {
            method: method.into(),
            params,
            text: text.clone(),
            generation: self.connect_generation,
            epoch: self.mods.epoch,
        });
        self.mode = Mode::Confirm(Confirm::Mods { text });
    }
    pub(super) fn mods_confirm(&mut self, k: KeyEvent) {
        if !matches!(k.code, KeyCode::Char('y' | 'Y' | 'n' | 'N') | KeyCode::Esc) {
            return;
        }
        self.mode = Mode::Mods {
            run_id: self.mods.target.clone(),
        };
        let Some(r) = self.mods.review.take() else {
            return;
        };
        if !matches!(k.code, KeyCode::Char('y' | 'Y')) {
            return;
        }
        if r.generation != self.connect_generation
            || r.epoch != self.mods.epoch
            || !self.mods_writable()
        {
            self.mods.error = Some("Mods changed. Refresh and review again.".into());
            return;
        }
        self.mods.busy = true;
        self.mods_request(&r.method, r.params, Purpose::Mutation);
    }
    fn mods_scope(&self, kind: &str) -> Option<Value> {
        match kind {
            "repository" => self
                .mods
                .why
                .as_ref()
                .and_then(|w| w["context"]["repo_key"].as_str())
                .filter(|s| !s.is_empty())
                .map(|s| json!({"kind":kind,"repo_key":s})),
            "agent" => self
                .mods
                .target
                .as_ref()
                .map(|s| json!({"kind":kind,"run_id":s})),
            _ => Some(json!({"kind":kind})),
        }
    }
    fn mods_version(&self, v: &Value, binding: bool) -> Option<Value> {
        if binding {
            self.mods.data.as_ref().and_then(|d| {
                array(d, "installed")
                    .into_iter()
                    .find(|x| x["fingerprint"] == v["fingerprint"])
            })
        } else {
            Some(v.clone())
        }
    }
    pub(super) fn mods_key(&mut self, k: KeyEvent) {
        if self.mods.busy {
            return;
        }
        if let Some(form) = self.mods.form.take() {
            self.mods_form_key(form, k);
            return;
        }
        if self.mods.preview.is_some() {
            match k.code {
                KeyCode::Esc => self.mods.preview = None,
                KeyCode::Enter => {
                    if !self.mods_writable() {
                        return;
                    }
                    let p = self.mods.preview.as_ref().unwrap();
                    let text = format!(
                        "{} {} · {}? Install never enables; existing bindings remain pinned.",
                        text(p, "operation"),
                        text(&p["version"], "id"),
                        text(p, "fingerprint")
                    );
                    self.mods_review(
                        "mods.install",
                        json!({"preview_id":p["id"],"confirm":true}),
                        text,
                    );
                }
                KeyCode::PageDown => self.mods.scroll = self.mods.scroll.saturating_add(10),
                KeyCode::PageUp => self.mods.scroll = self.mods.scroll.saturating_sub(10),
                _ => {}
            }
            return;
        }
        match k.code {
            KeyCode::Esc => {
                self.mods.epoch += 1;
                self.mode = self.mods.back.take().unwrap_or(Mode::Grid);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.mods.library = !self.mods.library;
                self.mods.scroll = 0;
            }
            KeyCode::Char('?') => {
                self.mods.help = !self.mods.help;
                self.mods.scroll = 0;
            }
            KeyCode::Char('r') => self.mods_refresh(),
            KeyCode::Char('O') => {
                self.mods.own = true;
                self.mods.library = false;
                self.mods_target_state();
            }
            KeyCode::Char('a') => self.mods.form = Some(Form::Target { cursor: 0 }),
            KeyCode::PageDown => self.mods.scroll = self.mods.scroll.saturating_add(10),
            KeyCode::PageUp => self.mods.scroll = self.mods.scroll.saturating_sub(10),
            KeyCode::Down | KeyCode::Char('j') => {
                self.mods.cursor =
                    (self.mods.cursor + 1).min(self.mods.rows().len().saturating_sub(1));
                self.mods.scroll = self.mods.scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.mods.cursor = self.mods.cursor.saturating_sub(1);
                self.mods.scroll = self.mods.scroll.saturating_sub(1);
            }
            KeyCode::Char('i' | 'u') if self.mods.library => {
                if !self.mods_writable() {
                    return;
                }
                let update = k.code == KeyCode::Char('u');
                let source = if update {
                    self.mods
                        .selected()
                        .and_then(|(b, v)| self.mods_version(&v, b))
                        .map(|v| text(&v, "source"))
                } else {
                    Some("bundled:clear-prose".into())
                };
                if let Some(source) = source {
                    self.mods.form = Some(Form::Source {
                        source,
                        operation: if update { "update" } else { "install" }.into(),
                    });
                    self.mods.scroll = 0;
                }
            }
            KeyCode::Char('b' | 'e' | 'd' | 'x') if self.mods.library => {
                if !self.mods_writable() {
                    return;
                }
                let Some((binding, v)) = self.mods.selected() else {
                    return;
                };
                let revision = self.mods.data.as_ref().unwrap()["revision"].clone();
                match k.code {
                    KeyCode::Char('b')=>if let Some(v)=self.mods_version(&v,binding){self.mods.form=Some(Form::Binding{value:json!({"id":null,"mod_id":v["id"],"version":v["version"],"fingerprint":v["fingerprint"],"scope":{"kind":"all_agents"},"enabled":true,"required":false,"locked":false,"filters":{"harnesses":[],"accounts":[],"models":[]}}),field:0});self.mods.scroll=0;},
                    KeyCode::Char('e') if binding=>{let mut v=v;v.as_object_mut().unwrap().remove("actor");v.as_object_mut().unwrap().remove("changed_ms");self.mods.form=Some(Form::Binding{value:v,field:0});self.mods.scroll=0;},
                    KeyCode::Char('d') if !binding=>self.mods_review("mods.remove",json!({"mod_id":v["id"],"fingerprint":v["fingerprint"],"expected_revision":revision,"confirm":true}),format!("Remove {} · {}? Bindings removed for future turns; historical instructions remain.",text(&v,"id"),text(&v,"fingerprint"))),
                    KeyCode::Char('x') if binding=>self.mods_review("mods.unbind",json!({"binding_id":v["id"],"expected_revision":revision}),format!("Remove override {}? An inherited binding may become enabled.",text(&v,"id"))),_=>{}
                }
            }
            _ => {}
        }
    }
    fn mods_form_key(&mut self, form: Form, k: KeyEvent) {
        match form {
            Form::Source {
                mut source,
                operation,
            } => {
                match k.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter => {
                        if self.mods_writable() && !source.is_empty() {
                            self.mods.busy = true;
                            self.mods_request(
                                "mods.preview",
                                json!({"source":source,"operation":operation}),
                                Purpose::Preview,
                            );
                        } else {
                            self.mods.form = Some(Form::Source { source, operation });
                        }
                        return;
                    }
                    KeyCode::Backspace => {
                        source.pop();
                    }
                    KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        source.clear()
                    }
                    KeyCode::Char(c) if !c.is_control() && source.len() + c.len_utf8() <= 4096 => {
                        source.push(c)
                    }
                    _ => {}
                }
                self.mods.form = Some(Form::Source { source, operation });
            }
            Form::Target { mut cursor } => {
                let runs = self.state.runs.clone();
                match k.code {
                    KeyCode::Esc => return,
                    KeyCode::Down | KeyCode::Char('j') => {
                        cursor = (cursor + 1).min(runs.len().saturating_sub(1))
                    }
                    KeyCode::Up | KeyCode::Char('k') => cursor = cursor.saturating_sub(1),
                    KeyCode::Enter => {
                        if let Some(run) = runs.get(cursor) {
                            self.mods.target = Some(run.id.clone());
                            self.mods.target_name = if run.title.trim().is_empty() {
                                "Untitled agent".into()
                            } else {
                                run.title.clone()
                            };
                            self.mods.own = false;
                            self.mods.target_read = false;
                            self.mods.own_due = None;
                            self.mods.epoch += 1;
                            self.mods.reads = 0;
                            self.mods.why = None;
                            self.mode = Mode::Mods {
                                run_id: self.mods.target.clone(),
                            };
                            self.mods.library = false;
                            self.mods_refresh();
                        }
                        return;
                    }
                    _ => {}
                }
                self.mods.form = Some(Form::Target { cursor });
            }
            Form::Binding {
                mut value,
                mut field,
            } => {
                match k.code {
                    KeyCode::Esc => return,
                    KeyCode::Tab => field = (field + 1) % 8,
                    KeyCode::BackTab => field = (field + 7) % 8,
                    KeyCode::Char(' ') if (1..=3).contains(&field) => {
                        let key = ["enabled", "required", "locked"][field - 1];
                        value[key] = json!(!value[key].as_bool().unwrap_or(false));
                    }
                    KeyCode::Left | KeyCode::Right if field == 0 => {
                        let at = SCOPES
                            .iter()
                            .position(|s| value["scope"]["kind"] == *s)
                            .unwrap_or(0);
                        let next = (at + if k.code == KeyCode::Right { 1 } else { 4 }) % 5;
                        // Keep all five choices reachable even without a current agent.
                        // A scope needing an identity is validated before a review/write.
                        value["scope"] = self
                            .mods_scope(SCOPES[next])
                            .unwrap_or(json!({"kind":SCOPES[next]}));
                    }
                    KeyCode::Left | KeyCode::Right if field == 7 => {
                        let kind = text(&value["scope"], "kind");
                        let options: Vec<Value> = if kind == "agent" {
                            self.state
                                .runs
                                .iter()
                                .map(|r| json!({"kind":"agent","run_id":r.id}))
                                .collect()
                        } else if kind == "repository" {
                            self.mods
                                .data
                                .as_ref()
                                .map(|d| {
                                    array(d, "bindings")
                                        .into_iter()
                                        .map(|b| b["scope"].clone())
                                        .filter(|s| s["kind"] == "repository")
                                        .chain(self.mods_scope("repository"))
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else {
                            vec![]
                        };
                        if !options.is_empty() {
                            let at = options
                                .iter()
                                .position(|s| *s == value["scope"])
                                .unwrap_or(0);
                            let next =
                                (at + if k.code == KeyCode::Right {
                                    1
                                } else {
                                    options.len() - 1
                                }) % options.len();
                            value["scope"] = options[next].clone();
                        }
                    }
                    KeyCode::Enter if (4..=6).contains(&field) => {
                        self.mods.form = Some(Form::Filter {
                            value,
                            field,
                            cursor: 0,
                            input: None,
                        });
                        return;
                    }
                    KeyCode::Enter => {
                        let kind = text(&value["scope"], "kind");
                        let missing = match kind.as_str() {
                            "repository" => text(&value["scope"], "repo_key").is_empty(),
                            "agent" => text(&value["scope"], "run_id").is_empty(),
                            _ => false,
                        };
                        if missing {
                            self.say("Choose an actual agent or repository target before reviewing this scope",true);
                            self.mods.form = Some(Form::Binding { value, field: 7 });
                            return;
                        }
                        if self.mods_writable() {
                            let revision = self.mods.data.as_ref().unwrap()["revision"].clone();
                            self.mods.form = Some(Form::Binding {
                                value: value.clone(),
                                field,
                            });
                            self.mods_review("mods.bind",json!({"binding":value,"expected_revision":revision}),format!("Save binding {} · scope {} · enabled={} required={} locked={} · filters {}?",text(&value,"mod_id"),value["scope"],value["enabled"],value["required"],value["locked"],value["filters"]));
                            return;
                        }
                    }
                    _ => {}
                }
                self.mods.form = Some(Form::Binding { value, field });
            }
            Form::Filter {
                mut value,
                field,
                mut cursor,
                mut input,
            } => {
                let key = filter_key(field);
                let mut ids = array(&value["filters"], key);
                if let Some(mut s) = input.take() {
                    match k.code {
                        KeyCode::Esc => {}
                        KeyCode::Enter => {
                            if !s.is_empty() && ids.len() < 64 {
                                ids.push(json!(s));
                            }
                        }
                        KeyCode::Backspace => {
                            s.pop();
                            input = Some(s);
                        }
                        KeyCode::Char(c) if !c.is_control() && s.len() + c.len_utf8() <= 512 => {
                            s.push(c);
                            input = Some(s);
                        }
                        _ => input = Some(s),
                    }
                } else {
                    match k.code {
                        KeyCode::Esc => {
                            self.mods.form = Some(Form::Binding { value, field });
                            return;
                        }
                        KeyCode::Char('a') => input = Some(String::new()),
                        KeyCode::Char('x') if cursor < ids.len() => {
                            ids.remove(cursor);
                            cursor = cursor.min(ids.len().saturating_sub(1));
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            cursor = (cursor + 1).min(ids.len().saturating_sub(1))
                        }
                        KeyCode::Up | KeyCode::Char('k') => cursor = cursor.saturating_sub(1),
                        _ => {}
                    }
                }
                value["filters"][key] = json!(ids);
                self.mods.form = Some(Form::Filter {
                    value,
                    field,
                    cursor,
                    input,
                });
            }
        }
    }
}
