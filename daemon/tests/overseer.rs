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
    assert_eq!(tools, ["roster", "agent", "conflicts", "conversation", "changes", "diff", "file", "search", "usage", "propose"]);
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
    assert_eq!(names, ["roster"]);
    let err = d.try_call("overseer.tool", json!({"token": agent, "name": "agent", "arguments": {"id": "x"}})).unwrap_err();
    assert!(err.contains("no tool agent"), "{err}");
    assert!(d.try_call("overseer.token", json!({"run_id": "r", "role": "king"})).is_err());
}

fn claude_fixture() -> String {
    repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()
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
