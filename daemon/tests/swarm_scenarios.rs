mod common;

use common::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn patch_from_replacement(checkout: &Path, relative: &str, replacement: &str) -> String {
    let path = checkout.join(relative);
    let before = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, replacement).unwrap();
    let patch = format!("{}\n", git(checkout, &["diff", "--", relative]));
    std::fs::write(&path, before).unwrap();
    assert_eq!(
        git(checkout, &["status", "--porcelain=v1"]),
        "",
        "patch preparation left the integration source dirty"
    );
    assert!(!patch.trim().is_empty(), "empty patch for {relative}");
    patch
}

fn accept_patch(d: &Daemon, run: &str, revision: i64, job: &str, artifact: &str, patch: &str) {
    let attempt = d.call(
        "swarm.attempt.register",
        json!({
            "run_id":run,"generation":1,"revision":revision,"job_id":job
        }),
    );
    d.call(
        "swarm.artifact.put",
        json!({
            "run_id":run,"job_id":job,"attempt_id":attempt["id"],"token":attempt["token"],
            "artifact_id":artifact,"source_revision":revision,"kind":"patch","content":patch
        }),
    );
    d.call(
        "swarm.report",
        json!({
            "run_id":run,"job_id":job,"attempt_id":attempt["id"],"token":attempt["token"],
            "message_id":format!("result-{job}-r{revision}"),"type":"result","revision":revision,
            "payload":{"artifact_ids":[artifact]}
        }),
    );
    d.call(
        "swarm.decide",
        json!({
            "run_id":run,"generation":1,"revision":revision,"job_id":job,
            "decision":"accept","evidence":[artifact]
        }),
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({
            "run_id":run,"generation":1,"revision":revision,"job_id":job,
            "attempt_id":attempt["id"]
        }),
    );
}

// Explicitly opt in because this real TypeScript backend requires Node 24 and
// opens a localhost socket. It never contacts a provider or external service.
#[test]
#[ignore = "requires Node.js 24 and local socket permission"]
fn catalog_s3_twenty_four_patches_need_a_combined_cursor_check() {
    let fixture = repo_root().join("fixtures/swarm/catalog-v1");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
    let names: Vec<String> = manifest["resource_modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap().to_string())
        .collect();
    assert_eq!(names.len(), 24);
    let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|directory| directory.join("node"))
        .find(|path| path.is_file())
        .expect("Node.js 24 executable on PATH");
    let version = std::process::Command::new(&node)
        .arg("--version")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("v24."));

    let temp = tmp();
    let verifier = temp.path().join("verify-catalog.sh");
    std::fs::write(
        &verifier,
        format!(
            "#!/bin/sh\nexec '{}' --test --test-reporter=dot acceptance.test.ts\n",
            node.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&verifier, std::fs::Permissions::from_mode(0o755)).unwrap();
    let d = Daemon::start(&[("OVERSEER_SWARM_VERIFIER_PATH", verifier.to_str().unwrap())]);
    let checkout = repo(&temp.path().join("catalog"));
    copy_tree(&fixture, &checkout);
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-q", "-m", "Catalog v1 fixture"]);
    let base = git(&checkout, &["rev-parse", "HEAD"]);
    let source_before = fingerprint(&checkout);

    let made = d.call(
        "swarm.create",
        json!({"category":"Catalog migration S3",
        "objective":"Migrate 24 endpoints to stable cursor pagination",
        "allowed_targets":["system-codex"],"source_change_permission":"isolated"}),
    );
    let run = made["id"].as_str().unwrap();
    let mut jobs = vec![json!({"id":"contract","title":"Cursor contract",
        "acceptance":"stable tuple cursor","deps":[]})];
    jobs.extend(names.iter().map(|name| {
        json!({"id":name,"title":format!("Migrate {name}"),
        "acceptance":"cursor pagination and response shape","deps":["contract"]})
    }));
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":jobs}),
    );
    let integrate = |revision: i64, job: &str, artifact: &str| {
        d.call(
            "swarm.integrate",
            json!({
                "run_id":run,"generation":1,"revision":revision,"job_id":job,
                "artifact_id":artifact,"repo":checkout,"base_revision":base
            }),
        )
    };

    let contract_v1 = std::fs::read_to_string(fixture.join("reference/contract-v1.ts")).unwrap();
    let contract_patch = patch_from_replacement(&checkout, "src/pagination.ts", &contract_v1);
    accept_patch(&d, run, 1, "contract", "contract-patch-v1", &contract_patch);
    let first = integrate(1, "contract", "contract-patch-v1");
    assert_eq!(first["status"], "integrated", "{first}");
    let workspace = PathBuf::from(first["workspace_path"].as_str().unwrap());
    let template = std::fs::read_to_string(fixture.join("reference/module.ts.template")).unwrap();
    let stale_attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"accounts"}),
    );
    let stale_patch = patch_from_replacement(
        &checkout,
        "src/resources/accounts.ts",
        &template.replace("RESOURCE_NAME", "accounts"),
    );
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":"accounts",
        "attempt_id":stale_attempt["id"],"token":stale_attempt["token"],
        "artifact_id":"stale-accounts","source_revision":1,"kind":"patch",
        "content":stale_patch}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"accounts",
        "attempt_id":stale_attempt["id"],"token":stale_attempt["token"],
        "message_id":"stale-accounts-result","type":"result","revision":1,
        "payload":{"artifact_ids":["stale-accounts"]}}),
    );
    let revised = d.call(
        "swarm.revise",
        json!({"id":run,"generation":1,
            "expected_revision":1,"reason":"Tied timestamps need an id tie-breaker",
            "jobs":std::iter::once(json!({"id":"contract","title":"Cursor contract",
                "acceptance":"stable createdAt plus id tuple cursor","deps":[]}))
                .chain(names.iter().map(|name| json!({"id":name,"title":format!("Migrate {name}"),
                    "acceptance":"cursor pagination and response shape","deps":["contract"]})))
                .collect::<Vec<_>>()
        }),
    );
    assert_eq!(revised["revision"], 2);
    assert_eq!(revised["affected"], 25);
    assert_eq!(revised["redirected"], 1);
    let messages = d.call(
        "swarm.messages",
        json!({"run_id":run,
        "recipient":stale_attempt["id"]}),
    );
    assert!(messages["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["type"] == "redirect"));
    assert!(d
        .try_call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,
        "revision":2,"job_id":"accounts","decision":"accept",
        "evidence":["stale-accounts"]})
        )
        .unwrap_err()
        .contains("stale"));
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":run,"generation":1,
        "revision":2,"job_id":"accounts","attempt_id":stale_attempt["id"]}),
    );
    let jobs = d.call("swarm.jobs", json!({"id":run,"limit":100}));
    let contract_job = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["id"] == "contract")
        .unwrap();
    assert_eq!(contract_job["attempt_count"], 1);
    assert_eq!(contract_job["status"], "ready");
    assert!(d
        .try_call(
            "swarm.integrate",
            json!({"run_id":run,"generation":1,
        "revision":2,"job_id":"contract","artifact_id":"contract-patch-v1",
        "repo":checkout,"base_revision":base})
        )
        .is_err());

    let contract_v2 = std::fs::read_to_string(fixture.join("reference/contract-v2.ts")).unwrap();
    let repaired_patch = patch_from_replacement(&workspace, "src/pagination.ts", &contract_v2);
    accept_patch(&d, run, 2, "contract", "contract-patch-v2", &repaired_patch);
    let second = integrate(2, "contract", "contract-patch-v2");
    assert_eq!(second["status"], "integrated", "{second}");
    let mut latest = second;
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"system-codex","account_id":"fixture-account","pool_ids":["fixture-pool"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"fixture-pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    let commit_batch = |ids: &[String]| {
        let workers = ids
            .iter()
            .map(|id| {
                json!({"id":id,"elapsed_ms":100,
            "usage_milli":{"points":10}})
            })
            .collect::<Vec<_>>();
        let serial = json!({"planning":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "context":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "integration":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "review":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "retries":{"elapsed_ms":0,"usage_milli":{"points":1}},"workers":workers});
        let mut parallel = serial.clone();
        parallel["context"]["elapsed_ms"] = json!(20);
        let decision = d.call(
            "swarm.benefit.commit",
            json!({"run_id":run,
            "generation":1,"revision":2,"estimate":{"independent":true,
            "max_workers":ids.len(),"allocation_milli":{"points":100000},
            "finishing_reserve_milli":{"points":20000},
            "serial":serial,"parallel":parallel}}),
        );
        assert_eq!(decision["decision"], "parallel", "{decision}");
        assert_eq!(decision["max_parallel_workers"], ids.len());
    };
    let admit = |name: &str, offset: i64| {
        d.call(
            "swarm.admit",
            json!({"run_id":run,
        "generation":1,"revision":2,"job_id":name,"target_id":"system-codex",
        "request_id":format!("catalog-{name}"),"snapshot":snapshot,"now_ms":at+offset,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
        )
    };
    let submit_patch = |name: &str, admitted: &serde_json::Value| {
        let relative = format!("src/resources/{name}.ts");
        let replacement = template.replace("RESOURCE_NAME", name);
        let patch = patch_from_replacement(&checkout, &relative, &replacement);
        let artifact = format!("patch-{name}");
        d.call(
            "swarm.artifact.put",
            json!({"run_id":run,"job_id":name,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "artifact_id":artifact,"source_revision":2,"kind":"patch","content":patch}),
        );
        d.call(
            "swarm.report",
            json!({"run_id":run,"job_id":name,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("result-{name}-r2"),"type":"result","revision":2,
            "payload":{"artifact_ids":[artifact]}}),
        );
        artifact
    };
    let finish = |name: &str, admitted: &serde_json::Value, artifact: &str| {
        d.call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,"revision":2,
            "job_id":name,"decision":"accept","evidence":[artifact]}),
        );
        d.call(
            "swarm.attempt.confirm_exit",
            json!({"run_id":run,"generation":1,
            "revision":2,"job_id":name,"attempt_id":admitted["attempt_id"]}),
        );
        let result = integrate(2, name, artifact);
        assert_eq!(result["status"], "integrated", "{name}: {result}");
        result
    };

    commit_batch(&names[..4]);
    let first_wave = names[..4]
        .iter()
        .map(|name| {
            let admitted = admit(name, 0);
            assert_eq!(admitted["status"], "admitted", "{name}: {admitted}");
            admitted
        })
        .collect::<Vec<_>>();
    assert_eq!(admit(&names[4], 0)["reason"], "growth_wave_full");
    for (name, admitted) in names[..4].iter().zip(&first_wave) {
        let artifact = submit_patch(name, admitted);
        latest = finish(name, admitted, &artifact);
    }
    let partial = d.call(
        "swarm.verify",
        json!({"run_id":run,"generation":1,
        "revision":2,"request_id":"four-of-twenty-four"}),
    );
    assert_eq!(partial["status"], "failed", "{partial}");

    commit_batch(&names[4..12]);
    let second_wave = names[4..12]
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let admitted = admit(name, if index < 4 { 5000 } else { 10000 });
            assert_eq!(admitted["status"], "admitted", "{name}: {admitted}");
            let artifact = submit_patch(name, &admitted);
            (admitted, artifact)
        })
        .collect::<Vec<_>>();
    assert_eq!(admit(&names[12], 15000)["reason"], "review_backlog");
    let first_review = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":run,
        "generation":1,"revision":2,"now_ms":at+15000}),
    );
    assert_eq!(first_review["status"], "claimed", "{first_review}");
    for (name, (admitted, artifact)) in names[4..9].iter().zip(&second_wave[..5]) {
        latest = finish(name, admitted, artifact);
    }
    let reviewed = d.call(
        "swarm.director.complete_batch",
        json!({"run_id":run,
        "generation":1,"turn_id":first_review["turn_id"],"token":first_review["token"],
        "outcome":"progress"}),
    );
    assert_eq!(reviewed["no_progress_turns"], 0, "{reviewed}");
    assert_ne!(admit(&names[12], 15000)["reason"], "review_backlog");
    for (name, (admitted, artifact)) in names[9..12].iter().zip(&second_wave[5..]) {
        latest = finish(name, admitted, artifact);
    }

    commit_batch(&names[12..20]);
    for (index, name) in names[12..20].iter().enumerate() {
        let admitted = admit(name, if index < 4 { 15000 } else { 20000 });
        assert_eq!(admitted["status"], "admitted", "{name}: {admitted}");
        let artifact = submit_patch(name, &admitted);
        latest = finish(name, &admitted, &artifact);
    }
    commit_batch(&names[20..24]);
    for name in &names[20..24] {
        let admitted = admit(name, 25000);
        assert_eq!(admitted["status"], "admitted", "{name}: {admitted}");
        let artifact = submit_patch(name, &admitted);
        latest = finish(name, &admitted, &artifact);
    }
    let verified = d.call(
        "swarm.verify",
        json!({"run_id":run,"generation":1,
        "revision":2,"request_id":"all-twenty-four"}),
    );
    assert_eq!(verified["status"], "passed", "{verified}");
    assert_eq!(verified["commit"], latest["commit"]);
    let final_jobs = d.call("swarm.jobs", json!({"id":run,"limit":100}));
    assert_eq!(final_jobs["jobs"].as_array().unwrap().len(), 25);
    assert!(final_jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|job| job["status"] == "accepted"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let count = |table: &str| -> i64 {
        db.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE run_id=?1"),
            [run],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(count("swarm_admissions"), 24);
    assert_eq!(count("swarm_benefit_decisions"), 4);
    assert_eq!(count("swarm_integrated_artifacts"), 26);
    let final_review = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":run,
        "generation":1,"revision":2,"now_ms":at+30000}),
    );
    assert_eq!(final_review["status"], "claimed", "{final_review}");
    assert_eq!(final_review["more_pending"], false);
    d.call(
        "swarm.director.complete_batch",
        json!({"run_id":run,
        "generation":1,"turn_id":final_review["turn_id"],"token":final_review["token"],
        "outcome":"no_progress"}),
    );
    let checks = std::iter::once(json!({"job_id":"contract","outcome":"passed",
        "evidence":["contract-patch-v2"]}))
    .chain(names.iter().map(|name| {
        json!({"job_id":name,"outcome":"passed",
            "evidence":[format!("patch-{name}")]})
    }))
    .collect::<Vec<_>>();
    let completed = d.call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":2,"request_id":"catalog-s3-complete",
        "summary":"Migrated 24 resource endpoints to tuple cursor pagination",
        "verification":"Combined Node 24 Catalog acceptance check passed on the final integration commit",
        "checks":checks}));
    assert_eq!(completed["status"], "completed", "{completed}");
    assert_eq!(
        d.call("swarm.get", json!({"id":run}))["status"],
        "completed"
    );
    assert_eq!(fingerprint(&checkout), source_before);
    assert!(workspace.starts_with(std::fs::canonicalize(d.home.path()).unwrap()));
    assert_eq!(
        git(
            &workspace,
            &["rev-list", "--count", &format!("{base}..HEAD")]
        ),
        "26"
    );
}

#[test]
fn catalog_s3_starts_four_then_eight_and_waits_for_review() {
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("fixtures/swarm/catalog-v1/manifest.json")).unwrap(),
    )
    .unwrap();
    let names: Vec<String> = manifest["resource_modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap().to_string())
        .collect();
    assert_eq!(names.len(), 24);
    let d = Daemon::start(&[]);
    let created = d.call(
        "swarm.create",
        json!({"category":"Catalog migration S3", "objective":"Migrate 24 resource modules",
            "allowed_targets":["fixture-codex"]}),
    );
    let run = created["id"].as_str().unwrap();
    let jobs = std::iter::once(json!({"id":"contract","title":"Cursor contract",
        "acceptance":"stable tuple cursor","deps":[]}))
    .chain(names.iter().map(|name| {
        json!({"id":name,"title":format!("Migrate {name}"),
            "acceptance":"cursor pagination and response shape","deps":["contract"]})
    }))
    .collect::<Vec<_>>();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":jobs}),
    );
    let contract = d.call(
        "swarm.attempt.register",
        json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"contract"}),
    );
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":"contract",
        "attempt_id":contract["id"],"token":contract["token"],"artifact_id":"tuple-contract",
        "source_revision":1,"kind":"finding","content":"createdAt plus id"}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"contract",
        "attempt_id":contract["id"],"token":contract["token"],"message_id":"contract-result",
        "type":"result","revision":1,"payload":{"artifact_ids":["tuple-contract"]}}),
    );
    d.call(
        "swarm.decide",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"contract","decision":"accept","evidence":["tuple-contract"]}),
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"contract","attempt_id":contract["id"]}),
    );

    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture-codex","account_id":"fixture-account","pool_ids":["fixture-pool"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"fixture-pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    let commit_batch = |ids: &[String]| {
        let workers = ids
            .iter()
            .map(|id| {
                json!({"id":id,"elapsed_ms":100,
            "usage_milli":{"points":10}})
            })
            .collect::<Vec<_>>();
        let serial = json!({"planning":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "context":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "integration":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "review":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "retries":{"elapsed_ms":0,"usage_milli":{"points":1}},"workers":workers});
        let mut parallel = serial.clone();
        parallel["context"]["elapsed_ms"] = json!(20);
        let decision = d.call(
            "swarm.benefit.commit",
            json!({"run_id":run,
            "generation":1,"revision":1,"estimate":{"independent":true,
            "max_workers":ids.len(),"allocation_milli":{"points":100000},
            "finishing_reserve_milli":{"points":20000},
            "serial":serial,"parallel":parallel}}),
        );
        assert_eq!(decision["decision"], "parallel", "{decision}");
        assert_eq!(decision["max_parallel_workers"], ids.len());
    };
    let admit = |name: &str, offset: i64| {
        d.call(
            "swarm.admit",
            json!({"run_id":run,
        "generation":1,"revision":1,"job_id":name,"target_id":"fixture-codex",
        "request_id":format!("admit-{name}"),"snapshot":snapshot,"now_ms":at+offset,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
        )
    };
    let submit = |name: &str, admitted: &serde_json::Value| {
        let artifact = format!("result-{name}");
        d.call("swarm.artifact.put", json!({"run_id":run,"job_id":name,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"finding","content":"module checked"}));
        d.call(
            "swarm.report",
            json!({"run_id":run,"job_id":name,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("message-{name}"),"type":"result","revision":1,
            "payload":{"artifact_ids":[artifact]}}),
        );
    };
    let finish = |name: &str, admitted: &serde_json::Value| {
        let artifact = format!("result-{name}");
        d.call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,"revision":1,
            "job_id":name,"decision":"accept","evidence":[artifact]}),
        );
        d.call(
            "swarm.attempt.confirm_exit",
            json!({"run_id":run,"generation":1,
            "revision":1,"job_id":name,"attempt_id":admitted["attempt_id"]}),
        );
    };

    commit_batch(&names[..4]);
    let first = names[..4]
        .iter()
        .map(|name| {
            let admitted = admit(name, 0);
            assert_eq!(admitted["status"], "admitted", "{name}: {admitted}");
            admitted
        })
        .collect::<Vec<_>>();
    assert_eq!(admit(&names[4], 0)["reason"], "growth_wave_full");
    for (name, admitted) in names[..4].iter().zip(&first) {
        submit(name, admitted);
        finish(name, admitted);
    }
    commit_batch(&names[4..12]);
    let second = names[4..12]
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let admitted = admit(name, if index < 4 { 5000 } else { 10000 });
            assert_eq!(admitted["status"], "admitted", "{name}: {admitted}");
            admitted
        })
        .collect::<Vec<_>>();
    for (name, admitted) in names[4..12].iter().zip(&second) {
        submit(name, admitted);
    }
    assert_eq!(admit(&names[12], 15000)["reason"], "review_backlog");
    for (name, admitted) in names[4..9].iter().zip(&second[..5]) {
        finish(name, admitted);
    }
    assert_ne!(admit(&names[12], 15000)["reason"], "review_backlog");
    for (name, admitted) in names[9..12].iter().zip(&second[5..]) {
        finish(name, admitted);
    }
    commit_batch(&names[12..20]);
    for (index, name) in names[12..20].iter().enumerate() {
        let admitted = admit(name, if index < 4 { 15000 } else { 20000 });
        assert_eq!(admitted["status"], "admitted", "{name}: {admitted}");
        submit(name, &admitted);
        finish(name, &admitted);
    }
    commit_batch(&names[20..24]);
    for name in &names[20..24] {
        let admitted = admit(name, 25000);
        assert_eq!(admitted["status"], "admitted", "{name}: {admitted}");
        submit(name, &admitted);
        finish(name, &admitted);
    }
    let final_jobs = d.call("swarm.jobs", json!({"id":run,"limit":100}));
    assert_eq!(final_jobs["jobs"].as_array().unwrap().len(), 25);
    assert!(final_jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|job| job["status"] == "accepted"));
}
