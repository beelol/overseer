//! Installing and running Ollama only when allowed (Continuity, AC-90), against the real daemon.
//! Homebrew and the `ollama` program are SYNTHETIC here (fixtures/fake-harness/brew-fixture.sh,
//! ollama-fixture.js), and the archive is a fixture made by the test. Nothing is downloaded and
//! nothing outside the test's own folders is touched. The live check with the real archive is
//! test/local/ollama-install-live.js.

mod common;
#[path = "common/ollama.rs"]
mod ollama;
#[path = "common/world.rs"]
mod world;

use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use world::*;

struct Machine {
    w: World,
    d: Daemon,
    url: String,
    /// Where an installed `ollama` would be.
    program: PathBuf,
}

/// A machine without Ollama: nothing answers, and the program is nowhere.
fn machine(extra: &[(&str, &str)]) -> Machine {
    let w = World::new();
    let url = no_ollama();
    let program = w.file("bin/ollama");
    let (brew_log, serve_log) = (w.file("brew.log"), w.file("serve.log"));
    let fixture = repo_root().join("fixtures/fake-harness/ollama-fixture.js");
    let mut env: Vec<(&str, &str)> = vec![
        ("OVERSEER_OLLAMA_CANDIDATES", program.to_str().unwrap()),
        ("OVERSEER_TEST_BREW", ""),
        ("BREW_FIXTURE_LOG", brew_log.to_str().unwrap()),
        ("BREW_FIXTURE_TARGET", program.to_str().unwrap()),
        ("BREW_FIXTURE_PROGRAM", fixture.to_str().unwrap()),
        ("OLLAMA_FIXTURE_LOG", serve_log.to_str().unwrap()),
    ];
    env.retain(|(name, _)| !extra.iter().any(|(k, _)| k == name));
    env.extend_from_slice(extra);
    let d = w.start(&url, &env);
    Machine { w, d, url, program }
}

impl Machine {
    fn status(&self) -> Value {
        self.d.call("ollama.status", json!({}))
    }
    fn allow(&self) {
        self.d.call(
            "settings.set",
            json!({"values": {"allowOllamaInstall": true}}),
        );
    }
    fn events(&self, kind: &str) -> Vec<Value> {
        all_events(&self.d, kind)
            .into_iter()
            .map(|e| e["payload"].clone())
            .collect()
    }
    fn answers(&self) -> bool {
        ureq::get(&format!("{}/api/version", self.url))
            .timeout(Duration::from_secs(2))
            .call()
            .is_ok()
    }
    fn own(&self) -> PathBuf {
        self.d.home.path().join("ollama")
    }
    fn until(&self, what: &str, pred: impl Fn() -> bool) {
        let at = Instant::now();
        while !pred() {
            assert!(at.elapsed() < Duration::from_secs(20), "never: {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn ac90_nothing_is_installed_or_started_unless_allowed() {
    let brew = repo_root().join("fixtures/fake-harness/brew-fixture.sh");
    let m = machine(&[("OVERSEER_TEST_BREW", brew.to_str().unwrap())]);
    let s = m.status();
    assert_eq!(
        (
            s["allowed"].clone(),
            s["ollama"]["installed"].clone(),
            s["ollama"]["running"].clone(),
            s["ollama"]["detail"].as_str()
        ),
        (
            json!(false),
            Value::Null,
            json!(false),
            Some("Ollama is not installed")
        )
    );
    assert_eq!(s["method"], "homebrew");
    // The machine is reported as it is, and asking changes nothing.
    assert_eq!(m.d.try_call("ollama.install", json!({})).unwrap_err(), "Ollama is not installed, and installing it is off (overseer.continuity.allowOllamaInstall)");
    assert_eq!(
        m.d.try_call("ollama.start", json!({})).unwrap_err(),
        "Ollama is not running, and starting it is off (overseer.continuity.allowOllamaInstall)"
    );
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let c = m.d.call("task.create", json!({"repo": repo, "harness": "opencode-serve", "prompt": "write x.txt x", "permission_mode": "auto"}));
    assert_eq!(
        c["launch_error"],
        "Ollama is not installed; a local model cannot run"
    );
    assert!(
        lines(&m.w.file("brew.log")).is_empty(),
        "Homebrew was never asked"
    );
    assert!(
        !m.program.exists() && !m.own().exists(),
        "nothing was installed"
    );
    assert!(m.events("ollama_install").is_empty() && m.events("ollama_server").is_empty());

    // Installed by the user but not running, and still not allowed: it is not started either.
    std::fs::create_dir_all(m.program.parent().unwrap()).unwrap();
    std::fs::copy(
        repo_root().join("fixtures/fake-harness/ollama-fixture.js"),
        &m.program,
    )
    .unwrap();
    assert!(m.status()["ollama"]["detail"]
        .as_str()
        .unwrap()
        .starts_with("Ollama is installed but not running"));
    let c = m.d.call("task.create", json!({"repo": repo, "harness": "opencode-serve", "prompt": "write x.txt x", "permission_mode": "auto"}));
    assert!(
        c["launch_error"]
            .as_str()
            .unwrap()
            .starts_with("Ollama is installed but not running"),
        "{c}"
    );
    assert!(
        lines(&m.w.file("serve.log")).is_empty() && !m.answers(),
        "and nothing was started"
    );
}

#[test]
fn ac90_homebrew_installs_it_and_the_server_is_loopback_only_and_stops_when_idle() {
    let brew = repo_root().join("fixtures/fake-harness/brew-fixture.sh");
    let m = machine(&[
        ("OVERSEER_TEST_BREW", brew.to_str().unwrap()),
        ("OVERSEER_TEST_OLLAMA_IDLE_MS", "1500"),
    ]);
    m.allow();
    let done = m.d.call("ollama.install", json!({}));
    assert_eq!(
        (
            done["installed"].clone(),
            done["already"].clone(),
            done["detail"]["method"].as_str()
        ),
        (json!(true), json!(false), Some("homebrew"))
    );
    assert_eq!(lines(&m.w.file("brew.log")), ["install --cask ollama"]);
    assert!(m.program.is_file());
    let steps: Vec<String> = m
        .events("ollama_install")
        .iter()
        .map(|e| e["step"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(steps, ["starting", "homebrew", "installed"]);
    // Asked again, nothing is installed twice.
    assert_eq!(m.d.call("ollama.install", json!({}))["already"], true);
    assert_eq!(lines(&m.w.file("brew.log")).len(), 1);

    // Nothing answers on the address: the server is started, on loopback and nowhere else.
    assert!(!m.answers());
    let started = m.d.call("ollama.start", json!({}));
    assert_eq!(
        (
            started["started"].clone(),
            started["ours"].clone(),
            started["host"].as_str()
        ),
        (
            json!(true),
            json!(true),
            Some(m.url.trim_start_matches("http://"))
        )
    );
    assert!(m.answers());
    let served: Vec<Value> = lines(&m.w.file("serve.log"))
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(served.len(), 1);
    assert_eq!(
        (
            served[0]["bound"]["address"].as_str(),
            served[0]["asked"].as_str()
        ),
        (Some("127.0.0.1"), Some(m.url.trim_start_matches("http://")))
    );
    let pid = served[0]["pid"].as_i64().unwrap();
    assert_eq!(m.status()["server"]["pid"], pid);
    assert!(pid_alive(pid));
    // Asked again while it answers, no second server is started.
    assert_eq!(m.d.call("ollama.start", json!({}))["started"], false);
    assert_eq!(lines(&m.w.file("serve.log")).len(), 1);

    // No local work for the idle time: Overseer stops the server it started.
    m.until("the idle stop", || {
        !pid_alive(pid)
            && m.events("ollama_server")
                .iter()
                .any(|e| e["action"] == "stopped")
    });
    let stopped = m
        .events("ollama_server")
        .into_iter()
        .find(|e| e["action"] == "stopped")
        .unwrap();
    assert_eq!(stopped["pid"], pid);
    assert!(
        stopped["why"]
            .as_str()
            .unwrap()
            .starts_with("no local work for "),
        "{stopped}"
    );
    assert_eq!(m.status()["server"], Value::Null);
    assert!(!m.answers());

    // Local work starts it again by itself.
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let c = m.d.call("task.create", json!({"repo": repo, "harness": "opencode-serve", "prompt": "write x.txt x", "permission_mode": "auto"}));
    assert!(
        c["launch_error"]
            .as_str()
            .unwrap()
            .starts_with("no local model that is installed and verified fits"),
        "the server answers now, and has no model: {c}"
    );
    assert_eq!(
        m.events("ollama_server")
            .iter()
            .filter(|e| e["action"] == "started")
            .count(),
        2
    );
    m.d.call("ollama.stop", json!({}));
}

#[cfg(target_os = "macos")]
#[test]
fn ac90_an_archive_that_does_not_verify_is_deleted() {
    // An application that calls itself Ollama and is signed by nobody.
    let t = tmp();
    let app = t.path().join("Ollama.app");
    std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(app.join("Contents/Info.plist"), "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.electron.ollama</string><key>CFBundleExecutable</key><string>Ollama</string></dict></plist>").unwrap();
    std::fs::write(app.join("Contents/MacOS/Ollama"), "#!/bin/sh\nexit 0\n").unwrap();
    let marker = t.path().join("it-ran");
    std::fs::write(
        app.join("Contents/Resources/ollama"),
        format!("#!/bin/sh\ntouch {}\n", marker.display()),
    )
    .unwrap();
    let archive = t.path().join("Ollama-darwin.zip");
    assert!(std::process::Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&app)
        .arg(&archive)
        .status()
        .unwrap()
        .success());

    let m = machine(&[("OVERSEER_TEST_OLLAMA_ARCHIVE", archive.to_str().unwrap())]);
    assert_eq!(m.status()["method"], "archive");
    m.allow();
    let refused = m.d.try_call("ollama.install", json!({})).unwrap_err();
    assert!(
        refused.starts_with(
            "the downloaded Ollama was not opened: its code signature does not verify"
        ) && refused.ends_with("; the download was deleted"),
        "{refused}"
    );
    // Nothing of the download is left, nothing was installed, and nothing in it was run.
    let left: Vec<String> = std::fs::read_dir(m.own())
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    assert!(!marker.exists());
    assert_eq!(m.status()["ollama"]["installed"], Value::Null);
    let steps: Vec<String> = m
        .events("ollama_install")
        .iter()
        .map(|e| e["step"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(steps, ["starting", "downloading", "verifying", "failed"]);
    assert_eq!(m.status()["last_error"], refused.as_str());
    assert!(
        archive.exists(),
        "the fixture itself is not Overseer's to delete"
    );
    // Offline nothing is fetched at all.
    m.w.net(json!({"system": "none"}));
    wait_conn(&m.d, "offline", |s| s["state"] == "offline");
    assert_eq!(
        m.d.try_call("ollama.install", json!({})).unwrap_err(),
        "Ollama is not installed, and installing it needs a connection"
    );
}

#[test]
fn ac90_an_ollama_the_user_runs_is_used_as_it_is_and_never_stopped() {
    let o = ollama::Ollama::start();
    o.install(ollama::qwen3_coder_30b())
        .install(ollama::qwen3_coder_30b_64k());
    let w = World::new();
    let serve_log = w.file("serve.log");
    let opencode = repo_root().join("fixtures/fake-harness/opencode-serve-fixture.js");
    let program = repo_root().join("fixtures/fake-harness/ollama-fixture.js");
    let d = w.start(
        &o.url(),
        &[
            ("OVERSEER_OPENCODE_PATH", opencode.to_str().unwrap()),
            ("OVERSEER_OLLAMA_CANDIDATES", program.to_str().unwrap()),
            ("OVERSEER_TEST_OLLAMA_IDLE_MS", "300"),
            ("OLLAMA_FIXTURE_LOG", serve_log.to_str().unwrap()),
        ],
    );
    d.call(
        "settings.set",
        json!({"values": {"allowOllamaInstall": true}}),
    );
    // It answers: nothing is installed and nothing is started beside it.
    assert_eq!(d.call("ollama.install", json!({}))["already"], true);
    let started = d.call("ollama.start", json!({}));
    assert_eq!(
        (started["started"].clone(), started["ours"].clone()),
        (json!(false), json!(false))
    );
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let c = d.call("task.create", json!({"repo": repo, "harness": "opencode-serve", "prompt": "write x.txt x; say done", "permission_mode": "auto"}));
    assert_eq!(c["launch_error"], Value::Null, "{c}");
    assert_eq!(d.wait_done(&run_id(&c), 30)["status"], "completed");
    // Long after the idle time it still answers: Overseer stops only what it started.
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(d.call("ollama.status", json!({}))["server"], Value::Null);
    assert_eq!(d.call("ollama.stop", json!({}))["stopped"], false);
    assert!(ureq::get(&format!("{}/api/version", o.url()))
        .call()
        .is_ok());
    assert!(lines(&serve_log).is_empty() && all_events(&d, "ollama_server").is_empty());
}
