//! Gate N: what a phone does through the daemon beyond talking to agents: review (AC-126),
//! pull requests and sign-in (AC-127), notifications (AC-129). A real daemon, fixture harnesses
//! and the reference phone of the gateway tests.

mod common;

use common::phone::{self, pair, Phone};
use common::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

fn daemon(mode: &str, extra: &[(&str, &str)]) -> Daemon {
    let claude = fixture("fake-harness/claude-fixture.js");
    let mut env: Vec<(&str, &str)> = vec![("OVERSEER_CLAUDE_PATH", &claude), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_SLOW_MS"), ("FIXTURE_MODE", mode), ("OVERSEER_GATEWAY_MDNS", "off")];
    env.extend_from_slice(extra);
    Daemon::start(&env)
}

fn events_of(d: &Daemon, kind: &str) -> Vec<Value> {
    d.call("events.list", json!({"limit": 5000}))["events"].as_array().unwrap().iter().filter(|e| e["kind"] == kind).cloned().collect()
}

fn wait_until(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("{what} did not happen in {secs} s");
}

/// An agent that edits files and ends: a worktree with changes to review.
fn edited(d: &Daemon, repo: &std::path::Path, script: &str) -> (Value, std::path::PathBuf, String) {
    let created = d.generic(repo, "worktree", "/bin/sh", &["-c", script]);
    let run = run_id(&created);
    d.wait_done(&run, 20);
    let ws = ws_path(d, &created);
    let base = option(d, &run, "task_start", None)["base"].as_str().unwrap_or("HEAD").to_string();
    (created, ws, base)
}

// ---------------------------------------------------------------- AC-126

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac126_review_on_the_phone() {
    let d = daemon("echo", &[]);
    phone::enable(&d);
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::write(repo.join("poem.txt"), "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "poem"]);
    let script = "printf 'one\\nTWO\\nthree\\nfour\\nfive\\nsix\\nseven\\neight\\nNINE\\nten\\neleven\\n' > poem.txt; printf 'new file\\nsecond line\\n' > added.txt; rm b.txt; printf '\\000\\001binary' > blob.bin; mkdir -p deep/er; echo nested > deep/er/n.txt; ln -s /etc/hosts link-out; echo done";
    let (created, ws, base) = edited(&d, &repo, script);
    let run = run_id(&created);
    let wsid = created["workspace"]["id"].as_str().unwrap().to_string();
    let (mut p, _) = pair(&d, "Reviewing Phone").await;

    // The changed files with their status, as VS Code lists them.
    let diff = p.call("workspace.diff", json!({"workspace_id": wsid, "base": base})).await;
    let mut listed: Vec<(String, String)> = diff["changes"].as_array().unwrap().iter().map(|c| (c["status"].as_str().unwrap().to_string(), c["path"].as_str().unwrap().to_string())).collect();
    listed.sort();
    assert_eq!(listed, diff_paths(&d, &created, &base), "the phone's list is the daemon's");
    assert!(listed.contains(&("M".into(), "poem.txt".into())) && listed.contains(&("A".into(), "added.txt".into())) && listed.contains(&("D".into(), "b.txt".into())), "{listed:?}");
    let options = p.call("comparison.options", json!({"run_id": run})).await;
    let modes: Vec<String> = options["options"].as_array().unwrap().iter().map(|o| o["mode"].as_str().unwrap().to_string()).collect();
    for mode in ["latest_run", "task_start"] {
        assert!(modes.contains(&mode.to_string()), "{mode} is offered: {modes:?}");
    }

    // A file at the comparison and now.
    let file = p.call("workspace.file", json!({"workspace_id": wsid, "path": "poem.txt", "base": base})).await;
    assert_eq!(file["before"]["text"], "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n");
    assert_eq!(file["now"]["text"], std::fs::read_to_string(ws.join("poem.txt")).unwrap());
    let added = p.call("workspace.file", json!({"workspace_id": wsid, "path": "added.txt", "base": base})).await;
    assert_eq!(added["before"]["exists"], false);
    assert_eq!(added["now"]["text"], "new file\nsecond line\n");
    let gone = p.call("workspace.file", json!({"workspace_id": wsid, "path": "b.txt", "base": base})).await;
    assert_eq!((gone["before"]["text"].as_str(), gone["now"]["exists"].as_bool()), (Some("b\n"), Some(false)));
    assert_eq!(p.call("workspace.file", json!({"workspace_id": wsid, "path": "deep/er/n.txt"})).await["now"]["text"], "nested\n");

    // The hunks equal what Git says changed.
    let hunks = p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "poem.txt", "base": base, "run_id": run})).await;
    let list = hunks["hunks"].as_array().unwrap().clone();
    let shape: Vec<(i64, Vec<String>, i64, Vec<String>)> = list.iter().map(|h| (h["base_start"].as_i64().unwrap(), strs(&h["base_lines"]), h["modified_start"].as_i64().unwrap(), strs(&h["modified_lines"]))).collect();
    assert_eq!(shape, vec![(2, vec!["two".into()], 2, vec!["TWO".into()]), (9, vec!["nine".into()], 9, vec!["NINE".into()]), (10, vec![], 11, vec!["eleven".into()])]);
    let from_git = git(&ws, &["diff", "-U0", "--no-color", &base, "--", "poem.txt"]);
    // Git adds the enclosing line after the second @@; the ranges are what is compared.
    let headers: Vec<String> = from_git.lines().filter(|l| l.starts_with("@@")).map(|l| format!("@@{}@@", l.split("@@").nth(1).unwrap())).collect();
    assert_eq!(headers, vec!["@@ -2 +2 @@", "@@ -9 +9 @@", "@@ -10,0 +11 @@"], "the same hunks as git diff");
    assert!(list.iter().all(|h| h["reviewed"] == false));
    let new_file = p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "added.txt", "base": base})).await;
    assert_eq!(strs(&new_file["hunks"][0]["modified_lines"]), vec!["new file", "second line"]);

    // Nothing leaves the workspace: traversal, absolute paths, .git, links, folders, binary, too large.
    for bad in ["../repo/a.txt", "/etc/hosts", "deep/../../x", ".git/config", "deep/.git/x", ""] {
        let reply = p.ask("workspace.file", json!({"workspace_id": wsid, "path": bad, "base": base}), None).await.unwrap();
        assert!(["outside_workspace", "failed"].contains(&Phone::code(&reply).as_str()) && reply.get("result").is_none(), "{bad:?}: {reply}");
    }
    let link = p.call("workspace.file", json!({"workspace_id": wsid, "path": "link-out"})).await;
    assert_eq!(link["now"]["kind"], "link");
    assert!(link["now"]["text"].is_null() && !link.to_string().contains("localhost"), "a link is not followed: {link}");
    std::os::unix::fs::symlink("/etc", ws.join("dir-link")).unwrap();
    let through = p.ask("workspace.file", json!({"workspace_id": wsid, "path": "dir-link/hosts"}), None).await.unwrap();
    assert_eq!(Phone::code(&through), "outside_workspace", "{through}");
    assert_eq!(p.call("workspace.file", json!({"workspace_id": wsid, "path": "blob.bin"})).await["now"]["kind"], "binary");
    assert_eq!(p.call("workspace.file", json!({"workspace_id": wsid, "path": "deep"})).await["now"]["kind"], "directory");
    std::fs::write(ws.join("huge.txt"), vec![b'x'; 3 * 1024 * 1024]).unwrap();
    let huge = p.call("workspace.file", json!({"workspace_id": wsid, "path": "huge.txt"})).await;
    assert_eq!(huge["now"]["kind"], "too_large");
    assert!(huge["now"]["text"].is_null());
    let not_shown = p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "blob.bin", "base": base})).await;
    assert_eq!(not_shown["shown"], false);
    for bad in ["--output=/tmp/x", "HEAD..main", "a b", "$(id)"] {
        let reply = p.ask("workspace.file", json!({"workspace_id": wsid, "path": "poem.txt", "base": bad}), None).await.unwrap();
        assert!(reply.get("error").is_some(), "{bad}");
    }

    // Accept marks a hunk reviewed, by its content; the mark shows on the Mac, and the Mac's on the phone.
    let first = &list[0];
    let accepted = p.act("review.accept", json!({"run_id": run, "path": "poem.txt", "key": first["key"], "modified_start": first["modified_start"], "modified_lines": first["modified_lines"], "base_lines": first["base_lines"]})).await;
    assert!(accepted.get("error").is_none(), "{accepted}");
    assert_eq!(d.call("review.marks", json!({"run_id": run}))["keys"], json!([first["key"]]), "the Mac sees the phone's mark");
    assert_eq!(d.call("review.marks", json!({"run_id": run}))["marks"][0]["by"], "phone:Reviewing Phone");
    let second = &list[1];
    d.call("review.accept", json!({"run_id": run, "path": "poem.txt", "key": second["key"], "modified_start": second["modified_start"], "modified_lines": second["modified_lines"], "anchor": "NINE"}));
    let again = p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "poem.txt", "base": base, "run_id": run})).await;
    let reviewed: Vec<bool> = again["hunks"].as_array().unwrap().iter().map(|h| h["reviewed"].as_bool().unwrap()).collect();
    assert_eq!(reviewed, vec![true, true, false], "the phone sees the Mac's mark");
    let marks = events_of(&d, "review_mark");
    assert_eq!(marks.len(), 2);
    assert_eq!((marks[0]["source"].as_str(), marks[1]["source"].as_str()), (Some("phone:Reviewing Phone"), Some("user")));
    // A key that does not name the hunk is refused.
    let wrong = p.act("review.accept", json!({"run_id": run, "path": "poem.txt", "key": "0123456789abcdef", "modified_start": 11, "modified_lines": ["eleven"], "base_lines": []})).await;
    assert!(wrong.get("error").is_some());

    // The agent changes a hunk while the phone accepts it: a conflict, and nothing is marked.
    let third = &list[2];
    std::fs::write(ws.join("poem.txt"), "one\nTWO\nthree\nfour\nfive\nsix\nseven\neight\nNINE\nten\ntwelve\n").unwrap();
    let conflict = p.act("review.accept", json!({"run_id": run, "path": "poem.txt", "key": third["key"], "modified_start": third["modified_start"], "modified_lines": third["modified_lines"], "base_lines": third["base_lines"]})).await;
    assert_eq!(Phone::code(&conflict), "conflict", "{conflict}");
    assert!(conflict["error"]["message"].as_str().unwrap().contains("changed while you were accepting"));
    assert_eq!(d.call("review.marks", json!({"run_id": run}))["keys"].as_array().unwrap().len(), 2);
    // And rejecting the hunk as it was shown is a conflict too; the file is untouched.
    let stale = p.act("review.reject", json!({"workspace_id": wsid, "path": "poem.txt", "base": base, "key": third["key"]})).await;
    assert_eq!(Phone::code(&stale), "conflict");
    assert!(std::fs::read_to_string(ws.join("poem.txt")).unwrap().ends_with("twelve\n"));

    // Reject puts the comparison's lines back, on disk, and only for that hunk.
    let now = p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "poem.txt", "base": base, "run_id": run})).await;
    let shout = now["hunks"].as_array().unwrap().iter().find(|h| strs(&h["modified_lines"]) == vec!["TWO".to_string()]).unwrap().clone();
    let rejected = p.act("review.reject", json!({"workspace_id": wsid, "path": "poem.txt", "base": base, "key": shout["key"]})).await;
    assert!(rejected.get("error").is_none(), "{rejected}");
    assert_eq!(std::fs::read_to_string(ws.join("poem.txt")).unwrap(), "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nNINE\nten\ntwelve\n");
    let tail = now["hunks"].as_array().unwrap().iter().find(|h| strs(&h["modified_lines"]) == vec!["twelve".to_string()]).unwrap().clone();
    let tail_now = p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "poem.txt", "base": base})).await["hunks"].as_array().unwrap().iter().find(|h| strs(&h["modified_lines"]) == vec!["twelve".to_string()]).unwrap().clone();
    assert_eq!(tail["key"], tail_now["key"], "a hunk keeps its key while its content is the same");
    p.act("review.reject", json!({"workspace_id": wsid, "path": "poem.txt", "base": base, "key": tail_now["key"]})).await;
    assert_eq!(std::fs::read_to_string(ws.join("poem.txt")).unwrap(), "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nNINE\nten\n");
    // A file the agent removed comes back; a file it added goes away.
    let removed = p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "b.txt", "base": base})).await["hunks"][0].clone();
    assert!(p.act("review.reject", json!({"workspace_id": wsid, "path": "b.txt", "base": base, "key": removed["key"]})).await.get("error").is_none());
    assert_eq!(std::fs::read_to_string(ws.join("b.txt")).unwrap(), "b\n");
    let whole = p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "added.txt", "base": base})).await["hunks"][0].clone();
    let gone = p.act("review.reject", json!({"workspace_id": wsid, "path": "added.txt", "base": base, "key": whole["key"]})).await;
    assert_eq!(gone["result"]["file_removed"], true, "{gone}");
    assert!(!ws.join("added.txt").exists());
    // Not through a link, not a binary file, not outside.
    for (path, key) in [("link-out", "0123456789abcdef"), ("blob.bin", "0123456789abcdef"), ("../repo/a.txt", "0123456789abcdef")] {
        let reply = p.act("review.reject", json!({"workspace_id": wsid, "path": path, "base": base, "key": key})).await;
        assert!(reply.get("error").is_some(), "{path}: {reply}");
    }
    assert_eq!(std::fs::read_to_string("/etc/hosts").is_ok(), true);
    assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "a\n", "the source checkout is untouched");
    // A watch-only phone reads the review and cannot change it.
    let (mut w, watch) = pair(&d, "Watching Phone").await;
    d.call("gateway.device_scope", json!({"id": watch.device, "scope": "watch"}));
    assert!(w.call("workspace.hunks", json!({"workspace_id": wsid, "path": "poem.txt", "base": base})).await["hunks"].is_array());
    let last = p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "poem.txt", "base": base})).await["hunks"][0].clone();
    assert_eq!(Phone::code(&w.act("review.reject", json!({"workspace_id": wsid, "path": "poem.txt", "base": base, "key": last["key"]})).await), "watch_only");
    assert_eq!(Phone::code(&w.act("review.accept", json!({"run_id": run, "path": "poem.txt", "key": last["key"]})).await), "watch_only");

    // Live: while an agent edits, a fresh read shows the new content with no reconnect.
    let slow = d.generic(&repo, "worktree", "/bin/sh", &["-c", "for n in 1 2 3 4 5 6; do echo step$n >> a.txt; sleep 0.3; done"]);
    let slow_ws = slow["workspace"]["id"].as_str().unwrap().to_string();
    let slow_base = "HEAD";
    let mut sizes = Vec::new();
    for _ in 0..8 {
        let h = p.call("workspace.hunks", json!({"workspace_id": slow_ws, "path": "a.txt", "base": slow_base})).await;
        sizes.push(h["hunks"].as_array().unwrap().first().map(|x| x["modified_lines"].as_array().unwrap().len()).unwrap_or(0));
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(sizes.windows(2).all(|w| w[0] <= w[1]) && sizes.first() < sizes.last(), "the review grows as the agent edits: {sizes:?}");
    d.wait_done(&run_id(&slow), 20);
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac126_a_large_repository_lists_quickly() {
    let d = daemon("echo", &[]);
    phone::enable(&d);
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    // 10,000 files, 200 of them changed by the agent.
    for dir in 0..100 {
        let path = repo.join(format!("d{dir:03}"));
        std::fs::create_dir_all(&path).unwrap();
        for f in 0..100 {
            std::fs::write(path.join(format!("f{f:03}.txt")), format!("file {dir} {f}\n")).unwrap();
        }
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "many files"]);
    let (created, _, base) = edited(&d, &repo, "for d in d000 d001; do for f in $d/*.txt; do echo changed >> $f; done; done");
    let wsid = created["workspace"]["id"].as_str().unwrap().to_string();
    let (mut p, _) = pair(&d, "Large Repo Phone").await;
    let mut timings = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        let diff = p.call("workspace.diff", json!({"workspace_id": wsid, "base": base})).await;
        assert_eq!(diff["changes"].as_array().unwrap().len(), 200);
        timings.push(started.elapsed().as_millis());
    }
    let tree_started = Instant::now();
    let tree = p.call("workspace.tree", json!({"workspace_id": wsid, "dir": ""})).await;
    assert!(tree["total"].as_u64().unwrap() >= 100);
    let tree_ms = tree_started.elapsed().as_millis();
    let hunk_started = Instant::now();
    p.call("workspace.hunks", json!({"workspace_id": wsid, "path": "d000/f000.txt", "base": base})).await;
    let hunk_ms = hunk_started.elapsed().as_millis();
    timings.sort();
    println!("10,000 files, 200 changed, through the gateway: list p50 {} ms, max {} ms; tree {tree_ms} ms; one file's hunks {hunk_ms} ms", timings[2], timings[4]);
    assert!(timings[4] < 3000 && tree_ms < 3000 && hunk_ms < 1000, "{timings:?} {tree_ms} {hunk_ms}");
}
