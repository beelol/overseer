mod common;

use common::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

fn accepted_patch(d: &Daemon, run: &str, job: &str, artifact: &str, patch: &str) {
    let attempt = d.call(
        "swarm.attempt.register",
        json!({
            "run_id":run,"generation":1,"revision":1,"job_id":job
        }),
    );
    d.call(
        "swarm.artifact.put",
        json!({
            "run_id":run,"job_id":job,"attempt_id":attempt["id"],"token":attempt["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"patch","content":patch
        }),
    );
    d.call(
        "swarm.report",
        json!({
            "run_id":run,"job_id":job,"attempt_id":attempt["id"],"token":attempt["token"],
            "message_id":format!("result-{job}"),"type":"result","revision":1,
            "payload":{"artifact_ids":[artifact]}
        }),
    );
    d.call(
        "swarm.decide",
        json!({
            "run_id":run,"generation":1,"revision":1,"job_id":job,
            "decision":"accept","evidence":[artifact]
        }),
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({
            "run_id":run,"generation":1,"revision":1,"job_id":job,
            "attempt_id":attempt["id"]
        }),
    );
}

#[test]
fn audit_only_run_cannot_integrate_a_worker_patch() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("audit-source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("a.txt"), "unauthorized change\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    let before = fingerprint(&checkout);
    assert!(d
        .try_call(
            "swarm.create",
            json!({"category":"Invalid source permission",
        "objective":"Audit", "allowed_targets":["system-codex"],
        "source_change_permission":"current_checkout"})
        )
        .unwrap_err()
        .contains("invalid source change permission"));
    let made = d.call(
        "swarm.create",
        json!({"category":"Audit-only source guard",
        "objective":"Audit a.txt without changing application code",
        "allowed_targets":["system-codex"]}),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
        "source_change_permission":"isolated",
        "jobs":[{"id":"audit","title":"Inspect a.txt","acceptance":"finding","deps":[]}]}),
    );
    accepted_patch(&d, run, "audit", "unauthorized-patch", &patch);
    d.kill9();
    d.spawn();
    let error = d
        .try_call(
            "swarm.integrate",
            json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"audit","artifact_id":"unauthorized-patch",
        "repo":checkout,"base_revision":base}),
        )
        .unwrap_err();
    assert!(
        error.contains("source changes are not permitted"),
        "{error}"
    );
    assert_eq!(fingerprint(&checkout), before);
    assert_eq!(
        d.call("swarm.get", json!({"id":run}))["source_change_permission"],
        "none"
    );
}

#[test]
fn accepted_patch_integrates_in_isolated_workspace_without_touching_checkout() {
    let d = Daemon::start(&[]);
    let t = tmp();
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("a.txt"), "changed\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    std::fs::write(checkout.join("b.txt"), "user edit\n").unwrap();
    let original = fingerprint(&checkout);

    let made = d.call(
        "swarm.create",
        json!({
            "category":"Integration fixture","objective":"Change a.txt",
            "allowed_targets":["system-codex"],"source_change_permission":"isolated"
        }),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
            "jobs":[{"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}]
        }),
    );
    accepted_patch(&d, run, "writer", "writer-patch", &patch);
    let request = json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"writer","artifact_id":"writer-patch", "repo":checkout,
        "base_revision":base});
    let integrated = d.call("swarm.integrate", request.clone());
    assert_eq!(integrated["status"], "integrated", "{integrated}");
    let workspace = std::path::PathBuf::from(integrated["workspace_path"].as_str().unwrap());
    assert!(workspace.starts_with(std::fs::canonicalize(d.home.path()).unwrap()));
    assert_eq!(
        std::fs::read_to_string(workspace.join("a.txt")).unwrap(),
        "changed\n"
    );
    assert_eq!(fingerprint(&checkout), original);
    let replay = d.call("swarm.integrate", request);
    assert_eq!(replay["duplicate"], true);
    assert_eq!(replay["commit"], integrated["commit"]);
}

#[test]
fn source_commit_change_blocks_stale_patch_integration() {
    let d = Daemon::start(&[]);
    let t = tmp();
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("a.txt"), "worker change\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    let made = d.call(
        "swarm.create",
        json!({
            "category":"Stale integration fixture","objective":"Change a.txt",
            "allowed_targets":["system-codex"],"source_change_permission":"isolated"
        }),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
            "jobs":[{"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}]
        }),
    );
    accepted_patch(&d, run, "writer", "writer-patch", &patch);
    std::fs::write(checkout.join("a.txt"), "upstream change\n").unwrap();
    git(&checkout, &["add", "a.txt"]);
    git(&checkout, &["commit", "-q", "-m", "upstream change"]);
    let current = git(&checkout, &["rev-parse", "HEAD"]);
    let error = d
        .try_call(
            "swarm.integrate",
            json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"writer","artifact_id":"writer-patch",
        "repo":checkout,"base_revision":base}),
        )
        .unwrap_err();
    assert!(error.contains("source commit changed"), "{error}");
    assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), current);
    assert_eq!(
        std::fs::read_to_string(checkout.join("a.txt")).unwrap(),
        "upstream change\n"
    );
}

#[test]
fn dependent_job_waits_for_accepted_patch_to_integrate() {
    let d = Daemon::start(&[]);
    let t = tmp();
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("a.txt"), "contract v2\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    let made = d.call(
        "swarm.create",
        json!({
            "category":"Dependency integration fixture","objective":"Change contract",
            "allowed_targets":["system-codex"],"source_change_permission":"isolated"
        }),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
            "jobs":[
                {"id":"contract","title":"Change contract","acceptance":"patch","deps":[]},
                {"id":"consumer","title":"Use contract","acceptance":"test","deps":["contract"]}
            ]
        }),
    );
    accepted_patch(&d, run, "contract", "contract-patch", &patch);
    let before = d.call("swarm.jobs", json!({"id":run}));
    let consumer_before = before["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == "consumer")
        .unwrap();
    assert_eq!(consumer_before["status"], "planned", "{before}");
    d.call(
        "swarm.integrate",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"contract","artifact_id":"contract-patch","repo":checkout,
        "base_revision":base}),
    );
    let after = d.call("swarm.jobs", json!({"id":run}));
    let consumer_after = after["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == "consumer")
        .unwrap();
    assert_eq!(consumer_after["status"], "ready", "{after}");
}

#[test]
fn completion_rejects_an_accepted_but_unintegrated_patch() {
    let d = Daemon::start(&[]);
    let t = tmp();
    let checkout = repo(&t.path().join("source"));
    std::fs::write(checkout.join("a.txt"), "new\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    let made = d.call(
        "swarm.create",
        json!({
            "category":"Completion integration fixture","objective":"Change a.txt",
            "allowed_targets":["system-codex"],"source_change_permission":"isolated"
        }),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
            "jobs":[{"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}]
        }),
    );
    accepted_patch(&d, run, "writer", "writer-patch", &patch);
    d.call(
        "swarm.ack",
        json!({"run_id":run,"message_id":"result-writer",
        "recipient":"director","generation":1,"revision":1,"phase":"applied"}),
    );
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap()
        .execute("UPDATE swarm_runs SET status='running' WHERE id=?1", [run])
        .unwrap();
    let error = d
        .try_call(
            "swarm.complete",
            json!({
                "run_id":run,"generation":1,"revision":1,"request_id":"finish-before-integration",
                "summary":"Change complete","verification":"fixture",
                "checks":[{"job_id":"writer","outcome":"passed","evidence":["writer-patch"]}]
            }),
        )
        .unwrap_err();
    assert!(error.contains("unintegrated patch"), "{error}");
}

#[test]
fn conflicting_accepted_patches_preserve_first_commit_and_second_artifact() {
    let d = Daemon::start(&[]);
    let t = tmp();
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    let make_patch = |value: &str| {
        std::fs::write(checkout.join("a.txt"), format!("{value}\n")).unwrap();
        let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
        std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
        patch
    };
    let first_patch = make_patch("first");
    let second_patch = make_patch("second");
    let original = fingerprint(&checkout);
    let made = d.call(
        "swarm.create",
        json!({"category":"Conflict integration fixture",
        "objective":"Change a.txt","allowed_targets":["system-codex"],"source_change_permission":"isolated"}),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"one","title":"First change","acceptance":"patch","deps":[]},
            {"id":"two","title":"Second change","acceptance":"patch","deps":[]}
        ]}),
    );
    accepted_patch(&d, run, "one", "patch-one", &first_patch);
    accepted_patch(&d, run, "two", "patch-two", &second_patch);
    let request = |job: &str, artifact: &str| {
        json!({"run_id":run,"generation":1,
        "revision":1,"job_id":job,"artifact_id":artifact,"repo":checkout,
        "base_revision":base})
    };
    let first = d.call("swarm.integrate", request("one", "patch-one"));
    let error = d
        .try_call("swarm.integrate", request("two", "patch-two"))
        .unwrap_err();
    assert!(error.contains("git apply"), "{error}");
    let workspace = std::path::PathBuf::from(first["workspace_path"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(workspace.join("a.txt")).unwrap(),
        "first\n"
    );
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"]), first["commit"]);
    assert_eq!(fingerprint(&checkout), original);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let remaining: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_integrated_artifacts
        WHERE run_id=?1",
            [run],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 1);
    let saved: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1
        AND id='patch-two'",
            [run],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(saved, 1);
}

#[test]
fn active_repository_commit_hook_blocks_integration_without_running() {
    let d = Daemon::start(&[]);
    let t = tmp();
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("a.txt"), "new\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    let made = d.call(
        "swarm.create",
        json!({"category":"Commit hook fixture",
        "objective":"Change a.txt","allowed_targets":["system-codex"],"source_change_permission":"isolated"}),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}
        ]}),
    );
    accepted_patch(&d, run, "writer", "writer-patch", &patch);
    let hook_marker = t.path().join("hook-fired");
    let hook = checkout.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\nprintf invoked > '{}'\n", hook_marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = d
        .try_call(
            "swarm.integrate",
            json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"writer","artifact_id":"writer-patch",
        "repo":checkout,"base_revision":base}),
        )
        .unwrap_err();
    assert!(error.contains("commit hook"), "{error}");
    assert!(!hook_marker.exists());
    assert_eq!(
        std::fs::read_to_string(checkout.join("a.txt")).unwrap(),
        "a\n"
    );
}

#[test]
fn interrupted_integration_recovers_without_a_second_commit() {
    for fault in ["after_apply", "after_commit"] {
        let mut d = Daemon::start(&[]);
        let t = tmp();
        let checkout = repo(&t.path().join("source"));
        let base = git(&checkout, &["rev-parse", "HEAD"]);
        std::fs::write(checkout.join("a.txt"), "recovered\n").unwrap();
        let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
        std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
        let made = d.call(
            "swarm.create",
            json!({
                "category":"Recovery fixture", "objective":"Change a.txt",
                "allowed_targets":["system-codex"],"source_change_permission":"isolated"
            }),
        );
        let run = made["id"].as_str().unwrap();
        d.call(
            "swarm.plan",
            json!({"id":run,"generation":1,"revision":0,
                "jobs":[{"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}]
            }),
        );
        accepted_patch(&d, run, "writer", "writer-patch", &patch);
        let request = json!({"run_id":run,"generation":1,"revision":1,
            "job_id":"writer","artifact_id":"writer-patch","repo":checkout,
            "base_revision":base});
        let mut interrupted = request.clone();
        interrupted["fixture_fault"] = json!(fault);
        let error = d.try_call("swarm.integrate", interrupted).unwrap_err();
        assert!(error.contains("fixture interruption"), "{fault}: {error}");
        let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
        let workspace: String = db
            .query_row(
                "SELECT workspace_path FROM swarm_integrations WHERE run_id=?1",
                [run],
                |r| r.get(0),
            )
            .unwrap();
        let workspace = std::path::PathBuf::from(workspace);
        let commit_before = git(&workspace, &["rev-parse", "HEAD"]);
        if fault == "after_commit" {
            std::fs::write(checkout.join("a.txt"), "upstream after integration\n").unwrap();
            git(&checkout, &["add", "a.txt"]);
            git(
                &checkout,
                &["commit", "-q", "-m", "upstream after integration"],
            );
        }
        let source_before_replay = fingerprint(&checkout);
        d.kill9();
        d.spawn();
        let integrated = d.call("swarm.integrate", request.clone());
        assert_eq!(integrated["status"], "integrated", "{fault}: {integrated}");
        let commit_after = git(&workspace, &["rev-parse", "HEAD"]);
        if fault == "after_commit" {
            assert_eq!(commit_before, commit_after);
        } else {
            assert_ne!(commit_before, commit_after);
        }
        assert_eq!(integrated["commit"], commit_after);
        assert_eq!(
            git(
                &workspace,
                &["rev-list", "--count", &format!("{base}..HEAD")]
            ),
            "1"
        );
        assert_eq!(fingerprint(&checkout), source_before_replay);
        let duplicate = d.call("swarm.integrate", request);
        assert_eq!(duplicate["duplicate"], true);
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM swarm_integrated_artifacts WHERE run_id=?1",
                [run],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}

#[test]
fn interrupted_integration_rejects_unexpected_workspace_edits() {
    let mut d = Daemon::start(&[]);
    let t = tmp();
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("a.txt"), "reviewed\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    let made = d.call(
        "swarm.create",
        json!({"category":"Tamper fixture",
        "objective":"Change a.txt","allowed_targets":["system-codex"],"source_change_permission":"isolated"}),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
        "jobs":[{"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}]}),
    );
    accepted_patch(&d, run, "writer", "writer-patch", &patch);
    let request = json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"writer","artifact_id":"writer-patch","repo":checkout,
        "base_revision":base});
    let mut interrupted = request.clone();
    interrupted["fixture_fault"] = json!("after_apply");
    assert!(d
        .try_call("swarm.integrate", interrupted)
        .unwrap_err()
        .contains("fixture interruption"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let workspace: String = db
        .query_row(
            "SELECT workspace_path FROM swarm_integrations WHERE run_id=?1",
            [run],
            |r| r.get(0),
        )
        .unwrap();
    std::fs::write(std::path::Path::new(&workspace).join("a.txt"), "intruder\n").unwrap();
    d.kill9();
    d.spawn();
    let error = d.try_call("swarm.integrate", request).unwrap_err();
    assert!(error.contains("requires reconciliation"), "{error}");
    assert_eq!(
        git(std::path::Path::new(&workspace), &["rev-parse", "HEAD"]),
        base
    );
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_integrated_artifacts WHERE run_id=?1",
            [run],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn individually_accepted_patches_cannot_complete_after_combined_check_fails() {
    let t = tmp();
    let verifier = t.path().join("verify-combination.py");
    std::fs::write(&verifier, "#!/usr/bin/env python3\nfrom pathlib import Path\na = Path('a.txt').read_text().strip()\nb = Path('b.txt').read_text().strip()\nraise SystemExit(1 if a == 'first' and b == 'second' else 0)\n").unwrap();
    std::fs::set_permissions(&verifier, std::fs::Permissions::from_mode(0o755)).unwrap();
    let d = Daemon::start(&[("OVERSEER_SWARM_VERIFIER_PATH", verifier.to_str().unwrap())]);
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    let make_patch = |name: &str, content: &str, original: &str| {
        std::fs::write(checkout.join(name), content).unwrap();
        assert!(
            std::process::Command::new(&verifier)
                .current_dir(&checkout)
                .status()
                .unwrap()
                .success(),
            "individual module check failed for {name}"
        );
        let patch = format!("{}\n", git(&checkout, &["diff", "--", name]));
        std::fs::write(checkout.join(name), original).unwrap();
        patch
    };
    let first_patch = make_patch("a.txt", "first\n", "a\n");
    let second_patch = make_patch("b.txt", "second\n", "b\n");
    let source_before = fingerprint(&checkout);
    let made = d.call(
        "swarm.create",
        json!({"category":"Combined verification fixture",
        "objective":"Change two modules", "allowed_targets":["system-codex"],"source_change_permission":"isolated"}),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"one","title":"Module one","acceptance":"patch","deps":[]},
            {"id":"two","title":"Module two","acceptance":"patch","deps":[]}
        ]}),
    );
    accepted_patch(&d, run, "one", "patch-one", &first_patch);
    accepted_patch(&d, run, "two", "patch-two", &second_patch);
    let integrate = |job: &str, artifact: &str| {
        json!({"run_id":run,"generation":1,
        "revision":1,"job_id":job,"artifact_id":artifact,"repo":checkout,
        "base_revision":base})
    };
    let first = d.call("swarm.integrate", integrate("one", "patch-one"));
    let before = d.call(
        "swarm.verify",
        json!({"run_id":run,"generation":1,"revision":1,
        "request_id":"check-first-commit"}),
    );
    assert_eq!(before["status"], "passed", "{before}");
    assert_eq!(before["commit"], first["commit"]);
    let second = d.call("swarm.integrate", integrate("two", "patch-two"));
    let after = d.call(
        "swarm.verify",
        json!({"run_id":run,"generation":1,"revision":1,
        "request_id":"check-second-commit"}),
    );
    assert_eq!(after["status"], "failed", "{after}");
    assert_eq!(after["commit"], second["commit"]);
    for message in ["result-one", "result-two"] {
        d.call(
            "swarm.ack",
            json!({"run_id":run,"message_id":message,
            "recipient":"director","generation":1,"revision":1,"phase":"applied"}),
        );
    }
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap()
        .execute("UPDATE swarm_runs SET status='running' WHERE id=?1", [run])
        .unwrap();
    let error = d
        .try_call(
            "swarm.complete",
            json!({"run_id":run,"generation":1,
            "revision":1,"request_id":"complete-combined", "summary":"Both modules updated",
            "verification":"Combined check", "checks":[
                {"job_id":"one","outcome":"passed","evidence":["patch-one"]},
                {"job_id":"two","outcome":"passed","evidence":["patch-two"]}
            ]}),
        )
        .unwrap_err();
    assert!(error.contains("combined verification"), "{error}");
    assert_eq!(fingerprint(&checkout), source_before);
    assert_eq!(d.call("swarm.get", json!({"id":run}))["status"], "running");
}

#[test]
fn completion_requires_a_current_passed_combined_check() {
    let t = tmp();
    let verifier = t.path().join("verify-module.py");
    let script = "#!/usr/bin/env python3\nfrom pathlib import Path\nraise SystemExit(0 if Path('a.txt').read_text() == 'changed\\n' else 1)\n";
    std::fs::write(&verifier, script).unwrap();
    std::fs::set_permissions(&verifier, std::fs::Permissions::from_mode(0o755)).unwrap();
    let d = Daemon::start(&[("OVERSEER_SWARM_VERIFIER_PATH", verifier.to_str().unwrap())]);
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("a.txt"), "changed\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    let made = d.call(
        "swarm.create",
        json!({"category":"Passing combined check",
        "objective":"Change a.txt", "allowed_targets":["system-codex"],"source_change_permission":"isolated"}),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
        "jobs":[{"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}]}),
    );
    accepted_patch(&d, run, "writer", "writer-patch", &patch);
    d.call(
        "swarm.integrate",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"writer","artifact_id":"writer-patch","repo":checkout,
        "base_revision":base}),
    );
    d.call(
        "swarm.ack",
        json!({"run_id":run,"message_id":"result-writer",
        "recipient":"director","generation":1,"revision":1,"phase":"applied"}),
    );
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap()
        .execute("UPDATE swarm_runs SET status='running' WHERE id=?1", [run])
        .unwrap();
    let completion = json!({"run_id":run,"generation":1,"revision":1,
    "request_id":"finish-verified-patch","summary":"Patch verified",
    "verification":"Combined a.txt check passed","checks":[
        {"job_id":"writer","outcome":"passed","evidence":["writer-patch"]}
    ]});
    assert!(d
        .try_call("swarm.complete", completion.clone())
        .unwrap_err()
        .contains("combined verification"));
    let request = json!({"run_id":run,"generation":1,"revision":1,
        "request_id":"check-current-commit"});
    let result = d.call("swarm.verify", request.clone());
    assert_eq!(result["status"], "passed", "{result}");
    assert_eq!(d.call("swarm.verify", request)["duplicate"], true);
    std::fs::write(&verifier, format!("{script}# changed verifier\n")).unwrap();
    assert!(d
        .try_call("swarm.complete", completion.clone())
        .unwrap_err()
        .contains("combined verification"));
    std::fs::write(&verifier, script).unwrap();
    assert_eq!(d.call("swarm.complete", completion)["status"], "completed");
}

#[test]
fn stop_remains_responsive_while_combined_checker_is_running() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    let t = tmp();
    let verifier = t.path().join("slow-check.py");
    std::fs::write(
        &verifier,
        "#!/usr/bin/env python3\nimport time\ntime.sleep(2)\n",
    )
    .unwrap();
    std::fs::set_permissions(&verifier, std::fs::Permissions::from_mode(0o755)).unwrap();
    let d = Daemon::start(&[("OVERSEER_SWARM_VERIFIER_PATH", verifier.to_str().unwrap())]);
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("a.txt"), "changed\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    let made = d.call(
        "swarm.create",
        json!({"category":"Responsive verification",
        "objective":"Check a.txt", "allowed_targets":["system-codex"],"source_change_permission":"isolated"}),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
        "jobs":[{"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}]}),
    );
    accepted_patch(&d, run, "writer", "writer-patch", &patch);
    d.call(
        "swarm.integrate",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"writer","artifact_id":"writer-patch","repo":checkout,
        "base_revision":base}),
    );
    let socket = d.socket();
    let run_owned = run.to_string();
    let checker = std::thread::spawn(move || {
        let mut conn = UnixStream::connect(socket).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        writeln!(
            conn,
            "{}",
            json!({"id":1,"method":"swarm.verify","params":{
            "run_id":run_owned,"generation":1,"revision":1,"request_id":"slow-check"}})
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(conn).read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    });
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let started: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM swarm_verifications WHERE run_id=?1 AND status='running'",
                [run],
                |r| r.get(0),
            )
            .unwrap();
        if started == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "verifier did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let overlapping = d
        .try_call(
            "swarm.verify",
            json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"overlapping-check"}),
        )
        .unwrap_err();
    assert!(overlapping.contains("already running"), "{overlapping}");
    let begin = Instant::now();
    let stopped = d.call(
        "swarm.stop",
        json!({"run_id":run,"generation":1,"revision":1}),
    );
    assert!(
        begin.elapsed() < Duration::from_secs(2),
        "Stop waited for checker"
    );
    assert_eq!(stopped["status"], "stopping");
    let result = checker.join().unwrap();
    assert_eq!(result["result"]["status"], "interrupted", "{result}");
    assert_eq!(d.call("swarm.get",json!({"id":run}))["status"],"stopped");
}

#[test]
fn stopped_run_recovers_after_orphaned_checker_exits() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    let t = tmp();
    let verifier = t.path().join("orphan-check.py");
    let entered = t.path().join("orphan-entered");
    let release = t.path().join("orphan-release");
    std::fs::write(&verifier,format!(
        "#!/usr/bin/env python3\nimport pathlib,time\npathlib.Path({:?}).write_text('started')\nfor _ in range(500):\n    if pathlib.Path({:?}).exists():\n        raise SystemExit(0)\n    time.sleep(0.02)\nraise SystemExit(1)\n",
        entered.to_string_lossy(),release.to_string_lossy())).unwrap();
    std::fs::set_permissions(&verifier,std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut d=Daemon::start(&[("OVERSEER_SWARM_VERIFIER_PATH",verifier.to_str().unwrap())]);
    let checkout=repo(&t.path().join("source"));
    let base=git(&checkout,&["rev-parse","HEAD"]);
    std::fs::write(checkout.join("a.txt"),"changed\n").unwrap();
    let patch=format!("{}\n",git(&checkout,&["diff","--","a.txt"]));
    std::fs::write(checkout.join("a.txt"),"a\n").unwrap();
    let made=d.call("swarm.create",json!({"category":"Orphan checker stop",
        "objective":"Check a.txt","allowed_targets":["system-codex"],
        "source_change_permission":"isolated"}));
    let run=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,
        "jobs":[{"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}]}));
    accepted_patch(&d,run,"writer","writer-patch",&patch);
    d.call("swarm.integrate",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"writer","artifact_id":"writer-patch","repo":checkout,
        "base_revision":base}));
    let socket=d.socket();
    let old_run=run.to_string();
    let pending=std::thread::spawn(move||{
        let mut conn=UnixStream::connect(socket).unwrap();
        writeln!(conn,"{}",json!({"id":1,"method":"swarm.verify","params":{
            "run_id":old_run,"generation":1,"revision":1,"request_id":"before-stop"}})).unwrap();
        let mut line=String::new();
        let _=BufReader::new(conn).read_line(&mut line);
    });
    let deadline=Instant::now()+Duration::from_secs(3);
    while !entered.exists() {
        assert!(Instant::now()<deadline,"verifier never started");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(d.call("swarm.stop",json!({"run_id":run,"generation":1,
        "revision":1}))["status"],"stopping");
    d.kill9();
    pending.join().unwrap();
    d.spawn();
    assert_eq!(d.call("swarm.get",json!({"id":run}))["status"],"stopping");
    std::fs::write(&release,"go").unwrap();
    let deadline=Instant::now()+Duration::from_secs(5);
    while d.call("swarm.get",json!({"id":run}))["status"]!="stopped" {
        assert!(Instant::now()<deadline,"orphaned checker kept run stopping");
        std::thread::sleep(Duration::from_millis(50));
    }
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let status:String=db.query_row("SELECT status FROM swarm_verifications WHERE run_id=?1",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(status,"interrupted");
}

#[test]
fn restart_waits_for_orphaned_verifier_then_retries_without_reusing_its_pass() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    let t = tmp();
    let verifier = t.path().join("restart-check.py");
    let entered = t.path().join("checker-entered");
    let release = t.path().join("checker-release");
    std::fs::write(&verifier, format!(
        "#!/usr/bin/env python3\nimport pathlib, time\npathlib.Path({:?}).write_text('started')\nfor _ in range(250):\n    if pathlib.Path({:?}).exists():\n        raise SystemExit(0)\n    time.sleep(0.02)\nraise SystemExit(1)\n",
        entered.to_string_lossy(), release.to_string_lossy()
    )).unwrap();
    std::fs::set_permissions(&verifier, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut d = Daemon::start(&[("OVERSEER_SWARM_VERIFIER_PATH", verifier.to_str().unwrap())]);
    let checkout = repo(&t.path().join("source"));
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("a.txt"), "changed\n").unwrap();
    let patch = format!("{}\n", git(&checkout, &["diff", "--", "a.txt"]));
    std::fs::write(checkout.join("a.txt"), "a\n").unwrap();
    let made = d.call(
        "swarm.create",
        json!({"category":"Verifier restart fixture",
        "objective":"Check a.txt", "allowed_targets":["system-codex"],
        "source_change_permission":"isolated"}),
    );
    let run = made["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
        "jobs":[{"id":"writer","title":"Change a.txt","acceptance":"patch","deps":[]}]}),
    );
    accepted_patch(&d, run, "writer", "writer-patch", &patch);
    d.call(
        "swarm.integrate",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"writer","artifact_id":"writer-patch","repo":checkout,
        "base_revision":base}),
    );

    let socket = d.socket();
    let old_run = run.to_string();
    let pending = std::thread::spawn(move || {
        let mut conn = UnixStream::connect(socket).unwrap();
        writeln!(
            conn,
            "{}",
            json!({"id":1,"method":"swarm.verify","params":{
            "run_id":old_run,"generation":1,"revision":1,"request_id":"before-crash"}})
        )
        .unwrap();
        let mut line = String::new();
        let _ = BufReader::new(conn).read_line(&mut line);
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !entered.exists() {
        assert!(Instant::now() < deadline, "verifier never started");
        std::thread::sleep(Duration::from_millis(10));
    }
    d.kill9();
    pending.join().unwrap();
    d.spawn();
    let next = json!({"run_id":run,"generation":1,"revision":1,"request_id":"after-crash"});
    let blocked = d.try_call("swarm.verify", next.clone()).unwrap_err();
    assert!(blocked.contains("already running"), "{blocked}");
    std::fs::write(&release, "go").unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let passed = loop {
        match d.try_call("swarm.verify", next.clone()) {
            Ok(result) => break result,
            Err(error) if error.contains("already running") && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("retry failed: {error}"),
        }
    };
    assert_eq!(passed["status"], "passed", "{passed}");
    let old = d.call(
        "swarm.verify",
        json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"before-crash"}),
    );
    assert_eq!(old["status"], "interrupted", "{old}");
    assert_eq!(old["duplicate"], true);
}
