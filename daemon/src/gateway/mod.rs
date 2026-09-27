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

/// The kind and the handshake message of a first frame, or `None` when it is not one.
pub fn first_frame(frame: &[u8]) -> Option<(noise::Kind, &[u8])> {
    if frame.len() <= 2 || frame.len() > 4096 || frame[0] != noise::FRAME_VERSION {
        return None;
    }
    Some((noise::Kind::from_byte(frame[1])?, &frame[2..]))
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
    let Some((kind, _)) = first_frame(&first) else {
        d.gateway.failed(addr, "malformed handshake");
        return Ok(());
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
            let Some(device) = device else {
                d.gateway.failed(addr, "unknown device");
                return Ok(());
            };
            if device.revoked_ms.is_some() {
                // A phone that was removed while it was away must learn it, or it would try
                // forever. Only the holder of the paired key can read this: the handshake is
                // completed, the notice is sent, and nothing else is ever answered.
                crate::log(&format!("gateway: {addr}: \"{}\" was removed on the Mac and was told so", device.name));
                return tell_revoked(hs, ws, &device, &identity).await;
            }
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
            let opened_with = psk;
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
            {
                // This request's pairing is over. One that was opened since is left alone.
                let mut guard = d.gateway.pairing.lock().unwrap();
                if guard.as_ref().is_some_and(|p| p.psk == opened_with) {
                    *guard = None;
                }
            }
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

/// Completes the handshake of a revoked device, tells it, and closes.
async fn tell_revoked(mut handshake: snow::HandshakeState, mut ws: WebSocketStream<TcpStream>, device: &Device, identity: &Identity) -> Result<()> {
    let reply = json!({"protocol": crate::server::PROTOCOL_VERSION, "device": device.id, "scope": "revoked", "gateway": net::host_name(), "fingerprint": identity.fingerprint()}).to_string();
    let mut message = vec![0u8; reply.len() + 128];
    let n = handshake.write_message(reply.as_bytes(), &mut message)?;
    message.truncate(n);
    ws.send(Message::Binary(message.into())).await?;
    let mut transport = handshake.into_transport_mode()?;
    for frame in noise::seal(&mut transport, json!({"method": "gateway", "params": {"state": "revoked"}}).to_string().as_bytes())? {
        ws.send(Message::Binary(frame.into())).await?;
    }
    let _ = ws.close(None).await;
    Ok(())
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

#[cfg(test)]
mod fuzz {
    //! A fuzz test of the handshake and the frame parser (AC-130): random, mutated, truncated and
    //! oversized input. Nothing may crash, nothing may be accepted, and nothing may be answered.
    //! Seeded; a failure names its round.
    use super::*;

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }

        fn bytes(&mut self, len: usize) -> Vec<u8> {
            (0..len).map(|_| self.next() as u8).collect()
        }
    }

    fn mutate(rng: &mut Rng, good: &[u8]) -> Vec<u8> {
        let mut out = good.to_vec();
        match rng.below(6) {
            0 => {
                let at = rng.below(out.len());
                out[at] ^= 1 << rng.below(8);
            }
            1 => out.truncate(rng.below(out.len())),
            2 => {
                let n = 1 + rng.below(64);
                out.extend(rng.bytes(n));
            }
            3 => {
                let at = rng.below(out.len());
                out.insert(at, rng.next() as u8);
            }
            4 => {
                let (a, b) = (rng.below(out.len()), rng.below(out.len()));
                out.swap(a, b);
            }
            _ => {
                let at = rng.below(out.len());
                let n = rng.below(out.len() - at).max(1);
                let noise = rng.bytes(n);
                out[at..at + n].copy_from_slice(&noise);
            }
        }
        out
    }

    /// The evidence file is written only when asked, so an ordinary test run leaves it alone.
    fn write_log(name: &str, text: &str) {
        if std::env::var_os("OVERSEER_WRITE_EVIDENCE").is_none() {
            return;
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/verification/evidence/phone/daemon");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(name), text);
    }

    #[test]
    fn the_handshake_and_the_frame_parser_survive_fuzzing() {
        let started = Instant::now();
        // The full run is the optimized build's; an ordinary debug run does a fortieth of it.
        let scale: u64 = if cfg!(debug_assertions) { 40 } else { 1 };
        let seed = 0x0_5EED_2026_0926u64;
        let mut rng = Rng(seed);
        // Keys from the seed, so every run sees the same inputs.
        let pair = |rng: &mut Rng| {
            let private = rng.bytes(32);
            noise::Keypair { public: public_of(&private), private }
        };
        let gateway = pair(&mut rng);
        let phone = pair(&mut rng);
        let secret = [9u8; 16];
        let psk = noise::psk_from_secret(&secret);
        let payload = br#"{"device":"d","name":"Fuzz","platform":"ios","app":"0","counter":1}"#;
        let (mut accepted_handshakes, mut rejected_handshakes, mut first_frames_rejected, mut first_frames_parsed) = (0u64, 0u64, 0u64, 0u64);

        // 1. First frames: random bytes of every length, and mutations of good ones.
        let mut good = Vec::new();
        for kind in [noise::Kind::Session, noise::Kind::Pairing] {
            let mut hs = noise::initiator(kind, &phone.private, &gateway.public, if kind == noise::Kind::Pairing { Some(&psk) } else { None }).unwrap();
            let mut m1 = vec![0u8; 1024];
            let n = hs.write_message(payload, &mut m1).unwrap();
            let mut frame = vec![noise::FRAME_VERSION, if kind == noise::Kind::Session { noise::KIND_SESSION } else { noise::KIND_PAIRING }];
            frame.extend_from_slice(&m1[..n]);
            good.push((kind, frame));
        }
        for round in 0..200_000 / scale {
            let (kind, base) = &good[rng.below(2)];
            let frame = if round % 3 == 0 { let n = rng.below(5000); rng.bytes(n) } else { mutate(&mut rng, base) };
            let Some((seen_kind, message)) = first_frame(&frame) else {
                first_frames_rejected += 1;
                continue;
            };
            first_frames_parsed += 1;
            let mut hs = noise::responder(seen_kind, &gateway.private, if seen_kind == noise::Kind::Pairing { Some(&psk) } else { None }).unwrap();
            let mut out = vec![0u8; frame.len() + 64];
            match hs.read_message(message, &mut out) {
                Ok(n) => {
                    // Only the untouched message may be accepted: anything else that decrypts is a break.
                    assert!(frame == *base && seen_kind == *kind, "seed {seed:#x} round {round}: a changed handshake was accepted");
                    assert_eq!(&out[..n], payload);
                    accepted_handshakes += 1;
                }
                Err(_) => rejected_handshakes += 1,
            }
        }

        // 2. Transport frames after a good handshake: random and mutated frames never open.
        let mut i = noise::initiator(noise::Kind::Session, &phone.private, &gateway.public, None).unwrap();
        let mut r = noise::responder(noise::Kind::Session, &gateway.private, None).unwrap();
        let (mut a, mut b) = (vec![0u8; 1024], vec![0u8; 1024]);
        let n = i.write_message(payload, &mut a).unwrap();
        r.read_message(&a[..n], &mut b).unwrap();
        let n = r.write_message(b"{}", &mut a).unwrap();
        i.read_message(&a[..n], &mut b).unwrap();
        let (mut phone_side, mut gateway_side) = (i.into_transport_mode().unwrap(), r.into_transport_mode().unwrap());
        let (mut frames_rejected, mut frames_opened) = (0u64, 0u64);
        for round in 0..100_000 / scale {
            let n = rng.below(300);
            let message = rng.bytes(n);
            let frames = noise::seal(&mut phone_side, &message).unwrap();
            let mut opener = noise::Opener::new(1 << 20);
            let bad = if round % 4 == 0 { let n = rng.below(70_000); rng.bytes(n) } else { mutate(&mut rng, &frames[0]) };
            if bad != frames[0] {
                assert!(opener.open(&mut gateway_side, &bad).is_err(), "seed {seed:#x} round {round}: a changed frame opened");
                frames_rejected += 1;
            }
            // The real frame still opens afterwards: a bad frame does not move the counters.
            let mut opener = noise::Opener::new(1 << 20);
            let mut joined = None;
            for f in &frames {
                joined = opener.open(&mut gateway_side, f).unwrap();
            }
            assert_eq!(joined.unwrap(), message, "seed {seed:#x} round {round}");
            frames_opened += 1;
        }

        // 3. The pairing code and the address rules with random text.
        for _ in 0..50_000 / scale {
            let n = rng.below(40);
            let text = String::from_utf8_lossy(&rng.bytes(n)).to_string();
            let _ = net::Cidr::parse(&text);
            let _ = base32::decode(&text);
            let _ = classes::class_of(&text);
            let _ = clean(&text, 60);
        }
        let log = [
            "fuzz of the gateway's handshake and frame parser (AC-130)".to_string(),
            format!("seed {seed:#x} for the static keys and the mutations; each handshake's own one-time keys are fresh, so counts differ a little between runs"),
            String::new(),
            format!("first frames: {} inputs (random bytes of 0 to 5000, and mutated good handshakes)", 200_000 / scale),
            format!("  refused by the frame parser: {first_frames_rejected}"),
            format!("  parsed and given to the handshake: {first_frames_parsed}"),
            format!("  handshakes refused: {rejected_handshakes}"),
            format!("  handshakes accepted: {accepted_handshakes} (each one byte for byte the untouched message)"),
            String::new(),
            format!("transport frames: {} rounds (random bytes of 0 to 70000, and mutated good frames)", 100_000 / scale),
            format!("  changed frames refused: {frames_rejected}"),
            format!("  untouched frames opened afterwards: {frames_opened}"),
            String::new(),
            format!("other parsers: {} random texts through the range, base32, method class and name cleaning", 50_000 / scale),
            String::new(),
            "crashes: 0".to_string(),
            "changed input accepted: 0".to_string(),
            format!("time: {:.1} s", started.elapsed().as_secs_f64()),
            String::new(),
        ]
        .join("\n");
        write_log("fuzz.log", &log);
        println!("{log}");
        assert_eq!(first_frames_rejected + first_frames_parsed, 200_000 / scale);
    }
}
