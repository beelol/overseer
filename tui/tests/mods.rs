//! Client-boundary fixtures: synthetic daemon facts, no process, provider or owner profile.
//! These deliberately use baseline App/Requests APIs so missing behavior fails assertions.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use overseer_tui::{
    app::{App, Mode},
    client::{Msg, Requests},
    ui,
};
use ratatui::{backend::TestBackend, Terminal};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct Fake {
    next: AtomicU64,
    online: AtomicBool,
    calls: Mutex<Vec<(u64, String, Value)>>,
}
impl Requests for Fake {
    fn request(&self, method: &str, params: Value) -> u64 {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        self.calls.lock().unwrap().push((id, method.into(), params));
        id
    }
    fn connected(&self) -> bool {
        self.online.load(Ordering::SeqCst)
    }
    fn set_cursor_if_unset(&self, _: i64) {}
    fn subscribe(&self) {}
}
impl Fake {
    fn take(&self, method: &str) -> (u64, Value) {
        let mut calls = self.calls.lock().unwrap();
        let at = calls
            .iter()
            .position(|(_, m, _)| m == method)
            .unwrap_or_else(|| panic!("missing {method} request: {calls:?}"));
        let (id, _, p) = calls.remove(at);
        (id, p)
    }
    fn count(&self, method: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, m, _)| m == method)
            .count()
    }
}
fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn reply(app: &mut App, id: u64, v: Value) {
    app.handle_msg(Msg::Reply { id, result: Ok(v) });
}
fn state(run: bool, overseer: Option<&str>) -> Value {
    let runs = if run {
        vec![
            json!({"id":"agent-a","task_id":"task","harness":"generic","workspace_id":"ws","status":"running","created_ms":1,"title":"Agent A"}),
        ]
    } else {
        vec![]
    };
    json!({"cursor":1,"tasks":[{"id":"task","repo_root":"/synthetic/repo","workspace_id":"ws","title":"Agent A"}],"runs":runs,"workspaces":[],"profiles":[],"overseer":{"session":"os-current","run_id":overseer}})
}
fn attached() -> (Arc<Fake>, App) {
    let f = Arc::new(Fake::default());
    f.online.store(true, Ordering::SeqCst);
    let mut a = App::new(f.clone());
    a.handle_msg(Msg::Connected);
    let (id, _) = f.take("state");
    reply(&mut a, id, state(true, Some("hidden-overseer")));
    a.focus = Some("agent-a".into());
    (f, a)
}
fn library(revision: i64) -> Value {
    json!({"revision":revision,"installed":[],"bindings":[],"available_bundled":[{"id":"clear-prose","version":"1.0.0","fingerprint":"fp","source":"bundled:clear-prose","manifest":{"name":"Clear prose"}}],"unavailable":[{"id":"less-tool-noise","name":"Less tool noise","reason":"Planned; external transformers are not implemented"}],"support":{"children":"unknown"}})
}
fn why(run: &str, revision: i64) -> Value {
    json!({"context":{"run_id":run,"role":"agent","repo_key":"/synthetic/repo/.git","harness":"generic","harness_version":null,"account_id":null,"model":null,"native_thread_exists":false,"local_model_selection":false},"desired":{"revision":revision,"decisions":[],"versions":[],"rules_text":"","style_text":""},"last_turn":null,"pending":false,"support":{"children":"unknown"},"notice":"Children and installed native runtime remain unqualified"})
}
fn open(f: &Fake, a: &mut App) {
    key(a, KeyCode::Char('m'));
    let (id, _) = f.take("mods.list");
    reply(a, id, library(7));
    let (id, p) = f.take("mods.why");
    assert_eq!(p, json!({"run_id":"agent-a"}));
    reply(a, id, why("agent-a", 7));
}
fn installed_library(revision: i64) -> Value {
    let mut v = library(revision);
    v["installed"] = json!([{"id":"clear-prose","version":"1.0.0","fingerprint":"fp","source":"bundled:clear-prose","manifest":{"name":"Clear prose"},"files":[]}]);
    v
}
fn open_installed(f: &Fake, a: &mut App, why_revision: i64) {
    key(a, KeyCode::Char('m'));
    let (id, _) = f.take("mods.list");
    reply(a, id, installed_library(7));
    let (id, _) = f.take("mods.why");
    reply(a, id, why("agent-a", why_revision));
    key(a, KeyCode::Tab);
}
fn screen(a: &mut App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, a)).unwrap();
    let b = t.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| b[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn grid_zoom_and_dashboard_entry_restore_the_original_view() {
    for original in [Mode::Grid, Mode::Zoom { scroll: 23 }] {
        let (f, mut a) = attached();
        a.mode = original.clone();
        open(&f, &mut a);
        assert!(screen(&mut a, 100, 30).contains("Applied"));
        key(&mut a, KeyCode::Tab);
        assert!(screen(&mut a, 100, 30).contains("Library"));
        key(&mut a, KeyCode::BackTab);
        key(&mut a, KeyCode::Esc);
        assert_eq!(a.mode, original);
        assert_eq!(a.focus.as_deref(), Some("agent-a"));
    }
    let (f, mut a) = attached();
    a.dashboard = true;
    a.dash_col = 2;
    a.size = (180, 40);
    open(&f, &mut a);
    key(&mut a, KeyCode::Esc);
    assert!(a.dashboard);
    assert_eq!(a.dash_col, 2);
}

#[test]
fn no_focus_opens_library_without_requesting_an_empty_target() {
    let (f, mut a) = attached();
    key(&mut a, KeyCode::Char('r'));
    let (id, _) = f.take("state");
    reply(&mut a, id, state(false, None));
    a.focus = None;
    key(&mut a, KeyCode::Char('m'));
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, library(7));
    assert_eq!(f.count("mods.why"), 0);
    assert!(screen(&mut a, 80, 24).contains("Library"));
}

#[test]
fn preview_confirmation_is_explicit_immutable_and_never_enables() {
    let (f, mut a) = attached();
    open(&f, &mut a);
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Char('i'));
    key(&mut a, KeyCode::Enter);
    let (id, p) = f.take("mods.preview");
    assert_eq!(
        p,
        json!({"source":"bundled:clear-prose","operation":"install"})
    );
    reply(
        &mut a,
        id,
        json!({"id":"immutable-preview","operation":"install","fingerprint":"fp","version":{"id":"clear-prose","version":"1.0.0","fingerprint":"fp","manifest":{"name":"Clear prose"}},"files":[{"path":"style.md","bytes":18,"sha256":"synthetic-digest"}],"contents":{"style.md":"Use full sentences."},"permissions":[],"unsupported":[]}),
    );
    assert_eq!(f.count("mods.install"), 0);
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('t'));
    assert_eq!(f.count("mods.install"), 0);
    assert!(screen(&mut a, 80, 24).contains("y / n"));
    key(&mut a, KeyCode::Char('n'));
    assert_eq!(f.count("mods.install"), 0);
    assert!(screen(&mut a, 80, 24).contains("Mods"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('y'));
    key(&mut a, KeyCode::Char('y'));
    assert_eq!(f.count("mods.install"), 1);
    let (_, p) = f.take("mods.install");
    assert_eq!(p, json!({"preview_id":"immutable-preview","confirm":true}));
    assert_eq!(f.count("mods.bind"), 0);
}

#[test]
fn actual_overseer_target_is_read_from_state_without_creating_a_session() {
    let (f, mut a) = attached();
    open(&f, &mut a);
    key(&mut a, KeyCode::Char('O'));
    let (id, p) = f.take("state");
    assert_eq!(p, json!({}));
    reply(&mut a, id, state(true, Some("new-hidden-overseer")));
    let (id, p) = f.take("mods.why");
    assert_eq!(p, json!({"run_id":"new-hidden-overseer"}));
    let mut v = why("new-hidden-overseer", 7);
    v["context"]["role"] = json!("overseer");
    reply(&mut a, id, v);
    assert_eq!(f.count("overseer.session"), 0);
    assert!(screen(&mut a, 100, 30).contains("new-hidden-overseer"));
}

#[test]
fn disconnect_invalidates_pending_reads_and_reconnect_refreshes() {
    let (f, mut a) = attached();
    key(&mut a, KeyCode::Char('m'));
    let (old, _) = f.take("mods.list");
    a.handle_msg(Msg::Disconnected("synthetic offline".into()));
    reply(&mut a, old, library(999));
    assert!(!screen(&mut a, 80, 24).contains("999"));
    key(&mut a, KeyCode::Char('i'));
    key(&mut a, KeyCode::Enter);
    assert_eq!(f.count("mods.preview"), 0);
    a.handle_msg(Msg::Connected);
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, library(8));
    assert!(screen(&mut a, 80, 24).contains("Mods"));
}

#[test]
fn control_events_refresh_only_global_or_current_envelope_target() {
    let (f, mut a) = attached();
    open(&f, &mut a);
    a.handle_msg(Msg::Event(json!({"seq":50,"kind":"mods_applied","run_id":"other-agent","payload":{"run_id":"agent-a"}})));
    a.tick(Instant::now() + Duration::from_secs(3));
    assert_eq!(f.count("mods.list"), 0);
    for seq in [51, 52] {
        a.handle_msg(Msg::Event(
            json!({"seq":seq,"kind":"mods_changed","run_id":null,"payload":{"revision":8}}),
        ));
    }
    a.tick(Instant::now() + Duration::from_secs(3));
    assert_eq!(f.count("mods.list"), 1);
}

#[test]
fn absent_turn_and_planned_bundle_remain_truthful_at_narrow_width() {
    let (f, mut a) = attached();
    open(&f, &mut a);
    assert!(screen(&mut a, 60, 30).contains("No recorded turn delivery"));
    key(&mut a, KeyCode::Tab);
    let rendered = screen(&mut a, 60, 30);
    assert!(rendered.contains("Less tool noise"));
    assert!(rendered.contains("Planned"));
    assert!(!rendered.contains("tokens saved"));
}

#[test]
fn compose_input_keeps_literal_m_and_does_not_open_mods() {
    let (f, mut a) = attached();
    key(&mut a, KeyCode::Char('i'));
    key(&mut a, KeyCode::Char('m'));
    assert_eq!(a.mode, Mode::Compose);
    assert_eq!(a.drafts.get("agent-a").map(String::as_str), Some("m"));
    assert_eq!(f.count("mods.list"), 0);
}

#[test]
fn scoped_enable_and_explicit_disable_pin_exact_daemon_identifiers() {
    let scopes = [
        json!({"kind":"all_agents"}),
        json!({"kind":"repository","repo_key":"/synthetic/repo/.git"}),
        json!({"kind":"watchers"}),
        json!({"kind":"agent","run_id":"agent-a"}),
        json!({"kind":"overseer"}),
    ];
    for (steps, scope) in scopes.into_iter().enumerate() {
        let (f, mut a) = attached();
        open_installed(&f, &mut a, 7);
        key(&mut a, KeyCode::Char('b'));
        // The form begins on Scope; right cycles the five explicitly named choices.
        for _ in 0..steps {
            key(&mut a, KeyCode::Right);
        }
        key(&mut a, KeyCode::Enter);
        assert_eq!(f.count("mods.bind"), 0);
        key(&mut a, KeyCode::Char('y'));
        let (_, p) = f.take("mods.bind");
        assert_eq!(
            p,
            json!({"expected_revision":7,"binding":{"id":null,"mod_id":"clear-prose","version":"1.0.0","fingerprint":"fp","scope":scope,"enabled":true,"required":false,"locked":false,"filters":{"harnesses":[],"accounts":[],"models":[]}}})
        );
        assert_eq!(f.count("mods.unbind"), 0);
    }
    let (f, mut a) = attached();
    open_installed(&f, &mut a, 7);
    key(&mut a, KeyCode::Char('b'));
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Char(' ')); // Enabled becomes false.
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('y'));
    let (_, p) = f.take("mods.bind");
    assert_eq!(p["binding"]["enabled"], false);
    assert_eq!(p["binding"]["scope"], json!({"kind":"all_agents"}));
    assert_eq!(
        f.count("mods.unbind"),
        0,
        "a disabled override must not remove inheritance precedence"
    );
}

#[test]
fn mixed_read_revisions_require_refresh_before_any_binding_write() {
    let (f, mut a) = attached();
    open_installed(&f, &mut a, 8);
    key(&mut a, KeyCode::Char('b'));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('y'));
    assert_eq!(
        f.count("mods.bind"),
        0,
        "list revision 7 and why revision 8 are not one reviewed snapshot"
    );
    assert!(screen(&mut a, 100, 30).to_lowercase().contains("refresh"));
}

#[test]
fn own_session_event_burst_coalesces_into_one_authoritative_state_read() {
    let (f, mut a) = attached();
    open(&f, &mut a);
    key(&mut a, KeyCode::Char('O'));
    let (id, _) = f.take("state");
    reply(&mut a, id, state(true, Some("hidden-overseer")));
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, library(7));
    let (id, _) = f.take("mods.why");
    reply(&mut a, id, why("hidden-overseer", 7));
    for seq in [60, 61, 62] {
        a.handle_msg(Msg::Event(json!({"seq":seq,"kind":"overseer_session","run_id":null,"payload":{"run_id":"forged-target"}})));
    }
    a.tick(Instant::now() + Duration::from_secs(3));
    assert_eq!(
        f.count("state"),
        1,
        "session event bursts need one fresh summary, not a request per event"
    );
    assert_eq!(f.count("overseer.session"), 0);
}

#[test]
fn narrow_binding_form_keeps_the_selected_field_visible() {
    let (f, mut a) = attached();
    open_installed(&f, &mut a, 7);
    key(&mut a, KeyCode::Char('b'));
    for _ in 0..7 {
        key(&mut a, KeyCode::Tab);
    }
    assert!(
        screen(&mut a, 60, 12).contains("> Target:"),
        "keyboard focus must remain visible in a short terminal"
    );
}

#[test]
fn update_uses_install_rpc_and_never_sends_a_new_binding() {
    let (f, mut a) = attached();
    open_installed(&f, &mut a, 7);
    key(&mut a, KeyCode::Char('u'));
    key(&mut a, KeyCode::Enter);
    let (id, p) = f.take("mods.preview");
    assert_eq!(
        p,
        json!({"source":"bundled:clear-prose","operation":"update"})
    );
    reply(
        &mut a,
        id,
        json!({"id":"update-preview","operation":"update","version":{"id":"clear-prose","version":"1.1.0","source":"bundled:clear-prose"},"fingerprint":"updated-fp","files":[],"contents":{},"permissions":[],"unsupported":[]}),
    );
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('y'));
    let (_, p) = f.take("mods.install");
    assert_eq!(p, json!({"preview_id":"update-preview","confirm":true}));
    assert_eq!(f.count("mods.update"), 0);
    assert_eq!(f.count("mods.bind"), 0);
}

#[test]
fn removal_reviews_exact_pin_and_conflict_never_replays_the_write() {
    let (f, mut a) = attached();
    open_installed(&f, &mut a, 7);
    key(&mut a, KeyCode::Char('d'));
    key(&mut a, KeyCode::Esc);
    assert_eq!(f.count("mods.remove"), 0);
    key(&mut a, KeyCode::Char('d'));
    key(&mut a, KeyCode::Char('y'));
    key(&mut a, KeyCode::Char('y'));
    assert_eq!(f.count("mods.remove"), 1);
    let (id, p) = f.take("mods.remove");
    assert_eq!(
        p,
        json!({"mod_id":"clear-prose","fingerprint":"fp","expected_revision":7,"confirm":true})
    );
    a.handle_msg(Msg::Reply {
        id,
        result: Err("revision_conflict: Mods changed; review again".into()),
    });
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, installed_library(8));
    let (id, _) = f.take("mods.why");
    reply(&mut a, id, why("agent-a", 8));
    assert_eq!(f.count("mods.remove"), 0);
    assert!(screen(&mut a, 100, 30).contains("revision_conflict"));
}

#[test]
fn binding_edit_preserves_exact_filters_lock_id_and_disabled_override() {
    let (f, mut a) = attached();
    key(&mut a, KeyCode::Char('m'));
    let mut v = installed_library(7);
    let binding = json!({"id":"binding-owner","mod_id":"clear-prose","version":"1.0.0","fingerprint":"fp","scope":{"kind":"overseer"},"enabled":true,"required":true,"locked":true,"filters":{"harnesses":["codex"],"accounts":["account, with space"],"models":["exact-model"]},"actor":"owner","changed_ms":1});
    v["bindings"] = json!([binding.clone()]);
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, v);
    let (id, _) = f.take("mods.why");
    reply(&mut a, id, why("agent-a", 7));
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Char('j'));
    key(&mut a, KeyCode::Char('e'));
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Char(' '));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('y'));
    let (_, p) = f.take("mods.bind");
    let mut expected = binding;
    expected.as_object_mut().unwrap().remove("actor");
    expected.as_object_mut().unwrap().remove("changed_ms");
    expected["enabled"] = json!(false);
    assert_eq!(p, json!({"binding":expected,"expected_revision":7}));
    assert_eq!(f.count("mods.unbind"), 0);
}

#[test]
fn late_original_target_reply_cannot_replace_the_explicit_overseer_target() {
    let (f, mut a) = attached();
    key(&mut a, KeyCode::Char('m'));
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, library(7));
    let (old, _) = f.take("mods.why");
    key(&mut a, KeyCode::Char('O'));
    let (id, _) = f.take("state");
    reply(&mut a, id, state(true, Some("hidden-overseer")));
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, library(7));
    let (id, _) = f.take("mods.why");
    reply(&mut a, id, why("hidden-overseer", 7));
    let mut old_value = why("agent-a", 99);
    old_value["notice"] = json!("OLD TARGET POISON");
    reply(&mut a, old, old_value);
    let rendered = screen(&mut a, 100, 30);
    assert!(rendered.contains("hidden-overseer"));
    assert!(!rendered.contains("OLD TARGET POISON"));
}

#[test]
fn switching_away_from_pending_overseer_read_does_not_poison_later_selection() {
    let (f, mut a) = attached();
    open(&f, &mut a);
    key(&mut a, KeyCode::Char('O'));
    let (old, _) = f.take("state");
    key(&mut a, KeyCode::Char('a'));
    key(&mut a, KeyCode::Enter);
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, library(7));
    let (id, p) = f.take("mods.why");
    assert_eq!(p, json!({"run_id":"agent-a"}));
    reply(&mut a, id, why("agent-a", 7));
    reply(&mut a, old, state(true, Some("stale-overseer")));
    key(&mut a, KeyCode::Char('O'));
    let (id, _) = f.take("state");
    reply(&mut a, id, state(true, Some("current-overseer")));
    let (_, p) = f.take("mods.why");
    assert_eq!(p, json!({"run_id":"current-overseer"}));
}

#[test]
fn mods_footer_describes_the_current_panel_instead_of_agent_mutations() {
    let (f, mut a) = attached();
    open(&f, &mut a);
    let rendered = screen(&mut a, 100, 30);
    let footer = rendered.lines().last().unwrap();
    assert!(footer.contains("refresh"));
    assert!(!footer.contains("message"));
}

#[test]
fn overseer_scope_binding_needs_no_agent_or_active_overseer_run() {
    let (f, mut a) = attached();
    key(&mut a, KeyCode::Char('r'));
    let (id, _) = f.take("state");
    reply(&mut a, id, state(false, None));
    key(&mut a, KeyCode::Char('m'));
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, installed_library(7));
    key(&mut a, KeyCode::Char('b'));
    for _ in 0..4 {
        key(&mut a, KeyCode::Right);
    }
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('y'));
    let (_, p) = f.take("mods.bind");
    assert_eq!(p["binding"]["scope"], json!({"kind":"overseer"}));
    assert_eq!(f.count("overseer.session"), 0);
    assert_eq!(f.count("mods.why"), 0);
}

#[test]
fn applied_uses_agent_title_and_readable_delivery_outcome_labels() {
    let (f, mut a) = attached();
    key(&mut a, KeyCode::Char('m'));
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, library(7));
    let mut v = why("agent-a", 7);
    v["last_turn"] = json!({"turn_id":"turn-technical-id","delivery":"message_text","outcome":"uncertain_after_effect","digest":"digest","added_bytes":128});
    let (id, _) = f.take("mods.why");
    reply(&mut a, id, v);
    let rendered = screen(&mut a, 100, 30);
    assert!(rendered.contains("Target: Agent A"));
    assert!(rendered.contains("Delivery uncertain"));
    assert!(rendered.contains("Text in turn message"));
    assert!(!rendered.contains("uncertain_after_effect"));
}

#[test]
fn transport_disconnect_before_app_message_still_refuses_a_preview() {
    let (f, mut a) = attached();
    open_installed(&f, &mut a, 7);
    f.online.store(false, Ordering::SeqCst);
    key(&mut a, KeyCode::Char('i'));
    key(&mut a, KeyCode::Enter);
    assert_eq!(f.count("mods.preview"), 0);
    assert!(screen(&mut a, 100, 30).contains("Reconnect"));
}

#[test]
fn outcome_facts_never_turn_prepared_or_failed_text_into_accepted_delivery() {
    for (outcome, label) in [
        ("prepared", "Prepared; not sent"),
        ("failed_before_effect", "Failed before delivery"),
        ("uncertain_after_effect", "Delivery uncertain"),
        ("transport_accepted", "Accepted by transport"),
    ] {
        let (f, mut a) = attached();
        key(&mut a, KeyCode::Char('m'));
        let (id, _) = f.take("mods.list");
        reply(&mut a, id, library(7));
        let mut v = why("agent-a", 7);
        v["pending"] = json!(true);
        v["last_turn"] = json!({"turn_id":"turn","delivery":"message_text","outcome":outcome,"digest":"original-digest","added_bytes":128,"text_redacted":true});
        let (id, _) = f.take("mods.why");
        reply(&mut a, id, v);
        let rendered = screen(&mut a, 100, 30);
        assert!(rendered.contains(label));
        assert!(rendered.contains("original-digest"));
        assert!(rendered.contains("redacted"));
        assert_eq!(
            rendered.contains("does not confirm successful delivery"),
            outcome != "transport_accepted"
        );
    }
}

#[test]
fn exact_filter_editor_keeps_commas_and_spaces_inside_one_identifier() {
    let (f, mut a) = attached();
    open_installed(&f, &mut a, 7);
    key(&mut a, KeyCode::Char('b'));
    for _ in 0..6 {
        key(&mut a, KeyCode::Tab);
    }
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('a'));
    for c in "model, with space".chars() {
        key(&mut a, KeyCode::Char(c));
    }
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('y'));
    let (_, p) = f.take("mods.bind");
    assert_eq!(
        p["binding"]["filters"]["models"],
        json!(["model, with space"])
    );
}

#[test]
fn unbind_is_a_separate_confirmed_override_removal() {
    let (f, mut a) = attached();
    key(&mut a, KeyCode::Char('m'));
    let mut v = installed_library(7);
    v["bindings"] = json!([{"id":"disabled-override","mod_id":"clear-prose","version":"1.0.0","fingerprint":"fp","scope":{"kind":"agent","run_id":"agent-a"},"enabled":false,"required":false,"locked":false,"filters":{"harnesses":[],"accounts":[],"models":[]}}]);
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, v);
    let (id, _) = f.take("mods.why");
    reply(&mut a, id, why("agent-a", 7));
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Char('j'));
    key(&mut a, KeyCode::Char('x'));
    key(&mut a, KeyCode::Char('n'));
    assert_eq!(f.count("mods.unbind"), 0);
    key(&mut a, KeyCode::Char('x'));
    key(&mut a, KeyCode::Char('y'));
    let (_, p) = f.take("mods.unbind");
    assert_eq!(
        p,
        json!({"binding_id":"disabled-override","expected_revision":7})
    );
    assert_eq!(f.count("mods.bind"), 0);
}

#[test]
fn incumbent_audio_preview_review_previous_change_and_uppercase_controls_survive() {
    let (f, mut a) = attached();
    a.mode = Mode::Audio;
    key(&mut a, KeyCode::Char('p'));
    assert_eq!(f.count("audio.preview"), 1);
    assert_eq!(a.mode, Mode::Audio);
    a.mode = Mode::Changes;
    key(&mut a, KeyCode::Char('p'));
    assert_eq!(a.mode, Mode::Changes);
    assert_eq!(f.count("mods.list"), 0);
    a.mode = Mode::Grid;
    a.state.runs[0].status = "completed".into();
    key(&mut a, KeyCode::Char('M'));
    let (_, p) = f.take("workspace.merge_plan");
    assert_eq!(p, json!({"workspace_id":"ws"}));
    key(&mut a, KeyCode::Char('P'));
    let (_, p) = f.take("workspace.pr_plan");
    assert_eq!(p, json!({"workspace_id":"ws"}));
    assert_eq!(f.count("mods.list"), 0);
}

#[test]
fn own_session_change_during_sent_preview_or_install_preserves_ack_and_refreshes() {
    for install in [false, true] {
        let (f, mut a) = attached();
        open_installed(&f, &mut a, 7);
        key(&mut a, KeyCode::Char('O'));
        let (id, _) = f.take("state");
        reply(&mut a, id, state(true, Some("old-overseer")));
        let (id, _) = f.take("mods.list");
        reply(&mut a, id, installed_library(7));
        let (id, _) = f.take("mods.why");
        let mut v = why("old-overseer", 7);
        v["context"]["role"] = json!("overseer");
        reply(&mut a, id, v);
        key(&mut a, KeyCode::Tab);
        key(&mut a, KeyCode::Char('i'));
        key(&mut a, KeyCode::Enter);
        let (preview, _) = f.take("mods.preview");
        let payload = json!({"id":"sent-preview","operation":"install","version":{"id":"clear-prose","manifest":{"name":"Clear prose"}},"fingerprint":"fp","files":[],"contents":{},"permissions":[],"unsupported":[]});
        let sent = if install {
            reply(&mut a, preview, payload.clone());
            key(&mut a, KeyCode::Enter);
            key(&mut a, KeyCode::Char('y'));
            let (id, _) = f.take("mods.install");
            id
        } else {
            preview
        };
        a.handle_msg(Msg::Event(
            json!({"seq":90,"kind":"status","run_id":"agent-a","payload":{"status":"running"}}),
        ));
        a.tick(Instant::now() + Duration::from_secs(3));
        let (id, _) = f.take("state");
        reply(&mut a, id, state(true, Some("new-overseer")));
        reply(
            &mut a,
            sent,
            if install {
                json!({"revision":8})
            } else {
                payload
            },
        );
        assert!(
            !a.mods.busy,
            "a sent operation's reply must release the pending control even after Fresh"
        );
        assert_eq!(
            f.count("mods.install"),
            0,
            "never replay a sent mutation when the target changes"
        );
        assert!(
            a.mods.preview.is_none(),
            "an old-target preview cannot become the new target's review"
        );
        let (_, p) = f.take("mods.why");
        assert_eq!(p, json!({"run_id":"new-overseer"}));
    }
}

#[test]
fn refreshed_revision_never_rebases_a_retained_binding_form_over_owner_edits() {
    let (f, mut a) = attached();
    key(&mut a, KeyCode::Char('m'));
    let mut original = installed_library(7);
    let binding = json!({"id":"owner-binding","mod_id":"clear-prose","version":"1.0.0","fingerprint":"fp","scope":{"kind":"all_agents"},"enabled":true,"required":false,"locked":false,"filters":{"harnesses":[],"accounts":[],"models":[]}});
    original["bindings"] = json!([binding.clone()]);
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, original);
    let (id, _) = f.take("mods.why");
    reply(&mut a, id, why("agent-a", 7));
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Char('j'));
    key(&mut a, KeyCode::Char('e'));
    a.handle_msg(Msg::Event(
        json!({"seq":100,"kind":"mods_changed","run_id":null,"payload":{"revision":8}}),
    ));
    a.tick(Instant::now() + Duration::from_secs(3));
    let mut current = installed_library(8);
    let mut edited = binding;
    edited["enabled"] = json!(false);
    edited["locked"] = json!(true);
    edited["filters"]["models"] = json!(["owner-new-model"]);
    current["bindings"] = json!([edited]);
    let (id, _) = f.take("mods.list");
    reply(&mut a, id, current);
    let (id, _) = f.take("mods.why");
    reply(&mut a, id, why("agent-a", 8));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char('y'));
    assert_eq!(
        f.count("mods.bind"),
        0,
        "old form fields must never be sent with newly adopted revision 8"
    );
}
