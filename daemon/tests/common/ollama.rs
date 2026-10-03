#![allow(dead_code)]
//! SYNTHETIC Ollama: a loopback HTTP server answering the endpoints Overseer uses
//! (`/api/version`, `/api/tags`, `/api/show`, `/api/ps`, `/api/generate`, `/api/create`), with
//! models the test installs. Every request is recorded, so a test can prove what was and was not
//! asked for. It never counts as a real model.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

pub const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Default)]
pub struct State {
    pub tags: Vec<Value>,
    pub show: HashMap<String, Value>,
    pub loaded: Vec<Value>,
    /// Bytes a tag takes once loaded (by tag); otherwise its size on disk plus 6 GiB.
    pub loaded_size: HashMap<String, u64>,
    /// How long a load takes.
    pub load_ms: u64,
    /// The next load can be held until a test has observed actual daemon memory samples.
    pub next_load_gate: Option<Arc<LoadBarrier>>,
    pub pending_loads: HashMap<String, Arc<LoadBarrier>>,
    /// (method, path, body) of every request, in order.
    pub requests: Vec<(String, String, Value)>,
    /// Models the registry offers, by the name asked for.
    pub registry: HashMap<String, (Value, Value)>,
    /// Bytes each pull has reached; a later pull continues from there.
    pub pulled: HashMap<String, u64>,
    /// A pull reports its progress in this many steps, this far apart.
    pub pull_steps: u64,
    pub pull_step_ms: u64,
    /// The machine whose memory follows what is loaded here, when a test asks for that.
    pub machine: Option<Machine>,
}

#[derive(Default)]
pub struct LoadBarrier {
    state: Mutex<(bool, bool, bool)>, // entered, released, cancelled
    changed: Condvar,
}

impl LoadBarrier {
    fn enter(&self) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 && !state.2 {
            let now = std::time::Instant::now();
            if now >= deadline { return false; }
            state = self.changed.wait_timeout(state, deadline - now).unwrap().0;
        }
        !state.2
    }

    fn cancelled(&self) -> bool { self.state.lock().unwrap().2 }

    fn cancel(&self) {
        self.state.lock().unwrap().2 = true;
        self.changed.notify_all();
    }
}

/// Only the synthetic HTTP response is held; the daemon sampler and RPCs remain real.
/// Dropping the guard cancels a held response so a failed test cannot leave a load blocked.
pub struct LoadGate(Arc<LoadBarrier>);

impl LoadGate {
    pub fn wait_entered(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut state = self.0.state.lock().unwrap();
        while !state.0 {
            let now = std::time::Instant::now();
            assert!(now < deadline, "synthetic load never reached its response gate");
            state = self.0.changed.wait_timeout(state, deadline - now).unwrap().0;
        }
    }

    pub fn release(&self) {
        self.0.state.lock().unwrap().1 = true;
        self.0.changed.notify_all();
    }
}

impl Drop for LoadGate {
    fn drop(&mut self) { self.0.cancel(); }
}

/// A machine's memory as a file the daemon reads (`OVERSEER_TEST_MEMORY`): what is available is
/// the total, less what everything else uses, less the models loaded here.
pub struct Machine {
    pub file: std::path::PathBuf,
    pub total: u64,
    pub others: u64,
    pub pressure: String,
}

impl State {
    pub fn write_memory(&self) {
        let Some(m) = &self.machine else { return };
        let held: u64 = self.loaded.iter().filter_map(|l| l["size"].as_u64()).sum();
        let available = m.total.saturating_sub(m.others).saturating_sub(held);
        let tmp = m.file.with_extension("tmp");
        std::fs::write(&tmp, json!({"total": m.total, "available": available, "pressure": m.pressure}).to_string()).unwrap();
        std::fs::rename(&tmp, &m.file).unwrap();
    }
}

pub struct Ollama {
    pub port: u16,
    pub state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

/// One installed model: the entry of `/api/tags` and the answer of `/api/show`.
#[allow(clippy::too_many_arguments)]
pub fn model(tag: &str, parent: &str, bytes: u64, arch: &str, blocks: u64, kv: Value, key: u64, parameters: u64, max_context: u64, num_ctx: Option<u64>, capabilities: &[&str]) -> (Value, Value) {
    let details = json!({"parent_model": parent, "format": "gguf", "family": arch, "families": [arch], "parameter_size": format!("{:.1}B", parameters as f64 / 1e9), "quantization_level": "Q4_K_M", "context_length": max_context});
    let entry = json!({"name": tag, "model": tag, "size": bytes, "digest": format!("{:064x}", bytes), "details": details, "capabilities": capabilities});
    let mut info = json!({"general.architecture": arch, "general.parameter_count": parameters});
    info[format!("{arch}.block_count")] = json!(blocks);
    info[format!("{arch}.context_length")] = json!(max_context);
    info[format!("{arch}.attention.head_count_kv")] = kv;
    info[format!("{arch}.attention.key_length")] = json!(key);
    info[format!("{arch}.attention.value_length")] = json!(key);
    let show = json!({"parameters": num_ctx.map(|n| format!("num_ctx                        {n}\ntemperature                    0.7")).unwrap_or_else(|| "temperature                    0.7".into()), "details": details, "capabilities": capabilities, "model_info": info});
    (entry, show)
}

pub fn qwen3_coder_30b() -> (Value, Value) {
    model("qwen3-coder:30b", "", 18_556_700_761, "qwen3moe", 48, json!(4), 128, 30_532_122_624, 262_144, None, &["completion", "tools"])
}
pub fn qwen3_coder_30b_64k() -> (Value, Value) {
    model("qwen3-coder:30b-64k", "qwen3-coder:30b", 18_556_700_444, "qwen3moe", 48, json!(4), 128, 30_532_122_624, 262_144, Some(65536), &["completion", "tools"])
}
pub fn qwen25_coder_14b() -> (Value, Value) {
    model("qwen2.5-coder:14b", "", 8_988_124_298, "qwen2", 48, json!(8), 128, 14_770_033_664, 32_768, None, &["completion", "tools", "insert"])
}
pub fn qwen25_coder_32b() -> (Value, Value) {
    model("qwen2.5-coder:32b", "", 19_851_349_898, "qwen2", 64, json!(8), 128, 32_763_876_352, 32_768, None, &["completion", "tools", "insert"])
}
pub fn qwen35_122b() -> (Value, Value) {
    let mut kv = vec![0u64; 48];
    for i in (3..48).step_by(4) {
        kv[i] = 2;
    }
    model("qwen3.5:122b", "", 81_400_000_000, "qwen35moe", 48, json!(kv), 256, 125_086_497_008, 262_144, None, &["completion", "vision", "tools", "thinking"])
}

impl Ollama {
    pub fn start() -> Ollama {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (state.clone(), stop.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(conn) = conn {
                    let s = s.clone();
                    std::thread::spawn(move || {
                        let _ = handle(conn, s);
                    });
                }
            }
        });
        Ollama { port, state, stop }
    }

    pub fn hold_next_load(&self) -> LoadGate {
        let barrier = Arc::new(LoadBarrier::default());
        let mut state = self.state.lock().unwrap();
        assert!(state.next_load_gate.is_none(), "a synthetic next-load gate is already armed");
        state.next_load_gate = Some(barrier.clone());
        LoadGate(barrier)
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn install(&self, (entry, show): (Value, Value)) -> &Self {
        let mut s = self.state.lock().unwrap();
        s.show.insert(entry["name"].as_str().unwrap().to_string(), show);
        s.tags.push(entry);
        self
    }

    pub fn set_loaded(&self, tag: &str, bytes: u64, context: u64) {
        let mut s = self.state.lock().unwrap();
        s.loaded.retain(|m| m["name"] != tag);
        s.loaded.push(json!({"name": tag, "model": tag, "size": bytes, "size_vram": bytes, "context_length": context, "expires_at": "2026-09-27T00:00:00Z"}));
    }

    /// From now on the machine's memory follows what is loaded: `others` GiB are used by
    /// everything else.
    pub fn machine(&self, file: &std::path::Path, total_gib: f64, others_gib: f64) {
        let mut s = self.state.lock().unwrap();
        s.machine = Some(Machine { file: file.to_path_buf(), total: (total_gib * GIB as f64) as u64, others: (others_gib * GIB as f64) as u64, pressure: "normal".into() });
        s.write_memory();
    }

    /// Everything else on the machine now uses this much, at this pressure.
    pub fn others(&self, others_gib: f64, pressure: &str) {
        let mut s = self.state.lock().unwrap();
        if let Some(m) = s.machine.as_mut() {
            m.others = (others_gib * GIB as f64) as u64;
            m.pressure = pressure.into();
        }
        s.write_memory();
    }

    pub fn loaded(&self) -> Vec<String> {
        self.state.lock().unwrap().loaded.iter().map(|l| l["name"].as_str().unwrap().to_string()).collect()
    }

    /// Offers a model for download.
    pub fn offer(&self, (entry, show): (Value, Value)) -> &Self {
        self.state.lock().unwrap().registry.insert(entry["name"].as_str().unwrap().to_string(), (entry, show));
        self
    }

    /// Bodies of the requests made to `path`.
    pub fn asked(&self, path: &str) -> Vec<Value> {
        self.state.lock().unwrap().requests.iter().filter(|(_, p, _)| p == path).map(|(_, _, b)| b.clone()).collect()
    }
}

impl Drop for Ollama {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn handle(conn: TcpStream, state: Arc<Mutex<State>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(conn.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut parts = first.split_whitespace();
    let (method, path) = (parts.next().unwrap_or_default().to_string(), parts.next().unwrap_or_default().to_string());
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.lock().unwrap().requests.push((method.clone(), path.clone(), body.clone()));
    if method == "POST" && path == "/api/pull" {
        return pull(conn, &body, &state);
    }
    let (code, answer) = respond(&method, &path, &body, &state);
    let text = answer.to_string();
    let mut conn = conn;
    write!(conn, "HTTP/1.1 {code} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", if code == 200 { "OK" } else { "Error" }, text.len())?;
    conn.flush()
}

fn latest(name: &str) -> String {
    match name.rsplit('/').next() {
        Some(last) if !last.contains(':') => format!("{name}:latest"),
        _ => name.to_string(),
    }
}

fn respond(method: &str, path: &str, body: &Value, state: &Arc<Mutex<State>>) -> (u16, Value) {
    match (method, path) {
        ("GET", "/api/version") => (200, json!({"version": "0.34.2"})),
        ("GET", "/api/tags") => (200, json!({"models": state.lock().unwrap().tags})),
        ("GET", "/api/ps") => (200, json!({"models": state.lock().unwrap().loaded})),
        ("POST", "/api/show") => match state.lock().unwrap().show.get(body["model"].as_str().unwrap_or_default()) {
            Some(s) => (200, s.clone()),
            None => (404, json!({"error": "model not found"})),
        },
        ("POST", "/api/create") => {
            let mut s = state.lock().unwrap();
            // As Ollama does, a name without a tag is kept as `<name>:latest`.
            let (name, from) = (latest(body["model"].as_str().unwrap_or_default()), body["from"].as_str().unwrap_or_default().to_string());
            let Some(base) = s.tags.iter().find(|t| t["name"] == from.as_str()).cloned() else { return (404, json!({"error": "model not found"})) };
            let mut entry = base;
            entry["name"] = json!(name);
            entry["model"] = json!(name);
            entry["details"]["parent_model"] = json!(from);
            let mut show = s.show.get(&from).cloned().unwrap_or(Value::Null);
            show["details"]["parent_model"] = json!(from);
            if let Some(n) = body["parameters"]["num_ctx"].as_u64() {
                show["parameters"] = json!(format!("num_ctx                        {n}"));
            }
            s.show.insert(name, show);
            s.tags.push(entry);
            (200, json!({"status": "success"}))
        }
        ("POST", "/api/generate") => {
            let tag = latest(body["model"].as_str().unwrap_or_default());
            if body["keep_alive"] == 0 {
                {
                    let mut s = state.lock().unwrap();
                    // Serialize cancellation with the final insertion into /api/ps. The gate
                    // wait releases its own lock and never holds this fixture-state lock.
                    if let Some(gate) = s.pending_loads.remove(&tag) { gate.cancel(); }
                    s.loaded.retain(|m| m["name"] != tag.as_str());
                    s.write_memory();
                }
                return (200, json!({"model": tag, "done": true, "done_reason": "unload"}));
            }
            let (wait, known, gate) = {
                let mut s = state.lock().unwrap();
                let gate = s.next_load_gate.take();
                if let Some(gate) = &gate { s.pending_loads.insert(tag.clone(), gate.clone()); }
                (s.load_ms, s.tags.iter().any(|t| t["name"] == tag.as_str()), gate)
            };
            if !known {
                return (404, json!({"error": format!("model '{tag}' not found")}));
            }
            if let Some(gate) = &gate {
                if !gate.enter() {
                    state.lock().unwrap().pending_loads.remove(&tag);
                    return (200, json!({"model": tag, "done": true, "done_reason": "cancelled"}));
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(wait));
            }
            let mut s = state.lock().unwrap();
            s.pending_loads.remove(&tag);
            if gate.as_ref().is_some_and(|gate| gate.cancelled()) {
                return (200, json!({"model": tag, "done": true, "done_reason": "cancelled"}));
            }
            let disk = s.tags.iter().find(|t| t["name"] == tag.as_str()).and_then(|t| t["size"].as_u64()).unwrap_or(0);
            let bytes = s.loaded_size.get(&tag).copied().unwrap_or(disk + 6 * GIB);
            let context = body["options"]["num_ctx"].as_u64().unwrap_or(4096);
            s.loaded.retain(|m| m["name"] != tag.as_str());
            s.loaded.push(json!({"name": tag, "model": tag, "size": bytes, "size_vram": bytes, "context_length": context, "expires_at": "2026-09-27T00:00:00Z"}));
            s.write_memory();
            (200, json!({"model": tag, "done": true, "done_reason": "load"}))
        }
        _ => (404, json!({"error": format!("the fixture does not answer {method} {path}")})),
    }
}

/// `/api/pull`: progress lines over time, as Ollama streams them. A client that goes away stops
/// the pull; what was reached is kept for the next one.
fn pull(mut conn: TcpStream, body: &Value, state: &Arc<Mutex<State>>) -> std::io::Result<()> {
    let name = body["model"].as_str().unwrap_or_default().to_string();
    let (offered, steps, wait, start) = {
        let s = state.lock().unwrap();
        (s.registry.get(&name).cloned(), s.pull_steps.max(1), s.pull_step_ms, s.pulled.get(&name).copied().unwrap_or(0))
    };
    conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")?;
    let mut send = |conn: &mut TcpStream, v: Value| -> std::io::Result<()> {
        let line = format!("{v}\n");
        write!(conn, "{:x}\r\n{line}\r\n", line.len())?;
        conn.flush()
    };
    let Some((entry, show)) = offered else {
        send(&mut conn, json!({"error": "pull model manifest: file does not exist"}))?;
        return conn.write_all(b"0\r\n\r\n");
    };
    let total = entry["size"].as_u64().unwrap_or(0);
    let digest = format!("sha256:{:x}", total);
    send(&mut conn, json!({"status": "pulling manifest"}))?;
    let first = start * steps / total.max(1);
    for i in first..=steps {
        let completed = total * i / steps;
        send(&mut conn, json!({"status": format!("pulling {}", &digest[7..]), "digest": digest, "total": total, "completed": completed}))?;
        state.lock().unwrap().pulled.insert(name.clone(), completed);
        std::thread::sleep(std::time::Duration::from_millis(wait));
    }
    for status in ["verifying sha256 digest", "writing manifest"] {
        send(&mut conn, json!({"status": status}))?;
    }
    {
        let mut s = state.lock().unwrap();
        s.show.insert(name.clone(), show);
        s.tags.retain(|t| t["name"] != name.as_str());
        s.tags.push(entry);
    }
    send(&mut conn, json!({"status": "success"}))?;
    conn.write_all(b"0\r\n\r\n")
}
