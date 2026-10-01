//! AC-232 (the review shows an agent's committed work) and AC-243 (merging from the agent, and
//! what it reads afterwards), against the real daemon binary, real Git and generic fixture agents.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;

fn sh(d: &Daemon, repo: &Path, script: &str) -> Value {
    d.generic(repo, "worktree", "/bin/sh", &["-c", script])
}
fn ws_id(created: &Value) -> String {
    created["workspace"]["id"].as_str().unwrap().to_string()
}
fn default_option(d: &Daemon, run: &str) -> Value {
    let opts = d.call("comparison.options", json!({"run_id": run}));
    let all: Vec<Value> = opts["options"].as_array().unwrap().iter().filter(|o| o["default"] == true).cloned().collect();
    assert_eq!(all.len(), 1, "exactly one default comparison: {opts}");
    all[0].clone()
}
fn landing(d: &Daemon, ws: &str) -> Value {
    d.call("state", json!({}))["landings"][ws].clone()
}
/// A pre-commit hook in the repository (worktrees share it) that logs each run to `log`.
fn hook(repo: &Path, log: &Path, exit: i32) {
    let hooks = repo.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let file = hooks.join("pre-commit");
    std::fs::write(&file, format!("#!/bin/sh\necho \"pre-commit ran in $(pwd)\" >> '{}'\n{}\n", log.display(), if exit == 0 { "exit 0".to_string() } else { format!("echo 'secret scan: .env must not be committed' >&2\nexit {exit}") })).unwrap();
    std::process::Command::new("chmod").args(["+x", file.to_str().unwrap()]).status().unwrap();
}

const COMMIT_FEATURES: &str = "mkdir -p data && printf 'export const features = [];\\nexport default features;\\n' > data/features.js && git add -A && git -c user.name=A -c user.email=a@x.invalid commit -qm 'add features' || true";

/// The owner's report: the review said "0 files" while the agent's committed data/features.js was
/// on screen with no diff. The agent committed it in one turn; a later turn changed nothing, and
/// the review opened on that latest turn. A finished agent's review now opens on everything it did.
#[test]
fn ac232_a_finished_agents_committed_new_file_is_in_its_default_review() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, COMMIT_FEATURES);
    let run = run_id(&created);
    d.wait_done(&run, 20);
    // A second turn that changes nothing (the generic agent runs its script again; the commit is already there).
    d.call("run.follow_up", json!({"run_id": run, "prompt": ""}));
    d.wait_done(&run, 20);
    let latest = option(&d, &run, "latest_run", None);
    assert_eq!(diff_paths(&d, &created, latest["base"].as_str().unwrap()), vec![], "the latest turn alone changed nothing");
    let chosen = default_option(&d, &run);
    assert_eq!(chosen["mode"], "task_start", "a finished agent's review opens on everything it did: {chosen}");
    assert_eq!(diff_paths(&d, &created, chosen["base"].as_str().unwrap()), vec![("A".into(), "data/features.js".into())], "the committed new file is added");
    // The same count the chat's Review button gives.
    let changes = d.call("workspace.changes", json!({"workspace_id": ws_id(&created)}));
    assert_eq!((changes["files"].as_u64(), changes["added"].as_u64()), (Some(1), Some(2)), "{changes}");
    // While an agent works, its review opens on the latest run (the owner's launch default).
    let busy = sh(&d, &repo, "sleep 30");
    d.wait_status(&run_id(&busy), |s| s == "running", 20);
    assert_eq!(default_option(&d, &run_id(&busy))["mode"], "latest_run");
    d.call("run.interrupt", json!({"run_id": run_id(&busy)}));
    // In the owner's own checkout too (AC-263, the owner's decision of 2026-09-29).
    let current = d.generic(&repo, "current", "/bin/sh", &["-c", "true"]);
    d.wait_done(&run_id(&current), 20);
    assert_eq!(default_option(&d, &run_id(&current))["mode"], "task_start");
}

/// AC-263: the review opens on Since task start whether the agent works in its own worktree or in
/// the owner's checkout; Latest run and Entire worktree (the branch against where it started,
/// with its uncommitted and untracked files) are there beside it, each with its own files.
#[test]
fn ac263_the_review_opens_on_since_task_start_with_latest_run_and_entire_worktree_one_click_away() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    // Its own worktree: turn 1 commits a file, turn 2 leaves an untracked one.
    let created = sh(&d, &repo, "if [ -f one.txt ]; then printf 'two\\n' > two.txt; else printf 'one\\n' > one.txt && git add one.txt && git -c user.name=A -c user.email=a@x.invalid commit -qm one; fi");
    let run = run_id(&created);
    d.wait_done(&run, 20);
    d.call("run.follow_up", json!({"run_id": run, "prompt": ""}));
    d.wait_done(&run, 20);
    let opts = d.call("comparison.options", json!({"run_id": run}));
    assert_eq!(opts["folder_edits"], false, "its own worktree holds only the agent's work: {opts}");
    assert_eq!(default_option(&d, &run)["mode"], "task_start");
    let base = |mode: &str| option(&d, &run, mode, None)["base"].as_str().unwrap().to_string();
    assert_eq!(diff_paths(&d, &created, &base("task_start")), vec![("A".into(), "one.txt".into()), ("A".into(), "two.txt".into())]);
    assert_eq!(diff_paths(&d, &created, &base("latest_run")), vec![("A".into(), "two.txt".into())], "the latest run is turn 2 alone");
    let entire = option(&d, &run, "entire_worktree", None);
    assert_eq!((entire["label"].as_str(), entire["available"].as_bool(), entire["provenance"].as_str()), (Some("Entire worktree"), Some(true), Some("recorded")), "{entire}");
    assert_eq!(entire["base"].as_str().unwrap(), git(&repo, &["rev-parse", "main"]), "the branch against the commit it started from");
    // Main moves on: the worktree's comparison still starts where its branch did.
    std::fs::write(repo.join("main-later.txt"), "later\n").unwrap();
    git(&repo, &["add", "main-later.txt"]);
    git(&repo, &["commit", "-qm", "main later"]);
    assert_eq!(diff_paths(&d, &created, &base("entire_worktree")), vec![("A".into(), "one.txt".into()), ("A".into(), "two.txt".into())]);

    // The owner's checkout, on a feature branch with a commit of its own and an edit left before the task.
    git(&repo, &["switch", "-q", "-c", "feature"]);
    std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
    git(&repo, &["add", "feature.txt"]);
    git(&repo, &["commit", "-qm", "feature"]);
    std::fs::write(repo.join("a.txt"), "owner edit before the task\n").unwrap();
    let current = d.generic(&repo, "current", "/bin/sh", &["-c", "if [ -f c2.txt ]; then :; elif [ -f c1.txt ]; then printf 'c2\\n' > c2.txt; else printf 'c1\\n' > c1.txt; fi"]);
    let crun = run_id(&current);
    d.wait_done(&crun, 20);
    // The owner edits the folder between turns.
    std::fs::write(repo.join("owner.txt"), "the owner's own edit\n").unwrap();
    d.call("run.follow_up", json!({"run_id": crun, "prompt": ""}));
    d.wait_done(&crun, 20);
    let copts = d.call("comparison.options", json!({"run_id": crun}));
    assert_eq!(copts["folder_edits"], true, "the review says the owner's checkout holds any edits made there: {copts}");
    assert_eq!(default_option(&d, &crun)["mode"], "task_start");
    let cbase = |mode: &str| option(&d, &crun, mode, None)["base"].as_str().unwrap().to_string();
    assert_eq!(diff_paths(&d, &current, &cbase("task_start")), vec![("A".into(), "c1.txt".into()), ("A".into(), "c2.txt".into()), ("A".into(), "owner.txt".into())], "since the task started, with the owner's edit between turns; not the edit made before it");
    assert_eq!(diff_paths(&d, &current, &cbase("latest_run")), vec![("A".into(), "c2.txt".into())]);
    let centire = option(&d, &crun, "entire_worktree", None);
    assert_eq!(centire["base"].as_str().unwrap(), git(&repo, &["rev-parse", "main"]), "the feature branch against where it left main: {centire}");
    assert_eq!(centire["label"], "Entire worktree");
    assert_eq!(diff_paths(&d, &current, &cbase("entire_worktree")), vec![("A".into(), "c1.txt".into()), ("A".into(), "c2.txt".into()), ("A".into(), "feature.txt".into()), ("A".into(), "owner.txt".into()), ("M".into(), "a.txt".into())]);
}

/// AC-232: Open PR needs a GitHub remote; the plan says which remote there is, so a repository
/// without one is offered the local merge instead.
#[test]
fn ac232_the_merge_plan_says_whether_there_is_a_github_remote() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "printf 'agent\\n' > b.txt");
    d.wait_done(&run_id(&created), 20);
    let id = ws_id(&created);
    let plan = d.call("workspace.merge_plan", json!({"workspace_id": id}));
    assert_eq!(plan["ok"], true, "{plan}");
    assert!(plan["remote"].is_null(), "no remote: {plan}");
    assert!(d.call("workspace.pr_plan", json!({"workspace_id": id}))["reason"].as_str().unwrap().contains("no Git remote"));
    git(&repo, &["remote", "add", "origin", "https://gitlab.example.invalid/a/b.git"]);
    let plan = d.call("workspace.merge_plan", json!({"workspace_id": id}));
    assert_eq!((plan["remote"]["name"].as_str(), plan["remote"]["github"].as_bool()), (Some("origin"), Some(false)), "{plan}");
    git(&repo, &["remote", "set-url", "origin", "git@github.com:test-owner/test-repo.git"]);
    let plan = d.call("workspace.merge_plan", json!({"workspace_id": id}));
    assert_eq!((plan["remote"]["github"].as_bool(), plan["remote"]["owner"].as_str()), (Some(true), Some("test-owner")), "{plan}");
}

/// AC-243: the confirmation lists every file that lands, the untracked ones (a stray .env) apart;
/// the repository's pre-commit hook runs; afterwards the state reads merged, with the commit.
#[test]
fn ac243_merge_lists_the_files_and_untracked_ones_runs_hooks_and_reads_merged() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let log = r.path().join("hook.log");
    hook(&repo, &log, 0);
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, &format!("{COMMIT_FEATURES}; printf 'a\\nagent line\\n' > a.txt; printf 'TOKEN=not-a-secret\\n' > .env"));
    let run = run_id(&created);
    d.wait_done(&run, 20);
    let id = ws_id(&created);
    let plan = d.call("workspace.merge_plan", json!({"workspace_id": id}));
    assert_eq!(plan["ok"], true, "{plan}");
    let mut files: Vec<(String, String)> = plan["files"].as_array().unwrap().iter().map(|f| (f["status"].as_str().unwrap().into(), f["path"].as_str().unwrap().into())).collect();
    files.sort();
    assert_eq!(files, vec![("A".into(), ".env".into()), ("A".into(), "data/features.js".into()), ("M".into(), "a.txt".into())], "committed and uncommitted work that lands");
    assert_eq!(plan["untracked"], json!([".env"]), "untracked files are listed before they are committed");
    let hook_runs = || std::fs::read_to_string(&log).unwrap_or_default().lines().count();
    let before = hook_runs();
    assert_eq!(d.call("workspace.merge_prepare", json!({"workspace_id": id}))["state"], "ready");
    assert_eq!(hook_runs(), before + 1, "the pre-commit hook ran for the worktree commit");
    let done = d.call("workspace.merge_complete", json!({"workspace_id": id}));
    assert_eq!(done["merged"], true, "{done}");
    let commit = git(&repo, &["rev-parse", "main"]);
    let l = landing(&d, &id);
    assert_eq!((l["state"].as_str(), l["target"].as_str(), l["commit"].as_str()), (Some("merged"), Some("main"), Some(commit.as_str())), "{l}");
    assert!(repo.join("data/features.js").exists() && repo.join(".env").exists());
    // The plan afterwards: nothing to merge, and it says it was merged.
    let again = d.call("workspace.merge_plan", json!({"workspace_id": id}));
    assert_eq!((again["ok"].as_bool(), again["landing"]["state"].as_str()), (Some(false), Some("merged")), "{again}");
    // Once the agent works again, it no longer reads merged.
    d.call("run.follow_up", json!({"run_id": run, "prompt": ""}));
    d.wait_done(&run, 20);
    assert!(landing(&d, &id).is_null(), "a new turn after the merge clears it");
}

/// AC-243: a hook that refuses the commit stops the merge with its words, and the work stays uncommitted.
#[test]
fn ac243_a_refusing_pre_commit_hook_stops_the_merge_and_keeps_the_work() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let log = r.path().join("hook.log");
    hook(&repo, &log, 1);
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "printf 'TOKEN=x\\n' > .env");
    d.wait_done(&run_id(&created), 20);
    let id = ws_id(&created);
    let ws = ws_path(&d, &created);
    let head = git(&ws, &["rev-parse", "HEAD"]);
    let err = d.try_call("workspace.merge_prepare", json!({"workspace_id": id})).unwrap_err();
    assert!(err.contains("secret scan") && err.contains("hook"), "{err}");
    assert_eq!(git(&ws, &["rev-parse", "HEAD"]), head, "nothing committed");
    assert_eq!(git(&ws, &["status", "--porcelain"]), "?? .env", "the file is untracked again");
    assert!(std::fs::read_to_string(&log).unwrap().contains("pre-commit ran"));
}

/// AC-243: a merge back stopped on conflicts reads so everywhere; Open PR refuses the worktree in
/// the middle of it (conflicted or resolved-but-unfinished); Cancel restores the worktree as it
/// was before the merge, the agent's work uncommitted again.
#[test]
fn ac243_cancel_restores_the_pre_merge_worktree_and_open_pr_refuses_mid_merge() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    git(&repo, &["remote", "add", "origin", "https://github.com/test-owner/test-repo.git"]);
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "printf 'agent version\\n' > a.txt; printf 'new\\n' > notes.txt");
    d.wait_done(&run_id(&created), 20);
    std::fs::write(repo.join("a.txt"), "main version\n").unwrap();
    git(&repo, &["commit", "-qam", "main edit"]);
    let id = ws_id(&created);
    let ws = ws_path(&d, &created);
    let before = fingerprint(&ws);
    let prep = d.call("workspace.merge_prepare", json!({"workspace_id": id, "handoff": false}));
    assert_eq!(prep["state"], "conflicts", "{prep}");
    let l = landing(&d, &id);
    assert_eq!((l["state"].as_str(), l["files"].clone()), (Some("conflicts"), json!(["a.txt"])), "{l}");
    let pr = d.call("workspace.pr_plan", json!({"workspace_id": id}));
    assert_eq!(pr["ok"], false);
    assert!(pr["reason"].as_str().unwrap().contains("unfinished"), "{pr}");
    // Resolved (no markers left) but the merge commit not made: still refused.
    std::fs::write(ws.join("a.txt"), "main version\nagent version\n").unwrap();
    git(&ws, &["add", "a.txt"]);
    let pr = d.call("workspace.pr_plan", json!({"workspace_id": id}));
    assert_eq!(pr["ok"], false);
    assert!(pr["reason"].as_str().unwrap().contains("still in progress"), "{pr}");
    let cancelled = d.call("workspace.merge_abort", json!({"workspace_id": id}));
    assert_eq!((cancelled["was_merging"].as_bool(), cancelled["uncommitted"].as_bool()), (Some(true), Some(true)), "{cancelled}");
    assert_eq!(fingerprint(&ws), before, "the worktree is as it was before the merge");
    assert!(landing(&d, &id).is_null(), "no longer reads as a stopped merge");
    assert!(d.events(&run_id(&created)).iter().any(|e| e["kind"] == "merge_back" && e["payload"]["state"] == "cancelled"));
    // And it can be merged again from the start.
    assert_eq!(d.call("workspace.merge_plan", json!({"workspace_id": id}))["state"], "idle");
}
