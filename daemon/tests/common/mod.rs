#![allow(dead_code)]
//! Black-box helpers: each test runs a real `overseerd` with its own OVERSEER_HOME,
//! talks to it over the Unix socket and uses real Git repositories.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

// The gateway tests' reference phone speaks the daemon's own Noise code.
#[path = "../../src/gateway/base32.rs"]
pub mod base32;
#[path = "../../src/gateway/noise.rs"]
pub mod noise;
pub mod phone;

pub const BIN: &str = env!("CARGO_BIN_EXE_overseerd");

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

pub struct Daemon {
    pub home: tempfile::TempDir,
    pub child: Option<Child>,
    pub env: Vec<(String, String)>,
}

impl Daemon {
    pub fn start(env: &[(&str, &str)]) -> Daemon {
        let home = tempfile::Builder::new().prefix("ovs-t").tempdir_in("/tmp").unwrap();
        let mut env: Vec<(String, String)> = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        // Tests must never reach a real (paid) harness: unset overrides point nowhere.
        for key in ["OVERSEER_CODEX_PATH", "OVERSEER_CLAUDE_PATH", "OVERSEER_OPENCODE_PATH"] {
            if !env.iter().any(|(k, _)| k == key) {
                env.push((key.to_string(), "/nonexistent/harness-disabled-in-tests".to_string()));
            }
        }
        let enable_auto = !env.iter().any(|(key, value)|
            key == "OVERSEER_TEST_AUTO_DISABLED" && value == "1");
        if !env.iter().any(|(k, _)| k == "OVERSEER_SWARM_FIXTURE_API") {
            env.push(("OVERSEER_SWARM_FIXTURE_API".to_string(), "1".to_string()));
        }
        // Nor the network: Continuity's probes stay off unless a test brings its own network fixture.
        if !env.iter().any(|(k, _)| k == "OVERSEER_TEST_NET" || k == "OVERSEER_CONTINUITY_PROBES") {
            env.push(("OVERSEER_CONTINUITY_PROBES".to_string(), "off".to_string()));
        }
        // OVERSEER_TEST_PHONE_ACCESS=on runs a whole suite with phone access turned on (AC-116):
        // what passed with it off must pass with it on.
        // The suites that switch phone access themselves (gateway, phone_methods) start from
        // off, as the owner's daemon does: for them the setting changes nothing.
        let own = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_default();
        let switches_itself = own.starts_with("gateway-") || own.starts_with("phone_methods-");
        let phone_access = !switches_itself && std::env::var("OVERSEER_TEST_PHONE_ACCESS").as_deref() == Ok("on");
        if phone_access && !env.iter().any(|(k, _)| k == "OVERSEER_GATEWAY_MDNS") {
            env.push(("OVERSEER_GATEWAY_MDNS".to_string(), "off".to_string()));
        }
        let mut d = Daemon { home, child: None, env };
        d.spawn();
        if enable_auto {
            // Most existing Auto protocol fixtures opt in explicitly; the
            // default-off regression opts out through this test-only flag.
            d.call("auto.mode.set", json!({"enabled":true}));
        }
        if phone_access {
            let status = d.call("gateway.enable", json!({"port": 0}));
            assert_eq!(status["enabled"], true);
        }
        d
    }

    pub fn spawn(&mut self) {
        self.spawn_from(Path::new(BIN));
    }

    pub fn spawn_from(&mut self, binary: &Path) {
        #[cfg(target_os = "macos")]
        let deny_egress = self.env.iter().any(|(key, value)|
            key == "OVERSEER_TEST_DENY_EGRESS" && value == "1");
        #[cfg(target_os = "macos")]
        let mut cmd = if deny_egress {
            let mut sandbox = Command::new("/usr/bin/sandbox-exec");
            sandbox.args(["-p", "(version 1) (allow default) (deny network-outbound)"])
                .arg(binary);
            sandbox
        } else { Command::new(binary) };
        #[cfg(not(target_os = "macos"))]
        let mut cmd = Command::new(binary);
        #[cfg(target_os = "macos")]
        if deny_egress {
            cmd.env_clear();
            for key in ["HOME", "PATH", "TMPDIR", "USER", "LOGNAME", "SHELL", "LANG"] {
                if let Some(value) = std::env::var_os(key) { cmd.env(key, value); }
            }
        }
        cmd.arg("serve").env("OVERSEER_HOME", self.home.path()).stdout(Stdio::null()).stderr(Stdio::null());
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        self.child = Some(cmd.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.try_call("hello", json!({})).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("daemon did not start");
    }

    pub fn socket(&self) -> PathBuf {
        let out = Command::new(BIN).arg("socket-path").env("OVERSEER_HOME", self.home.path()).output().unwrap();
        PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
    }

    pub fn try_call(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut conn = UnixStream::connect(self.socket()).map_err(|e| e.to_string())?;
        conn.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        conn.write_all(format!("{}\n", json!({"id": 1, "method": method, "params": params})).as_bytes()).map_err(|e| e.to_string())?;
        let mut line = String::new();
        BufReader::new(conn).read_line(&mut line).map_err(|e| e.to_string())?;
        let msg: Value = serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))?;
        if let Some(err) = msg.get("error") {
            return Err(err["message"].as_str().unwrap_or_default().to_string());
        }
        Ok(msg["result"].clone())
    }

    pub fn call(&self, method: &str, params: Value) -> Value {
        self.try_call(method, params.clone()).unwrap_or_else(|e| panic!("{method} {params} failed: {e}"))
    }

    pub fn raw(&self, text: &[u8]) -> String {
        let mut conn = UnixStream::connect(self.socket()).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let _ = conn.write_all(text); // the server may close early on oversized input
        let mut line = String::new();
        let _ = BufReader::new(conn).read_line(&mut line);
        line
    }

    pub fn run(&self, id: &str) -> Value {
        self.call("state", json!({}))["runs"].as_array().unwrap().iter().find(|r| r["id"] == id).cloned().unwrap()
    }

    pub fn runs(&self) -> Vec<Value> {
        self.call("state", json!({}))["runs"].as_array().unwrap().clone()
    }

    pub fn wait_status(&self, id: &str, pred: impl Fn(&str) -> bool, secs: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let run = self.run(id);
            if pred(run["status"].as_str().unwrap()) {
                return run;
            }
            if Instant::now() > deadline {
                let errors: Vec<String> = self.events(id).iter().filter(|e| e["kind"] == "error" || e["kind"] == "daemon_error" || (e["kind"] == "output" && e["payload"]["role"] == "stderr")).map(|e| e["payload"].to_string().chars().take(300).collect()).collect();
                panic!("run {id} stuck in {} (exit reason {}; errors {errors:?})", run["status"], run["exit_reason"]);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn wait_done(&self, id: &str, secs: u64) -> Value {
        self.wait_status(id, |s| !["queued", "starting", "running", "waiting_for_user"].contains(&s), secs)
    }

    pub fn events(&self, run: &str) -> Vec<Value> {
        self.call("events.list", json!({"run_id": run, "limit": 5000}))["events"].as_array().unwrap().clone()
    }

    pub fn kill9(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.try_call("daemon.shutdown", json!({}));
        if let Some(mut c) = self.child.take() {
            let _ = c.wait();
        }
    }

    pub fn generic(&self, repo: &Path, mode: &str, program: &str, args: &[&str]) -> Value {
        self.call("task.create", json!({"repo": repo, "harness": "generic", "workspace_mode": mode, "program": program, "args": args, "prompt": "", "title": format!("{program} {}", args.join(" "))}))
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // Stop supervisors this test started, then the daemon.
        if let Ok(state) = self.try_call("run.active", json!({})) {
            for run in state.as_array().cloned().unwrap_or_default() {
                let _ = self.try_call("run.interrupt", json!({"run_id": run["id"]}));
            }
        }
        self.kill9();
        let _ = Command::new("pkill").arg("-9").arg("-f").arg(self.home.path()).status();
    }
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim_end().to_string()
}

pub fn repo(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.name", "T"]);
    git(dir, &["config", "user.email", "t@example.invalid"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "ignored/\n*.log\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "base"]);
    std::fs::canonicalize(dir).unwrap()
}

/// Content fingerprint of a checkout: files, index, status, stash, HEAD.
pub fn fingerprint(dir: &Path) -> String {
    let mut files = Vec::new();
    for entry in walk(dir) {
        let rel = entry.strip_prefix(dir).unwrap().display().to_string();
        files.push(format!("{rel}={}", std::fs::read_to_string(&entry).unwrap_or_default()));
    }
    files.sort();
    format!("{}\n--status\n{}\n--index\n{}\n--stash\n{}\n--head\n{}", files.join("\n"), git(dir, &["status", "--porcelain=v1"]), git(dir, &["diff", "--cached"]), git(dir, &["stash", "list"]), git(dir, &["rev-parse", "HEAD"]))
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.file_name().unwrap() == ".git" {
            continue;
        }
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

pub fn tmp() -> tempfile::TempDir {
    tempfile::Builder::new().prefix("ovs-r").tempdir_in("/tmp").unwrap()
}

pub fn run_id(created: &Value) -> String {
    created["run"]["id"].as_str().unwrap().to_string()
}

pub fn commit_beneficial_batch(d: &Daemon, run_id: &str, job_ids: &[String]) -> Value {
    assert!(job_ids.len() >= 2);
    let workers: Vec<Value> = job_ids.iter().map(|id| json!({
        "id":id,"elapsed_ms":100,"usage_milli":{"points":10}
    })).collect();
    let serial = json!({
        "planning":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "context":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "integration":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "review":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "retries":{"elapsed_ms":0,"usage_milli":{"points":1}},
        "workers":workers
    });
    let parallel = json!({
        "planning":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "context":{"elapsed_ms":20,"usage_milli":{"points":1}},
        "integration":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "review":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "retries":{"elapsed_ms":0,"usage_milli":{"points":1}},
        "workers":workers
    });
    let committed = d.call("swarm.benefit.commit",json!({
        "run_id":run_id,"generation":1,"revision":1,
        "estimate":{"independent":true,"max_workers":job_ids.len(),
            "allocation_milli":{"points":100000},
            "finishing_reserve_milli":{"points":20000},
            "serial":serial,"parallel":parallel}
    }));
    assert_eq!(committed["decision"],"parallel","{committed}");
    committed
}

pub fn ws_path(d: &Daemon, created: &Value) -> PathBuf {
    let ws = created["workspace"]["id"].as_str().unwrap();
    PathBuf::from(d.call("state", json!({}))["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == ws).unwrap()["path"].as_str().unwrap())
}

pub fn option(d: &Daemon, run: &str, mode: &str, branch: Option<&str>) -> Value {
    let opts = d.call("comparison.options", json!({"run_id": run, "branch": branch}));
    opts["options"].as_array().unwrap().iter().find(|o| o["mode"] == mode).cloned().unwrap_or(Value::Null)
}

pub fn diff_paths(d: &Daemon, created: &Value, base: &str) -> Vec<(String, String)> {
    let ws = created["workspace"]["id"].as_str().unwrap();
    let diff = d.call("workspace.diff", json!({"workspace_id": ws, "base": base}));
    let mut v: Vec<(String, String)> = diff["changes"].as_array().unwrap().iter().map(|c| (c["status"].as_str().unwrap().to_string(), c["path"].as_str().unwrap().to_string())).collect();
    v.sort();
    v
}

pub fn pid_alive(pid: i64) -> bool {
    unsafe { libc_kill(pid as i32, 0) == 0 }
}

extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

pub fn signal(pid: i64, sig: i32) {
    unsafe {
        libc_kill(pid as i32, sig);
    }
}

pub fn launch_info(d: &Daemon, run: &str) -> (Value, PathBuf) {
    let dir = d.home.path().join("runs").join(run);
    let mut gens: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    gens.sort();
    let last = gens.pop().unwrap();
    let shim: Value = loop {
        if let Ok(b) = std::fs::read(last.join("shim.json")) {
            break serde_json::from_slice(&b).unwrap();
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    (shim, last)
}

/// A fixture account for `profile`, written as Auto's own collector stores
/// one: an account identity (generation 1, fingerprint derived from
/// `account`) and one structured quota observation with a five-hour and a
/// weekly window, `used_percent` reported used in each. Returns the
/// `shared_booking` inputs a fixture caller cites, drawing `draw`
/// thousandths of a reported percentage point in each window. No provider is
/// asked and no allowance is spent; the daemon needs
/// OVERSEER_SHARED_BOOKING_FIXTURE_API=1 to accept the inputs.
pub fn fixture_account_booking(d: &Daemon, profile: &str, account: &str, used_percent: f64, draw: i64) -> Value {
    use sha2::{Digest, Sha256};
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(10)).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let fingerprint = format!("{:x}", Sha256::digest(account.as_bytes()));
    db.execute("INSERT INTO auto_account_identity(profile_id,fingerprint,generation,observed_ms)
        VALUES(?1,?2,1,?3) ON CONFLICT(profile_id) DO UPDATE SET fingerprint=excluded.fingerprint",
        rusqlite::params![profile, fingerprint, now]).unwrap();
    let window = |name: &str, minutes: i64| json!({"pool_id":profile,"bucket_id":name,"window":name,
        "model":null,"model_family":null,"plan_type":"fixture","used_percent":used_percent,
        "reset_ms":now + minutes * 60_000,"duration_mins":minutes,
        "observed_ms":now,"expires_ms":now + 600_000});
    let snapshot = json!({"ordinary_usage_allowed":true,"observed_ms":now,"expires_ms":now + 600_000,
        "windows":[window("five_hour", 300), window("weekly", 10_080)]});
    db.execute("INSERT INTO events(ts,task_id,run_id,kind,source,confidence,payload)
        VALUES(?1,NULL,NULL,'quota_observed','fixture','exact','{}')", [now]).unwrap();
    let seq = db.last_insert_rowid();
    db.execute("INSERT INTO auto_quota_observations(event_seq,pool_id,source,observed_ms,snapshot)
        VALUES(?1,?2,'fixture/structured',?3,?4)",
        rusqlite::params![seq, profile, now, snapshot.to_string()]).unwrap();
    json!({"account_generation":1,"quota_event_seq":seq,"upper_draw_milli":[draw, draw]})
}
