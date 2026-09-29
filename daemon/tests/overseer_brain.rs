//! Overseer's brain (the daemon side of the zero-friction goal): it starts agents on the right
//! harness, model and account (AC-237), checks finished work and offers the next step (AC-238),
//! brings stuck, failed and limited agents back (AC-239), never drops what it was told (AC-248)
//! and leads with what happened while the owner was away (AC-253).
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn claude_fixture() -> String {
    repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()
}

fn codex_app_fixture() -> String {
    repo_root().join("fixtures/fake-harness/codex-app-fixture.js").display().to_string()
}

/// The tests that spawn many fixture processes and assert on timing run one at a time.
fn heavy() -> std::sync::MutexGuard<'static, ()> {
    static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    HEAVY.lock().unwrap_or_else(|e| e.into_inner())
}

/// A daemon whose Claude is the fixture (Overseer mode unless a task's mode file says otherwise).
fn overseer_daemon(mode_file: &Path, extra: &[(&str, &str)]) -> Daemon {
    std::fs::write(mode_file, "overseer").unwrap();
    let mode = mode_file.display().to_string();
    let fixture = claude_fixture();
    let mut env: Vec<(&str, &str)> = vec![("OVERSEER_CLAUDE_PATH", &fixture), ("CLAUDE_FIXTURE_MODE_FILE", &mode), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE")];
    for (k, v) in extra {
        env.retain(|(key, _)| key != k);
        env.push((k, v));
    }
    Daemon::start(&env)
}

fn claude_task(d: &Daemon, repo: &Path, mode_file: &Path, mode: &str, title: &str, prompt: &str) -> String {
    std::fs::write(mode_file, mode).unwrap();
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": prompt, "title": title}));
    run_id(&created)
}

fn session(d: &Daemon) -> Value {
    d.call("overseer.session", json!({}))
}

fn wait_overseer_idle(d: &Daemon, secs: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let s = session(d);
        let status = s["run_status"].as_str().unwrap_or("");
        if !s["run_id"].is_null() && !["queued", "starting", "running", "waiting_for_user"].contains(&status) {
            return s;
        }
        assert!(Instant::now() < deadline, "Overseer's run did not finish: {s}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Waits until the conversation has a message the predicate accepts.
fn wait_message(d: &Daemon, what: &str, secs: u64, pred: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let all = d.call("overseer.messages", json!({"after": 0, "limit": 500}))["messages"].as_array().cloned().unwrap_or_default();
        if let Some(m) = all.iter().find(|m| pred(m)) {
            return m.clone();
        }
        assert!(Instant::now() < deadline, "no message: {what}\n{}", all.iter().map(|m| format!("[{}] {}", m["source"], m["text"])).collect::<Vec<_>>().join("\n"));
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn replies(d: &Daemon) -> Vec<String> {
    d.call("overseer.messages", json!({"after": 0, "limit": 500}))["messages"].as_array().cloned().unwrap_or_default().iter().filter(|m| m["source"] == "overseer").map(|m| m["text"].as_str().unwrap_or("").to_string()).collect()
}

/// The SQL the tests use to put the daemon's store into a state a client cannot ask for.
fn sql(d: &Daemon, statement: &str) {
    let db = d.home.path().join("overseer.sqlite");
    let out = Command::new("sqlite3").args(["-cmd", ".timeout 5000"]).arg(&db).arg(statement).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn turns(d: &Daemon, run: &str) -> Vec<Value> {
    d.call("run.turns", json!({"run_id": run})).as_array().cloned().unwrap_or_default()
}

/// What Overseer's own run said and hit, for a failing assertion.
fn overseer_trace(d: &Daemon) -> String {
    let s = session(d);
    let run = s["run_id"].as_str().unwrap_or("").to_string();
    d.events(&run).iter().filter(|e| ["output", "error", "tool", "tool_result", "turn_started", "overseer_tool_call"].contains(&e["kind"].as_str().unwrap_or(""))).map(|e| format!("{} {}", e["kind"], serde_json::to_string(&e["payload"]).unwrap_or_default().chars().take(900).collect::<String>())).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------- AC-248

/// AC-248: an owner's message whose turn cannot start is not lost. It is kept, the conversation
/// says so in plain words, and it is sent again, as its own turn, once a turn can start.
#[test]
fn ac248_a_failed_turn_start_is_retried() {
    let r = tmp();
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file, &[]);
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    let s = wait_overseer_idle(&d, 30);
    let run = s["run_id"].as_str().unwrap().to_string();
    assert_eq!(replies(&d).len(), 1);
    // Overseer's scratch folder is taken away: its next turn cannot start.
    sql(&d, &format!("UPDATE workspaces SET removed_ms=1 WHERE id=(SELECT workspace_id FROM runs WHERE id='{run}')"));
    let sent = d.call("overseer.send", json!({"text": "Anything new?", "surface": "ctl"}));
    assert_eq!(sent["queued"], true, "the words are kept: {sent}");
    assert_eq!(session(&d)["pending"], 1);
    let note = wait_message(&d, "the words are kept", 10, |m| m["source"] == "system" && m["text"].as_str().unwrap_or("").contains("Your words are kept"));
    assert!(!note["text"].as_str().unwrap().contains("anyhow"), "{note}");
    // Still failing: tried again, and said once.
    std::thread::sleep(Duration::from_secs(5));
    let said = d.call("overseer.messages", json!({"after": 0, "limit": 500}))["messages"].as_array().unwrap().iter().filter(|m| m["source"] == "system").count();
    assert_eq!(said, 1, "the reason is said once while it keeps failing");
    // The folder is back: the kept message goes out as a turn of its own and is answered.
    sql(&d, &format!("UPDATE workspaces SET removed_ms=NULL WHERE id=(SELECT workspace_id FROM runs WHERE id='{run}')"));
    let deadline = Instant::now() + Duration::from_secs(40);
    while (replies(&d).len() < 2 || session(&d)["pending"] != 0) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_eq!(session(&d)["pending"], 0, "the kept message went out\n{}", overseer_trace(&d));
    assert_eq!(replies(&d).len(), 2, "and was answered\n{}", overseer_trace(&d));
    assert!(turns(&d, &run).iter().any(|t| t["prompt"].as_str().unwrap_or("").ends_with("Anything new?")));
}

// ---------------------------------------------------------------------- AC-237

/// Overseer's own tools, as a harness would list them.
fn overseer_tools(d: &Daemon) -> Vec<Value> {
    let token = d.call("overseer.token", json!({"run_id": "t-overseer", "role": "overseer"}))["token"].as_str().unwrap().to_string();
    d.call("overseer.tools", json!({"token": token}))["tools"].as_array().unwrap().clone()
}

/// Sends the owner's words and waits for Overseer's answer; returns the open proposal it made.
fn ask_overseer(d: &Daemon, text: &str) -> Value {
    d.call("overseer.send", json!({"text": text, "surface": "ctl", "harness": "claude"}));
    std::thread::sleep(Duration::from_millis(200));
    let s = wait_overseer_idle(d, 60);
    s["proposals"].as_array().unwrap().last().cloned().unwrap_or_else(|| panic!("no proposal for {text:?}: {s}\n{}", overseer_trace(d)))
}

/// Says yes to a proposal and returns the agent it started.
fn yes_start(d: &Daemon, proposal: &Value) -> Value {
    let before: Vec<String> = d.runs().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect();
    let answer = d.call("overseer.answer", json!({"id": proposal["id"], "yes": true, "surface": "ctl", "by": "owner"}));
    assert_eq!(answer["state"], "yes", "{answer}");
    assert!(answer["result"].as_str().unwrap().contains("started"), "{answer}");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(r) = d.runs().into_iter().find(|r| !before.contains(&r["id"].as_str().unwrap().to_string())) {
            return r;
        }
        assert!(Instant::now() < deadline, "no new agent after {answer}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A daemon with Claude (the fixture) and Codex (the app-server fixture, with models to list),
/// and one agent in a repository so Overseer knows where to start new ones.
fn routing_daemon(r: &tempfile::TempDir) -> (Daemon, std::path::PathBuf, std::path::PathBuf) {
    let checkout = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let codex = codex_app_fixture();
    let d = overseer_daemon(&mode_file, &[("OVERSEER_CODEX_PATH", &codex), ("FIXTURE_MODE", "managed-models"), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_MODE")]);
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    let seed = claude_task(&d, &checkout, &mode_file, "echo", "Seed", "look around");
    d.wait_done(&seed, 30);
    std::fs::write(&mode_file, "overseer").unwrap();
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);
    (d, checkout, mode_file)
}

/// AC-237: the tool schema lists what a start can name (harness, model, account, effort,
/// permission mode) and Overseer can list the accounts to name one.
#[test]
fn ac237_the_start_schema_names_harness_model_account_effort_and_mode() {
    let d = Daemon::start(&[]);
    let tools = overseer_tools(&d);
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"accounts"), "{names:?}");
    let propose = tools.iter().find(|t| t["name"] == "propose").unwrap();
    let fields = &propose["inputSchema"]["properties"]["actions"]["items"]["properties"];
    for f in ["harness", "model", "profile", "effort", "permission_mode"] {
        assert!(fields.get(f).is_some(), "propose's start takes {f}: {fields}");
    }
    assert_eq!(fields["harness"]["enum"], json!(["claude", "codex", "opencode"]));
    let text = propose["description"].as_str().unwrap();
    assert!(text.contains("harness") && text.contains("Auto routing"), "{text}");
}

/// AC-237: the owner names the model, the account or the harness; the card says what was picked
/// and why, the reply says it in one line, and the run starts on exactly that.
#[test]
fn ac237_named_harness_model_and_account_start_there() {
    let _one = heavy();
    let r = tmp();
    let (d, _checkout, mode_file) = routing_daemon(&r);
    let work = d.call("profile.create", json!({"name": "Work", "harness": "claude"}))["id"].as_str().unwrap().to_string();
    std::fs::write(&mode_file, "echo").unwrap();

    // A named model.
    let p = ask_overseer(&d, "start an agent to write the notes with the model opus-fixture");
    let line = p["lines"][0].as_str().unwrap();
    assert!(line.contains("opus-fixture") && line.contains("as asked"), "{line}");
    let run = yes_start(&d, &p);
    assert_eq!(run["harness"], "claude");
    assert_eq!(run["model"], "opus-fixture");
    let echo = d.wait_done(run["id"].as_str().unwrap(), 30);
    let _ = echo;
    let said = d.events(run["id"].as_str().unwrap()).iter().find(|e| e["kind"] == "output" && e["payload"]["text"].as_str().unwrap_or("").starts_with("ECHO ")).map(|e| e["payload"]["text"].as_str().unwrap().to_string()).unwrap();
    assert!(said.contains("\"--model\",\"opus-fixture\""), "the run's arguments: {said}");
    let reply = replies(&d).last().cloned().unwrap();
    assert!(reply.contains("opus-fixture") && reply.lines().count() == 1, "one line says the pick: {reply}");

    // A named account ("my other account": Overseer lists the accounts and picks the other one).
    let p = ask_overseer(&d, "start an agent to draft the plan on my other account");
    assert!(p["lines"][0].as_str().unwrap().contains("Work"), "{p}");
    let run = yes_start(&d, &p);
    assert_eq!(run["profile_id"], work.as_str());
    assert_eq!(run["harness"], "claude");
    d.wait_done(run["id"].as_str().unwrap(), 30);

    // A named harness.
    let p = ask_overseer(&d, "start an agent to fix the tests on codex");
    assert!(p["lines"][0].as_str().unwrap().contains("Codex"), "{p}");
    let run = yes_start(&d, &p);
    assert_eq!(run["harness"], "codex-app");
    assert_eq!(run["profile_id"], "system-codex");
    let reply = replies(&d).last().cloned().unwrap();
    assert!(reply.contains("Codex"), "{reply}");
    d.wait_done(run["id"].as_str().unwrap(), 30);

    // A name that is not an account: refused with the accounts there are, nothing proposed.
    let refused = d.try_call("overseer.propose", json!({"actions": [{"action": "start", "repo": _checkout, "prompt": "x", "profile": "Nope"}], "source": "ctl"})).unwrap_err();
    assert!(refused.contains("no account named Nope") && refused.contains("Work"), "{refused}");
}

/// AC-237: a start that names nothing follows Auto's route pick when Auto routing is on, and its
/// reason is on the card; with Auto routing off it runs on Overseer's own harness and says so.
#[test]
fn ac237_an_unnamed_start_follows_the_route_pick() {
    let _one = heavy();
    let r = tmp();
    let (d, _checkout, mode_file) = routing_daemon(&r);
    std::fs::write(&mode_file, "echo").unwrap();
    let p = ask_overseer(&d, "start an agent to tidy the readme");
    let action = &p["actions"][0];
    assert_eq!(action["route"]["how"], "auto", "{p}");
    let line = p["lines"][0].as_str().unwrap();
    assert!(line.contains("Auto's pick"), "the reason is on the card: {line}");
    assert!(!line.contains('_'), "in plain words: {line}");
    let run = yes_start(&d, &p);
    assert_eq!(run["harness"], action["harness"], "{run}");
    assert_eq!(run["model"], action["model"], "{run}");
    assert_eq!(run["profile_id"], action["profile_id"], "{run}");
    assert!(d.call("auto.usage.list", json!({"limit": 50})).is_object());
    d.wait_status(run["id"].as_str().unwrap(), |s| !["queued", "starting", "running"].contains(&s), 30);

    d.call("auto.mode.set", json!({"enabled": false}));
    let p = ask_overseer(&d, "start an agent to tidy the changelog");
    assert_eq!(p["actions"][0]["route"]["how"], "default", "{p}");
    let line = p["lines"][0].as_str().unwrap();
    assert!(line.contains("Auto routing is off"), "{line}");
    let run = yes_start(&d, &p);
    assert_eq!(run["harness"], "claude");
    assert_eq!(run["profile_id"], "system-claude");
    d.wait_done(run["id"].as_str().unwrap(), 30);
}

// ---------------------------------------------------------------------- AC-239

/// The causes of the turns Overseer's run has taken, oldest first.
fn turn_causes(d: &Daemon) -> Vec<String> {
    let db = d.home.path().join("overseer.sqlite");
    let out = Command::new("sqlite3").args(["-cmd", ".timeout 5000"]).arg(&db).arg("SELECT cause FROM overseer_turns ORDER BY ts;").output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

fn trouble_turns(d: &Daemon) -> usize {
    turn_causes(d).iter().filter(|c| *c == "trouble").count()
}

/// Waits for Overseer's trouble turn number `n` to end with an open proposal whose first action
/// is `action` on `agent`.
fn wait_trouble_proposal(d: &Daemon, n: usize, action: &str, agent: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if trouble_turns(d) >= n {
            let s = wait_overseer_idle(d, 60);
            if let Some(p) = s["proposals"].as_array().unwrap().iter().find(|p| p["actions"][0]["action"] == action && p["actions"][0]["agent"] == agent) {
                return p.clone();
            }
        }
        assert!(Instant::now() < deadline, "no {action} proposal for {agent} after trouble turn {n}: causes {:?}\n{}", turn_causes(d), overseer_trace(d));
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn trouble_card(d: &Daemon, agent: &str) -> Value {
    wait_message(d, "a trouble card", 30, |m| m["card"]["kind"] == "trouble" && m["card"]["agent"] == agent)
}

/// AC-239: a limited, a failed and a silent agent each come back to Overseer as one turn and one
/// card with a plain reason; Overseer offers the fix; "continue on the other account" goes on in
/// the same worktree on the second profile; no surface shows an error class or an HTTP code.
#[test]
fn ac239_stuck_failed_and_limited_agents_come_back_to_overseer() {
    let _one = heavy();
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file, &[("FIXTURE_SLOW_MS", "60000"), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS")]);
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    let work = d.call("profile.create", json!({"name": "Work", "harness": "claude"}))["id"].as_str().unwrap().to_string();
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);

    // A usage limit: the card says so plainly and offers the other account; Overseer proposes it.
    let limited = claude_task(&d, &checkout, &mode_file, "ratelimit", "Limited", "write the API");
    assert_eq!(d.wait_done(&limited, 30)["status"], "failed");
    let card = trouble_card(&d, &limited);
    assert_eq!(card["text"], "Limited reached its account's usage limit.", "{card}");
    let shown = d.call("state", json!({}))["runs"].as_array().unwrap().iter().find(|x| x["id"] == limited.as_str()).unwrap()["plain_reason"].clone();
    assert_eq!(shown, "Reached its account's usage limit", "what every surface can show instead of the raw reason");
    assert!(card["card"]["offers"].as_array().unwrap().iter().any(|o| o["action"] == "continue" && o["profile"] == work.as_str()), "{card}");
    let p = wait_trouble_proposal(&d, 1, "continue", &limited);
    assert_eq!(p["actions"][0]["profile"], work.as_str());
    assert_eq!(trouble_turns(&d), 1, "one turn");
    let reply = replies(&d).last().cloned().unwrap();
    assert!(reply.contains("Limited reached its account's usage limit") && reply.contains("Work"), "{reply}");
    // Continue on the other account: the same worktree, the second profile.
    std::fs::write(&mode_file, "echo").unwrap();
    let yes = d.call("overseer.answer", json!({"id": p["id"], "yes": true, "surface": "ctl", "by": "owner"}));
    assert!(yes["result"].as_str().unwrap().contains("continued Limited on Claude Code (account Work)"), "{yes}");
    let before = d.run(&limited);
    assert_eq!(before["status"], "handed_off");
    let next = d.runs().into_iter().find(|x| x["profile_id"] == work.as_str()).expect("a run on Work");
    assert_eq!(next["workspace_id"], before["workspace_id"], "the same worktree");
    assert_eq!(next["task_id"], before["task_id"], "the same task");
    d.wait_done(next["id"].as_str().unwrap(), 30);

    // A failure with its reason: said in the harness's own words, offered a retry.
    let broken = claude_task(&d, &checkout, &mode_file, "failed-reason", "Broken", "migrate the users");
    assert_eq!(d.wait_done(&broken, 30)["status"], "failed");
    let card = trouble_card(&d, &broken);
    assert_eq!(card["text"], "Broken failed: Migration failed: relation users_v2 does not exist.", "{card}");
    let p = wait_trouble_proposal(&d, 2, "retry", &broken);
    assert_eq!(trouble_turns(&d), 2, "one turn");
    std::fs::write(&mode_file, "echo").unwrap();
    let yes = d.call("overseer.answer", json!({"id": p["id"], "yes": true, "surface": "ctl", "by": "owner"}));
    assert!(yes["result"].as_str().unwrap().contains("retried Broken"), "{yes}");
    assert!(turns(&d, &broken).last().unwrap()["prompt"].as_str().unwrap().contains("migrate the users"), "the turn that did not finish, again");
    assert_eq!(d.wait_done(&broken, 30)["status"], "completed", "the retry went through");

    // Silence while running: stuck. At the Auto level Overseer does the fix itself (a stop).
    d.call("overseer.level", json!({"level": "auto"}));
    sql(&d, "INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.silence_ms', '3000')");
    let quiet = claude_task(&d, &checkout, &mode_file, "slow", "Quiet", "think hard");
    d.wait_status(&quiet, |s| s == "running", 20);
    let card = trouble_card(&d, &quiet);
    assert_eq!(card["text"], "Quiet has said nothing for a while as it works.", "{card}");
    d.wait_status(&quiet, |s| s == "interrupted", 40);
    wait_overseer_idle(&d, 30);
    assert_eq!(trouble_turns(&d), 3, "one turn");
    let reply = replies(&d).last().cloned().unwrap();
    assert!(reply.starts_with("Quiet has said nothing") && reply.contains("and did"), "{reply}");

    // A failure that repeats is not a loop: at Auto, Overseer retries once by itself; the second
    // time the retry waits for the owner's yes.
    let again = claude_task(&d, &checkout, &mode_file, "failed-reason", "Again", "migrate the orders");
    let deadline = Instant::now() + Duration::from_secs(60);
    while turns(&d, &again).len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(turns(&d, &again).len(), 2, "retried once by itself");
    let p = wait_trouble_proposal(&d, 5, "retry", &again);
    assert_eq!(p["state"], "open", "the second retry waits for a yes: {p}");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(turns(&d, &again).len(), 2, "and nothing more ran");

    // No surface shows an error class or an HTTP code: the conversation, its cards, Overseer's
    // words and the agents' reasons in the daemon's state.
    let all = d.call("overseer.messages", json!({"after": 0, "limit": 500}))["messages"].to_string();
    let state = d.call("state", json!({}));
    let plain: Vec<String> = state["runs"].as_array().unwrap().iter().filter_map(|r| r["plain_reason"].as_str().map(str::to_string)).collect();
    assert!(plain.iter().any(|p| p.contains("users_v2")), "{plain:?}");
    for raw in ["[rate_limit]", "rate_limit", "429", "turn reported failure"] {
        assert!(!all.contains(raw), "{raw} in the conversation: {all}");
        assert!(plain.iter().all(|p| !p.contains(raw)), "{raw} in a plain reason: {plain:?}");
    }
}

// ---------------------------------------------------------------------- AC-238

fn done_card(d: &Daemon, agent: &str) -> Value {
    wait_message(d, "a done card", 60, |m| m["card"]["kind"] == "done" && m["card"]["agent"] == agent)
}

fn open_proposal_for(d: &Daemon, action: &str, agent: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let s = wait_overseer_idle(d, 60);
        if let Some(p) = s["proposals"].as_array().unwrap().iter().find(|p| p["actions"][0]["action"] == action && p["actions"][0]["agent"] == agent) {
            return p.clone();
        }
        assert!(Instant::now() < deadline, "no open {action} proposal for {agent}: {s}\n{}", overseer_trace(d));
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// AC-238: when an agent finishes, Overseer reads its final message, whole diff and test output,
/// says whether it did what was asked (citing the tests) and proposes the next step from a turn
/// the owner did not start: the merge when the tests pass, a fix message when one fails. Neither
/// happens without the owner's yes at Ask first.
#[test]
fn ac238_overseer_checks_finished_work_and_offers_the_next_step() {
    let _one = heavy();
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file, &[]);
    sql(&d, "INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.grace_ms', '500')");
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);
    let head_before = git(&checkout, &["rev-parse", "HEAD"]);

    // Finished with passing tests: "did it", the test output cited, a merge proposed.
    let good = claude_task(&d, &checkout, &mode_file, "tested", "Totals", "add a total() function with tests");
    assert_eq!(d.wait_done(&good, 30)["status"], "completed");
    let card = done_card(&d, &good);
    let text = card["text"].as_str().unwrap();
    assert!(text.starts_with("Totals did it: ") && text.contains("npm test") && text.contains("3 passing"), "{card}");
    assert_eq!(card["card"]["did_it"], true);
    assert_eq!(card["card"]["tests"]["ok"], true, "{card}");
    assert_eq!(card["card"]["tests"]["command"], "npm test");
    let merge = open_proposal_for(&d, "merge_back", &good);
    assert_eq!(merge["cause"], "check_in", "from a turn the owner did not start: {merge}");
    // What Overseer read: the final message in full, the whole diff and the test output.
    let overseer = session(&d)["run_id"].as_str().unwrap().to_string();
    let prompt = turns(&d, &overseer).iter().map(|t| t["prompt"].as_str().unwrap_or("").to_string()).find(|p| p.contains("\"finished\"")).expect("a check-in with the finished work");
    for seen in ["Added total() in src/total.js with three tests; npm test passes.", "+module.exports = (a, b) => a + b;", "3 passing"] {
        assert!(prompt.contains(seen), "the check-in carries {seen:?}: {prompt}");
    }

    // Finished with a failing test: "not yet", the failure cited, a fix message proposed.
    let bad = claude_task(&d, &checkout, &mode_file, "tested-fail", "Negatives", "make total() handle negatives");
    assert_eq!(d.wait_done(&bad, 30)["status"], "completed");
    let card = done_card(&d, &bad);
    let text = card["text"].as_str().unwrap();
    assert!(text.starts_with("Negatives is not done yet: ") && text.contains("AssertionError"), "{card}");
    assert_eq!(card["card"]["did_it"], false);
    let fix = open_proposal_for(&d, "message", &bad);
    assert!(fix["actions"][0]["text"].as_str().unwrap().contains("npm test` fails"), "{fix}");

    // Nothing happened without a yes: no merge, no message.
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), head_before, "not merged");
    assert_eq!(turns(&d, &bad).len(), 1, "no fix message sent");
    let s = session(&d);
    for p in [&merge, &fix] {
        assert!(s["proposals"].as_array().unwrap().iter().any(|o| o["id"] == p["id"] && o["state"] == "open"), "still waiting: {p}");
    }
    // The owner's yes sends the fix.
    std::fs::write(&mode_file, "echo").unwrap();
    let yes = d.call("overseer.answer", json!({"id": fix["id"], "yes": true, "surface": "ctl", "by": "owner"}));
    assert_eq!(yes["state"], "yes", "{yes}");
    assert!(turns(&d, &bad).last().unwrap()["prompt"].as_str().unwrap().contains("Please fix it"));
    // An archive stays the owner's own to ask for: a check-in cannot propose it.
    sql(&d, "UPDATE overseer_sessions SET last_cause='check_in'");
    let refused = d.try_call("overseer.propose", json!({"actions": [{"action": "archive", "agent": good}], "source": "ctl"})).unwrap_err();
    assert!(refused.contains("only when the owner asks"), "{refused}");
}
