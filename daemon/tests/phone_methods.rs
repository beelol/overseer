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

// ---------------------------------------------------------------- AC-127

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac127_everything_else_overseer_has() {
    let t = tmp();
    let sys = t.path().join("desktop-home");
    std::fs::create_dir_all(&sys).unwrap();
    let next = t.path().join("next-login");
    let approval = t.path().join("approved-in-the-browser");
    let cli = fixture("fake-harness/account-cli.js");
    // A fake GitHub CLI that records its arguments, and a local repository that stands in for github.com.
    let gh_log = t.path().join("gh-args.json");
    let gh = t.path().join("gh");
    std::fs::write(&gh, format!("#!/bin/sh\nnode -e 'require(\"fs\").writeFileSync(process.argv[1], JSON.stringify(process.argv.slice(2)))' '{}' \"$@\"\necho 'https://github.com/test-owner/pr-demo/pull/7'\n", gh_log.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &cli), ("OVERSEER_CLAUDE_PATH", &cli), ("OVERSEER_TEST_SYSTEM_HOME", sys.to_str().unwrap()), ("OVERSEER_GATEWAY_MDNS", "off"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME,FIXTURE_DEVICE_APPROVAL_FILE"), ("FIXTURE_LOGIN_ACCOUNT_FILE", next.to_str().unwrap()),
        ("FIXTURE_DEVICE_APPROVAL_FILE", approval.to_str().unwrap()), ("OVERSEER_GH", gh.to_str().unwrap())]);
    phone::enable(&d);
    let (mut p, _) = pair(&d, "Everything Phone").await;
    p.send(&json!({"id": 5, "method": "events.subscribe", "params": {"after": 0}})).await.unwrap();

    // Accounts: sign-in state and plan, as the Mac shows them.
    let accounts = p.call("account.list", json!({})).await;
    assert_eq!(accounts, d.call("account.list", json!({})), "the phone's accounts are the daemon's");
    let openai = p.act("account.create", json!({"provider": "openai", "name": "From the phone"})).await;
    assert!(openai.get("error").is_none(), "{openai}");
    let account = openai["result"]["account"]["id"].as_str().unwrap().to_string();
    assert_eq!(p.call("profile.status", json!({"id": account})).await["logged_in"], false);

    // Sign-in with the provider's device code: the address and the code reach the phone, the
    // person approves in a browser, and the account is signed in. No credential reaches the phone.
    std::fs::write(&next, "phone-user:pro").unwrap();
    let login = p.act("profile.device_login", json!({"id": account})).await;
    assert!(login.get("error").is_none(), "{login}");
    assert_eq!(login["result"]["url"], "https://auth.example.invalid/codex/device");
    assert_eq!(login["result"]["code"], "FXTR-C0DE1");
    assert_eq!(login["result"]["finished"], false);
    assert_eq!(d.call("profile.status", json!({"id": account}))["logged_in"], false, "not signed in before the person approves");
    std::fs::write(&approval, "yes").unwrap();
    wait_until("the sign-in", 15, || d.call("profile.status", json!({"id": account}))["logged_in"] == true);
    wait_until("the sign-in event", 10, || events_of(&d, "profile").iter().any(|e| e["payload"]["action"] == "login" && e["payload"]["logged_in"] == true));
    let status = p.call("profile.status", json!({"id": account})).await;
    assert_eq!(status["logged_in"], true);
    assert_eq!(status["identity"]["plan"], "pro");
    let usage = p.ask("account.usage", json!({"id": account}), None).await.unwrap();
    assert!(usage.get("error").is_none() || Phone::code(&usage) == "failed", "{usage}");
    // A provider without a code flow is signed in on the Mac, and says so.
    let claude = p.act("account.create", json!({"provider": "anthropic", "name": "Claude from the phone"})).await;
    let claude_id = claude["result"]["account"]["id"].as_str().unwrap().to_string();
    let on_mac = p.act("profile.device_login", json!({"id": claude_id})).await;
    assert_eq!(Phone::code(&on_mac), "mac_only");
    assert!(on_mac["error"]["message"].as_str().unwrap().starts_with("Sign in on the Mac"), "{on_mac}");
    assert_eq!(Phone::code(&p.act("profile.login_command", json!({"id": claude_id})).await), "mac_only");

    // An agent's work: a worktree with changes.
    let repo = repo(&t.path().join("repo"));
    let bare = t.path().join("github-standin.git");
    assert!(std::process::Command::new("git").args(["init", "-q", "--bare"]).arg(&bare).status().unwrap().success());
    git(&repo, &["remote", "add", "origin", "https://github.com/test-owner/pr-demo.git"]);
    git(&repo, &["config", &format!("url.{}.insteadOf", bare.display()), "https://github.com/test-owner/pr-demo.git"]);
    let (created, ws, _) = edited(&d, &repo, "printf 'notes\\n' > NOTES.md; echo wrote notes");
    let wsid = created["workspace"]["id"].as_str().unwrap().to_string();

    // A pull request, opened by the daemon with the Mac's Git and GitHub CLI.
    let plan = p.call("workspace.pr_plan", json!({"workspace_id": wsid})).await;
    assert_eq!(plan["ok"], true, "{plan}");
    assert_eq!(plan["uncommitted"], json!(["NOTES.md"]));
    let main_before = git(&repo, &["rev-parse", "main"]);
    let opened = p.act("workspace.pr_open", json!({"workspace_id": wsid, "title": "Add notes (from the phone)"})).await;
    assert!(opened.get("error").is_none(), "{opened}");
    assert_eq!(opened["result"]["url"], "https://github.com/test-owner/pr-demo/pull/7");
    assert_eq!(opened["result"]["number"], 7);
    let args: Vec<String> = serde_json::from_str(&std::fs::read_to_string(&gh_log).unwrap()).unwrap();
    let branch = plan["branch"].as_str().unwrap();
    assert_eq!(&args[..10], ["pr", "create", "--repo", "test-owner/pr-demo", "--head", branch, "--base", "main", "--title", "Add notes (from the phone)"]);
    assert!(args[11].contains("Overseer never merges automatically"));
    let pushed = String::from_utf8(std::process::Command::new("git").arg("-C").arg(&bare).args(["log", "--format=%s", "-1", branch]).output().unwrap().stdout).unwrap();
    assert!(!pushed.trim().is_empty(), "the branch was pushed");
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before, "nothing was merged");
    let recorded = events_of(&d, "pull_request");
    assert_eq!(recorded.last().unwrap()["payload"]["url"], "https://github.com/test-owner/pr-demo/pull/7");
    assert_eq!(recorded.last().unwrap()["source"], "phone:Everything Phone");

    // Merge back, step by step, with its plan first.
    let (second, second_ws, _) = edited(&d, &repo, "echo merged-from-the-phone > merged.txt");
    let second_id = second["workspace"]["id"].as_str().unwrap().to_string();
    let merge_plan = p.call("workspace.merge_plan", json!({"workspace_id": second_id})).await;
    assert!(merge_plan.is_object());
    let prepared = p.act("workspace.merge_prepare", json!({"workspace_id": second_id})).await;
    assert!(prepared.get("error").is_none(), "{prepared}");
    assert_eq!(prepared["result"]["state"], "ready", "{prepared}");
    let completed = p.act("workspace.merge_complete", json!({"workspace_id": second_id})).await;
    assert!(completed.get("error").is_none(), "{completed}");
    assert_eq!(std::fs::read_to_string(repo.join("merged.txt")).unwrap(), "merged-from-the-phone\n", "merged into the checkout");
    // Abort is there too.
    let (third, _, _) = edited(&d, &repo, "echo x > third.txt");
    let third_id = third["workspace"]["id"].as_str().unwrap().to_string();
    p.act("workspace.merge_prepare", json!({"workspace_id": third_id})).await;
    assert!(p.act("workspace.merge_abort", json!({"workspace_id": third_id})).await.get("error").is_none());
    assert!(!repo.join("third.txt").exists());

    // Cleaning up lists what would be lost first, and keeps it unless told otherwise.
    std::fs::write(second_ws.join("unsaved-work.txt"), "not committed\n").unwrap();
    let cleanup_plan = p.call("workspace.cleanup_plan", json!({"workspace_id": second_id})).await;
    assert_eq!(cleanup_plan["removable"], true);
    assert!(cleanup_plan["dirty"].to_string().contains("unsaved-work.txt"), "{cleanup_plan}");
    let refused = p.act("workspace.cleanup", json!({"workspace_id": second_id})).await;
    assert!(refused.get("error").is_some() && second_ws.exists(), "uncommitted files are kept: {refused}");
    let cleaned = p.act("workspace.cleanup", json!({"workspace_id": second_id, "discard_dirty": true})).await;
    assert!(cleaned.get("error").is_none(), "{cleaned}");
    assert!(!second_ws.exists() && ws.exists());

    // History, search and archive.
    let found = p.call("search", json!({"query": "wrote notes"})).await;
    assert!(found.to_string().contains(created["task"]["id"].as_str().unwrap()), "{found}");
    assert!(p.act("task.archive", json!({"task_id": created["task"]["id"]})).await.get("error").is_none());
    assert!(!d.call("state", json!({}))["tasks"].as_array().unwrap().iter().find(|t| t["id"] == created["task"]["id"]).unwrap()["archived_ms"].is_null());
    assert!(p.act("task.archive", json!({"task_id": created["task"]["id"], "archived": false})).await.get("error").is_none());
    assert_eq!(p.call("harness.list", json!({})).await, d.call("harness.list", json!({})));
    // Stop all agents; the daemon keeps running.
    let busy = d.generic(&repo, "worktree", "/bin/sh", &["-c", "sleep 30"]);
    d.wait_status(&run_id(&busy), |s| s == "running", 10);
    let stopped = p.act("runs.stop_all", json!({})).await;
    assert_eq!(stopped["result"]["interrupted"], json!([run_id(&busy)]), "{stopped}");
    d.wait_done(&run_id(&busy), 20);
    assert_eq!(d.call("hello", json!({}))["protocol"], 1, "the daemon is still running");

    // No credential ever reached the phone: everything it received, decrypted, is searched.
    while p.next(Duration::from_millis(300)).await.is_some() {}
    let received = p.transcript.clone();
    assert!(received.len() > 20_000, "the audit reads the whole session ({} bytes)", received.len());
    fn find(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = find(&path, name) {
                    return Some(found);
                }
            } else if path.file_name().is_some_and(|n| n == name) {
                return Some(path);
            }
        }
        None
    }
    let auth = std::fs::read_to_string(find(d.home.path(), "auth.json").expect("the signed-in account's credential file")).unwrap();
    let stored: Value = serde_json::from_str(&auth).unwrap();
    let id_token = stored["tokens"]["id_token"].as_str().unwrap();
    assert!(id_token.starts_with("eyJ") && id_token.len() > 40);
    for secret in [id_token, &id_token[..40], "refresh_token", "access_token", "id_token", "auth.json", "\"tokens\"", "Bearer ", "gh auth token", ".fixture-login.json"] {
        assert!(!received.contains(secret), "{secret:?} reached the phone");
    }
    assert!(!received.contains("eyJ"), "no token of any kind reached the phone");
    assert!(received.contains("FXTR-C0DE1") && received.contains("https://auth.example.invalid/codex/device"), "the code and the address did");
}

// ---------------------------------------------------------------- AC-129

fn pushes(dir: &std::path::Path) -> Vec<Value> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    files.sort();
    files.iter().map(|f| serde_json::from_slice(&std::fs::read(f).unwrap()).unwrap()).collect()
}

fn leaves(value: &Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => map.iter().for_each(|(k, v)| leaves(v, &if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") }, out)),
        _ => out.push(prefix.to_string()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac129_needs_you_notifications_you_can_switch() {
    let sent = tmp();
    let modes = tmp();
    let mode_file = modes.path().join("mode");
    let claude = fixture("fake-harness/claude-fixture.js");
    // Test-only: notifications are written to a directory instead of being sent.
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE"), ("CLAUDE_FIXTURE_MODE_FILE", mode_file.to_str().unwrap()),
        ("OVERSEER_GATEWAY_MDNS", "off"), ("OVERSEER_TEST_PUSH_DIR", sent.path().to_str().unwrap())]);
    phone::enable(&d);
    let (mut p, paired) = pair(&d, "Notified Phone").await;
    let r = tmp();
    let repo = repo(&r.path().join("shop"));
    let secret_prompt = "Rotate the key sk-live-00000000000000000000 in src/billing.ts";
    let ask = |mode: &str| {
        std::fs::write(&mode_file, mode).unwrap();
        let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": secret_prompt, "title": secret_prompt}));
        (run_id(&created), created)
    };
    let count = || pushes(sent.path()).len();
    let last_log = |d: &Daemon| events_of(d, "push").last().cloned().unwrap_or(Value::Null);

    // Off until the phone turns them on: nothing is sent, and the log says why.
    let hello = p.call("hello", json!({"client": "phone"})).await;
    assert_eq!(hello["notifications"]["enabled"], false);
    let (quiet, _) = ask("echo");
    d.wait_done(&quiet, 15);
    wait_until("the log entry", 5, || !events_of(&d, "push").is_empty());
    assert_eq!(count(), 0);
    assert_eq!(last_log(&d)["payload"]["why"], "notifications are off on this phone");

    // On: a permission request reaches the phone within five seconds.
    let settings = p.call("device.notifications", json!({"enabled": true, "token": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "environment": "development", "bundle": "com.beelol.overseer.phone"})).await;
    assert_eq!(settings["enabled"], true);
    assert_eq!(settings["kinds"], json!({"permission": true, "question": true, "failure": true, "finished": true}));
    let (run, created) = ask("permission");
    let asked = Instant::now();
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 15);
    wait_until("the notification", 5, || count() == 1);
    assert!(asked.elapsed() < Duration::from_secs(5), "within five seconds: {:?}", asked.elapsed());
    let note = pushes(sent.path()).pop().unwrap();
    assert_eq!(note["device"], json!(paired.device));
    let payload = &note["payload"];
    assert_eq!(payload["aps"]["alert"]["title"], "Claude · shop");
    assert_eq!(payload["aps"]["alert"]["body"], "Needs your permission");
    assert_eq!(payload["aps"]["category"], "OVERSEER_PERMISSION");
    assert_eq!(payload["overseer"]["run_id"], json!(run));
    assert_eq!(payload["overseer"]["request_id"], waiting["attention"]["request_id"]);
    // Only the allowed fields, and nothing of the work: no prompt, no path, no tool input.
    let allowed = ["aps.alert.title", "aps.alert.body", "aps.category", "aps.thread-id", "aps.sound", "aps.interruption-level", "overseer.v", "overseer.kind", "overseer.run_id", "overseer.task_id", "overseer.request_id", "overseer.device"];
    let mut got = Vec::new();
    leaves(payload, "", &mut got);
    assert!(got.iter().all(|f| allowed.contains(&f.as_str())), "{got:?}");
    let text = payload.to_string();
    for private in ["Rotate", "sk-live", "billing", "perm.txt", "allowed", repo.to_str().unwrap(), "Write"] {
        assert!(!text.contains(private), "{private:?} is in the notification: {text}");
    }
    assert_eq!(last_log(&d)["payload"]["outcome"], "sent");
    assert_eq!(last_log(&d)["payload"]["fields"].as_array().unwrap().len(), got.len());

    // Allow, from the notification: the phone answers with what the notification carries.
    let answered = p.act("run.permission", json!({"run_id": payload["overseer"]["run_id"], "request_id": payload["overseer"]["request_id"], "allow": true})).await;
    assert!(answered.get("error").is_none(), "{answered}");
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    assert!(ws_path(&d, &created).join("perm.txt").exists(), "the agent was unblocked");
    wait_until("the finished notification", 5, || count() == 2);
    assert_eq!(pushes(sent.path()).pop().unwrap()["payload"]["aps"]["alert"]["body"], "Finished");
    assert_eq!(pushes(sent.path()).pop().unwrap()["payload"]["aps"]["category"], "OVERSEER_AGENT");

    // One kind off: that kind is not sent, the others are.
    p.call("device.notifications", json!({"kinds": {"finished": false}})).await;
    let (run, _) = ask("permission");
    let w = d.wait_status(&run, |s| s == "waiting_for_user", 15);
    wait_until("the permission notification", 5, || count() == 3);
    d.call("run.permission", json!({"run_id": run, "request_id": w["attention"]["request_id"], "allow": false}));
    d.wait_done(&run, 15);
    wait_until("the log entry", 5, || last_log(&d)["payload"]["kind"] == "finished");
    assert_eq!(last_log(&d)["payload"]["why"], "this kind of notification is off on this phone");
    assert_eq!(count(), 3, "nothing was sent for the kind that is off");
    // A failure is its own kind.
    let (failed, _) = ask("auth");
    d.wait_done(&failed, 15);
    wait_until("the failure notification", 5, || count() == 4);
    assert_eq!(pushes(sent.path()).pop().unwrap()["payload"]["aps"]["alert"]["body"], "Stopped with an error");

    // The Mac's switch turns them off for every phone.
    assert_eq!(d.call("gateway.settings", json!({"notifications": false}))["notifications"], false);
    let (run, _) = ask("permission");
    let w = d.wait_status(&run, |s| s == "waiting_for_user", 15);
    wait_until("the log entry", 5, || last_log(&d)["run_id"] == json!(run));
    assert_eq!(last_log(&d)["payload"]["why"], "notifications to phones are off on the Mac");
    assert_eq!(count(), 4);
    assert_eq!(p.call("hello", json!({"client": "phone"})).await["mac_notifications"], false, "the phone can say why");
    d.call("run.permission", json!({"run_id": run, "request_id": w["attention"]["request_id"], "allow": false}));
    d.wait_done(&run, 15);
    d.call("gateway.settings", json!({"notifications": true}));
    // A phone cannot change the Mac's switch.
    assert_eq!(Phone::code(&p.act("gateway.settings", json!({"notifications": true})).await), "mac_only");

    // While a window on the Mac is looking at the agent, no notification is sent for it.
    p.call("device.notifications", json!({"kinds": {"finished": true}})).await;
    let (watched, _) = ask("slow");
    let mut window = std::os::unix::net::UnixStream::connect(d.socket()).unwrap();
    {
        use std::io::{BufRead, BufReader, Write};
        window.write_all(format!("{}\n", json!({"id": 1, "method": "ui.focus", "params": {"run_id": watched, "focused": true}})).as_bytes()).unwrap();
        let mut line = String::new();
        BufReader::new(window.try_clone().unwrap()).read_line(&mut line).unwrap();
        assert!(line.contains("\"ok\":true"), "{line}");
    }
    d.wait_done(&watched, 20);
    wait_until("the log entry", 5, || last_log(&d)["run_id"] == json!(watched) && last_log(&d)["payload"]["kind"] == "finished");
    assert_eq!(last_log(&d)["payload"]["why"], "the owner is looking at this agent on the Mac");
    assert_eq!(count(), 4);
    // The window looks elsewhere (or closes): the next moment is sent again.
    drop(window);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (unwatched, _) = ask("echo");
    d.wait_done(&unwatched, 15);
    wait_until("the notification", 5, || count() == 5);

    // Text only when the owner asked for it, per phone.
    p.call("device.notifications", json!({"show_text": true})).await;
    let (shown, _) = ask("echo");
    d.wait_done(&shown, 15);
    wait_until("the notification", 5, || count() == 6);
    let with_text = pushes(sent.path()).pop().unwrap();
    assert!(with_text["payload"]["aps"]["alert"]["title"].as_str().unwrap().starts_with("Rotate the key"));
    assert!(with_text["payload"]["aps"]["alert"]["body"].as_str().unwrap().starts_with("Finished: "), "{with_text}");
    assert!(!with_text.to_string().contains("sk-live-0000") && with_text.to_string().contains("[redacted]"), "secrets are still redacted: {with_text}");
    p.call("device.notifications", json!({"show_text": false})).await;

    // All off on the phone, and a revoked phone, get nothing.
    p.call("device.notifications", json!({"enabled": false})).await;
    let (run, _) = ask("echo");
    d.wait_done(&run, 15);
    wait_until("the log entry", 5, || last_log(&d)["run_id"] == json!(run));
    assert_eq!(count(), 6);
    p.call("device.notifications", json!({"enabled": true})).await;
    // A watch-only phone may still choose its own notifications.
    d.call("gateway.device_scope", json!({"id": paired.device, "scope": "watch"}));
    assert_eq!(p.call("device.notifications", json!({"kinds": {"failure": false}})).await["kinds"]["failure"], false);
    d.call("gateway.device_revoke", json!({"id": paired.device}));
    let before = events_of(&d, "push").len();
    let (run, _) = ask("echo");
    d.wait_done(&run, 15);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(count(), 6);
    assert_eq!(events_of(&d, "push").len(), before, "a revoked phone is not considered at all");
    // Every payload that was sent held only the allowed fields.
    for sent in pushes(sent.path()) {
        let mut got = Vec::new();
        leaves(&sent["payload"], "", &mut got);
        assert!(got.iter().all(|f| allowed.contains(&f.as_str())), "{got:?}");
    }
}
