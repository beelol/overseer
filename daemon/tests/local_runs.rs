//! Local runs through the `opencode-serve` harness (Continuity, AC-138 and AC-140) against the
//! real daemon and the real bridge. OpenCode and Ollama are SYNTHETIC here
//! (fixtures/fake-harness/opencode-serve-fixture.js and a loopback server): there is no model, and
//! a prompt is a script the fixture acts out. The live check with a real model is
//! test/local/opencode-serve-live.js.

mod common;
#[path = "common/ollama.rs"]
mod ollama;
#[path = "common/world.rs"]
mod world;

use common::*;
use ollama::Ollama;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use world::*;

struct Local {
    w: World,
    o: Ollama,
    d: Daemon,
    _r: tempfile::TempDir,
    repo: PathBuf,
}

fn local() -> Local {
    let o = Ollama::start();
    o.install(ollama::qwen3_coder_30b())
        .install(ollama::qwen3_coder_30b_64k())
        .install(ollama::qwen25_coder_14b())
        .install(ollama::qwen35_122b());
    o.state
        .lock()
        .unwrap()
        .loaded_size
        .insert("qwen3-coder:30b-64k".into(), 25_411_736_042);
    let w = World::new();
    let fixture = repo_root().join("fixtures/fake-harness/opencode-serve-fixture.js");
    let d = w.start(
        &o.url(),
        &[("OVERSEER_OPENCODE_PATH", fixture.to_str().unwrap())],
    );
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    Local {
        w,
        o,
        d,
        _r: r,
        repo,
    }
}

impl Local {
    fn start(&self, prompt: &str, mode: Option<&str>) -> Value {
        let mut p = json!({"repo": self.repo, "harness": "opencode-serve", "prompt": prompt, "title": prompt});
        if let Some(m) = mode {
            p["permission_mode"] = json!(m);
        }
        let created = self.d.call("task.create", p);
        assert_eq!(created["launch_error"], Value::Null, "{created}");
        created
    }

    /// Waits until the run asks for permission; returns the request.
    fn asked(&self, run: &str) -> Value {
        let r = self.d.wait_status(
            run,
            |s| s != "starting" && s != "running" && s != "queued",
            20,
        );
        assert_eq!(r["status"], "waiting_for_user", "{}", r["exit_reason"]);
        r["attention"].clone()
    }

    fn answer(&self, run: &str, request: &Value, allow: bool) {
        self.d.call(
            "run.permission",
            json!({"run_id": run, "request_id": request["request_id"], "allow": allow}),
        );
    }

    fn done(&self, run: &str) -> Value {
        self.d.wait_done(run, 30)
    }

    /// What the bridge asked of the (synthetic) OpenCode server, from the server's own log.
    fn asked_of_opencode(&self) -> Vec<Value> {
        let log = self
            .d
            .home
            .path()
            .join("profiles/local-ollama/data/opencode/fixture-requests.jsonl");
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn kinds(&self, run: &str, kind: &str) -> Vec<Value> {
        self.d
            .events(run)
            .into_iter()
            .filter(|e| e["kind"] == kind)
            .map(|e| e["payload"].clone())
            .collect()
    }

    fn replies(&self, run: &str) -> Vec<String> {
        self.kinds(run, "output")
            .iter()
            .filter(|p| p["role"] == "assistant")
            .map(|p| p["text"].as_str().unwrap().to_string())
            .collect()
    }
}

fn rule(rules: &Value, permission: &str) -> String {
    rules
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["permission"] == permission)
        .map(|r| r["action"].as_str().unwrap().to_string())
        .unwrap_or_else(|| "agent default".into())
}

// ---------------------------------------------------------------- AC-138

#[test]
fn ac138_permission_modes_through_opencode() {
    let l = local();

    // Plan only: nothing is changed and nothing is asked.
    let c = l.start("write plan.txt nope; say blocked", Some("plan"));
    let (run, ws) = (run_id(&c), ws_path(&l.d, &c));
    assert_eq!(l.done(&run)["status"], "completed");
    assert!(!ws.join("plan.txt").exists());
    assert!(l.kinds(&run, "permission").is_empty());
    assert!(l
        .kinds(&run, "tool_result")
        .iter()
        .any(|t| t["is_error"] == true
            && t["output"]
                .as_str()
                .unwrap()
                .contains("rule which prevents")));
    assert!(l.kinds(&run, "file_activity").is_empty());
    let r = l.d.run(&run);
    assert_eq!(
        (
            r["model"].as_str(),
            r["profile_id"].as_str(),
            r["harness"].as_str()
        ),
        (
            Some("ollama/qwen3-coder:30b-64k"),
            Some("local-ollama"),
            Some("opencode-serve")
        )
    );

    // Ask first: the write asks before anything is written; Allow lets it through; a command
    // in the same turn asks again.
    let c = l.start(
        "write asked.txt hello from ask first; bash ls; say done",
        Some("manual"),
    );
    let (run, ws) = (run_id(&c), ws_path(&l.d, &c));
    let ask = l.asked(&run);
    assert_eq!(
        (ask["kind"].as_str(), ask["tool"].as_str()),
        (Some("permission"), Some("edit: asked.txt"))
    );
    assert!(ask["input"]["path"]
        .as_str()
        .unwrap()
        .ends_with("/asked.txt"));
    assert!(ask["input"]["diff"]
        .as_str()
        .unwrap()
        .contains("+hello from ask first"));
    assert!(
        !ws.join("asked.txt").exists(),
        "nothing is written before the answer"
    );
    l.answer(&run, &ask, true);
    let again = l.asked(&run);
    assert_eq!(again["tool"], "command: ls");
    assert!(ws.join("asked.txt").exists());
    l.answer(&run, &again, true);
    assert_eq!(l.done(&run)["status"], "completed");
    assert_eq!(
        std::fs::read_to_string(ws.join("asked.txt")).unwrap(),
        "hello from ask first\n"
    );
    assert_eq!(
        l.kinds(&run, "file_activity")[0]["paths"],
        json!(["asked.txt"])
    );
    assert_eq!(l.replies(&run), ["done"]);
    assert_eq!(l.kinds(&run, "permission_answered").len(), 2);
    let usage = l.kinds(&run, "usage");
    assert_eq!(
        (
            usage[0]["local"].clone(),
            usage[0]["cost"].clone(),
            usage[0]["model"].as_str()
        ),
        (json!(true), json!(0), Some("ollama/qwen3-coder:30b-64k"))
    );

    // Ask first: Deny blocks the write, and a denied write is not file activity.
    let c = l.start(
        "write denied.txt should not exist; say refused",
        Some("manual"),
    );
    let (run, ws) = (run_id(&c), ws_path(&l.d, &c));
    let ask = l.asked(&run);
    l.answer(&run, &ask, false);
    assert_eq!(l.done(&run)["status"], "completed");
    assert!(!ws.join("denied.txt").exists());
    assert!(l.kinds(&run, "file_activity").is_empty());
    assert_eq!(l.replies(&run), ["refused"]);

    // Accept edits: the edit is made without asking; the command asks.
    let c = l.start(
        "write accepted.txt hello; bash ls; say done",
        Some("acceptEdits"),
    );
    let (run, ws) = (run_id(&c), ws_path(&l.d, &c));
    let ask = l.asked(&run);
    assert_eq!(ask["tool"], "command: ls");
    assert!(
        ws.join("accepted.txt").exists(),
        "the edit was already made when the command asked"
    );
    l.answer(&run, &ask, true);
    assert_eq!(l.done(&run)["status"], "completed");

    // Auto: both run without asking.
    let c = l.start("write auto.txt hello; bash ls; say done", Some("auto"));
    let (run, ws) = (run_id(&c), ws_path(&l.d, &c));
    assert_eq!(l.done(&run)["status"], "completed");
    assert!(ws.join("auto.txt").exists() && l.kinds(&run, "permission").is_empty());
    assert!(l
        .kinds(&run, "tool")
        .iter()
        .any(|t| t["name"] == "bash" && t["summary"].as_str().unwrap().ends_with("[completed]")));

    // No mode given: Ask first.
    let c = l.start("write default.txt hello; say done", None);
    let run = run_id(&c);
    let ask = l.asked(&run);
    assert_eq!(ask["tool"], "edit: default.txt");
    l.answer(&run, &ask, true);
    l.done(&run);

    // A mode that would bypass everything does not exist.
    let e = l.d.call("task.create", json!({"repo": l.repo, "harness": "opencode-serve", "prompt": "write x.txt x", "permission_mode": "bypassPermissions"}));
    assert!(e["launch_error"].as_str().unwrap().contains("does not take permission mode \"bypassPermissions\" (choose plan, manual, acceptEdits, auto)"), "{e}");

    // What the bridge asked of OpenCode: the rules travel with the session, every mode denies
    // the question tool, the model is the local one, and the server is behind a password.
    let asked = l.asked_of_opencode();
    let sessions: Vec<&Value> = asked
        .iter()
        .filter(|r| r["method"] == "POST" && r["path"] == "/session")
        .map(|r| &r["body"])
        .collect();
    let by_mode: Vec<(String, String, String, String)> = sessions
        .iter()
        .map(|s| {
            (
                s["agent"].as_str().unwrap().to_string(),
                rule(&s["permission"], "edit"),
                rule(&s["permission"], "bash"),
                rule(&s["permission"], "question"),
            )
        })
        .collect();
    let t = |a: &str, e: &str, b: &str| {
        (
            a.to_string(),
            e.to_string(),
            b.to_string(),
            "deny".to_string(),
        )
    };
    assert_eq!(
        by_mode,
        [
            t("plan", "agent default", "agent default"),
            t("build", "ask", "ask"),
            t("build", "ask", "ask"),
            t("build", "allow", "ask"),
            t("build", "agent default", "agent default"),
            t("build", "ask", "ask")
        ]
    );
    for p in asked.iter().filter(|r| {
        r["path"]
            .as_str()
            .is_some_and(|p| p.ends_with("/prompt_async"))
    }) {
        assert_eq!(
            p["body"]["model"],
            json!({"providerID": "ollama", "modelID": "qwen3-coder:30b-64k"})
        );
    }
    let replies: Vec<&str> = asked
        .iter()
        .filter(|r| {
            r["path"]
                .as_str()
                .is_some_and(|p| p.starts_with("/permission/"))
        })
        .map(|r| r["body"]["reply"].as_str().unwrap())
        .collect();
    assert_eq!(
        replies,
        ["once", "once", "reject", "once", "once"],
        "only once and reject are ever sent"
    );
    let starts: Vec<&Value> = asked.iter().filter(|r| r["start"] == true).collect();
    assert_eq!(starts.len(), 6, "one server per turn");
    for s in starts {
        assert_eq!(s["password"], true);
        assert_eq!(s["config"]["enabled_providers"], json!(["ollama"]));
        assert_eq!(
            (
                s["config"]["model"].as_str(),
                s["config"]["small_model"].as_str()
            ),
            (
                Some("ollama/qwen3-coder:30b-64k"),
                Some("ollama/qwen3-coder:30b-64k")
            )
        );
        assert!(s["config"].get("permission").is_none());
    }
    // The model was loaded once, under the watchdog, and found loaded by the later runs.
    let loads = l.o.asked("/api/generate");
    assert_eq!(
        loads,
        vec![
            json!({"model": "qwen3-coder:30b-64k", "prompt": "", "keep_alive": "30m", "options": {"num_ctx": 65536}})
        ]
    );
}

#[test]
fn ac138_interrupt_and_follow_up_in_the_same_session() {
    let l = local();
    let c = l.start("sleep 30; say done", Some("auto"));
    let run = run_id(&c);
    let until = Instant::now() + Duration::from_secs(20);
    while !l
        .kinds(&run, "tool")
        .iter()
        .any(|t| t["summary"].as_str().unwrap().contains("sleep 30"))
    {
        assert!(Instant::now() < until, "the command never started");
        std::thread::sleep(Duration::from_millis(50));
    }
    let at = Instant::now();
    l.d.call("run.interrupt", json!({"run_id": run}));
    let r = l.done(&run);
    assert_eq!(r["status"], "interrupted");
    assert!(
        at.elapsed() < Duration::from_secs(8),
        "long before the 30 s command would end ({:?})",
        at.elapsed()
    );
    assert!(l
        .kinds(&run, "error")
        .iter()
        .any(|e| e["class"] == "aborted"));
    let home = l.d.home.path().display().to_string();
    std::thread::sleep(Duration::from_millis(300));
    let left = std::process::Command::new("pgrep")
        .args([
            "-f",
            &format!("opencode-serve-fixture.js serve.*|{home}.*opencode"),
        ])
        .output()
        .unwrap();
    let mine: Vec<String> = String::from_utf8_lossy(&left.stdout)
        .lines()
        .filter(|pid| {
            std::process::Command::new("ps")
                .args(["-Eww", "-p", pid, "-o", "command="])
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&home))
        })
        .map(str::to_string)
        .collect();
    assert!(mine.is_empty(), "no server is left running: {mine:?}");

    // A follow-up continues the same session, and its mode applies from then on.
    let c = l.start("write first.txt one; say done", Some("auto"));
    let (run, ws) = (run_id(&c), ws_path(&l.d, &c));
    assert_eq!(l.done(&run)["status"], "completed");
    let session = l.d.run(&run)["native_id"].as_str().unwrap().to_string();
    l.d.call("run.follow_up", json!({"run_id": run, "prompt": "recall; write second.txt two", "permission_mode": "manual"}));
    let ask = l.asked(&run);
    assert_eq!(ask["tool"], "edit: second.txt");
    l.answer(&run, &ask, true);
    assert_eq!(l.done(&run)["status"], "completed");
    assert_eq!(l.d.run(&run)["native_id"], session.as_str());
    assert_eq!(
        l.replies(&run),
        ["done", "first.txt"],
        "the continued session knows what it did before"
    );
    assert!(ws.join("second.txt").exists());
    let asked = l.asked_of_opencode();
    let patched = asked
        .iter()
        .find(|r| r["method"] == "PATCH" && r["path"] == format!("/session/{session}"))
        .expect("the rules were set again on the continued session");
    assert_eq!(
        (
            rule(&patched["body"]["permission"], "edit"),
            rule(&patched["body"]["permission"], "question")
        ),
        ("ask".to_string(), "deny".to_string())
    );
    assert_eq!(
        asked
            .iter()
            .filter(|r| r["method"] == "POST" && r["path"] == "/session")
            .count(),
        2,
        "no new session for the follow-up"
    );
}

#[test]
fn ac138_a_tool_call_written_as_text_is_retried_once() {
    let l = local();
    let c = l.start("astext t.txt hello", Some("auto"));
    let (run, ws) = (run_id(&c), ws_path(&l.d, &c));
    assert_eq!(l.done(&run)["status"], "completed");
    assert!(
        ws.join("t.txt").exists(),
        "after the nudge the tool was really called"
    );
    let notes: Vec<String> = l
        .kinds(&run, "output")
        .iter()
        .filter(|p| p["role"] == "system")
        .map(|p| p["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(notes, ["The model wrote a tool call as text, so nothing ran. Asking it once more to call the tool."]);
    // Twice in a row: the turn fails and says why.
    let c = l.start("astext-always", Some("auto"));
    let run = run_id(&c);
    let r = l.done(&run);
    assert_eq!(r["status"], "failed");
    let e = l.kinds(&run, "error");
    assert_eq!(
        (e[0]["class"].as_str(), e[0]["message"].as_str()),
        (
            Some("tool_call_as_text"),
            Some("qwen3-coder:30b-64k wrote its tool call as text twice; nothing ran")
        )
    );
    assert!(
        r["exit_reason"]
            .as_str()
            .unwrap()
            .contains("turn reported failure"),
        "{r}"
    );
}

#[test]
fn ac138_children_and_failures_of_the_local_model() {
    let l = local();
    let c = l.start("child hi from child; say delegated", Some("auto"));
    let run = run_id(&c);
    assert_eq!(l.done(&run)["status"], "completed");
    let kids: Vec<Value> =
        l.d.runs()
            .into_iter()
            .filter(|r| r["parent_run_id"] == run.as_str())
            .collect();
    assert_eq!(kids.len(), 1);
    assert_eq!(
        (kids[0]["status"].as_str(), kids[0]["title"].as_str()),
        (Some("completed"), Some("hi from child (@general subagent)"))
    );
    assert!(kids[0]["native_id"].as_str().unwrap().starts_with("ses_"));
    let child_out: Vec<Value> =
        l.d.events(kids[0]["id"].as_str().unwrap())
            .into_iter()
            .filter(|e| e["kind"] == "output")
            .collect();
    assert!(child_out
        .iter()
        .any(|e| e["payload"]["text"] == "hi from child"));
    assert_eq!(
        l.replies(&run),
        ["delegated"],
        "the child's reply is the child's, not the root's"
    );

    // Ollama stops answering in the middle of a turn: the error is a local one.
    let c = l.start("fail connect ECONNREFUSED 127.0.0.1:11434", Some("auto"));
    let run = run_id(&c);
    assert_eq!(l.done(&run)["status"], "failed");
    let e = l.kinds(&run, "error");
    assert_eq!(
        (e[0]["class"].as_str(), e[0]["message"].as_str()),
        (
            Some("local_model"),
            Some("APIError: connect ECONNREFUSED 127.0.0.1:11434")
        )
    );
    assert_eq!(
        conn(&l.d)["state"],
        "online",
        "a local model failing is not the connection failing"
    );
}

// ---------------------------------------------------------------- AC-140

#[test]
fn ac140_a_local_run_passes_the_guard_before_it_starts() {
    let l = local();
    let refused = |params: Value, why: &str| {
        let c = l.d.call("task.create", params);
        assert!(
            c["launch_error"].as_str().unwrap_or_default().contains(why),
            "{c}"
        );
        let r = l.d.run(c["run"]["id"].as_str().unwrap());
        assert_eq!(
            r["status"], "failed",
            "a refused run ends; it does not wait to launch"
        );
        assert!(
            r["exit_reason"]
                .as_str()
                .unwrap()
                .starts_with("not launched: ")
                && r["exit_reason"].as_str().unwrap().contains(why),
            "{r}"
        );
    };
    let task = |extra: Value| {
        let mut p = json!({"repo": l.repo, "harness": "opencode-serve", "prompt": "write x.txt x", "permission_mode": "auto"});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    };
    // The 122B model, asked for by name as the composer would.
    refused(
        task(json!({"model": "ollama/qwen3.5:122b"})),
        "qwen3.5:122b is too big to load: 77.2 GiB at a 16k context is over the budget of 51.2 GiB",
    );
    refused(task(json!({"model": "qwen3.5:122b"})), "is too big to load");
    refused(
        task(json!({"model": "ollama/not-installed:1b"})),
        "not-installed:1b is not installed in Ollama",
    );
    // Memory is short now: the pick that fitted a moment ago is refused.
    l.w.memory(128.0, 20.0, "normal");
    refused(
        task(json!({})),
        "no local model that is installed and verified fits the memory budget of 7.2 GiB",
    );
    refused(
        task(json!({"model": "ollama/qwen3-coder:30b-64k"})),
        "24.3 GiB at a 64k context is over the budget of 7.2 GiB",
    );
    l.w.memory(128.0, 115.2, "critical");
    refused(
        task(json!({"model": "ollama/qwen2.5-coder:14b"})),
        "critical memory pressure; nothing may be loaded",
    );
    l.w.memory(128.0, 115.2, "normal");
    // The user's own OpenCode profile is never used for a local run.
    refused(
        task(json!({"profile_id": "system-opencode"})),
        "local runs use Overseer's own OpenCode profile, never your own OpenCode configuration",
    );
    assert!(
        l.o.asked("/api/generate").is_empty() && l.o.asked("/api/create").is_empty(),
        "nothing was loaded or created for a refused run"
    );
    assert!(
        l.asked_of_opencode().is_empty(),
        "and OpenCode was never started"
    );

    // A tag that sets no context gets the longest context that fits, through a tag of Overseer's
    // own that shares the weights.
    let c = l.d.call(
        "task.create",
        task(json!({"model": "ollama/qwen3-coder:30b"})),
    );
    assert_eq!(c["launch_error"], Value::Null, "{c}");
    let run = run_id(&c);
    assert_eq!(l.d.wait_done(&run, 30)["status"], "completed");
    assert_eq!(
        l.d.run(&run)["model"],
        "ollama/qwen3-coder:30b-64k",
        "the installed tag that already sets 64k is used"
    );
    let m = &l.kinds(&run, "local_model")[0];
    assert_eq!(
        (
            m["context"].as_u64(),
            m["already_loaded"].clone(),
            gib(&m["budget"]["budget"])
        ),
        (Some(65536), json!(false), 51.2)
    );
    let loads = l.kinds(&run, "local_load");
    assert_eq!(loads.len(), 1);
    assert!(
        loads[0]["memory_before"]["available"].is_u64()
            && loads[0]["measured"]["size"] == 25_411_736_042u64
    );
    let c = l.d.call(
        "task.create",
        task(json!({"model": "ollama/qwen2.5-coder:14b"})),
    );
    let run = run_id(&c);
    assert_eq!(l.d.wait_done(&run, 30)["status"], "completed");
    assert_eq!(
        l.d.run(&run)["model"],
        "ollama/overseer/qwen2.5-coder-14b:32k"
    );
    assert_eq!(
        l.o.asked("/api/create"),
        vec![
            json!({"model": "overseer/qwen2.5-coder-14b:32k", "from": "qwen2.5-coder:14b", "parameters": {"num_ctx": 32768}, "stream": false})
        ]
    );

    // Without Ollama nothing local can run, and the run says so.
    let w = World::new();
    let fixture = repo_root().join("fixtures/fake-harness/opencode-serve-fixture.js");
    let d = w.start(
        &no_ollama(),
        &[("OVERSEER_OPENCODE_PATH", fixture.to_str().unwrap())],
    );
    let c = d.call("task.create", task(json!({})));
    assert_eq!(
        c["launch_error"],
        "Ollama is not installed; a local model cannot run"
    );
    assert_eq!(d.run(c["run"]["id"].as_str().unwrap())["status"], "failed");
}

fn _unused(_: &Path) {}
