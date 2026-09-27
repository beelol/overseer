//! Overseer itself (Gate S): the daemon half and the tools it gives a run.
mod common;
use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Runs `overseerd mcp` as a harness would (an MCP server over stdio) and replays a recorded
/// JSON-RPC exchange, returning one reply per request.
fn mcp_exchange(d: &Daemon, token: &str, requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(BIN)
        .args(["mcp", "--socket", &d.socket().display().to_string()])
        .env("OVERSEER_MCP_TOKEN", token)
        .env_clear()
        .env("OVERSEER_MCP_TOKEN", token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for r in requests {
        stdin.write_all(format!("{r}\n").as_bytes()).unwrap();
    }
    drop(stdin);
    let out = BufReader::new(child.stdout.take().unwrap());
    let replies: Vec<Value> = out.lines().map(|l| serde_json::from_str(&l.unwrap()).unwrap()).collect();
    child.wait().unwrap();
    replies
}

/// AC-180: the recorded exchange of the spike (initialize, initialized, tools/list, tools/call
/// roster), replayed through the real shim against a real daemon. The shim serves what the
/// daemon holds and nothing of its own.
#[test]
fn ac180_mcp_shim_serves_overseers_tools_from_the_daemon() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo hi"]);
    let run = run_id(&created);
    d.wait_done(&run, 20);
    let token = d.call("overseer.token", json!({"run_id": "t-overseer", "role": "overseer"}))["token"].as_str().unwrap().to_string();
    let replies = mcp_exchange(
        &d,
        &token,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "roster", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "agent", "arguments": {"id": run}}}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "shell", "arguments": {"command": "ls"}}}),
            json!({"jsonrpc": "2.0", "id": 6, "method": "nonsense"}),
        ],
    );
    // A notification gets no reply: six replies for seven messages.
    assert_eq!(replies.len(), 6, "{replies:?}");
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "overseer");
    assert_eq!(replies[0]["result"]["capabilities"]["tools"]["listChanged"], false);
    let tools: Vec<&str> = replies[1]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(tools, ["roster", "agent", "conflicts", "conversation", "changes", "diff", "file", "search", "usage", "check_in", "rally", "answer", "propose"]);
    let roster = replies[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(roster.contains(&run) && roster.contains("completed"), "roster names the run: {roster}");
    assert_eq!(replies[2]["result"]["isError"], false);
    let digest = replies[3]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(digest.contains(&format!("id: {run}")) && digest.contains("status: completed"), "{digest}");
    // A tool the daemon does not give this role is refused as a tool error the model can read.
    assert_eq!(replies[4]["result"]["isError"], true);
    assert!(replies[4]["result"]["content"][0]["text"].as_str().unwrap().contains("no tool shell"));
    assert_eq!(replies[5]["error"]["code"], -32601);
    // Every call is an event on the calling run, with the role and the tool.
    let events = d.events("t-overseer");
    let calls: Vec<&str> = events.iter().filter(|e| e["kind"] == "overseer_tool_call").map(|e| e["payload"]["name"].as_str().unwrap()).collect();
    assert_eq!(calls, ["roster", "agent"]);
}

/// Who is speaking comes from the token, never from the text: an unknown token gets nothing,
/// and an agent's token does not open Overseer's tools.
#[test]
fn ac180_tokens_decide_who_may_call_what() {
    let d = Daemon::start(&[]);
    let err = d.try_call("overseer.tools", json!({"token": "made-up"})).unwrap_err();
    assert!(err.contains("unknown token"), "{err}");
    let replies = mcp_exchange(&d, "made-up", &[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})]);
    assert!(replies[0]["error"]["message"].as_str().unwrap().contains("unknown token"));
    let agent = d.call("overseer.token", json!({"run_id": "r-agent", "role": "agent"}))["token"].as_str().unwrap().to_string();
    let tools = d.call("overseer.tools", json!({"token": agent}));
    let names: Vec<&str> = tools["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["roster", "report", "ask", "claim"], "an agent has the roster and its channel");
    let err = d.try_call("overseer.tool", json!({"token": agent, "name": "agent", "arguments": {"id": "x"}})).unwrap_err();
    assert!(err.contains("no tool agent"), "{err}");
    assert!(d.try_call("overseer.token", json!({"run_id": "r", "role": "king"})).is_err());
}

fn claude_fixture() -> String {
    repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()
}

/// The tests that spawn many fixture processes and assert on timing run one at a time; the
/// light ones still run alongside.
fn heavy() -> std::sync::MutexGuard<'static, ()> {
    static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    HEAVY.lock().unwrap_or_else(|e| e.into_inner())
}

/// A daemon whose Claude is the fixture, with the mode chosen per task through a mode file.
fn claude_daemon(mode_file: &Path) -> Daemon {
    Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude_fixture()), ("CLAUDE_FIXTURE_MODE_FILE", &mode_file.display().to_string()), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE")])
}

fn claude_task(d: &Daemon, repo: &Path, mode_file: &Path, mode: &str, title: &str, prompt: &str) -> String {
    std::fs::write(mode_file, mode).unwrap();
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": prompt, "title": title}));
    run_id(&created)
}

/// AC-183: the digest of an agent comes from the daemon's own records and events, with no model
/// and no git in the path, and says what the agent was asked, did, changed and waits for.
#[test]
fn ac183_digest_says_what_an_agent_was_asked_did_and_changed() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    std::fs::write(repo.join("README.md"), "# Demo\n\nSign-in notes.\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "readme"]);
    let d = claude_daemon(&mode_file);
    let run = claude_task(&d, &repo, &mode_file, "showcase", "Sessions", "Make expired sessions refresh once");
    d.wait_done(&run, 30);
    let started = std::time::Instant::now();
    let reply = d.call("agent.digest", json!({"run_id": run}));
    let took = started.elapsed();
    let digest = &reply["digest"];
    assert_eq!(digest["title"], "Sessions");
    assert_eq!(digest["role"], "agent");
    assert_eq!(digest["status"], "completed");
    assert_eq!(digest["harness"], "claude");
    assert_eq!(digest["asked"][0]["text"], "Make expired sessions refresh once");
    assert_eq!(digest["asked"][0]["source"], "task");
    assert_eq!(digest["repository"], repo.display().to_string());
    let changed: Vec<&str> = digest["changed"].as_array().unwrap().iter().map(|c| c["path"].as_str().unwrap()).collect();
    assert!(changed.contains(&"README.md") && changed.contains(&"src/auth/session-refresh-coordinator.ts"), "changed files from the harness's own events: {changed:?}");
    // What git sees in the worktree agrees.
    let ws = PathBuf::from(d.call("state", json!({}))["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == d.run(&run)["workspace_id"]).unwrap()["path"].as_str().unwrap());
    let porcelain = git(&ws, &["status", "--porcelain"]);
    assert!(porcelain.contains("README.md") && porcelain.contains("src/"), "{porcelain}");
    let last = digest["last_messages"].as_array().unwrap();
    assert!(last.last().unwrap().as_str().unwrap().starts_with("## Done: sessions refresh once"), "{last:?}");
    assert!(last.len() <= 3);
    assert_eq!(digest["usage"]["usage"]["input_tokens"], 18423, "{}", digest["usage"]);
    assert!(digest["waiting"].is_null());
    assert!(digest["children"].as_array().unwrap().is_empty());
    let text = reply["text"].as_str().unwrap();
    assert!(text.len() <= 4096 && text.contains("status: completed") && text.contains("- edit README.md") || text.contains("README.md"), "{text}");
    assert!(took < Duration::from_secs(2), "digest took {took:?}");
    // Building digests caused no turn on any run.
    let turns_before = d.call("run.turns", json!({"run_id": run})).as_array().unwrap().len();
    for _ in 0..5 {
        d.call("agent.digest", json!({"run_id": run}));
        d.call("agents.roster", json!({}));
    }
    assert_eq!(d.call("run.turns", json!({"run_id": run})).as_array().unwrap().len(), turns_before);
    assert_eq!(d.runs().len(), 1, "no run was started to build a digest");
}

/// AC-183: native children sit under their parent; nine agents and a nested child give a roster
/// equal to the daemon's state; a credential in an agent's output never reaches the digest; a
/// burst of events leaves the digest within its size and current within 2 s.
#[test]
fn ac183_roster_equals_state_and_digests_stay_bounded_and_clean() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = claude_daemon(&mode_file);
    let nested = claude_task(&d, &repo, &mode_file, "nested", "Nested", "delegate");
    d.wait_done(&nested, 30);
    for i in 0..7 {
        let id = claude_task(&d, &repo, &mode_file, "echo", &format!("Agent {i}"), "hello");
        d.wait_done(&id, 30);
    }
    let secret = d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo token sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"]);
    let secret = run_id(&secret);
    d.wait_done(&secret, 20);
    let digest = d.call("agent.digest", json!({"run_id": nested}))["digest"].clone();
    let children = digest["children"].as_array().unwrap();
    assert_eq!(children.len(), 2, "child and grandchild: {children:?}");
    let child_digest = d.call("agent.digest", json!({"run_id": children[0]["id"].as_str().unwrap()}))["digest"].clone();
    assert_eq!(child_digest["role"], "child");
    let state = d.call("state", json!({}));
    let top: std::collections::BTreeSet<String> = state["runs"].as_array().unwrap().iter().filter(|r| r["parent_run_id"].is_null()).map(|r| r["id"].as_str().unwrap().to_string()).collect();
    let roster = d.call("agents.roster", json!({}));
    let listed: std::collections::BTreeSet<String> = roster["roster"].as_array().unwrap().iter().map(|l| l["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(listed, top, "the roster lists every top-level run and nothing else");
    assert_eq!(listed.len(), 9);
    assert_eq!(roster["roster"].as_array().unwrap().iter().find(|l| l["id"] == nested).unwrap()["children"], 2);
    let text = roster["text"].as_str().unwrap();
    assert!(text.len() <= 16 * 1024 && text.lines().count() == 9, "{text}");
    let clean = d.call("agent.digest", json!({"run_id": secret}));
    let all = format!("{} {}", clean["text"].as_str().unwrap(), clean["digest"]);
    assert!(!all.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"), "the credential is not in the digest: {all}");
    assert!(all.contains("token"), "the message itself is there");
    // A burst: 2,000 lines of output, then the digest is still small and immediately current.
    let burst = d.generic(&repo, "worktree", "/bin/sh", &["-c", "i=0; while [ $i -lt 2000 ]; do echo line $i; i=$((i+1)); done"]);
    let burst = run_id(&burst);
    d.wait_done(&burst, 60);
    let started = std::time::Instant::now();
    let reply = d.call("agent.digest", json!({"run_id": burst}));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(reply["text"].as_str().unwrap().len() <= 4096);
    assert_eq!(reply["digest"]["status"], "completed");
    assert!(reply["digest"]["last_messages"].as_array().unwrap().len() <= 3);
}

fn write_lines(path: &Path, lines: &[&str]) {
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

/// A long-lived agent in its own worktree that edits files as told through a command file.
fn sleeper(d: &Daemon, repo: &Path, title: &str) -> (String, PathBuf) {
    let created = d.call("task.create", json!({"repo": repo, "harness": "generic", "workspace_mode": "worktree", "program": "/bin/sh", "args": ["-c", "sleep 120"], "prompt": "", "title": title}));
    let id = run_id(&created);
    let ws = ws_path(d, &created);
    d.wait_status(&id, |s| s == "running", 20);
    (id, ws)
}

fn refs(repo: &Path) -> String {
    git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"])
}

/// AC-192: same lines, same file, gone when the overlap goes, dismissed by the owner, and the
/// target branch moving under an agent; found with no model and touching no worktree, index or
/// branch.
#[test]
fn ac192_conflicts_between_agents_in_flight() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let twenty: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
    let twenty: Vec<&str> = twenty.iter().map(String::as_str).collect();
    write_lines(&repo.join("a.txt"), &twenty);
    write_lines(&repo.join("b.txt"), &twenty);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "twenty lines"]);
    let d = Daemon::start(&[]);
    let (a, ws_a) = sleeper(&d, &repo, "Agent A");
    let (b, ws_b) = sleeper(&d, &repo, "Agent B");
    let (_c, ws_c) = sleeper(&d, &repo, "Agent C");
    // A and B change line 1 of a.txt differently; both change b.txt on lines far apart; C touches c.txt.
    let mut a_lines = twenty.clone();
    a_lines[0] = "line 1 by A";
    write_lines(&ws_a.join("a.txt"), &a_lines);
    let mut ab = twenty.clone();
    ab[0] = "b.txt line 1 by A";
    write_lines(&ws_a.join("b.txt"), &ab);
    let mut b_lines = twenty.clone();
    b_lines[0] = "line 1 by B";
    write_lines(&ws_b.join("a.txt"), &b_lines);
    let mut bb = twenty.clone();
    bb[10] = "b.txt line 11 by B";
    write_lines(&ws_b.join("b.txt"), &bb);
    std::fs::write(ws_c.join("c.txt"), "c\n").unwrap();
    let before = (fingerprint(&ws_a), fingerprint(&ws_b), fingerprint(&repo), refs(&repo));
    // Found by the sweep (no harness reported these edits), within the bound.
    let started = std::time::Instant::now();
    let found = loop {
        let list = d.call("conflicts.list", json!({"run_id": a}));
        let items = list["conflicts"].as_array().unwrap().clone();
        if items.iter().any(|c| c["kind"] == "same_lines") && items.iter().any(|c| c["kind"] == "same_file") {
            break items;
        }
        assert!(started.elapsed() < Duration::from_secs(12), "conflicts not found in time: {items:?}");
        std::thread::sleep(Duration::from_millis(200));
    };
    let same_lines = found.iter().find(|c| c["kind"] == "same_lines").unwrap();
    assert_eq!(same_lines["paths"], json!(["a.txt"]));
    assert_eq!(same_lines["needs_decision"], true);
    let same_file = found.iter().find(|c| c["kind"] == "same_file").unwrap();
    assert_eq!(same_file["paths"], json!(["b.txt"]));
    assert_eq!(same_file["needs_decision"], false);
    let pair: std::collections::BTreeSet<&str> = [same_lines["run_a"].as_str().unwrap(), same_lines["run_b"].as_str().unwrap()].into();
    assert_eq!(pair, [a.as_str(), b.as_str()].into());
    assert_eq!(found.len(), 2, "C collides with nobody: {found:?}");
    let after = (fingerprint(&ws_a), fingerprint(&ws_b), fingerprint(&repo), refs(&repo));
    assert_eq!(before, after, "detection touched no worktree, index or branch");
    // Both agents were told, and the roster and digest carry it.
    let ev = d.events(&b);
    assert!(ev.iter().any(|e| e["kind"] == "conflict" && e["payload"]["kind"] == "same_lines" && (e["payload"]["title_a"] == "Agent A" || e["payload"]["title_b"] == "Agent A")), "{ev:?}");
    let roster = d.call("agents.roster", json!({}));
    assert_eq!(roster["roster"].as_array().unwrap().iter().find(|l| l["id"] == a).unwrap()["open_conflicts"], 2);
    let digest = d.call("agent.digest", json!({"run_id": b}))["digest"].clone();
    assert_eq!(digest["conflicts"].as_array().unwrap().len(), 2);
    assert_eq!(digest["conflicts"][0]["other_title"], "Agent A");
    // B reverts its a.txt change: the same-lines conflict goes away by itself.
    write_lines(&ws_b.join("a.txt"), &twenty);
    d.call("overseer.scan", json!({"run_id": b}));
    let list = d.call("conflicts.list", json!({"run_id": a, "include_closed": true}));
    let gone = list["conflicts"].as_array().unwrap().iter().find(|c| c["kind"] == "same_lines").unwrap();
    assert_eq!(gone["state"], "gone");
    assert!(d.events(&a).iter().any(|e| e["kind"] == "conflict_closed" && e["payload"]["state"] == "gone"));
    // The owner dismisses the same-file one.
    let id = same_file["id"].as_str().unwrap();
    d.call("conflict.dismiss", json!({"id": id, "by": "user"}));
    assert!(d.try_call("conflict.dismiss", json!({"id": id})).is_err());
    assert!(d.call("conflicts.list", json!({"run_id": a}))["conflicts"].as_array().unwrap().is_empty());
    // main moves under A with a change to the line A changed: target moved.
    let mut main_lines = twenty.clone();
    main_lines[0] = "line 1 on main";
    write_lines(&repo.join("a.txt"), &main_lines);
    git(&repo, &["commit", "-qam", "main moves"]);
    let scan = d.call("overseer.scan", json!({"run_id": a}));
    assert!(scan["ms"].as_u64().unwrap() < 10_000);
    let list = d.call("conflicts.list", json!({"run_id": a}));
    let moved = list["conflicts"].as_array().unwrap().iter().find(|c| c["kind"] == "target_moved").expect("target moved");
    assert_eq!(moved["paths"], json!(["a.txt"]));
    assert_eq!(moved["target"], "main");
    assert_eq!(moved["needs_decision"], false);
    // Detection caused no turn and started no run.
    assert_eq!(d.call("run.turns", json!({"run_id": a})).as_array().unwrap().len(), 1);
    assert_eq!(d.runs().len(), 3);
}

/// AC-192: sixteen agents in a 10,000-file repository: one scan stays within the bound and the
/// daemon keeps answering meanwhile.
#[test]
fn ac192_sixteen_agents_in_a_large_repository() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    for dnum in 0..100 {
        let dir = repo.join(format!("src/m{dnum:03}"));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..100 {
            std::fs::write(dir.join(format!("f{f:03}.txt")), format!("file {dnum} {f}\nline 2\nline 3\n")).unwrap();
        }
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "ten thousand files"]);
    let d = Daemon::start(&[]);
    let mut agents = Vec::new();
    for i in 0..16 {
        let (id, ws) = sleeper(&d, &repo, &format!("Agent {i}"));
        // Each edits its own module, and every second one also the shared file.
        std::fs::write(ws.join(format!("src/m{i:03}/f000.txt")), format!("agent {i}\nline 2\nline 3\n")).unwrap();
        if i % 2 == 0 {
            std::fs::write(ws.join("src/m099/f099.txt"), format!("shared by agent {i}\nline 2\nline 3\n")).unwrap();
        }
        agents.push(id);
    }
    let started = std::time::Instant::now();
    let scan = d.call("overseer.scan", json!({"run_id": agents[0]}));
    let took = started.elapsed();
    assert_eq!(scan["compared_with"], 15);
    assert!(took < Duration::from_secs(10), "scan of 16 agents took {took:?}");
    let list = d.call("conflicts.list", json!({"run_id": agents[0]}));
    let same_lines: Vec<&Value> = list["conflicts"].as_array().unwrap().iter().filter(|c| c["kind"] == "same_lines").collect();
    assert_eq!(same_lines.len(), 7, "agent 0 collides with the other seven even agents on the shared file: {list}");
    // The daemon answers while a scan runs.
    let d2 = std::sync::Arc::new(d);
    let d3 = d2.clone();
    let id = agents[2].clone();
    let scanner = std::thread::spawn(move || d3.call("overseer.scan", json!({"run_id": id})));
    let t = std::time::Instant::now();
    d2.call("state", json!({}));
    assert!(t.elapsed() < Duration::from_secs(2), "state answered during a scan");
    scanner.join().unwrap();
    std::sync::Arc::try_unwrap(d2).ok().expect("daemon still shared");
}

/// A daemon whose Claude is the fixture in Overseer mode: it speaks MCP when Overseer's run gives
/// it the daemon's tools, and answers from the state otherwise.
fn overseer_daemon(mode_file: &Path) -> Daemon {
    std::fs::write(mode_file, "overseer").unwrap();
    claude_daemon(mode_file)
}

fn session(d: &Daemon) -> Value {
    d.call("overseer.session", json!({}))
}

fn wait_overseer_idle(d: &Daemon, secs: u64) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let s = session(d);
        let status = s["run_status"].as_str().unwrap_or("");
        if !s["run_id"].is_null() && !["queued", "starting", "running", "waiting_for_user"].contains(&status) {
            return s;
        }
        assert!(std::time::Instant::now() < deadline, "Overseer's run did not finish: {s}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// AC-181: the conversation lives in the daemon and is reached over the socket with no UI; the
/// run it uses is listed in no agents list; a proposal is answered once; a declined one changes
/// nothing; two clients see the same messages in the same order.
#[test]
fn ac181_the_conversation_lives_in_the_daemon() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    std::fs::write(&mode_file, "echo").unwrap();
    let api = claude_task(&d, &repo, &mode_file, "echo", "API tests", "write the API");
    let front = claude_task(&d, &repo, &mode_file, "echo", "Frontend fixer", "fix the header");
    d.wait_done(&api, 30);
    d.wait_done(&front, 30);
    std::fs::write(&mode_file, "overseer").unwrap();
    let before = session(&d);
    assert_eq!(before["level"], "ask_first");
    assert!(before["run_id"].is_null() && before["messages"].as_array().unwrap().is_empty());
    // The first message starts Overseer's run; the answer comes from the roster the tools serve.
    let sent = d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    assert_eq!(sent["queued"], false);
    let s = wait_overseer_idle(&d, 30);
    let run = s["run_id"].as_str().unwrap().to_string();
    let messages = s["messages"].as_array().unwrap();
    assert_eq!(messages[0]["source"], "owner");
    assert_eq!(messages[0]["text"], "What is everyone doing?");
    assert_eq!(messages[0]["surface"], "ctl");
    let reply = messages.iter().find(|m| m["source"] == "overseer").expect("Overseer answered");
    let text = reply["text"].as_str().unwrap();
    assert!(text.contains("API tests") && text.contains("Frontend fixer") && text.contains("completed"), "{text}");
    // Its run took the daemon's tools (the fixture reports the MCP server it spoke to) and the
    // daemon answered the tool's permission request itself.
    let events = d.events(&run);
    assert!(events.iter().any(|e| e["kind"] == "tool" && e["payload"]["name"] == "mcp__overseer__roster"), "roster called through MCP");
    assert!(events.iter().any(|e| e["kind"] == "permission" && e["payload"]["auto_allowed"].is_string()), "auto-allowed");
    assert!(!events.iter().any(|e| e["kind"] == "status" && e["payload"]["status"] == "waiting_for_user"));
    let calls: Vec<&Value> = events.iter().filter(|e| e["kind"] == "overseer_tool_call").collect();
    assert!(calls.iter().any(|c| c["payload"]["name"] == "roster"));
    // Listed in no agents list, counted nowhere; asked for, it is there.
    let state = d.call("state", json!({}));
    assert!(state["runs"].as_array().unwrap().iter().all(|r| r["id"] != run), "hidden from state");
    assert!(state["tasks"].as_array().unwrap().iter().all(|t| t["id"] != s["task_id"]));
    assert_eq!(state["runs"].as_array().unwrap().len(), 2);
    let full = d.call("state", json!({"include_hidden": true}));
    assert!(full["runs"].as_array().unwrap().iter().any(|r| r["id"] == run));
    assert!(d.call("agents.roster", json!({}))["roster"].as_array().unwrap().iter().all(|l| l["id"] != run));
    assert!(d.call("run.active", json!({})).as_array().unwrap().iter().all(|r| r["id"] != run));
    // A proposal: nothing happens before the yes; the yes sends the message from Overseer; a
    // second answer gets the first one's outcome.
    let turns_before = d.call("run.turns", json!({"run_id": api})).as_array().unwrap().len();
    d.call("overseer.send", json!({"text": "Tell API tests to add tests", "surface": "ctl"}));
    let s = wait_overseer_idle(&d, 30);
    let open = s["proposals"].as_array().unwrap();
    assert_eq!(open.len(), 1, "{s}");
    let proposal = open[0]["id"].as_str().unwrap().to_string();
    assert_eq!(open[0]["actions"][0]["action"], "message");
    assert_eq!(open[0]["actions"][0]["agent"], api);
    assert_eq!(open[0]["actions"][0]["text"], "Please add tests.");
    assert_eq!(d.call("run.turns", json!({"run_id": api})).as_array().unwrap().len(), turns_before, "nothing happened yet");
    let card = d.events(&run).into_iter().find(|e| e["kind"] == "proposal").unwrap();
    assert_eq!(card["payload"]["lines"][0], "Send API tests: “Please add tests.”");
    let answer = d.call("overseer.answer", json!({"id": proposal, "yes": true, "surface": "ctl", "by": "owner"}));
    assert_eq!(answer["state"], "yes");
    assert!(answer["result"].as_str().unwrap().starts_with("Done: sent \"Please add tests.\" to API tests"), "{answer}");
    let turns = d.call("run.turns", json!({"run_id": api}));
    let last = turns.as_array().unwrap().last().unwrap();
    assert_eq!(last["prompt"], "From Overseer: Please add tests.");
    let again = d.try_call("overseer.answer", json!({"id": proposal, "yes": false, "surface": "phone", "by": "phone"})).unwrap_err();
    assert!(again.contains("already_answered") && again.contains("yes"), "{again}");
    assert!(d.events(&run).iter().any(|e| e["kind"] == "proposal_answered" && e["payload"]["id"] == proposal && e["payload"]["state"] == "yes"));
    d.wait_done(&api, 30);
    // A declined proposal changes nothing.
    let front_turns = d.call("run.turns", json!({"run_id": front})).as_array().unwrap().len();
    d.call("overseer.send", json!({"text": "Tell Frontend fixer to update the docs", "surface": "ctl"}));
    let s = wait_overseer_idle(&d, 30);
    let proposal = s["proposals"][0]["id"].as_str().unwrap().to_string();
    let no = d.call("overseer.answer", json!({"id": proposal, "yes": false, "surface": "ctl", "by": "owner"}));
    assert_eq!(no["state"], "no");
    assert_eq!(d.call("run.turns", json!({"run_id": front})).as_array().unwrap().len(), front_turns);
    // Two clients, the same messages in the same order.
    let a = d.call("overseer.messages", json!({"after": 0}))["messages"].clone();
    let b = d.call("overseer.messages", json!({"after": 0}))["messages"].clone();
    assert_eq!(a, b);
    let seqs: Vec<i64> = a.as_array().unwrap().iter().map(|m| m["seq"].as_i64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]) && seqs.len() >= 5, "{seqs:?}");
}

/// AC-181: what Overseer coordinates survives a daemon restart; an action approved but not
/// carried out when the daemon died is reported as not done, never done twice.
#[test]
fn ac181_restart_keeps_the_conversation_and_never_repeats_an_action() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let mut d = overseer_daemon(&mode_file);
    std::fs::write(&mode_file, "echo").unwrap();
    let api = claude_task(&d, &repo, &mode_file, "echo", "API tests", "write the API");
    d.wait_done(&api, 30);
    std::fs::write(&mode_file, "overseer").unwrap();
    d.call("overseer.send", json!({"text": "Tell API tests to add tests", "surface": "ctl", "harness": "claude"}));
    let s = wait_overseer_idle(&d, 30);
    let proposal = s["proposals"][0]["id"].as_str().unwrap().to_string();
    let messages = s["messages"].as_array().unwrap().len();
    // The daemon dies between the owner's yes and the action: the proposal is left 'answering'.
    let db = d.home.path().join("overseer.sqlite");
    d.kill9();
    let sql = format!("UPDATE overseer_proposals SET state='answering', answered_by='owner', answered_ms=1 WHERE id='{proposal}';");
    let out = Command::new("sqlite3").arg(&db).arg(&sql).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    d.spawn();
    let s = session(&d);
    assert_eq!(s["messages"].as_array().unwrap().len(), messages, "the conversation is as before");
    let turns = d.call("run.turns", json!({"run_id": api})).as_array().unwrap().len();
    assert_eq!(turns, 1, "the action did not happen");
    let all = d.call("overseer.session", json!({}));
    assert!(all["proposals"].as_array().unwrap().is_empty(), "no open proposal");
    let err = d.try_call("overseer.answer", json!({"id": proposal, "yes": true, "surface": "ctl"})).unwrap_err();
    assert!(err.contains("already_answered") && err.contains("Not done"), "{err}");
    assert_eq!(d.call("run.turns", json!({"run_id": api})).as_array().unwrap().len(), 1, "never twice");
    // The level survives too.
    d.call("overseer.level", json!({"level": "steer"}));
    d.kill9();
    d.spawn();
    assert_eq!(d.call("overseer.level", json!({}))["level"], "steer");
    assert!(d.try_call("overseer.level", json!({"level": "king"})).is_err());
}

/// AC-184: Overseer's run reads through bounded, redacted tools that stay inside a worktree;
/// its launch takes away the harness's own shell, file and network tools.
#[test]
fn ac184_overseer_reads_on_demand_and_only_reads() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::write(repo.join("README.md"), "# Demo\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "readme"]);
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    std::fs::write(&mode_file, "showcase").unwrap();
    let run = claude_task(&d, &repo, &mode_file, "showcase", "Sessions", "Make expired sessions refresh once");
    d.wait_done(&run, 30);
    let token = d.call("overseer.token", json!({"run_id": "t-overseer", "role": "overseer"}))["token"].as_str().unwrap().to_string();
    let tool = |name: &str, args: Value| d.try_call("overseer.tool", json!({"token": token, "name": name, "arguments": args}));
    let names: Vec<String> = d.call("overseer.tools", json!({"token": token}))["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for n in ["roster", "agent", "conversation", "changes", "diff", "file", "search", "conflicts", "usage", "propose"] {
        assert!(names.contains(&n.to_string()), "{n} missing from {names:?}");
    }
    // Reads that answer, bounded.
    let conv = tool("conversation", json!({"id": run})).unwrap();
    assert!(conv["text"].as_str().unwrap().contains("[assistant]") && conv["text"].as_str().unwrap().contains("last sequence"));
    let changes = tool("changes", json!({"id": run})).unwrap();
    assert!(changes["text"].as_str().unwrap().contains("README.md"), "{changes}");
    let diff = tool("diff", json!({"id": run, "path": "README.md"})).unwrap();
    assert!(diff["text"].as_str().unwrap().contains("+## Sessions"), "{diff}");
    let file = tool("file", json!({"id": run, "path": "src/auth/session-refresh-coordinator.ts"})).unwrap();
    assert!(file["text"].as_str().unwrap().contains("SessionRefreshCoordinator"));
    assert!(tool("search", json!({"query": "expired sessions"})).unwrap()["text"].as_str().unwrap().contains("Sessions"));
    assert!(tool("usage", json!({"id": run})).unwrap()["text"].as_str().unwrap().contains("18423"));
    assert_eq!(tool("conflicts", json!({})).unwrap()["text"], "No open conflicts.");
    // Reads that are refused: outside the worktree, through a symlink, a folder, a missing agent.
    let ws = PathBuf::from(d.call("state", json!({}))["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == d.run(&run)["workspace_id"]).unwrap()["path"].as_str().unwrap());
    std::os::unix::fs::symlink(r.path().join("repo"), ws.join("escape")).unwrap();
    std::fs::write(r.path().join("secret.txt"), "outside\n").unwrap();
    std::os::unix::fs::symlink(r.path().join("secret.txt"), ws.join("leak.txt")).unwrap();
    for (name, args) in [("file", json!({"id": run, "path": "../secret.txt"})), ("file", json!({"id": run, "path": "/etc/hosts"})), ("file", json!({"id": run, "path": "leak.txt"})), ("file", json!({"id": run, "path": "escape/README.md"})), ("diff", json!({"id": run, "path": "../a.txt"})), ("file", json!({"id": run, "path": "src"})), ("agent", json!({"id": "r-nobody"}))] {
        assert!(tool(name, args.clone()).is_err(), "{name} {args} should be refused");
    }
    // Bounded: a big file reads back cut at the bound, with a note.
    let big = "x".repeat(100 * 1024);
    std::fs::write(ws.join("big.txt"), &big).unwrap();
    let text = tool("file", json!({"id": run, "path": "big.txt"})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(text.len() < 33 * 1024 && text.contains("[cut at"), "{}", text.len());
    // Overseer's own run is launched with the harness's shell, file and network tools taken away
    // and only Overseer's tools allowed.
    std::fs::write(&mode_file, "overseer").unwrap();
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    let s = wait_overseer_idle(&d, 30);
    let (_, dir) = launch_info(&d, s["run_id"].as_str().unwrap());
    let launch: Value = serde_json::from_slice(&std::fs::read(dir.join("launch.json")).unwrap()).unwrap();
    let args: Vec<String> = launch["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
    let joined = args.join(" ");
    assert!(joined.contains("--mcp-config") && joined.contains("--strict-mcp-config"), "{joined}");
    assert!(joined.contains("--disallowedTools Bash,Edit,Write"), "{joined}");
    assert!(joined.contains("--allowedTools mcp__overseer__roster"), "{joined}");
    assert!(!joined.contains("--permission-mode plan"));
    let mcp: Value = serde_json::from_slice(&std::fs::read(d.home.path().join("overseer/scratch/mcp.json")).unwrap()).unwrap();
    assert_eq!(mcp["mcpServers"]["overseer"]["args"][0], "mcp");
    // Its folder holds nothing but its own files: no code was produced.
    let scratch = d.home.path().join("overseer/scratch");
    let entries: Vec<String> = std::fs::read_dir(&scratch).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert!(entries.iter().all(|e| [".git", "README.md", "mcp.json"].contains(&e.as_str())), "{entries:?}");
}

/// AC-184: a binary file is refused as text; Overseer quotes a diff it read through its tool;
/// with sixteen agents the turn's input stays within the bound; a harness without tools gets
/// the state with the message and its proposal comes from its text, said so on the card.
#[test]
fn ac184_quotes_diffs_bounds_turns_and_falls_back_without_tools() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::write(repo.join("README.md"), "# Demo\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "readme"]);
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    let sessions = claude_task(&d, &repo, &mode_file, "showcase", "Sessions", "Make expired sessions refresh once");
    d.wait_done(&sessions, 30);
    let token = d.call("overseer.token", json!({"run_id": "t-overseer", "role": "overseer"}))["token"].as_str().unwrap().to_string();
    let ws = PathBuf::from(d.call("state", json!({}))["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == d.run(&sessions)["workspace_id"]).unwrap()["path"].as_str().unwrap());
    std::fs::write(ws.join("blob.bin"), [0u8, 159, 146, 150, 255, 0, 1]).unwrap();
    let err = d.try_call("overseer.tool", json!({"token": token, "name": "file", "arguments": {"id": sessions, "path": "blob.bin"}})).unwrap_err();
    assert!(err.contains("not a text file"), "{err}");
    // Overseer answers about a file with the diff it read through its tool.
    std::fs::write(&mode_file, "overseer").unwrap();
    d.call("overseer.send", json!({"text": "What did Sessions change in README.md?", "surface": "ctl", "harness": "claude"}));
    let s = wait_overseer_idle(&d, 30);
    let reply = s["messages"].as_array().unwrap().iter().rev().find(|m| m["source"] == "overseer").unwrap();
    assert!(reply["text"].as_str().unwrap().contains("+## Sessions"), "{}", reply["text"]);
    let run = s["run_id"].as_str().unwrap().to_string();
    assert!(d.events(&run).iter().any(|e| e["kind"] == "overseer_tool_call" && e["payload"]["name"] == "diff"));
    // Sixteen agents: the turn's input stays within 32 KiB.
    for i in 0..15 {
        let id = claude_task(&d, &repo, &mode_file, "showcase", &format!("Agent {i}"), &format!("Task {i}: {}", "make it good ".repeat(30)));
        d.wait_done(&id, 60);
    }
    std::fs::write(&mode_file, "overseer").unwrap();
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl"}));
    let s = wait_overseer_idle(&d, 60);
    let turns = d.call("run.turns", json!({"run_id": run}));
    let last = turns.as_array().unwrap().last().unwrap();
    let prompt = last["prompt"].as_str().unwrap();
    assert!(prompt.len() <= 32 * 1024, "turn input {} bytes", prompt.len());
    assert!(prompt.contains("Agents (JSON):") && prompt.ends_with("What is everyone doing?"));
    let reply = s["messages"].as_array().unwrap().iter().rev().find(|m| m["source"] == "overseer").unwrap();
    assert!(reply["text"].as_str().unwrap().contains("Agent 14"));
    // A harness without tools: the state goes with the message, the proposal comes from the text.
    let d2 = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude_fixture()), ("CLAUDE_FIXTURE_MODE_FILE", &mode_file.display().to_string()), ("CLAUDE_FIXTURE_NO_MCP", "1"), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,CLAUDE_FIXTURE_NO_MCP")]);
    std::fs::write(&mode_file, "echo").unwrap();
    let api = claude_task(&d2, &repo, &mode_file, "echo", "API tests", "write the API");
    d2.wait_done(&api, 30);
    std::fs::write(&mode_file, "overseer").unwrap();
    d2.call("overseer.send", json!({"text": "Tell API tests to add tests", "surface": "ctl", "harness": "claude"}));
    let s = wait_overseer_idle(&d2, 30);
    let open = s["proposals"].as_array().unwrap();
    assert_eq!(open.len(), 1, "{s}");
    let run2 = s["run_id"].as_str().unwrap();
    let card = d2.events(run2).into_iter().find(|e| e["kind"] == "proposal").unwrap();
    assert_eq!(card["payload"]["via"], "text");
    assert!(card["payload"]["note"].as_str().unwrap().contains("no tools"));
    assert!(!d2.events(run2).iter().any(|e| e["kind"] == "tool"), "no tool was called");
    let said = s["messages"].as_array().unwrap().iter().rev().find(|m| m["source"] == "overseer").unwrap();
    assert!(!said["text"].as_str().unwrap().contains("overseer-actions"), "the block is not shown as text: {}", said["text"]);
    let yes = d2.call("overseer.answer", json!({"id": open[0]["id"], "yes": true, "surface": "ctl"}));
    assert!(yes["result"].as_str().unwrap().starts_with("Done: sent"));
}

/// The SQL the tests use to put the daemon's store into a state a client cannot ask for.
fn sql(d: &Daemon, statement: &str) {
    let db = d.home.path().join("overseer.sqlite");
    let out = Command::new("sqlite3").arg(&db).arg(statement).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// AC-185: every daemon method has a class; a Confirm action with no request from the owner is
/// refused; "stop everyone" is one card with a row per agent; a card's text is the text sent;
/// cards survive a restart; a native child is refused.
#[test]
fn ac185_actions_have_classes_and_cards() {
    let _one_at_a_time = heavy();
    // Every method dispatched by the daemon is in the class table (the table is the source of
    // the phone's classes too); a method without one is a test failure.
    let source = std::fs::read_to_string(repo_root().join("daemon/src/server.rs")).unwrap();
    let classes = std::fs::read_to_string(repo_root().join("daemon/src/overseer/control.rs")).unwrap();
    let mut missing = Vec::new();
    for line in source.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix('"') {
            if let Some(end) = rest.find("\" =>") {
                let method = &rest[..end];
                if method.contains('.') || method == "hello" || method == "state" || method == "search" {
                    if !classes.contains(&format!("(\"{method}\", ")) {
                        missing.push(method.to_string());
                    }
                }
            }
        }
    }
    assert!(classes.contains("(\"events.subscribe\", \"read\")"));
    assert!(missing.is_empty(), "methods without a class: {missing:?}");
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let mut d = overseer_daemon(&mode_file);
    let mut agents = Vec::new();
    for i in 0..4 {
        let created = d.call("task.create", json!({"repo": repo, "harness": "generic", "workspace_mode": "worktree", "program": "/bin/sh", "args": ["-c", "sleep 120"], "prompt": "", "title": format!("Agent {i}")}));
        agents.push(run_id(&created));
    }
    for a in &agents {
        d.wait_status(a, |s| s == "running", 20);
    }
    // A nested fixture run for the native-child refusal.
    let nested = claude_task(&d, &repo, &mode_file, "nested", "Nested", "delegate");
    d.wait_done(&nested, 30);
    let child = d.call("state", json!({}))["runs"].as_array().unwrap().iter().find(|r| r["parent_run_id"] == nested).unwrap()["id"].as_str().unwrap().to_string();
    // The session exists; the owner asked for nothing yet, so a Confirm action from Overseer itself is refused.
    d.call("overseer.session", json!({}));
    sql(&d, "UPDATE overseer_sessions SET last_cause='check_in';");
    let err = d.try_call("overseer.propose", json!({"actions": [{"action": "archive", "agent": agents[0]}], "source": "test"})).unwrap_err();
    assert!(err.contains("only when the owner asks"), "{err}");
    let err = d.try_call("overseer.propose", json!({"actions": [{"action": "message", "agent": child, "text": "hi"}], "source": "test"})).unwrap_err();
    assert!(err.contains("native child"), "{err}");
    let err = d.try_call("overseer.propose", json!({"actions": [{"action": "cleanup", "agent": agents[0]}], "source": "test"})).unwrap_err();
    assert!(err.contains("not an action Overseer has"), "{err}");
    // Stop everyone: one card with four rows; four interrupts within a second of the yes.
    sql(&d, "UPDATE overseer_sessions SET last_cause='owner';");
    let stops: Vec<Value> = agents.iter().map(|a| json!({"action": "stop", "agent": a})).collect();
    let proposed = d.call("overseer.propose", json!({"actions": stops, "source": "test"}));
    let card_id = proposed["proposal"].as_str().unwrap().to_string();
    assert_eq!(proposed["state"], "open");
    let started = std::time::Instant::now();
    let answer = d.call("overseer.answer", json!({"id": card_id, "yes": true, "surface": "ctl", "by": "owner"}));
    assert_eq!(answer["state"], "yes");
    for a in &agents {
        d.wait_status(a, |s| s == "interrupted", 5);
    }
    assert!(started.elapsed() < Duration::from_secs(2), "four stops took {:?}", started.elapsed());
    let card = d.call("overseer.card", json!({"id": card_id}));
    assert_eq!(card["rows"].as_array().unwrap().len(), 4);
    assert!(card["rows"].as_array().unwrap().iter().all(|r| r["action"] == "stop" && r["delivery"] == "stop"));
    assert_eq!(card["actions"].as_array().unwrap().len(), 4);
    // A message's card row holds the text that was sent, byte for byte.
    let msg = d.call("overseer.propose", json!({"actions": [{"action": "message", "agent": agents[1], "text": "Please add tests — carefully."}], "source": "test"}));
    let mid = msg["proposal"].as_str().unwrap().to_string();
    d.call("overseer.answer", json!({"id": mid, "yes": true, "surface": "ctl", "by": "owner"}));
    let card = d.call("overseer.card", json!({"id": mid}));
    let row = &card["rows"][0];
    assert_eq!(row["message"], "Please add tests — carefully.");
    let turns = d.call("run.turns", json!({"run_id": agents[1]}));
    let last = turns.as_array().unwrap().last().unwrap();
    assert_eq!(last["prompt"], "From Overseer: Please add tests — carefully.");
    assert!(["delivered", "answered", "picked_up"].contains(&row["state"].as_str().unwrap()), "{row}");
    // Cards are the same after a restart.
    let before = d.call("overseer.card", json!({"id": card_id}));
    d.kill9();
    d.spawn();
    let after = d.call("overseer.card", json!({"id": card_id}));
    assert_eq!(before["rows"], after["rows"]);
    assert_eq!(before["state"], after["state"]);
}

/// AC-186: at Steer what the owner asked for goes out after the settle window and can be
/// cancelled inside it; what Overseer starts by itself is done at once when quiet and proposed
/// when not; at Auto everything Steer goes at once and Confirm still waits; two clients
/// answering one proposal at the same moment get one outcome; a stale proposal is not done.
#[test]
fn ac186_levels_decide_how_steer_actions_happen() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    let (a, _) = sleeper(&d, &repo, "Agent A");
    let (b, _) = sleeper(&d, &repo, "Agent B");
    let (c, _) = sleeper(&d, &repo, "Agent C");
    d.call("overseer.session", json!({}));
    let turns = |id: &str| d.call("run.turns", json!({"run_id": id})).as_array().unwrap().len();
    // Steer, the owner asked: a message settles, then goes.
    d.call("overseer.level", json!({"level": "steer"}));
    sql(&d, "UPDATE overseer_sessions SET last_cause='owner';");
    let p = d.call("overseer.propose", json!({"actions": [{"action": "message", "agent": a, "text": "Owner says hi"}], "source": "test"}));
    assert_eq!(p["state"], "settling");
    let before = turns(&a);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(turns(&a), before, "nothing yet inside the window");
    std::thread::sleep(Duration::from_millis(2200));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let card = loop {
        let card = d.call("overseer.card", json!({"id": p["proposal"]}));
        if card["state"] != "answering" || std::time::Instant::now() > deadline {
            break card;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(card["state"], "yes", "{card}");
    // The sleeper is a generic program, which takes a follow-up on its stdin at once (AC-60).
    assert_eq!(turns(&a), before + 1, "the message went after the window");
    // A cancel inside the window sends nothing.
    let p = d.call("overseer.propose", json!({"actions": [{"action": "redirect", "agent": b, "text": "Change course"}], "source": "test"}));
    assert_eq!(p["state"], "settling");
    d.call("overseer.cancel", json!({"id": p["proposal"], "by": "owner"}));
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(d.call("overseer.card", json!({"id": p["proposal"]}))["state"], "cancelled");
    assert_eq!(d.run(&b)["status"], "running", "not stopped");
    // Steer, Overseer by itself: a hold at once, a redirect waits.
    sql(&d, "UPDATE overseer_sessions SET last_cause='check_in';");
    let p = d.call("overseer.propose", json!({"actions": [{"action": "hold", "agent": c, "reason": "drifting"}], "source": "test"}));
    assert_eq!(p["done"], true, "{p}");
    assert!(d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().any(|h| h["run_id"] == c));
    let p = d.call("overseer.propose", json!({"actions": [{"action": "redirect", "agent": b, "text": "Change course"}], "source": "test"}));
    assert_eq!(p["state"], "open");
    assert_eq!(d.run(&b)["status"], "running");
    // Auto: the redirect goes at once, with its card and cause; a Confirm action still waits for
    // the owner (and needs the owner to have asked).
    d.call("overseer.level", json!({"level": "auto"}));
    let p = d.call("overseer.propose", json!({"actions": [{"action": "redirect", "agent": b, "text": "Change course now"}], "source": "test"}));
    assert_eq!(p["done"], true, "{p}");
    d.wait_status(&b, |s| s != "running", 10);
    let card = d.call("overseer.card", json!({"id": p["proposal"]}));
    assert_eq!(card["rows"][0]["delivery"], "redirect");
    assert_eq!(card["via"], "test");
    let ev = d.events(&b);
    assert!(ev.iter().any(|e| e["kind"] == "redirect" && e["payload"]["detail"]["by"].as_str().unwrap_or("").contains("auto")), "{:?}", ev.iter().filter(|e| e["kind"] == "redirect").collect::<Vec<_>>());
    sql(&d, "UPDATE overseer_sessions SET last_cause='owner';");
    let p = d.call("overseer.propose", json!({"actions": [{"action": "archive", "agent": c}], "source": "test"}));
    assert_eq!(p["state"], "open");
    assert_eq!(p["done"], false);
    // A stale proposal: the agent changed state since it was made.
    d.call("overseer.level", json!({"level": "ask_first"}));
    let (e, _) = sleeper(&d, &repo, "Agent E");
    let p = d.call("overseer.propose", json!({"actions": [{"action": "message", "agent": e, "text": "hi"}], "source": "test"}));
    d.call("run.interrupt", json!({"run_id": e}));
    d.wait_status(&e, |s| s == "interrupted", 10);
    let answer = d.call("overseer.answer", json!({"id": p["proposal"], "yes": true, "surface": "ctl", "by": "owner"}));
    assert_eq!(answer["state"], "stale");
    assert!(answer["result"].as_str().unwrap().contains("Ask again"));
    // Two clients answer one proposal within 50 ms of each other, 100 times: one outcome each time.
    let d = std::sync::Arc::new(d);
    for i in 0..100 {
        let p = d.call("overseer.propose", json!({"actions": [{"action": "pin", "agent": a}], "source": "test"}));
        let id = p["proposal"].as_str().unwrap().to_string();
        let (d1, d2) = (d.clone(), d.clone());
        let (i1, i2) = (id.clone(), id.clone());
        let t1 = std::thread::spawn(move || d1.try_call("overseer.answer", json!({"id": i1, "yes": true, "surface": "vscode", "by": "owner"})));
        let t2 = std::thread::spawn(move || d2.try_call("overseer.answer", json!({"id": i2, "yes": false, "surface": "phone", "by": "phone"})));
        let (r1, r2) = (t1.join().unwrap(), t2.join().unwrap());
        assert!(r1.is_ok() != r2.is_ok(), "round {i}: exactly one answer counts: {r1:?} {r2:?}");
        let card = d.call("overseer.card", json!({"id": id}));
        let winner = if r1.is_ok() { "yes" } else { "no" };
        assert_eq!(card["state"], winner, "round {i}");
        let loser = if r1.is_ok() { r2.unwrap_err() } else { r1.unwrap_err() };
        assert!(loser.contains("already_answered") && loser.contains(winner), "round {i}: {loser}");
    }
}

/// AC-187: a held agent starts no turn until released, from a queued message or from Overseer;
/// each release condition; hold everything; a write across a guardrail is reported within 2 s
/// and holds the agent when the guardrail says so; the label per harness; a restart keeps both.
#[test]
fn ac187_holds_and_guardrails() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::create_dir_all(repo.join("api")).unwrap();
    std::fs::create_dir_all(repo.join("docs")).unwrap();
    std::fs::write(repo.join("api/x.txt"), "x\n").unwrap();
    std::fs::write(repo.join("docs/y.txt"), "y\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "dirs"]);
    let mode_file = r.path().join("mode");
    let mut d = overseer_daemon(&mode_file);
    let turns = |d: &Daemon, id: &str| d.call("run.turns", json!({"run_id": id})).as_array().unwrap().len();
    // An idle agent, held: a queued message, Overseer's message and the owner's own message all wait.
    let done = d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo done"]);
    let done = run_id(&done);
    d.wait_done(&done, 20);
    let held = d.call("agent.hold", json!({"run_id": done, "reason": "wait for the review", "by": "owner"}));
    assert_eq!(held["held"], true);
    assert_eq!(d.call("run.queue", json!({"run_id": done, "text": "queued while held", "source": "owner"}))["delivery"], "queued");
    d.call("overseer.session", json!({}));
    d.call("overseer.level", json!({"level": "auto"}));
    sql(&d, "UPDATE overseer_sessions SET last_cause='check_in';");
    let p = d.call("overseer.propose", json!({"actions": [{"action": "message", "agent": done, "text": "from Overseer while held"}], "source": "test"}));
    assert_eq!(p["done"], true);
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(turns(&d, &done), 1, "no turn while held");
    let err = d.try_call("run.follow_up", json!({"run_id": done, "prompt": "owner's own"})).unwrap_err();
    assert!(err.contains("held") && err.contains("Release and send"), "{err}");
    // A restart keeps the hold.
    d.kill9();
    d.spawn();
    assert!(d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().any(|h| h["run_id"] == done && h["reason"] == "wait for the review"));
    // Released: what waited goes as one turn.
    d.call("agent.release", json!({"run_id": done, "by": "owner"}));
    d.wait_done(&done, 20);
    let t = d.call("run.turns", json!({"run_id": done}));
    assert_eq!(t.as_array().unwrap().len(), 2, "{t}");
    assert!(t[1]["prompt"].as_str().unwrap().contains("queued while held") && t[1]["prompt"].as_str().unwrap().contains("From Overseer: from Overseer while held"));
    // Release and send: the owner's message releases the hold.
    d.call("agent.hold", json!({"run_id": done, "reason": "again", "by": "owner"}));
    d.call("run.follow_up", json!({"run_id": done, "prompt": "owner's own", "release": true}));
    d.wait_done(&done, 20);
    assert!(d.call("agent.holds", json!({}))["holds"].as_array().unwrap().is_empty());
    // Release conditions: another agent finishing, a conflict closed, a time.
    let (x, _) = sleeper(&d, &repo, "X");
    let (y, _) = sleeper(&d, &repo, "Y");
    d.call("agent.hold", json!({"run_id": x, "reason": "after Y", "by": "owner", "release_on": {"kind": "agent_done", "id": y}}));
    d.call("run.interrupt", json!({"run_id": y}));
    d.wait_status(&y, |s| s == "interrupted", 10);
    let started = std::time::Instant::now();
    loop {
        if d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().all(|h| h["run_id"] != x) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5), "hold on X not released after Y finished");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(d.events(&x).iter().any(|e| e["kind"] == "release" && e["payload"]["why"].as_str().unwrap().contains("Y finished")));
    let at = crate::common::repo_root().display().to_string().len() as i64; // any small number of ms
    let _ = at;
    d.call("agent.hold", json!({"run_id": x, "reason": "for a moment", "by": "owner", "release_on": {"kind": "time", "at_ms": d.call("hello", json!({}))["pid"].as_i64().map(|_| 0).unwrap_or(0)}}));
    // at_ms 0 is in the past: released on the next tick.
    let started = std::time::Instant::now();
    loop {
        if d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().all(|h| h["run_id"] != x) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5), "timed hold not released");
        std::thread::sleep(Duration::from_millis(100));
    }
    // Hold everything: one proposal over every agent.
    let (p1, _) = sleeper(&d, &repo, "P1");
    let (p2, _) = sleeper(&d, &repo, "P2");
    sql(&d, "UPDATE overseer_sessions SET last_cause='owner';");
    let p = d.call("overseer.propose", json!({"actions": [{"action": "hold", "agent": p1, "reason": "everyone"}, {"action": "hold", "agent": p2, "reason": "everyone"}, {"action": "hold", "agent": x, "reason": "everyone"}], "source": "test"}));
    d.call("overseer.answer", json!({"id": p["proposal"], "yes": true, "surface": "ctl", "by": "owner"}));
    let holds = d.call("agent.holds", json!({}));
    assert_eq!(holds["holds"].as_array().unwrap().iter().filter(|h| h["reason"] == "everyone").count(), 3);
    // Guardrails: words repeated each later turn; a write inside a forbidden path is reported
    // within 2 s and holds the agent when the guardrail says so.
    let (w, ws) = sleeper(&d, &repo, "Writer");
    let g = d.call("agent.guardrail", json!({"run_id": w, "words": "Stay in docs.", "deny": ["api"], "hold_on_cross": true, "by": "owner"}));
    assert_eq!(g["enforcement"], "watched", "a generic harness cannot refuse writes itself");
    std::fs::write(ws.join("api/x.txt"), "changed by the writer\n").unwrap();
    let started = std::time::Instant::now();
    loop {
        let ev = d.events(&w);
        if ev.iter().any(|e| e["kind"] == "guardrail_crossed" && e["payload"]["paths"][0] == "api/x.txt") {
            break;
        }
        // Harnesses that report file activity are caught within 2 s of the event; a generic
        // program's edits are seen by the sweep.
        assert!(started.elapsed() < Duration::from_secs(12), "crossing not reported");
        std::thread::sleep(Duration::from_millis(100));
    }
    d.wait_status(&w, |s| s == "interrupted", 5);
    assert!(d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().any(|h| h["run_id"] == w && h["reason"].as_str().unwrap().contains("guardrail")));
    // A harness that reports its edits is caught from the event itself: the guardrail is set
    // while the agent is busy on a turn that writes nothing; its next turn writes in src/.
    std::fs::write(repo.join("README.md"), "# Demo\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "readme"]);
    let c = claude_task(&d, &repo, &mode_file, "slow", "Claude writer", "write sessions");
    d.wait_status(&c, |s| s == "running", 20);
    d.call("agent.guardrail", json!({"run_id": c, "words": "Only docs.", "deny": ["src"], "by": "owner"}));
    d.wait_done(&c, 30);
    std::fs::write(&mode_file, "showcase").unwrap();
    d.call("run.follow_up", json!({"run_id": c, "prompt": "now write the sessions code"}));
    d.wait_done(&c, 30);
    let ev = d.events(&c);
    let crossed = ev.iter().find(|e| e["kind"] == "guardrail_crossed").expect("the showcase run wrote in src/");
    let activity = ev.iter().find(|e| e["kind"] == "file_activity" && e["payload"]["paths"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().starts_with("src/"))).unwrap();
    assert!(crossed["ts"].as_i64().unwrap() - activity["ts"].as_i64().unwrap() <= 2000);
    assert_eq!(crossed["payload"]["enforcement"], "enforced", "Claude Code takes deny rules");
    // The words and the deny rules went with that turn.
    let rails = d.call("agent.guardrails", json!({"run_id": c}));
    assert_eq!(rails["guardrails"].as_array().unwrap().len(), 1);
    let t = d.call("run.turns", json!({"run_id": c}));
    let last = t.as_array().unwrap().last().unwrap();
    assert!(last["prompt"].as_str().unwrap().starts_with("[Guardrails from Overseer: Only docs. Do not change: src.]"), "{}", last["prompt"]);
    let (_, dir) = launch_info(&d, &c);
    let launch: Value = serde_json::from_slice(&std::fs::read(dir.join("launch.json")).unwrap()).unwrap();
    let args = launch["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect::<Vec<_>>().join(" ");
    assert!(args.contains("--disallowedTools Edit(src/**),Write(src/**),MultiEdit(src/**)"), "{args}");
    // A restart keeps the guardrail.
    d.kill9();
    d.spawn();
    assert_eq!(d.call("agent.guardrails", json!({"run_id": c}))["guardrails"].as_array().unwrap().len(), 1);
    d.call("agent.guardrail_remove", json!({"id": rails["guardrails"][0]["id"], "by": "owner"}));
    assert!(d.call("agent.guardrails", json!({"run_id": c}))["guardrails"].as_array().unwrap().is_empty());
}

/// AC-188: a redirect keeps a snapshot, stops the turn, sends the direction as the next turn and
/// shows delivered then answered; the review gains "since the change of direction"; a queued
/// message survives a restart and is delivered once; nothing uncommitted is lost.
#[test]
fn ac188_redirect_and_the_queue() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let mut d = overseer_daemon(&mode_file);
    // A Claude fixture busy for five seconds, then redirected mid-turn.
    std::fs::write(&mode_file, "slow").unwrap();
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "first direction", "title": "Slow"}));
    let slow = run_id(&created);
    let ws = ws_path(&d, &created);
    d.wait_status(&slow, |s| s == "running", 20);
    std::fs::write(ws.join("draft.txt"), "uncommitted work\n").unwrap();
    let redirected = d.call("agent.redirect", json!({"run_id": slow, "text": "Do the other thing instead", "source": "overseer"}));
    assert!(redirected["delivery"].as_str().unwrap().starts_with("stopping"));
    let snap_id = redirected["snapshot"].as_str().unwrap().to_string();
    d.wait_status(&slow, |s| s == "interrupted" || s == "running", 15);
    // The next turn carries the direction, from Overseer.
    let started = std::time::Instant::now();
    loop {
        let t = d.call("run.turns", json!({"run_id": slow}));
        if t.as_array().unwrap().len() == 2 {
            assert_eq!(t[1]["prompt"], "From Overseer: Do the other thing instead");
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(15), "no second turn: {t}");
        std::thread::sleep(Duration::from_millis(100));
    }
    d.wait_done(&slow, 30);
    assert_eq!(std::fs::read_to_string(ws.join("draft.txt")).unwrap(), "uncommitted work\n", "nothing uncommitted is lost");
    let ev = d.events(&slow);
    assert!(ev.iter().any(|e| e["kind"] == "redirect" && e["payload"]["snapshot"] == snap_id));
    let last = d.call("run.turns", json!({"run_id": slow}))[1].clone();
    assert_eq!(last["status"], "completed");
    // The review offers "since the change of direction" from that snapshot.
    let options = d.call("comparison.options", json!({"run_id": slow}));
    let since = options["options"].as_array().unwrap().iter().find(|o| o["mode"] == "redirect").expect("redirect comparison");
    assert_eq!(since["available"], true);
    assert_eq!(since["snapshot"]["id"], snap_id);
    // An edit after the redirect shows against it; the earlier draft does not.
    std::fs::write(ws.join("after.txt"), "after\n").unwrap();
    let paths: Vec<String> = diff_paths(&d, &created, since["base"].as_str().unwrap()).into_iter().map(|(_, p)| p).collect();
    assert!(paths.contains(&"after.txt".to_string()) && !paths.contains(&"draft.txt".to_string()), "{paths:?}");
    // The dispatch of a message: delivered when its turn starts, answered when it ends.
    std::fs::write(&mode_file, "echo").unwrap();
    let echo = claude_task(&d, &repo, &mode_file, "echo", "Echo", "hello");
    d.wait_done(&echo, 30);
    d.call("overseer.session", json!({}));
    sql(&d, "UPDATE overseer_sessions SET last_cause='owner';");
    let p = d.call("overseer.propose", json!({"actions": [{"action": "message", "agent": echo, "text": "second"}], "source": "test"}));
    d.call("overseer.answer", json!({"id": p["proposal"], "yes": true, "surface": "ctl", "by": "owner"}));
    d.wait_done(&echo, 30);
    std::thread::sleep(Duration::from_millis(500));
    let card = d.call("overseer.card", json!({"id": p["proposal"]}));
    assert_eq!(card["rows"][0]["state"], "answered", "{card}");
    assert!(card["rows"][0]["delivered_ms"].is_number() && card["rows"][0]["answered_ms"].is_number());
    // A queued message survives a restart and is delivered exactly once: a Claude fixture busy
    // for eight seconds keeps running under its supervisor while the daemon is down.
    let mut d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude_fixture()), ("CLAUDE_FIXTURE_MODE_FILE", &mode_file.display().to_string()), ("FIXTURE_SLOW_MS", "8000"), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS")]);
    std::fs::write(&mode_file, "slow").unwrap();
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "long job", "title": "Busy"}));
    let busy = run_id(&created);
    d.wait_status(&busy, |s| s == "running", 20);
    assert_eq!(d.call("run.queue", json!({"run_id": busy, "text": "after you finish", "source": "owner"}))["delivery"], "queued");
    d.kill9();
    d.spawn();
    assert_eq!(d.call("run.queued", json!({"run_id": busy}))["queued"].as_array().unwrap().len(), 1);
    d.wait_status(&busy, |s| s != "running", 30);
    let started = std::time::Instant::now();
    loop {
        let t = d.call("run.turns", json!({"run_id": busy}));
        if t.as_array().unwrap().len() >= 2 {
            assert_eq!(t.as_array().unwrap().len(), 2);
            assert_eq!(t[1]["prompt"], "after you finish");
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(15), "queued message not delivered: {t}");
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(d.call("run.turns", json!({"run_id": busy})).as_array().unwrap().len(), 2, "delivered once");
    assert!(d.call("run.queued", json!({"run_id": busy}))["queued"].as_array().unwrap().is_empty());
    // A redirect the harness cannot pick up: the direction is still delivered once.
    let (gen, _) = sleeper(&d, &repo, "Gen");
    d.call("agent.redirect", json!({"run_id": gen, "text": "turn around", "source": "overseer"}));
    d.wait_status(&gen, |s| s != "running", 15);
    let started = std::time::Instant::now();
    loop {
        if d.call("run.turns", json!({"run_id": gen})).as_array().unwrap().len() == 2 {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(d.call("run.turns", json!({"run_id": gen})).as_array().unwrap().len(), 2, "one delivery");
}

/// Turns Overseer's own run has taken, and the reasons it started them.
fn overseer_turn_causes(d: &Daemon) -> Vec<String> {
    let db = d.home.path().join("overseer.sqlite");
    let out = Command::new("sqlite3").arg(&db).arg("SELECT cause FROM overseer_turns ORDER BY ts;").output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

fn check_ins(d: &Daemon, run: &str) -> Vec<Value> {
    d.call("agent.check_ins", json!({"run_id": run}))["check_ins"].as_array().unwrap().clone()
}

fn wait_check_ins(d: &Daemon, run: &str, n: usize, secs: u64) -> Vec<Value> {
    let started = std::time::Instant::now();
    loop {
        let c = check_ins(d, run);
        if c.len() >= n {
            return c;
        }
        assert!(started.elapsed() < Duration::from_secs(secs), "{} check-ins on {run}, wanted {n}: {c:?}", c.len());
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// AC-189: an agent on task gets check-ins after turns 3 and 6 and when it finishes, and never a
/// message; the owner's direction changes the cadence; an agent that writes outside its area is
/// found by the free check and by the check-in that follows, which acts at the level; an agent
/// that finishes with part left out gets a done card naming it; the same failure three times
/// trips a check-in; check-ins off leaves the free checks running; several agents finishing
/// together are one Overseer turn; an agent that did nothing causes none.
#[test]
fn ac189_overseer_keeps_agents_on_task() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::create_dir_all(repo.join("docs")).unwrap();
    std::fs::write(repo.join("docs/notes.md"), "notes\n").unwrap();
    std::fs::write(repo.join("README.md"), "# Demo\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    d.call("overseer.session", json!({}));
    // An agent is finished when it stays idle for the grace period; one second here.
    sql(&d, "INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.grace_ms', '1000');");
    // Overseer's run must exist before check-ins, so the first turn is the owner's.
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);
    // On task through seven turns: check-ins after turns 3 and 6 and at the end; no message.
    let steady = claude_task(&d, &repo, &mode_file, "echo", "Steady", "keep the docs tidy");
    d.wait_done(&steady, 30);
    for i in 2..=7 {
        d.call("run.follow_up", json!({"run_id": steady, "prompt": format!("turn {i}")}));
        d.wait_done(&steady, 30);
        std::thread::sleep(Duration::from_millis(300));
    }
    // Every completion queues a "finished" check-in too; cadence turns 3 and 6 add theirs. The
    // batch window folds those due within 5 s, so wait for the queue to drain.
    std::thread::sleep(Duration::from_secs(7));
    wait_overseer_idle(&d, 60);
    let c = check_ins(&d, &steady);
    assert!(c.iter().all(|x| x["result"] == "on_task" || x["result"] == "done"), "{c:?}");
    let ev = d.events(&steady);
    let reasons: Vec<String> = ev.iter().filter(|e| e["kind"] == "check_in_started").flat_map(|e| e["payload"]["reasons"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect::<Vec<_>>()).collect();
    assert!(reasons.contains(&"turn 3".to_string()) && reasons.contains(&"turn 6".to_string()) && reasons.contains(&"finished".to_string()), "{reasons:?}");
    assert!(!reasons.iter().any(|r| r == "turn 2" || r == "turn 4" || r == "turn 5"), "{reasons:?}");
    assert_eq!(reasons.iter().filter(|r| *r == "finished").count(), 1, "finished once, after the last turn: {reasons:?}");
    let turns = d.call("run.turns", json!({"run_id": steady}));
    assert!(turns.as_array().unwrap().iter().all(|t| !t["prompt"].as_str().unwrap().starts_with("From Overseer")), "an agent on task hears nothing");
    // "Check on it every turn" and "leave it alone until it is done".
    let close = claude_task(&d, &repo, &mode_file, "echo", "Close", "small task");
    d.call("agent.cadence", json!({"run_id": close, "cadence": "every_turn", "by": "owner"}));
    d.wait_done(&close, 30);
    d.call("run.follow_up", json!({"run_id": close, "prompt": "turn 2"}));
    d.wait_done(&close, 30);
    std::thread::sleep(Duration::from_secs(6));
    wait_overseer_idle(&d, 60);
    let ev = d.events(&close);
    let started: Vec<&Value> = ev.iter().filter(|e| e["kind"] == "check_in_started").collect();
    assert!(started.iter().any(|e| e["payload"]["reasons"].as_array().unwrap().iter().any(|r| r == "turn 1")) && started.iter().any(|e| e["payload"]["reasons"].as_array().unwrap().iter().any(|r| r == "turn 2")), "{started:?}");
    let alone = claude_task(&d, &repo, &mode_file, "echo", "Alone", "long task");
    d.call("agent.cadence", json!({"run_id": alone, "cadence": "done", "by": "owner"}));
    d.wait_done(&alone, 30);
    for i in 2..=4 {
        d.call("run.follow_up", json!({"run_id": alone, "prompt": format!("turn {i}")}));
        d.wait_done(&alone, 30);
    }
    std::thread::sleep(Duration::from_secs(6));
    wait_overseer_idle(&d, 60);
    let ev = d.events(&alone);
    let reasons: Vec<String> = ev.iter().filter(|e| e["kind"] == "check_in_started").flat_map(|e| e["payload"]["reasons"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect::<Vec<_>>()).collect();
    assert!(reasons.iter().all(|r| r == "finished"), "only when done: {reasons:?}");
    // An agent that writes outside its area: the free check within 2 s of its own file event,
    // then the check-in that follows acts at the level: a proposal at Ask first.
    std::fs::write(&mode_file, "slow").unwrap();
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "tidy the docs", "title": "Drifter"}));
    let drifter = run_id(&created);
    d.wait_status(&drifter, |s| s == "running", 20);
    sql(&d, &format!("INSERT INTO areas(run_id, path, set_by, created_ms) VALUES('{drifter}', 'docs', 'owner', 1);"));
    d.wait_done(&drifter, 30);
    std::fs::write(&mode_file, "showcase").unwrap();
    d.call("run.follow_up", json!({"run_id": drifter, "prompt": "now do it"}));
    d.wait_done(&drifter, 30);
    let ev = d.events(&drifter);
    let outside = ev.iter().find(|e| e["kind"] == "outside_area").expect("free check");
    let activity = ev.iter().find(|e| e["kind"] == "file_activity" && e["payload"]["paths"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().starts_with("src/"))).unwrap();
    assert!(outside["ts"].as_i64().unwrap() - activity["ts"].as_i64().unwrap() <= 2000);
    std::thread::sleep(Duration::from_secs(6));
    let s = wait_overseer_idle(&d, 60);
    let c = check_ins(&d, &drifter);
    assert!(c.iter().any(|x| x["result"] == "drifting"), "{c:?}");
    let open = s["proposals"].as_array().unwrap();
    assert!(open.iter().any(|p| p["actions"][0]["action"] == "redirect" && p["actions"][0]["agent"] == drifter), "at Ask first a proposal: {open:?}");
    assert!(d.call("agent.holds", json!({}))["holds"].as_array().unwrap().is_empty());
    // At Steer the same check-in holds; at Auto it redirects.
    for p in open {
        d.call("overseer.answer", json!({"id": p["id"], "yes": false, "surface": "ctl", "by": "owner"}));
    }
    d.call("overseer.level", json!({"level": "steer"}));
    sql(&d, "DELETE FROM free_checks;");
    d.call("run.follow_up", json!({"run_id": drifter, "prompt": "and again"}));
    d.wait_done(&drifter, 30);
    std::thread::sleep(Duration::from_secs(6));
    wait_overseer_idle(&d, 60);
    assert!(d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().any(|h| h["run_id"] == drifter), "held at Steer");
    d.call("agent.release", json!({"run_id": drifter, "by": "owner"}));
    d.call("overseer.level", json!({"level": "auto"}));
    sql(&d, "DELETE FROM free_checks;");
    d.call("run.follow_up", json!({"run_id": drifter, "prompt": "once more"}));
    d.wait_done(&drifter, 30);
    std::thread::sleep(Duration::from_secs(6));
    wait_overseer_idle(&d, 60);
    let ev = d.events(&drifter);
    assert!(ev.iter().any(|e| e["kind"] == "redirect" && e["payload"]["detail"]["by"].as_str().unwrap_or("").contains("auto")), "redirected at Auto");
    d.call("overseer.level", json!({"level": "ask_first"}));
    // Finished with part left out: a done card that names it.
    std::fs::write(&mode_file, "echo").unwrap();
    let partial = claude_task(&d, &repo, &mode_file, "echo", "Partial", "write the API and the docs [leave out: the docs]");
    d.wait_done(&partial, 30);
    std::thread::sleep(Duration::from_secs(6));
    let s = wait_overseer_idle(&d, 60);
    let done = s["messages"].as_array().unwrap().iter().find(|m| m["source"] == "card" && m["card"]["kind"] == "done" && m["card"]["agent"] == partial).expect("done card");
    assert_eq!(done["card"]["left_out"], "the docs");
    // The same failure three times trips a check-in.
    let circles = claude_task(&d, &repo, &mode_file, "circles", "Loops", "fix the tests");
    d.wait_done(&circles, 30);
    let ev = d.events(&circles);
    assert!(ev.iter().any(|e| e["kind"] == "going_in_circles" && e["payload"]["times"] == 3), "{:?}", ev.iter().map(|e| e["kind"].clone()).collect::<Vec<_>>());
    std::thread::sleep(Duration::from_secs(6));
    wait_overseer_idle(&d, 60);
    let reasons: Vec<String> = d.events(&circles).iter().filter(|e| e["kind"] == "check_in_started").flat_map(|e| e["payload"]["reasons"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect::<Vec<_>>()).collect();
    assert!(reasons.iter().any(|r| r.contains("three times")), "{reasons:?}");
    // Check-ins off: none runs; the free checks still do.
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    let quiet = claude_task(&d, &repo, &mode_file, "circles", "Quiet", "fix the tests");
    d.wait_done(&quiet, 30);
    std::thread::sleep(Duration::from_secs(6));
    wait_overseer_idle(&d, 60);
    assert!(d.events(&quiet).iter().any(|e| e["kind"] == "going_in_circles"));
    assert!(check_ins(&d, &quiet).is_empty() && !d.events(&quiet).iter().any(|e| e["kind"] == "check_in_started"));
    d.call("agent.cadence", json!({"cadence": "every:3", "by": "owner"}));
    // Four agents finishing together: one Overseer turn; an agent that did nothing: none.
    let before = overseer_turn_causes(&d).len();
    let mut four = Vec::new();
    for i in 0..4 {
        four.push(claude_task(&d, &repo, &mode_file, "echo", &format!("Four {i}"), "hello"));
    }
    for id in &four {
        d.wait_done(id, 30);
    }
    std::thread::sleep(Duration::from_secs(7));
    wait_overseer_idle(&d, 60);
    let causes = overseer_turn_causes(&d);
    assert_eq!(causes.len(), before + 1, "one turn for four agents: {:?}", &causes[before..]);
    assert_eq!(causes.last().unwrap(), "check_in");
    for id in &four {
        assert!(!check_ins(&d, id).is_empty(), "each of the four was checked");
    }
    let idle = d.generic(&repo, "worktree", "/bin/sh", &["-c", "sleep 120"]);
    let idle = run_id(&idle);
    d.wait_status(&idle, |s| s == "running", 20);
    std::thread::sleep(Duration::from_secs(7));
    assert!(check_ins(&d, &idle).is_empty(), "no turn, no check-in");
    assert_eq!(overseer_turn_causes(&d).len(), before + 1);
    // A question after agents finished meanwhile (however long ago) is answered from their
    // current digests: the envelope is built when the owner asks, not from a check-in.
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    let s = wait_overseer_idle(&d, 30);
    let turns = d.call("run.turns", json!({"run_id": s["run_id"]}));
    let prompt = turns.as_array().unwrap().last().unwrap()["prompt"].as_str().unwrap().to_string();
    let agents: Value = serde_json::from_str(prompt.split("Agents (JSON):\n").nth(1).unwrap().split("\n</overseer-state>").next().unwrap()).unwrap();
    assert!(agents.as_array().unwrap().iter().any(|a| a["id"] == four[0] && a["status"] == "completed"), "{agents}");
    // The cap: a turn that answers the owner is never counted; self-started turns are.
    let cap = d.call("overseer.cap", json!({}));
    assert!(cap["self_started_today"].as_i64().unwrap() >= 5 && cap["cap"] == 100, "{cap}");
    assert_eq!(overseer_turn_causes(&d).iter().filter(|c| *c == "owner").count() as i64 + cap["self_started_today"].as_i64().unwrap(), overseer_turn_causes(&d).len() as i64);
}

/// Turns of a run, oldest first.
fn turns(d: &Daemon, run: &str) -> Vec<Value> {
    d.call("run.turns", json!({"run_id": run})).as_array().cloned().unwrap_or_default()
}

/// What Overseer's own run said and hit, for a failing assertion.
fn overseer_trace(d: &Daemon) -> String {
    let s = session(d);
    let run = s["run_id"].as_str().unwrap_or("").to_string();
    let lines: Vec<String> = d.events(&run).iter().filter(|e| ["output", "error", "tool", "tool_result", "turn_started", "overseer_tool_call"].contains(&e["kind"].as_str().unwrap_or(""))).map(|e| format!("{} {}", e["kind"], serde_json::to_string(&e["payload"]).unwrap_or_default().chars().take(700).collect::<String>())).collect();
    format!("causes {:?}\n{}", overseer_turn_causes(d), lines.join("\n"))
}

/// Wait until a run has a turn whose prompt contains the text.
fn wait_turn_with(d: &Daemon, run: &str, text: &str, secs: u64) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(t) = turns(d, run).into_iter().find(|t| t["prompt"].as_str().unwrap_or("").contains(text)) {
            return t;
        }
        assert!(std::time::Instant::now() < deadline, "{run} never got a turn with {text:?}: {:?}", turns(d, run).iter().map(|t| t["prompt"].clone()).collect::<Vec<_>>());
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_event(d: &Daemon, run: &str, pred: impl Fn(&Value) -> bool, secs: u64) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(e) = d.events(run).into_iter().find(|e| pred(e)) {
            return e;
        }
        assert!(std::time::Instant::now() < deadline, "{run}: no such event; kinds {:?}; findings {:?}; watcher prompts {:?}", d.events(run).iter().map(|e| e["kind"].clone()).collect::<Vec<_>>(), d.events(run).iter().filter(|e| e["kind"] == "finding").map(|e| e["payload"]["text"].clone()).collect::<Vec<_>>(), d.events(run).iter().find(|e| e["kind"] == "watch_wake").and_then(|e| e["payload"]["watcher"].as_str().map(|w| turns(d, w).iter().map(|t| t["prompt"].as_str().unwrap_or("").chars().take(1500).collect::<String>()).collect::<Vec<_>>())));
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// AC-190: a lone agent works as before; a second agent in the repository gives both a briefing
/// (the first as a queued message) and a channel; report, ask and claim reach the digest and the
/// conversation with the sender the token names; a repeated report is stored once; a question is
/// answered by Overseer's next turn and the answer reaches the agent.
#[test]
fn ac190_briefing_and_channel() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    d.call("overseer.session", json!({}));
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.level", json!({"level": "steer"}));
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);
    // A lone agent: no briefing, no channel, its task exactly as typed.
    let lone = claude_task(&d, &repo, &mode_file, "echo", "Lone", "tidy the docs");
    d.wait_done(&lone, 30);
    assert_eq!(turns(&d, &lone)[0]["prompt"], "tidy the docs");
    let echo = d.events(&lone).iter().find(|e| e["kind"] == "output" && e["payload"]["text"].as_str().unwrap_or("").starts_with("ECHO")).expect("echo")["payload"]["text"].as_str().unwrap().to_string();
    assert!(!echo.contains("--mcp-config"), "a lone agent has no channel: {echo}");
    assert!(!d.events(&lone).iter().any(|e| e["kind"] == "briefing"));
    assert_eq!(d.call("agent.channel", json!({"run_id": lone}))["channel"], false);
    // A second agent while the first works: both get a briefing, the first as a queued message.
    let first = claude_task(&d, &repo, &mode_file, "slow", "Login API", "build the login API");
    d.wait_status(&first, |s| s == "running", 20);
    sql(&d, &format!("INSERT INTO areas(run_id, path, set_by, created_ms) VALUES('{first}', 'api', 'owner', 1);"));
    let second = claude_task(&d, &repo, &mode_file, "channel", "Login page", "claim: web/login; report: building the login page; ask: what does the login endpoint return?; write: web/login/page.ts");
    d.wait_done(&second, 40);
    let prompt = turns(&d, &second)[0]["prompt"].as_str().unwrap().to_string();
    assert!(prompt.starts_with("[Briefing from Overseer:") && prompt.contains("Login API") && prompt.contains("in api") && prompt.ends_with("write: web/login/page.ts"), "second {second}: {:?}\nfirst {first}: {:?}", turns(&d, &second), turns(&d, &first));
    let briefing = d.events(&second).iter().find(|e| e["kind"] == "briefing").expect("briefing event").clone();
    assert_eq!(briefing["payload"]["how"], "task");
    assert!(briefing["payload"]["text"].as_str().unwrap().len() <= 1024);
    assert_eq!(d.call("agent.channel", json!({"run_id": second}))["channel"], true);
    // The channel: report, ask and claim, each with the right sender.
    let ev = d.events(&second);
    assert_eq!(ev.iter().find(|e| e["kind"] == "report").expect("report")["payload"]["doing"], "building the login page");
    assert_eq!(ev.iter().find(|e| e["kind"] == "ask").expect("ask")["payload"]["question"], "what does the login endpoint return?");
    assert_eq!(ev.iter().find(|e| e["kind"] == "claim").expect("claim")["payload"]["paths"], json!(["web/login"]));
    let digest = d.call("agent.digest", json!({"run_id": second}))["digest"].clone();
    assert_eq!(digest["area"], json!(["web/login"]));
    assert_eq!(digest["last_report"]["doing"], "building the login page");
    assert_eq!(digest["asks"][0]["question"], "what does the login endpoint return?");
    let s = session(&d);
    let cards: Vec<&Value> = s["messages"].as_array().unwrap().iter().filter(|m| m["card"]["agent"] == second).collect();
    for kind in ["report", "ask", "claim"] {
        assert!(cards.iter().any(|m| m["card"]["kind"] == kind && m["source"] == "agent"), "{kind} card from the agent: {cards:?}");
    }
    // The first agent's briefing waited for its turn to end; it names the second and its area.
    d.wait_done(&first, 30);
    let queued = wait_turn_with(&d, &first, "[Briefing from Overseer:", 20);
    let text = queued["prompt"].as_str().unwrap();
    assert!(text.contains("Login page") && text.contains("web/login"), "{text}");
    assert!(d.events(&first).iter().any(|e| e["kind"] == "briefing" && e["payload"]["how"] == "queued"));
    // The question: answered by Overseer's next turn (at Steer, at once), and the answer reaches the agent.
    std::thread::sleep(Duration::from_secs(6));
    wait_overseer_idle(&d, 60);
    let asked = d.call("channel.messages", json!({"run_id": second}))["messages"].as_array().unwrap().iter().find(|m| m["kind"] == "ask").cloned().expect("ask row");
    assert!(asked["answer"].as_str().map(|a| a.contains("From the roster")).unwrap_or(false), "{asked}\n{}", overseer_trace(&d));
    wait_turn_with(&d, &second, "Answer to your question “what does the login endpoint return?”", 20);
    assert!(session(&d)["messages"].as_array().unwrap().iter().any(|m| m["card"]["kind"] == "answer" && m["card"]["agent"] == second));
    assert!(overseer_turn_causes(&d).contains(&"ask".to_string()), "{:?}", overseer_turn_causes(&d));
    // A report sent three times is stored once (the channel on by the owner's default: the
    // other agents have finished, so the repository would count as one agent's).
    d.call("agent.channel", json!({"default": "on", "by": "owner"}));
    let thrice = claude_task(&d, &repo, &mode_file, "channel", "Repeater", "report x3: still going");
    d.wait_done(&thrice, 40);
    assert_eq!(d.events(&thrice).iter().filter(|e| e["kind"] == "report").count(), 1);
    assert_eq!(d.call("channel.messages", json!({"run_id": thrice}))["messages"].as_array().unwrap().len(), 1);
    // A token from one run cannot report as another: the sender is the token's run, whatever the text says.
    let token = d.call("overseer.token", json!({"run_id": thrice, "role": "agent"}))["token"].as_str().unwrap().to_string();
    let r = d.call("overseer.tool", json!({"token": token, "name": "report", "arguments": {"agent": second, "run_id": second, "doing": "spoof"}}));
    assert_eq!(r["is_error"], false, "{r}");
    assert!(d.events(&thrice).iter().any(|e| e["kind"] == "report" && e["payload"]["doing"] == "spoof"));
    assert!(!d.events(&second).iter().any(|e| e["kind"] == "report" && e["payload"]["doing"] == "spoof"));
    let r = d.try_call("overseer.tool", json!({"token": token, "name": "agent", "arguments": {"id": second}}));
    assert!(r.is_err() && r.unwrap_err().contains("no tool agent"), "an agent's token reads no digest");
    // The owner's setting: off for every agent, then on for one.
    d.call("agent.channel", json!({"default": "off", "by": "owner"}));
    let quiet = claude_task(&d, &repo, &mode_file, "echo", "Quiet", "hello");
    d.wait_done(&quiet, 30);
    assert_eq!(turns(&d, &quiet)[0]["prompt"], "hello");
    assert!(!d.events(&quiet).iter().any(|e| e["kind"] == "briefing"));
    d.call("agent.channel", json!({"run_id": quiet, "briefing": true, "channel": true, "by": "owner"}));
    let c = d.call("agent.channel", json!({"run_id": quiet}));
    assert!(c["briefing"] == true && c["channel"] == true, "{c}");
    d.call("agent.channel", json!({"default": "auto", "by": "owner"}));
}

/// AC-190, Rally: the map comes from the digests; only the agents whose digests cannot answer
/// (no area, no report) are asked for a report, with the cost said first; their reports come
/// back through the channel and Overseer proposes the areas; one yes records them.
#[test]
fn ac190_rally_asks_only_where_the_digests_cannot_answer() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    d.call("overseer.session", json!({}));
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("agent.channel", json!({"default": "on", "by": "owner"}));
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);
    // Four agents in different roles: two claimed areas, two only wrote files.
    let api = claude_task(&d, &repo, &mode_file, "channel", "API", "claim: api; write: api/login.ts");
    d.wait_done(&api, 40);
    let web = claude_task(&d, &repo, &mode_file, "channel", "Web", "claim: web; write: web/page.ts");
    d.wait_done(&web, 40);
    let docs = claude_task(&d, &repo, &mode_file, "channel", "Docs", "write: docs/guide.md");
    d.wait_done(&docs, 40);
    let tests = claude_task(&d, &repo, &mode_file, "channel", "Tests", "write: tests/login.test.ts");
    d.wait_done(&tests, 40);
    // The daemon's map, with no model.
    let map = d.call("overseer.rally", json!({"repo": repo}));
    assert_eq!(map["agents"].as_array().unwrap().len(), 4, "{map}");
    let ask: std::collections::BTreeSet<String> = map["ask"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
    assert_eq!(ask, [docs.clone(), tests.clone()].into_iter().collect(), "{map}");
    assert_eq!(map["cost"], "2 agent turns");
    let docs_line = map["agents"].as_array().unwrap().iter().find(|a| a["id"] == docs).unwrap();
    assert_eq!(docs_line["suggested_area"], json!(["docs"]));
    // From the conversation: the report requests are one proposal that says the cost.
    d.call("overseer.send", json!({"text": format!("Rally my agents in {}", repo.display()), "surface": "ctl", "harness": "claude"}));
    let s = wait_overseer_idle(&d, 30);
    let open = s["proposals"].as_array().unwrap().clone();
    assert_eq!(open.len(), 1, "{open:?}\n{}", overseer_trace(&d));
    let actions = open[0]["actions"].as_array().unwrap();
    assert!(actions.iter().all(|a| a["action"] == "report"), "{actions:?}");
    let asked: std::collections::BTreeSet<String> = actions.iter().map(|a| a["agent"].as_str().unwrap().to_string()).collect();
    assert_eq!(asked, ask);
    let said = s["messages"].as_array().unwrap().iter().rev().find(|m| m["source"] == "overseer").unwrap()["text"].as_str().unwrap().to_string();
    assert!(said.contains("2 agent turns"), "the cost before it is spent: {said}");
    // One yes: the two are asked, their reports come back through the channel, and Overseer's
    // next turn (the reports' own) proposes the areas.
    d.call("overseer.answer", json!({"id": open[0]["id"], "yes": true, "surface": "ctl", "by": "owner"}));
    for id in [&docs, &tests] {
        wait_event(&d, id, |e| e["kind"] == "report" && e["payload"]["doing"].as_str().unwrap_or("").starts_with("working in"), 30);
    }
    for id in [&api, &web] {
        assert!(!turns(&d, id).iter().any(|t| t["prompt"].as_str().unwrap().contains("Report, with your report tool")), "not asked: its digest answers");
    }
    let card = d.call("overseer.card", json!({"id": open[0]["id"]}));
    // Through the channel a request reads picked up (the tool call) and then answered (the report).
    assert!(card["rows"].as_array().unwrap().iter().all(|x| x["state"] == "answered" && x["picked_ms"].is_number() && x["answered_ms"].is_number()), "the reports came back: {card}");
    std::thread::sleep(Duration::from_secs(6));
    let s = wait_overseer_idle(&d, 60);
    let open = s["proposals"].as_array().unwrap().clone();
    assert_eq!(open.len(), 1, "{open:?}");
    let actions = open[0]["actions"].as_array().unwrap();
    assert!(actions.iter().all(|a| a["action"] == "area"), "{actions:?}");
    assert!(actions.iter().any(|a| a["agent"] == docs && a["paths"] == json!(["docs"])) && actions.iter().any(|a| a["agent"] == tests && a["paths"] == json!(["tests"])), "{actions:?}");
    let said = s["messages"].as_array().unwrap().iter().rev().find(|m| m["source"] == "overseer").unwrap()["text"].as_str().unwrap().to_string();
    assert!(said.starts_with("Map:") && said.contains("API owns api") && said.contains("Web owns web"), "{said}");
    assert!(overseer_turn_causes(&d).contains(&"report".to_string()), "{:?}", overseer_turn_causes(&d));
    d.call("overseer.answer", json!({"id": open[0]["id"], "yes": true, "surface": "ctl", "by": "owner"}));
    assert_eq!(d.call("agent.digest", json!({"run_id": docs}))["digest"]["area"], json!(["docs"]));
    assert_eq!(d.call("agent.digest", json!({"run_id": tests}))["digest"]["area"], json!(["tests"]));
    assert_eq!(d.call("agent.digest", json!({"run_id": api}))["digest"]["area"], json!(["api"]));
    let map = d.call("overseer.rally", json!({"repo": repo}));
    assert!(map["ask"].as_array().unwrap().is_empty(), "every digest answers now: {map}");
}

/// AC-191: a share is a message from Overseer that names its source; large pieces go as a file;
/// across repositories it waits for a yes at every level; a denied destination gets nothing;
/// redaction applies; a withdrawn finding reaches everyone who received it.
#[test]
fn ac191_context_passed_between_agents() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo2 = repo(&r.path().join("repo2"));
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    d.call("overseer.session", json!({}));
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.level", json!({"level": "steer"}));
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);
    let alpha = claude_task(&d, &repo, &mode_file, "channel", "Alpha", "write: src/a.ts");
    d.wait_done(&alpha, 40);
    let beta = claude_task(&d, &repo, &mode_file, "channel", "Beta", "hello");
    d.wait_done(&beta, 40);
    // Alpha's diff reaches Beta with its source named, and Beta's reply refers to it.
    let p = d.call("overseer.propose", json!({"actions": [{"action": "share", "to": beta, "from": alpha, "what": "diff", "path": "src/a.ts"}], "source": "ctl"}));
    assert_eq!(p["state"], "settling", "the owner asked at Steer: {p}");
    let t = wait_turn_with(&d, &beta, "Shared by Overseer from Alpha (diff of src/a.ts)", 15);
    let text = t["prompt"].as_str().unwrap();
    assert!(text.starts_with("From Overseer: Shared by Overseer from Alpha") && text.contains("+// written by the fixture"), "{text}");
    wait_event(&d, &beta, |e| e["kind"] == "output" && e["payload"]["text"] == "Read the share from Alpha; using it.", 30);
    d.wait_done(&beta, 30);
    let shares = d.call("share.list", json!({"run_id": beta}))["shares"].as_array().unwrap().clone();
    assert_eq!(shares.len(), 1);
    assert!(shares[0]["kind"] == "diff" && shares[0]["file"].is_null() && shares[0]["from"] == alpha, "{shares:?}");
    // A 100 KiB diff arrives as a patch file in Beta's run folder, the inline part within 8 KiB.
    let big = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "yes 'a line of text for the patch' | head -c 102400 > big.txt"]));
    d.wait_done(&big, 30);
    d.call("overseer.propose", json!({"actions": [{"action": "share", "to": beta, "from": big, "what": "diff"}], "source": "ctl"}));
    let t = wait_turn_with(&d, &beta, "the whole piece at", 15);
    let text = t["prompt"].as_str().unwrap();
    assert!(text.len() <= 8 * 1024 + 512, "inline part within the bound: {} bytes", text.len());
    let share = d.call("share.list", json!({"run_id": beta}))["shares"].as_array().unwrap().iter().find(|s| s["from"] == big).cloned().unwrap();
    let file = share["file"].as_str().expect("a patch file").to_string();
    assert!(text.contains(&file) && file.contains(&format!("/{beta}/shares/")), "{file}");
    assert!(std::fs::metadata(&file).unwrap().len() >= 102400 && share["bytes"].as_i64().unwrap() >= 102400 && share["inline_bytes"].as_i64().unwrap() <= 8 * 1024);
    d.wait_done(&beta, 30);
    // Across repositories: a Confirm action, waiting for a yes at Steer and at Auto.
    let gamma = claude_task(&d, &repo2, &mode_file, "echo", "Gamma", "hello");
    d.wait_done(&gamma, 30);
    for level in ["steer", "auto"] {
        d.call("overseer.level", json!({"level": level}));
        let p = d.call("overseer.propose", json!({"actions": [{"action": "share", "to": gamma, "from": alpha, "what": "diff", "path": "src/a.ts"}], "source": "ctl"}));
        assert!(p["state"] == "open" && p["done"] != true, "at {level} a share across repositories waits for a yes: {p}");
        std::thread::sleep(Duration::from_secs(3));
        let open = session(&d)["proposals"].as_array().unwrap().clone();
        assert!(open.iter().any(|o| o["id"] == p["proposal"]), "still waiting at {level}");
        assert!(!turns(&d, &gamma).iter().any(|t| t["prompt"].as_str().unwrap().contains("Shared by Overseer")));
        d.call("overseer.answer", json!({"id": p["proposal"], "yes": false, "surface": "ctl", "by": "owner"}));
    }
    d.call("overseer.level", json!({"level": "steer"}));
    // A denied destination receives nothing.
    d.call("agent.share_deny", json!({"run_id": gamma, "denied": true, "by": "owner"}));
    let refused = d.try_call("overseer.propose", json!({"actions": [{"action": "share", "to": gamma, "what": "note", "text": "hello"}], "source": "ctl"}));
    assert!(refused.is_err() && refused.unwrap_err().contains("denied"), "a denied destination is never used");
    d.call("agent.share_deny", json!({"run_id": gamma, "denied": false, "by": "owner"}));
    // A credential-shaped string is redacted.
    d.call("overseer.propose", json!({"actions": [{"action": "share", "to": beta, "what": "note", "text": "use the key sk-ant-api03-abcdefghijklmnopqrstuvwxyz for staging"}], "source": "ctl"}));
    let t = wait_turn_with(&d, &beta, "(Overseer's note)", 15);
    let text = t["prompt"].as_str().unwrap();
    assert!(text.contains("[redacted]") && !text.contains("sk-ant-api03"), "{text}");
    d.wait_done(&beta, 30);
    // A withdrawn finding reaches both earlier recipients.
    let eps = claude_task(&d, &repo, &mode_file, "channel", "Epsilon", "hello");
    d.wait_done(&eps, 40);
    let p = d.call("overseer.propose", json!({"actions": [
        {"action": "share", "to": beta, "what": "finding", "text": "the login endpoint returns 500 on an empty body"},
        {"action": "share", "to": eps, "what": "finding", "text": "the login endpoint returns 500 on an empty body"}], "source": "ctl"}));
    for id in [&beta, &eps] {
        wait_turn_with(&d, id, "(Overseer's finding)", 15);
        d.wait_done(id, 30);
    }
    let finding = d.call("share.list", json!({"run_id": beta}))["shares"].as_array().unwrap().iter().find(|s| s["kind"] == "finding" && s["proposal"] == p["proposal"]).cloned().unwrap();
    let w = d.call("share.withdraw", json!({"id": finding["id"], "by": "owner"}));
    assert_eq!(w["told"].as_array().unwrap().len(), 2, "{w}");
    for id in [&beta, &eps] {
        wait_turn_with(&d, id, "Withdrawn: what Overseer shared (Overseer's finding)", 15);
    }
    assert!(d.call("share.list", json!({}))["shares"].as_array().unwrap().iter().filter(|s| s["kind"] == "finding").all(|s| !s["withdrawn_ms"].is_null()));
}

fn wait_count(d: &Daemon, run: &str, kind: &str, n: usize, secs: u64) -> Vec<Value> {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let found: Vec<Value> = d.events(run).into_iter().filter(|e| e["kind"] == kind).collect();
        if found.len() >= n {
            return found;
        }
        if std::time::Instant::now() >= deadline {
            let watcher = d.events(run).iter().find(|e| e["kind"] == "watch_wake").and_then(|e| e["payload"]["watcher"].as_str().map(str::to_string));
            let detail = watcher.map(|w| format!("watcher {w} status {:?}; turns {:?}; queued {}; events {:?}", d.call("agent.digest", json!({"run_id": w}))["digest"]["status"], turns(d, &w).iter().map(|t| (t["status"].clone(), t["prompt"].as_str().unwrap_or("").chars().take(120).collect::<String>())).collect::<Vec<_>>(), d.call("run.queued", json!({"run_id": w})), d.events(&w).iter().map(|e| format!("{} {}", e["kind"], e["payload"]["text"].as_str().unwrap_or("").chars().take(100).collect::<String>())).collect::<Vec<_>>())).unwrap_or_default();
            panic!("{run}: {} of {n} {kind} events; {detail}", found.len());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Every file's bytes under a worktree, for a before-and-after comparison.
fn tree_bytes(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.file_name().map(|n| n == ".git").unwrap_or(false) {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(path.strip_prefix(root).unwrap().display().to_string(), std::fs::read(&path).unwrap());
            }
        }
    }
    walk(root, root, &mut out);
    out
}

/// AC-193: a watcher wakes when its subject's turn ends and once when it finishes, each wake
/// carrying only what is new; an idle subject causes no wake; a stop finding with hold on stop
/// holds the subject within 2 s with no model turn in between, and without it Overseer acts at
/// its level; a watcher only reads; no watcher of a watcher, no circle, two per subject, the cap.
#[test]
fn ac193_one_agent_watches_another() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    d.call("overseer.session", json!({}));
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);
    // A subject is finished when it stays idle for the grace period; one second here.
    sql(&d, "INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.grace_ms', '1000');");
    // A subject with three turns, watched from its first: three wakes and one at the end.
    std::fs::write(&mode_file, "slow").unwrap();
    let subject = claude_task(&d, &repo, &mode_file, "slow", "Subject", "turn 1");
    d.wait_status(&subject, |s| s == "running", 20);
    let w = d.call("watch.start", json!({"subject": subject, "brief": "watch for deleted tests", "harness": "claude", "by": "owner"}));
    let watch = w["id"].as_str().unwrap().to_string();
    assert!(w["watcher"].is_null(), "the watcher comes with the first wake");
    std::fs::write(&mode_file, "watcher").unwrap();
    d.wait_done(&subject, 30);
    let wake1 = wait_count(&d, &subject, "watch_wake", 1, 20);
    let watcher = wake1[0]["payload"]["watcher"].as_str().unwrap().to_string();
    assert_eq!(d.call("agent.digest", json!({"run_id": watcher}))["digest"]["role"], "watcher");
    // Overseer's roster lists the watcher as such; the subject's digest names its watcher.
    let digest = d.call("agent.digest", json!({"run_id": subject}))["digest"].clone();
    assert_eq!(digest["watches"][0]["watcher"], watcher);
    for i in 2..=3 {
        // The watcher has filed on the last wake before the next turn, so no two wakes fold
        // into one of its turns (they would when it is busy).
        wait_count(&d, &subject, "finding", i - 1, 30);
        std::fs::write(&mode_file, "slow").unwrap();
        d.call("run.follow_up", json!({"run_id": subject, "prompt": format!("turn {i}")}));
        d.wait_status(&subject, |s| s == "running", 20);
        std::fs::write(&mode_file, "watcher").unwrap();
        d.wait_done(&subject, 30);
        wait_count(&d, &subject, "watch_wake", i, 20);
    }
    let wakes = wait_count(&d, &subject, "watch_wake", 4, 30);
    let reasons: Vec<&str> = wakes.iter().map(|e| e["payload"]["reason"].as_str().unwrap()).collect();
    assert_eq!(reasons.iter().filter(|r| r.contains("turn ended")).count(), 3, "{reasons:?}");
    assert_eq!(reasons.iter().filter(|r| *r == &"finished").count(), 1, "{reasons:?}");
    // Each wake carries only what is new: the end of turn 1 (the watch started mid-turn) is in
    // the first wake and no later one; turn 2 is in the second, without a word of turn 3.
    wait_count(&d, &subject, "finding", 4, 40);
    d.wait_done(&watcher, 30);
    let turns_w = turns(&d, &watcher);
    assert!(turns_w.len() >= 4, "{}", turns_w.len());
    let carrying: Vec<bool> = turns_w.iter().map(|t| t["prompt"].as_str().unwrap().contains("finished: turn 1")).collect();
    assert_eq!(carrying[0], true, "{}", turns_w[0]["prompt"]);
    assert!(carrying[1..].iter().all(|c| !c), "{carrying:?}");
    assert!(turns_w[1]["prompt"].as_str().unwrap().contains("working on: turn 2") && !turns_w[1]["prompt"].as_str().unwrap().contains("turn 3"));
    assert!(turns_w.iter().all(|t| t["prompt"].as_str().unwrap().len() <= 32 * 1024));
    let findings = d.call("watch.findings", json!({"watch": watch}))["findings"].as_array().unwrap().clone();
    assert!(findings.len() >= 4 && findings.iter().all(|f| f["result"] == "fine"), "{findings:?}");
    assert!(!session(&d)["messages"].as_array().unwrap().iter().any(|m| m["card"]["kind"] == "finding"), "fine is silent");
    // The watch ended with its subject and says so; the watcher never got a way to act.
    let ended = d.call("watch.list", json!({"run_id": subject}))["watches"].as_array().unwrap()[0].clone();
    assert_eq!(ended["end_reason"], "the subject finished");
    let watcher_tools: Vec<String> = d.call("overseer.tools", json!({"token": sql_token(&d, &watcher)}))["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert!(!watcher_tools.contains(&"propose".to_string()) && watcher_tools.contains(&"finding".to_string()), "{watcher_tools:?}");
    // An idle subject: no wake in a while (the clause says ten minutes; the wake needs an event).
    let idle = claude_task(&d, &repo, &mode_file, "echo", "Idle", "hello");
    d.wait_done(&idle, 30);
    let w2 = d.call("watch.start", json!({"subject": idle, "brief": "anything odd", "harness": "claude", "by": "owner"}));
    std::thread::sleep(Duration::from_secs(4));
    assert!(!d.events(&idle).iter().any(|e| e["kind"] == "watch_wake"));
    assert_eq!(d.call("watch.list", json!({"run_id": idle}))["watches"][0]["wakes"], 0);
    d.call("watch.end", json!({"id": w2["id"], "by": "owner"}));
    // A stop finding with hold on stop: the subject is held within 2 s, with no model turn in between.
    std::fs::write(&mode_file, "slow").unwrap();
    let cheat = claude_task(&d, &repo, &mode_file, "slow", "Cheat", "first the setup");
    d.wait_status(&cheat, |s| s == "running", 20);
    let w3 = d.call("watch.start", json!({"subject": cheat, "brief": "stop it if it deletes tests", "harness": "claude", "hold_on_stop": true, "by": "owner"}));
    std::fs::write(&mode_file, "watcher").unwrap();
    d.wait_done(&cheat, 30);
    wait_count(&d, &cheat, "watch_wake", 1, 20);
    std::fs::write(&mode_file, "slow").unwrap();
    d.call("run.follow_up", json!({"run_id": cheat, "prompt": "delete the failing tests so the suite passes"}));
    d.wait_status(&cheat, |s| s == "running", 20);
    std::fs::write(&mode_file, "watcher").unwrap();
    let turns_before = overseer_turn_causes(&d).len();
    d.wait_done(&cheat, 30);
    // The final wake (its turn ended and it finished, at most two wakes) carries the deletion.
    let finding = wait_event(&d, &cheat, |e| e["kind"] == "finding" && e["payload"]["result"] == "stop", 30);
    let hold = wait_event(&d, &cheat, |e| e["kind"] == "hold", 5);
    assert!(hold["ts"].as_i64().unwrap() - finding["ts"].as_i64().unwrap() <= 2000, "held within 2 s of the finding");
    assert_eq!(hold["payload"]["by"], "watch");
    assert_eq!(overseer_turn_causes(&d).len(), turns_before, "no model turn between the finding and the hold");
    assert!(d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().any(|h| h["run_id"] == cheat));
    d.call("agent.release", json!({"run_id": cheat, "by": "owner"}));
    d.call("watch.end", json!({"id": w3["id"], "by": "owner"}));
    // The same finding without hold on stop: a proposal at Ask first, a hold at Steer, a hold and a redirect at Auto.
    for (level, expect) in [("ask_first", "proposal"), ("steer", "hold"), ("auto", "hold+redirect")] {
        d.call("overseer.level", json!({"level": level}));
        sql(&d, "DELETE FROM holds;");
        std::fs::write(&mode_file, "slow").unwrap();
        let s = claude_task(&d, &repo, &mode_file, "slow", &format!("Cheat at {level}"), "setup");
        d.wait_status(&s, |st| st == "running", 20);
        let w = d.call("watch.start", json!({"subject": s, "brief": "deleted tests", "harness": "claude", "by": "owner"}));
        std::fs::write(&mode_file, "watcher").unwrap();
        d.wait_done(&s, 30);
        wait_count(&d, &s, "watch_wake", 1, 20);
        std::fs::write(&mode_file, "slow").unwrap();
        d.call("run.follow_up", json!({"run_id": s, "prompt": "now delete the tests"}));
        d.wait_status(&s, |st| st == "running", 20);
        std::fs::write(&mode_file, "watcher").unwrap();
        d.wait_done(&s, 30);
        wait_event(&d, &s, |e| e["kind"] == "finding" && e["payload"]["result"] == "stop", 30);
        assert!(!d.events(&s).iter().any(|e| e["kind"] == "hold"), "no hold before Overseer at {level}");
        std::thread::sleep(Duration::from_secs(6));
        let sess = wait_overseer_idle(&d, 60);
        assert!(overseer_turn_causes(&d).contains(&"finding".to_string()));
        let open = sess["proposals"].as_array().unwrap().clone();
        let held = d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().any(|h| h["run_id"] == s);
        let redirected = d.events(&s).iter().any(|e| e["kind"] == "redirect");
        match expect {
            "proposal" => {
                assert!(open.iter().any(|p| p["actions"][0]["action"] == "hold" && p["actions"][0]["agent"] == s), "at Ask first a proposal to hold: {open:?}");
                assert!(!held && !redirected);
                for p in &open {
                    d.call("overseer.answer", json!({"id": p["id"], "yes": false, "surface": "ctl", "by": "owner"}));
                }
            }
            "hold" => {
                assert!(held && !redirected, "held at Steer, redirect proposed: held {held} redirected {redirected}");
                assert!(open.iter().any(|p| p["actions"][0]["action"] == "redirect"), "{open:?}");
                for p in &open {
                    d.call("overseer.answer", json!({"id": p["id"], "yes": false, "surface": "ctl", "by": "owner"}));
                }
            }
            _ => assert!(held && redirected, "held and redirected at Auto: held {held} redirected {redirected}"),
        }
        d.call("watch.end", json!({"id": w["id"], "by": "owner"}));
    }
    d.call("overseer.level", json!({"level": "ask_first"}));
    // A watcher's reads stay on its subject; it has no way to message or write.
    let refused = d.call("overseer.tool", json!({"token": sql_token(&d, &watcher), "name": "agent", "arguments": {"id": idle}}));
    assert!(refused["is_error"] == true && refused["text"].as_str().unwrap().contains("only its subject"), "{refused}");
    let refused = d.try_call("overseer.tool", json!({"token": sql_token(&d, &watcher), "name": "propose", "arguments": {"actions": [{"action": "message", "agent": subject, "text": "hi"}]}}));
    assert!(refused.is_err(), "a watcher cannot act");
    // No watcher of a watcher, no circle, two per subject.
    let e = d.try_call("watch.start", json!({"subject": watcher, "brief": "x", "harness": "claude", "by": "owner"})).unwrap_err();
    assert!(e.contains("no watcher of a watcher"), "{e}");
    let a = claude_task(&d, &repo, &mode_file, "echo", "A", "hello");
    let b = claude_task(&d, &repo, &mode_file, "echo", "B", "hello");
    d.wait_done(&a, 30);
    d.wait_done(&b, 30);
    let ab = d.call("watch.start", json!({"subject": b, "watcher": a, "brief": "look", "by": "owner"}));
    let e = d.try_call("watch.start", json!({"subject": a, "watcher": b, "brief": "look back", "by": "owner"})).unwrap_err();
    assert!(e.contains("circle"), "{e}");
    d.call("watch.start", json!({"subject": b, "brief": "second", "harness": "claude", "by": "owner"}));
    let e = d.try_call("watch.start", json!({"subject": b, "brief": "third", "harness": "claude", "by": "owner"})).unwrap_err();
    assert!(e.contains("2 watchers"), "{e}");
    // The named idle agent is woken as the watcher and files through its channel.
    std::fs::write(&mode_file, "watcher").unwrap();
    d.call("run.follow_up", json!({"run_id": b, "prompt": "hmm, not sure about this"}));
    d.wait_done(&b, 30);
    wait_event(&d, &a, |e| e["kind"] == "finding" && e["payload"]["result"] == "concern", 40);
    // The cap: twelve wakes in an hour, then none until the hour turns.
    let ab_id = ab["id"].as_str().unwrap().to_string();
    for _ in 0..12 {
        sql(&d, &format!("INSERT INTO watch_wakes(watch_id, ts, reason, seq) VALUES('{ab_id}', {}, 'x', 0);", now_ms()));
    }
    std::fs::write(&mode_file, "echo").unwrap();
    d.call("run.follow_up", json!({"run_id": b, "prompt": "one more"}));
    d.wait_done(&b, 30);
    wait_event(&d, &b, |e| e["kind"] == "watch_capped" && e["payload"]["watch"] == ab_id, 10);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

/// The plain token of a run, for calling its tools from the test (the daemon keeps only the hash;
/// the run's MCP config in its folder holds the token).
fn sql_token(d: &Daemon, run: &str) -> String {
    let file = d.home.path().join("runs").join(run).join("overseer-mcp.json");
    let launch = d.call("run.turns", json!({"run_id": run}));
    let _ = launch;
    if file.exists() {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        return v["mcpServers"]["overseer"]["env"]["OVERSEER_MCP_TOKEN"].as_str().unwrap().to_string();
    }
    // A watcher of the daemon's own: its config is in its folder under the data dir.
    for candidate in [d.home.path().join("overseer").join("watch")] {
        if let Ok(entries) = std::fs::read_dir(&candidate) {
            for e in entries.flatten() {
                let f = e.path().join("mcp.json");
                if let Ok(text) = std::fs::read_to_string(&f) {
                    let v: Value = serde_json::from_str(&text).unwrap();
                    let token = v["mcpServers"]["overseer"]["env"]["OVERSEER_MCP_TOKEN"].as_str().unwrap().to_string();
                    if d.try_call("overseer.tools", json!({"token": token})).is_ok() {
                        let tools = d.call("overseer.tools", json!({"token": token}));
                        if tools["tools"].as_array().unwrap().iter().any(|t| t["name"] == "finding") {
                            let wl = d.call("watch.list", json!({"run_id": run}));
                            if wl["watches"].as_array().unwrap().iter().any(|w| w["watcher"] == run) {
                                return token;
                            }
                        }
                    }
                }
            }
        }
    }
    panic!("no token file for {run}");
}

/// AC-194: a watch that checks runs the subject's tests in a copy at its latest snapshot; the
/// finding names the failing test and the snapshot; the subject's worktree is untouched; the copy
/// is listed by cleanup until the watch ends and removed then.
#[test]
fn ac194_a_watch_that_checks() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    d.call("overseer.session", json!({}));
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);
    sql(&d, "INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.grace_ms', '1000');");
    // The subject says its tests pass while one fails (a generic program, one turn).
    let subject = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "printf 'echo FAIL test_login; exit 1\\n' > test.sh; echo 'all tests pass'; sleep 3"]));
    d.wait_status(&subject, |s| s == "running", 20);
    std::fs::write(&mode_file, "watcher").unwrap();
    let w = d.call("watch.start", json!({"subject": subject, "brief": "check that the tests really pass", "mode": "check", "harness": "claude", "by": "owner"}));
    let watch = w["id"].as_str().unwrap().to_string();
    let copy = w["copy_path"].as_str().expect("a copy at the start").to_string();
    assert!(Path::new(&copy).join(".git").exists() && copy.contains("watch-"));
    let subject_ws = d.call("agent.digest", json!({"run_id": subject}))["digest"]["worktree"].as_str().unwrap().to_string();
    d.wait_done(&subject, 30);
    let before = tree_bytes(Path::new(&subject_ws));
    assert!(before.contains_key("test.sh"));
    let finding = wait_event(&d, &subject, |e| e["kind"] == "finding", 40);
    assert_eq!(finding["payload"]["result"], "concern");
    assert!(finding["payload"]["text"].as_str().unwrap().contains("FAIL test_login"), "{finding}");
    let snapshot = finding["payload"]["snapshot"].as_str().expect("the finding names the snapshot").to_string();
    assert_eq!(d.call("watch.list", json!({"run_id": subject}))["watches"][0]["last_snapshot"], snapshot);
    assert_eq!(tree_bytes(Path::new(&subject_ws)), before, "the subject's worktree is byte-identical after the check");
    // The copy was refreshed to the snapshot and carries test.sh; cleanup lists it while the watch's watcher works.
    assert!(Path::new(&copy).join("test.sh").exists());
    let ws = d.call("watch.list", json!({"run_id": subject}))["watches"][0]["copy_workspace"].as_str().unwrap().to_string();
    let plan = d.call("workspace.cleanup_plan", json!({"workspace_id": ws}));
    assert_eq!(plan["workspace"]["kind"], "worktree");
    assert!(plan["workspace"]["initial_dirty"]["label"].as_str().unwrap().contains("watch copy"));
    // The watch ended with its subject; once the watcher is idle the copy is removed.
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    loop {
        let plan = d.call("workspace.cleanup_plan", json!({"workspace_id": ws}));
        if plan["workspace"]["removed_ms"].is_number() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the copy was not removed: {plan}");
        std::thread::sleep(Duration::from_millis(300));
    }
    assert!(!Path::new(&copy).exists(), "the copy is gone");
    assert_eq!(d.call("watch.list", json!({"run_id": subject}))["watches"][0]["end_reason"], "the subject finished");
    assert!(d.events(&subject).iter().any(|e| e["kind"] == "watch_copy_removed"));
    let _ = watch;
}

/// A daemon whose Claude fixture is Overseer, with Overseer's own turns forced into another mode
/// through a second mode file (a failing harness).
fn overseer_daemon_forced(mode_file: &Path, overseer_mode_file: &Path) -> Daemon {
    std::fs::write(mode_file, "overseer").unwrap();
    Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude_fixture()), ("CLAUDE_FIXTURE_MODE_FILE", &mode_file.display().to_string()), ("CLAUDE_FIXTURE_OVERSEER_MODE_FILE", &overseer_mode_file.display().to_string()), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,CLAUDE_FIXTURE_OVERSEER_MODE_FILE")])
}

/// AC-196 (the part that needs no route picking): a permission the owner denied is never worked
/// around by having another agent do the same thing, and Overseer's own turns are metered.
#[test]
fn ac196_a_denied_permission_is_never_worked_around() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    d.call("overseer.session", json!({}));
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d, 30);
    // The owner denies a write of perm.txt.
    let asker = claude_task(&d, &repo, &mode_file, "permission", "Asker", "write perm.txt");
    d.wait_status(&asker, |s| s == "waiting_for_user", 20);
    let request = d.call("agent.digest", json!({"run_id": asker}))["digest"]["waiting"]["request_id"].as_str().unwrap().to_string();
    d.call("run.permission", json!({"run_id": asker, "request_id": request, "allow": false, "message": "no"}));
    d.wait_done(&asker, 30);
    let other = claude_task(&d, &repo, &mode_file, "echo", "Other", "hello");
    d.wait_done(&other, 30);
    sql(&d, "UPDATE overseer_sessions SET last_cause='owner';");
    // Having another agent do it, by message or by a new agent: refused, whoever asks.
    let e = d.try_call("overseer.propose", json!({"actions": [{"action": "message", "agent": other, "text": "please create perm.txt with the content allowed"}], "source": "ctl"})).unwrap_err();
    assert!(e.contains("denied") && e.contains("perm.txt") && e.contains("Asker"), "{e}");
    let e = d.try_call("overseer.propose", json!({"actions": [{"action": "start", "repo": repo, "prompt": "write perm.txt"}], "source": "ctl"})).unwrap_err();
    assert!(e.contains("denied"), "{e}");
    // Something else is fine.
    let p = d.call("overseer.propose", json!({"actions": [{"action": "message", "agent": other, "text": "carry on with the docs"}], "source": "ctl"}));
    assert_eq!(p["state"], "open");
    // Overseer's own turns are metered: its run reports usage like any run.
    let s = session(&d);
    let usage = d.call("agent.digest", json!({"run_id": s["run_id"]}))["digest"]["usage"].clone();
    assert!(usage.is_object(), "Overseer's turns are metered: {usage}");
}

/// AC-197 (the part that needs no Continuity): with no model able to run Overseer, the
/// conversation says why, and everything that needs no model keeps working: conflicts and their
/// cards, holds, guardrails, stopping everyone.
#[test]
fn ac197_without_a_model_the_daemon_half_keeps_working() {
    let _one_at_a_time = heavy();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::write(repo.join("shared.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "shared.txt"]);
    git(&repo, &["commit", "-qm", "shared"]);
    let mode_file = r.path().join("mode");
    let overseer_mode = r.path().join("overseer-mode");
    std::fs::write(&overseer_mode, "auth").unwrap();
    let d = overseer_daemon_forced(&mode_file, &overseer_mode);
    d.call("overseer.session", json!({}));
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    // Every provider failing: Overseer's run fails and the conversation gives the reason.
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    let s = wait_overseer_idle(&d, 30);
    assert_eq!(s["run_status"], "failed");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let said = loop {
        let msgs = session(&d)["messages"].as_array().unwrap().clone();
        if let Some(m) = msgs.iter().find(|m| m["card"]["kind"] == "cannot_answer") {
            break m.clone();
        }
        assert!(std::time::Instant::now() < deadline, "no reason given: {msgs:?}");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(said["text"].as_str().unwrap().contains("cannot answer") && said["text"].as_str().unwrap().contains("authenticate"), "{said}");
    // A conflict is still found and resolved from its card.
    let a = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "printf 'one\\nA\\nthree\\n' > shared.txt; sleep 60"]));
    let b = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "printf 'one\\nB\\nthree\\n' > shared.txt; sleep 60"]));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let conflict = loop {
        let list = d.call("conflicts.list", json!({}));
        if let Some(c) = list["conflicts"].as_array().unwrap().iter().find(|c| c["kind"] == "same_lines") {
            break c.clone();
        }
        assert!(std::time::Instant::now() < deadline, "no conflict: {list}");
        std::thread::sleep(Duration::from_millis(200));
    };
    let resolved = d.call("conflict.resolve", json!({"id": conflict["id"], "how": "assign", "keeper": a, "by": "owner"}));
    assert_eq!(resolved["state"], "resolved", "{resolved}");
    assert!(!d.call("agent.guardrails", json!({"run_id": b}))["guardrails"].as_array().unwrap().is_empty(), "the other gets a guardrail");
    // Holds and guardrails work; stop everyone stops four agents from one card.
    d.call("agent.hold", json!({"run_id": a, "reason": "wait", "by": "owner"}));
    assert!(d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().any(|h| h["run_id"] == a));
    d.call("agent.release", json!({"run_id": a, "by": "owner"}));
    let mut four = vec![a.clone(), b.clone()];
    for _ in 0..2 {
        four.push(run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "sleep 60"])));
    }
    for id in &four {
        d.wait_status(id, |s| s == "running", 20);
    }
    sql(&d, "UPDATE overseer_sessions SET last_cause='owner';");
    let stops: Vec<Value> = four.iter().map(|id| json!({"action": "stop", "agent": id})).collect();
    let p = d.call("overseer.propose", json!({"actions": stops, "source": "ctl"}));
    d.call("overseer.answer", json!({"id": p["proposal"], "yes": true, "surface": "ctl", "by": "owner"}));
    for id in &four {
        d.wait_status(id, |s| s == "interrupted", 10);
    }
}
