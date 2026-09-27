//! The phone gateway (Gate N): a second, authenticated entrance to the daemon, off by default.
//! The Unix socket in `server.rs` and its owner-only boundary (AC-08) are unchanged.
//! Wire format: docs/rfcs/phone-remote-protocol.md.

pub mod base32;
pub mod classes;
pub mod devices;
pub mod local;
pub mod net;
pub mod noise;
pub mod pairing;
pub mod power;
pub mod push;
pub mod remote;

use crate::daemon::Daemon;
use crate::paths;
use anyhow::{anyhow, bail, Context, Result};
use devices::Device;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

pub const DEFAULT_PORT: u16 = 47810;
pub const PATH: &str = "/v1";
const UPGRADE_WAIT: Duration = Duration::from_secs(5);
const FIRST_FRAME_WAIT: Duration = Duration::from_secs(10);
/// A session that sent nothing for this long is ended.
const IDLE: Duration = Duration::from_secs(60);
const FAILURE_WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILURES_PER_ADDR: u32 = 10;
/// Sessions get this long to receive the notice before they are closed.
const NOTICE_GRACE: Duration = Duration::from_millis(200);

pub fn now_ms() -> i64 {
    crate::shim::now_ms() as i64 + test_clock_offset_ms()
}

/// Test-only: a duration from the environment that can only be shorter than the real one.
pub fn test_shorter(name: &str, real: Duration) -> Duration {
    match std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok()) {
        Some(ms) => real.min(Duration::from_millis(ms)),
        None => real,
    }
}

/// Test-only: moves the gateway's clock forward, to show that a pairing does not expire with time.
fn test_clock_offset_ms() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| std::env::var("OVERSEER_TEST_CLOCK_OFFSET_MS").ok().and_then(|v| v.parse::<i64>().ok()).filter(|v| *v > 0).unwrap_or(0))
}

/// Test-only: every peer is treated as coming from this address. It can only refuse more peers:
/// a peer must still pass the range check with the address given here.
fn test_peer() -> Option<IpAddr> {
    std::env::var("OVERSEER_TEST_GATEWAY_PEER").ok().and_then(|v| v.parse().ok())
}

fn idle() -> Duration {
    test_shorter("OVERSEER_TEST_GATEWAY_IDLE_MS", IDLE)
}

pub struct Identity {
    pub private: Vec<u8>,
    pub public: Vec<u8>,
}

impl Identity {
    pub fn fingerprint(&self) -> String {
        noise::fingerprint(&self.public)
    }
}

pub struct Session {
    pub device_id: String,
    pub device_name: String,
    pub addr: String,
    pub since_ms: i64,
    pub tx: mpsc::Sender<Value>,
    pub close: watch::Sender<bool>,
}

struct Listening {
    port: u16,
    stop: watch::Sender<bool>,
    mdns: Option<std::process::Child>,
}

/// One request id being run right now; later callers with the same id wait for its reply.
pub struct Inflight {
    reply: Mutex<Option<Value>>,
    done: Condvar,
}

pub struct Gateway {
    rt: OnceLock<tokio::runtime::Handle>,
    identity: Mutex<Option<Arc<Identity>>>,
    listening: Mutex<Option<Listening>>,
    pub sessions: Mutex<HashMap<u64, Session>>,
    next_session: AtomicU64,
    pub pairing: Mutex<Option<pairing::Pairing>>,
    pub inflight: Mutex<HashMap<(String, String), Arc<Inflight>>>,
    failures: Mutex<HashMap<IpAddr, (u32, Instant)>>,
    pub power: power::Power,
    /// The agent each window on the Mac is focused on, by connection.
    pub focus: Mutex<HashMap<u64, String>>,
}

impl Gateway {
    pub fn new() -> Self {
        Self {
            rt: OnceLock::new(),
            identity: Mutex::new(None),
            listening: Mutex::new(None),
            sessions: Mutex::new(HashMap::new()),
            next_session: AtomicU64::new(1),
            pairing: Mutex::new(None),
            inflight: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
            power: power::Power::default(),
            focus: Mutex::new(HashMap::new()),
        }
    }

    pub fn port(&self) -> Option<u16> {
        self.listening.lock().unwrap().as_ref().map(|l| l.port)
    }

    fn handle(&self) -> Result<tokio::runtime::Handle> {
        self.rt.get().cloned().ok_or_else(|| anyhow!("the gateway has no runtime yet"))
    }

    /// The gateway's key pair, created the first time phone access is turned on.
    pub fn identity(&self) -> Result<Arc<Identity>> {
        let mut slot = self.identity.lock().unwrap();
        if let Some(id) = slot.as_ref() {
            return Ok(id.clone());
        }
        let dir = paths::data_dir().join("gateway");
        paths::ensure_private_dir(&dir)?;
        let file = dir.join("identity.key");
        let private = match std::fs::read_to_string(&file) {
            Ok(text) => noise::unhex(text.trim()).context("gateway/identity.key is damaged")?,
            Err(_) => {
                let kp = noise::generate_keypair()?;
                write_private(&file, &noise::hex(&kp.private))?;
                kp.private
            }
        };
        if private.len() != 32 {
            bail!("gateway/identity.key is damaged");
        }
        let public = public_of(&private);
        let id = Arc::new(Identity { private, public });
        *slot = Some(id.clone());
        Ok(id)
    }

    /// True when this address failed too many handshakes in the last minute.
    fn limited(&self, ip: IpAddr) -> bool {
        let mut map = self.failures.lock().unwrap();
        map.retain(|_, (_, since)| since.elapsed() < FAILURE_WINDOW);
        map.get(&ip).is_some_and(|(n, _)| *n >= MAX_FAILURES_PER_ADDR)
    }

    fn failed(&self, addr: SocketAddr, why: &str) {
        let mut map = self.failures.lock().unwrap();
        let entry = map.entry(addr.ip()).or_insert((0, Instant::now()));
        entry.0 += 1;
        crate::log(&format!("gateway: refused {addr}: {why} (failure {} of {MAX_FAILURES_PER_ADDR} this minute)", entry.0));
    }

    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

impl Default for Gateway {
    fn default() -> Self {
        Self::new()
    }
}

fn write_private(path: &std::path::Path, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(path)?;
    f.write_all(text.as_bytes())?;
    Ok(())
}

pub fn public_of(private: &[u8]) -> Vec<u8> {
    use snow::resolvers::{CryptoResolver, DefaultResolver};
    let mut dh = DefaultResolver.resolve_dh(&snow::params::DHChoice::Curve25519).expect("curve25519");
    dh.set(private);
    dh.pubkey().to_vec()
}

// ------------------------------------------------------------------ settings

pub fn setting(d: &Daemon, key: &str) -> Option<String> {
    d.store.lock().unwrap().meta_get(&format!("gateway.{key}")).ok().flatten()
}

pub fn set_setting(d: &Daemon, key: &str, value: &str) -> Result<()> {
    d.store.lock().unwrap().meta_set(&format!("gateway.{key}"), value)
}

pub fn allowed_ranges(d: &Daemon) -> Vec<net::Cidr> {
    setting(d, "allow").unwrap_or_default().split(',').filter_map(net::Cidr::parse).collect()
}

pub fn configured_port(d: &Daemon) -> u16 {
    std::env::var("OVERSEER_GATEWAY_PORT").ok().and_then(|v| v.parse().ok()).or_else(|| setting(d, "port").and_then(|v| v.parse().ok())).unwrap_or(DEFAULT_PORT)
}

// ------------------------------------------------------------------ on and off

/// Called once when the daemon starts serving: phone access comes back on when it was on.
pub fn start(d: &Arc<Daemon>) {
    let _ = d.gateway.rt.set(tokio::runtime::Handle::current());
    let _ = d.store.lock().unwrap().requests_prune(now_ms());
    stop_stale_advertiser();
    power::watch(d.clone());
    push::watch(d.clone());
    if setting(d, "enabled").as_deref() == Some("1") {
        match enable(d, None) {
            Ok(v) => crate::log(&format!("gateway: phone access is on (port {})", v["port"])),
            Err(e) => crate::log(&format!("gateway: phone access could not start: {e:#}")),
        }
    }
}

pub fn enable(d: &Arc<Daemon>, port: Option<u16>) -> Result<Value> {
    let identity = d.gateway.identity()?;
    if let Some(l) = d.gateway.listening.lock().unwrap().as_ref() {
        return Ok(json!({"enabled": true, "port": l.port, "fingerprint": identity.fingerprint(), "changed": false}));
    }
    let wanted = port.unwrap_or_else(|| configured_port(d));
    let handle = d.gateway.handle()?;
    let std_listener = std::net::TcpListener::bind(("0.0.0.0", wanted)).with_context(|| format!("port {wanted} is not available"))?;
    std_listener.set_nonblocking(true)?;
    let actual = std_listener.local_addr()?.port();
    let (stop, stop_rx) = watch::channel(false);
    {
        let daemon = d.clone();
        let _guard = handle.enter();
        let listener = TcpListener::from_std(std_listener)?;
        handle.spawn(accept_loop(daemon, listener, stop_rx));
    }
    let mdns = advertise(actual, &identity.fingerprint());
    *d.gateway.listening.lock().unwrap() = Some(Listening { port: actual, stop, mdns });
    set_setting(d, "enabled", "1")?;
    if port.is_some() && wanted != 0 {
        set_setting(d, "port", &wanted.to_string())?;
    }
    d.emit(None, None, "gateway_state", "gateway", "exact", json!({"state": "on", "port": actual, "fingerprint": identity.fingerprint()}))?;
    Ok(json!({"enabled": true, "port": actual, "fingerprint": identity.fingerprint(), "changed": true}))
}

pub fn disable(d: &Arc<Daemon>) -> Result<Value> {
    set_setting(d, "enabled", "0")?;
    *d.gateway.pairing.lock().unwrap() = None;
    let Some(mut listening) = d.gateway.listening.lock().unwrap().take() else {
        return Ok(json!({"enabled": false, "changed": false, "closed": 0}));
    };
    let _ = listening.stop.send(true);
    if let Some(mut child) = listening.mdns.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(mdns_pid_file());
    let closed = end_sessions(d, |_| true, "off");
    d.emit(None, None, "gateway_state", "gateway", "exact", json!({"state": "off", "closed": closed}))?;
    Ok(json!({"enabled": false, "changed": true, "closed": closed}))
}

/// Tells the matching sessions why, then closes them. Returns how many.
pub fn end_sessions(d: &Arc<Daemon>, which: impl Fn(&Session) -> bool, state: &str) -> usize {
    let targets: Vec<(mpsc::Sender<Value>, watch::Sender<bool>)> = d.gateway.sessions.lock().unwrap().values().filter(|s| which(s)).map(|s| (s.tx.clone(), s.close.clone())).collect();
    let n = targets.len();
    for (tx, _) in &targets {
        let _ = tx.try_send(json!({"method": "gateway", "params": {"state": state}}));
    }
    if let Ok(handle) = d.gateway.handle() {
        handle.spawn(async move {
            tokio::time::sleep(NOTICE_GRACE).await;
            for (_, close) in targets {
                let _ = close.send(true);
            }
        });
    }
    n
}

// ------------------------------------------------------------------ discovery

fn mdns_pid_file() -> std::path::PathBuf {
    paths::data_dir().join("gateway").join("advertiser.pid")
}

/// Advertises `_overseer._tcp` while phone access is on. Bonjour on macOS, Avahi on Linux.
fn advertise(port: u16, fingerprint: &str) -> Option<std::process::Child> {
    if std::env::var("OVERSEER_GATEWAY_MDNS").as_deref() == Ok("off") {
        return None;
    }
    let name = format!("Overseer on {}", net::host_name());
    let txt = format!("fp={fingerprint}");
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("/usr/bin/dns-sd");
        c.args(["-R", &name, "_overseer._tcp", "local", &port.to_string(), &txt]);
        c
    } else {
        let mut c = std::process::Command::new("avahi-publish");
        c.args(["-s", &name, "_overseer._tcp", &port.to_string(), &txt]);
        c
    };
    match cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        Ok(child) => {
            let _ = std::fs::write(mdns_pid_file(), child.id().to_string());
            Some(child)
        }
        Err(e) => {
            crate::log(&format!("gateway: not advertised on the network: {e}"));
            None
        }
    }
}

/// An advertiser left behind by a daemon that was killed would announce a closed port.
fn stop_stale_advertiser() {
    let Ok(text) = std::fs::read_to_string(mdns_pid_file()) else { return };
    let _ = std::fs::remove_file(mdns_pid_file());
    let Ok(pid) = text.trim().parse::<i32>() else { return };
    let out = std::process::Command::new("/bin/ps").args(["-o", "command=", "-p", &pid.to_string()]).output();
    if out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("_overseer._tcp")) {
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
}

// ------------------------------------------------------------------ connections

async fn accept_loop(d: Arc<Daemon>, listener: TcpListener, mut stop: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, addr)) = accepted else { continue };
                let seen_as = test_peer().unwrap_or(addr.ip());
                if !net::is_allowed(addr.ip(), &allowed_ranges(&d)) || !net::is_allowed(seen_as, &allowed_ranges(&d)) {
                    crate::log(&format!("gateway: refused {seen_as}: outside the local network"));
                    continue;
                }
                if d.gateway.limited(addr.ip()) {
                    continue;
                }
                let d = d.clone();
                tokio::spawn(async move {
                    if let Err(e) = connection(d, stream, addr).await {
                        crate::log(&format!("gateway: {addr}: {e:#}"));
                    }
                });
            }
        }
    }
    crate::log("gateway: phone access is off");
}

fn only_our_path(req: &Request, resp: Response) -> std::result::Result<Response, ErrorResponse> {
    if req.uri().path() == PATH {
        Ok(resp)
    } else {
        let mut no = ErrorResponse::new(None);
        *no.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::NOT_FOUND;
        Err(no)
    }
}

async fn next_binary(ws: &mut WebSocketStream<TcpStream>) -> Option<Vec<u8>> {
    loop {
        match ws.next().await? {
            Ok(Message::Binary(b)) => return Some(b.to_vec()),
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => continue,
            _ => return None,
        }
    }
}

fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

async fn connection(d: Arc<Daemon>, stream: TcpStream, addr: SocketAddr) -> Result<()> {
    let _ = stream.set_nodelay(true);
    let config = WebSocketConfig::default().max_message_size(Some(70_000)).max_frame_size(Some(70_000));
    let mut ws = match tokio::time::timeout(UPGRADE_WAIT, tokio_tungstenite::accept_hdr_async_with_config(stream, only_our_path, Some(config))).await {
        Ok(Ok(ws)) => ws,
        _ => {
            d.gateway.failed(addr, "no WebSocket upgrade");
            return Ok(());
        }
    };
    let Some(first) = tokio::time::timeout(FIRST_FRAME_WAIT, next_binary(&mut ws)).await.ok().flatten() else {
        d.gateway.failed(addr, "no handshake");
        return Ok(());
    };
    let kind = match (first.first(), first.get(1).and_then(|b| noise::Kind::from_byte(*b))) {
        (Some(&noise::FRAME_VERSION), Some(kind)) if first.len() > 2 && first.len() <= 4096 => kind,
        _ => {
            d.gateway.failed(addr, "malformed handshake");
            return Ok(());
        }
    };
    let identity = d.gateway.identity()?;
    let mut payload = vec![0u8; first.len()];
    let (mut handshake, device) = match kind {
        noise::Kind::Session => {
            let mut hs = noise::responder(kind, &identity.private, None)?;
            let Ok(n) = hs.read_message(&first[2..], &mut payload) else {
                d.gateway.failed(addr, "the handshake did not decrypt");
                return Ok(());
            };
            let hello: Value = serde_json::from_slice(&payload[..n]).unwrap_or(Value::Null);
            let key = noise::hex(hs.get_remote_static().unwrap_or_default());
            let device = d.store.lock().unwrap().device_by_key(&key)?;
            let Some(device) = device.filter(|dev| dev.revoked_ms.is_none()) else {
                d.gateway.failed(addr, "unknown or revoked device");
                return Ok(());
            };
            let counter = hello["counter"].as_i64().unwrap_or(0);
            let app = hello["app"].as_str().map(|a| clean(a, 40));
            if !d.store.lock().unwrap().device_accept_counter(&device.id, counter, now_ms(), &addr.ip().to_string(), app.as_deref())? {
                d.gateway.failed(addr, "a replayed handshake");
                return Ok(());
            }
            (hs, device)
        }
        noise::Kind::Pairing => {
            let psk = match d.gateway.pairing.lock().unwrap().as_ref() {
                Some(p) if p.accepts_attempts() => p.psk,
                _ => {
                    d.gateway.failed(addr, "pairing is not open");
                    return Ok(());
                }
            };
            let mut hs = noise::responder(kind, &identity.private, Some(&psk))?;
            let Ok(n) = hs.read_message(&first[2..], &mut payload) else {
                let closed = {
                    let mut guard = d.gateway.pairing.lock().unwrap();
                    let closed = guard.as_mut().map(|p| {
                        p.failures += 1;
                        p.failures >= pairing::MAX_FAILURES
                    });
                    if closed == Some(true) {
                        *guard = None;
                    }
                    closed == Some(true)
                };
                if closed {
                    d.emit(None, None, "pairing_closed", "gateway", "exact", json!({"reason": "too many failed attempts"}))?;
                }
                d.gateway.failed(addr, "a wrong pairing secret");
                return Ok(());
            };
            let hello: Value = serde_json::from_slice(&payload[..n]).unwrap_or(Value::Null);
            let key = hs.get_remote_static().unwrap_or_default().to_vec();
            let name = clean(hello["name"].as_str().unwrap_or("Phone"), 60);
            let name = if name.is_empty() { "Phone".to_string() } else { name };
            let platform = match hello["platform"].as_str() {
                Some("ios") => "ios",
                Some("android") => "android",
                _ => "other",
            };
            if d.store.lock().unwrap().device_by_key(&noise::hex(&key))?.is_some() {
                d.gateway.failed(addr, "this device key was paired before");
                return Ok(());
            }
            let request = uuid::Uuid::new_v4().to_string();
            let (decide, decided) = oneshot::channel();
            {
                let mut guard = d.gateway.pairing.lock().unwrap();
                let Some(p) = guard.as_mut().filter(|p| p.accepts_attempts()) else {
                    drop(guard);
                    d.gateway.failed(addr, "the pairing secret was used already");
                    return Ok(());
                };
                p.used = true;
                p.pending.insert(request.clone(), pairing::Pending { name: name.clone(), platform: platform.into(), addr: addr.ip().to_string(), fingerprint: noise::fingerprint(&key), decide });
            }
            d.emit(None, None, "pairing_request", "gateway", "exact", json!({"request": request, "name": name, "platform": platform, "address": addr.ip().to_string(), "fingerprint": noise::fingerprint(&key), "wait_ms": pairing::confirm_wait().as_millis() as u64}))?;
            let accepted = matches!(tokio::time::timeout(pairing::confirm_wait(), decided).await, Ok(Ok(true)));
            *d.gateway.pairing.lock().unwrap() = None;
            if !accepted {
                d.emit(None, None, "pairing_closed", "gateway", "exact", json!({"reason": "declined or not confirmed", "request": request, "name": name}))?;
                crate::log(&format!("gateway: {addr}: pairing of \"{name}\" was not confirmed"));
                return Ok(());
            }
            let device = Device {
                id: uuid::Uuid::new_v4().to_string(),
                name,
                platform: platform.into(),
                public_key: noise::hex(&key),
                scope: classes::Scope::Full.as_str().into(),
                paired_ms: now_ms(),
                last_seen_ms: Some(now_ms()),
                last_addr: Some(addr.ip().to_string()),
                last_counter: hello["counter"].as_i64().unwrap_or(0),
                revoked_ms: None,
                notifications: Value::Null,
                app: hello["app"].as_str().map(|a| clean(a, 40)),
            };
            d.store.lock().unwrap().insert_device(&device)?;
            d.emit(None, None, "device_paired", "gateway", "exact", json!({"device": device.id, "name": device.name, "platform": device.platform, "scope": device.scope}))?;
            (hs, device)
        }
    };
    let reply = json!({"protocol": crate::server::PROTOCOL_VERSION, "device": device.id, "scope": device.scope, "gateway": net::host_name(), "fingerprint": identity.fingerprint()}).to_string();
    let mut message = vec![0u8; reply.len() + 128];
    let n = handshake.write_message(reply.as_bytes(), &mut message)?;
    message.truncate(n);
    ws.send(Message::Binary(message.into())).await?;
    let transport = handshake.into_transport_mode()?;
    session(d, device, transport, ws, addr).await
}

async fn session(d: Arc<Daemon>, device: Device, transport: snow::TransportState, ws: WebSocketStream<TcpStream>, addr: SocketAddr) -> Result<()> {
    let (mut sink, mut stream) = ws.split();
    let transport = Arc::new(Mutex::new(transport));
    let (tx, mut rx) = mpsc::channel::<Value>(1024);
    let (close, mut closed) = watch::channel(false);
    let id = d.gateway.next_session.fetch_add(1, Ordering::SeqCst);
    d.gateway.sessions.lock().unwrap().insert(id, Session { device_id: device.id.clone(), device_name: device.name.clone(), addr: addr.ip().to_string(), since_ms: now_ms(), tx: tx.clone(), close });
    // Revoked, or phone access turned off, between the handshake and here.
    let still_welcome = d.gateway.port().is_some() && d.store.lock().unwrap().device(&device.id)?.is_some_and(|dev| dev.revoked_ms.is_none());
    d.emit(None, None, "gateway_sessions", "gateway", "exact", json!({"sessions": d.gateway.session_count(), "device": device.id, "name": device.name, "change": "connected"}))?;
    crate::log(&format!("gateway: \"{}\" connected from {addr}", device.name));

    let sealing = transport.clone();
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            let frames = {
                let mut t = sealing.lock().unwrap();
                noise::seal(&mut t, message.to_string().as_bytes())
            };
            let Ok(frames) = frames else { break };
            for frame in frames {
                if sink.send(Message::Binary(frame.into())).await.is_err() {
                    return;
                }
            }
        }
        let _ = sink.close().await;
    });

    let ctx = remote::Ctx { device_id: device.id.clone(), device_name: device.name.clone() };
    let mut opener = noise::Opener::new(crate::server::MAX_REQUEST_BYTES as usize);
    let mut why = "closed by the device";
    while still_welcome {
        tokio::select! {
            _ = closed.changed() => { why = "closed by the Mac"; break }
            next = tokio::time::timeout(idle(), stream.next()) => match next {
                Err(_) => { why = "silent for a minute"; break }
                Ok(Some(Ok(Message::Binary(frame)))) => {
                    let joined = {
                        let mut t = transport.lock().unwrap();
                        opener.open(&mut t, &frame)
                    };
                    match joined {
                        Ok(Some(message)) => remote::handle(&d, &ctx, message, &tx).await,
                        Ok(None) => {}
                        Err(_) => { why = "a frame that did not decrypt"; break }
                    }
                }
                Ok(Some(Ok(Message::Ping(_)))) | Ok(Some(Ok(Message::Pong(_)))) => {}
                Ok(Some(Err(_))) => { why = "the connection was lost"; break }
                _ => break,
            },
        }
    }
    d.gateway.sessions.lock().unwrap().remove(&id);
    drop(tx);
    writer.abort();
    let _ = d.store.lock().unwrap().device_touch(&device.id, now_ms());
    let _ = d.emit(None, None, "gateway_sessions", "gateway", "exact", json!({"sessions": d.gateway.session_count(), "device": device.id, "name": device.name, "change": "disconnected"}));
    crate::log(&format!("gateway: \"{}\" disconnected ({why})", device.name));
    Ok(())
}

impl Inflight {
    fn new() -> Self {
        Self { reply: Mutex::new(None), done: Condvar::new() }
    }

    fn finish(&self, reply: Value) {
        *self.reply.lock().unwrap() = Some(reply);
        self.done.notify_all();
    }

    fn wait(&self) -> Value {
        let mut guard = self.reply.lock().unwrap();
        while guard.is_none() {
            guard = self.done.wait(guard).unwrap();
        }
        guard.clone().unwrap()
    }
}

/// Runs a changing request at most once per device and request id, and answers every retry
/// with the first outcome. The returned value is the body of the reply: `result` or `error`.
pub fn once(d: &Arc<Daemon>, device: &str, request_id: &str, method: &str, run: impl FnOnce() -> Value) -> Value {
    let key = (device.to_string(), request_id.to_string());
    let (entry, first) = {
        let mut map = d.gateway.inflight.lock().unwrap();
        match map.get(&key) {
            Some(e) => (e.clone(), false),
            None => {
                let e = Arc::new(Inflight::new());
                map.insert(key.clone(), e.clone());
                (e, true)
            }
        }
    };
    if !first {
        return entry.wait();
    }
    let claim = d.store.lock().unwrap().request_claim(device, request_id, method, now_ms());
    let reply = match claim {
        Err(e) => json!({"error": {"code": "failed", "message": e.to_string()}}),
        Ok(devices::Earlier::Done(earlier)) => {
            let same = d.store.lock().unwrap().request_method(device, request_id).ok().flatten().as_deref() == Some(method);
            if same {
                earlier
            } else {
                json!({"error": {"code": "request_id_reused", "message": "this request id was used for another action"}})
            }
        }
        Ok(devices::Earlier::Interrupted) => json!({"error": {"code": "outcome_unknown", "message": "the Mac stopped while this was running, so it was not run again; check the agent before sending it again"}}),
        Ok(devices::Earlier::New) => {
            let reply = run();
            let _ = d.store.lock().unwrap().request_store(device, request_id, &reply);
            reply
        }
    };
    entry.finish(reply.clone());
    d.gateway.inflight.lock().unwrap().remove(&key);
    reply
}
