mod common;

use common::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

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
        "allowed_targets":["system-codex"]}),
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
    for (index, name) in names.iter().enumerate() {
        let relative = format!("src/resources/{name}.ts");
        let replacement = template.replace("RESOURCE_NAME", name);
        let patch = patch_from_replacement(&checkout, &relative, &replacement);
        let artifact = format!("patch-{name}");
        accept_patch(&d, run, 2, name, &artifact, &patch);
        latest = integrate(2, name, &artifact);
        assert_eq!(latest["status"], "integrated", "{latest}");
        if index == 3 {
            let partial = d.call(
                "swarm.verify",
                json!({"run_id":run,"generation":1,
                "revision":2,"request_id":"four-of-twenty-four"}),
            );
            assert_eq!(partial["status"], "failed", "{partial}");
        }
    }
    let verified = d.call(
        "swarm.verify",
        json!({"run_id":run,"generation":1,
        "revision":2,"request_id":"all-twenty-four"}),
    );
    assert_eq!(verified["status"], "passed", "{verified}");
    assert_eq!(verified["commit"], latest["commit"]);
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
