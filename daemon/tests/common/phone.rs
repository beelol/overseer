#![allow(dead_code)]
//! A reference phone for the gateway tests: the device's side of
//! docs/rfcs/phone-remote-protocol.md, written against the same Noise code the daemon uses.
//! The app's own implementation (`phone/core`) is checked against the shared vectors instead.

use super::noise;
use super::Daemon;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone)]
pub struct Keys {
    pub private: Vec<u8>,
    pub public: Vec<u8>,
}

impl Keys {
    pub fn new() -> Self {
        let kp = noise::generate_keypair().unwrap();
        Self { private: kp.private, public: kp.public }
    }
}

/// What a phone keeps after pairing.
#[derive(Clone)]
pub struct Paired {
    pub keys: Keys,
    pub device: String,
    pub gateway_public: Vec<u8>,
    pub port: u16,
    pub counter: i64,
    pub name: String,
}

pub struct Code {
    pub gateway_public: Vec<u8>,
    pub secret: Vec<u8>,
    pub port: u16,
    pub addresses: Vec<String>,
}

pub fn parse_code(text: &str) -> Code {
    let body = text.strip_prefix("OVSR1-").expect("pairing code prefix");
    let bytes = super::base32::decode(body).expect("base32");
    assert_eq!(bytes[0], 1);
    let mut addresses = Vec::new();
    let mut at = 52;
    for _ in 0..bytes[51] {
        let len = bytes[at] as usize;
        addresses.push(String::from_utf8(bytes[at + 1..at + 1 + len].to_vec()).unwrap());
        at += 1 + len;
    }
    assert_eq!(at, bytes.len(), "no trailing bytes");
    Code { gateway_public: bytes[1..33].to_vec(), secret: bytes[33..49].to_vec(), port: u16::from_be_bytes([bytes[49], bytes[50]]), addresses }
}

pub struct Phone {
    pub ws: Ws,
    pub transport: snow::TransportState,
    opener: noise::Opener,
    next_id: i64,
    /// Notifications received while waiting for something else, in order.
    pub inbox: Vec<Value>,
    pub hello: Value,
}

pub fn url(port: u16) -> String {
    format!("ws://127.0.0.1:{port}/v1")
}

pub async fn open(port: u16) -> Result<Ws, String> {
    let (ws, _) = tokio::time::timeout(Duration::from_secs(5), tokio_tungstenite::connect_async(url(port))).await.map_err(|_| "upgrade timed out".to_string())?.map_err(|e| e.to_string())?;
    Ok(ws)
}

/// The next binary frame, or `None` when the gateway closed or stayed silent for `wait`.
pub async fn frame(ws: &mut Ws, wait: Duration) -> Option<Vec<u8>> {
    let end = Instant::now() + wait;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, ws.next()).await {
            Ok(Some(Ok(Message::Binary(b)))) => return Some(b.to_vec()),
            Ok(Some(Ok(Message::Ping(_)))) | Ok(Some(Ok(Message::Pong(_)))) => continue,
            _ => return None,
        }
    }
}

/// True when the gateway closes the connection within `wait` without sending a frame.
pub async fn closes_silently(ws: &mut Ws, wait: Duration) -> bool {
    let end = Instant::now() + wait;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        match tokio::time::timeout(left, ws.next()).await {
            Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(Message::Close(_)))) => return true,
            Ok(Some(Ok(Message::Binary(_)))) | Ok(Some(Ok(Message::Text(_)))) => return false,
            Ok(Some(Ok(_))) => continue,
            Err(_) => return false,
        }
    }
}

pub fn first_frame(kind: u8, message1: &[u8]) -> Vec<u8> {
    let mut out = vec![noise::FRAME_VERSION, kind];
    out.extend_from_slice(message1);
    out
}

pub fn hello_payload(device: &str, name: &str, counter: i64) -> Vec<u8> {
    json!({"device": device, "name": name, "platform": "ios", "app": "0.1.0-test", "counter": counter}).to_string().into_bytes()
}

impl Phone {
    async fn finish(mut ws: Ws, mut hs: snow::HandshakeState, wait: Duration) -> Result<Self, String> {
        let reply = frame(&mut ws, wait).await.ok_or("no reply to the handshake")?;
        let mut payload = vec![0u8; reply.len()];
        let n = hs.read_message(&reply, &mut payload).map_err(|e| format!("message 2: {e}"))?;
        let hello: Value = serde_json::from_slice(&payload[..n]).map_err(|e| e.to_string())?;
        let transport = hs.into_transport_mode().map_err(|e| e.to_string())?;
        Ok(Self { ws, transport, opener: noise::Opener::new(64 << 20), next_id: 1000, inbox: Vec::new(), hello })
    }

    /// The pairing handshake. Waits while the owner decides.
    pub async fn pair_on(port: u16, code: &Code, keys: &Keys, name: &str, wait: Duration) -> Result<Self, String> {
        let mut ws = open(port).await?;
        let psk = noise::psk_from_secret(&code.secret);
        let mut hs = noise::initiator(noise::Kind::Pairing, &keys.private, &code.gateway_public, Some(&psk)).unwrap();
        let mut m1 = vec![0u8; 1024];
        let n = hs.write_message(&hello_payload("", name, now_ms()), &mut m1).unwrap();
        ws.send(Message::Binary(first_frame(noise::KIND_PAIRING, &m1[..n]).into())).await.map_err(|e| e.to_string())?;
        Self::finish(ws, hs, wait).await
    }

    pub async fn session_on(port: u16, gateway_public: &[u8], keys: &Keys, device: &str, counter: i64) -> Result<Self, String> {
        let mut ws = open(port).await?;
        let mut hs = noise::initiator(noise::Kind::Session, &keys.private, gateway_public, None).unwrap();
        let mut m1 = vec![0u8; 1024];
        let n = hs.write_message(&hello_payload(device, "test phone", counter), &mut m1).unwrap();
        ws.send(Message::Binary(first_frame(noise::KIND_SESSION, &m1[..n]).into())).await.map_err(|e| e.to_string())?;
        Self::finish(ws, hs, Duration::from_secs(5)).await
    }

    /// A session as the paired phone, with the next counter.
    pub async fn connect(p: &mut Paired) -> Result<Self, String> {
        p.counter = p.counter.max(now_ms() - 1) + 1;
        Self::session_on(p.port, &p.gateway_public, &p.keys, &p.device, p.counter).await
    }

    pub async fn send(&mut self, message: &Value) -> Result<(), String> {
        for f in noise::seal(&mut self.transport, message.to_string().as_bytes()).map_err(|e| e.to_string())? {
            self.ws.send(Message::Binary(f.into())).await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// The next whole message, or `None` when the session ended or stayed silent for `wait`.
    pub async fn next(&mut self, wait: Duration) -> Option<Value> {
        let end = Instant::now() + wait;
        loop {
            let f = frame(&mut self.ws, end.saturating_duration_since(Instant::now())).await?;
            match self.opener.open(&mut self.transport, &f) {
                Ok(Some(bytes)) => return serde_json::from_slice(&bytes).ok(),
                Ok(None) => continue,
                Err(_) => return None,
            }
        }
    }

    /// Sends a request and returns its whole reply; notifications that arrive first go to the inbox.
    pub async fn ask(&mut self, method: &str, params: Value, request_id: Option<&str>) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(r) = request_id {
            msg["request_id"] = json!(r);
        }
        self.send(&msg).await?;
        self.reply_to(id, Duration::from_secs(30)).await
    }

    pub async fn reply_to(&mut self, id: i64, wait: Duration) -> Result<Value, String> {
        let end = Instant::now() + wait;
        loop {
            let Some(m) = self.next(end.saturating_duration_since(Instant::now())).await else { return Err("the session ended before the reply".into()) };
            if m["id"] == json!(id) {
                return Ok(m);
            }
            self.inbox.push(m);
        }
    }

    /// `result` of a request, panicking on an error reply.
    pub async fn call(&mut self, method: &str, params: Value) -> Value {
        let reply = self.ask(method, params.clone(), None).await.unwrap_or_else(|e| panic!("{method}: {e}"));
        assert!(reply.get("error").is_none(), "{method} {params}: {}", reply["error"]);
        reply["result"].clone()
    }

    /// A changing request with a fresh request id.
    pub async fn act(&mut self, method: &str, params: Value) -> Value {
        let rid = uuid();
        self.ask(method, params, Some(&rid)).await.unwrap_or_else(|e| panic!("{method}: {e}"))
    }

    /// The error code of a reply, or an empty string.
    pub fn code(reply: &Value) -> String {
        reply["error"]["code"].as_str().unwrap_or_default().to_string()
    }

    /// True when the session ends within `wait`. Messages that arrive first go to the inbox.
    pub async fn ends_within(&mut self, wait: Duration) -> bool {
        let end = Instant::now() + wait;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            match tokio::time::timeout(left, self.ws.next()).await {
                Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(Message::Close(_)))) => return true,
                Ok(Some(Ok(Message::Binary(f)))) => match self.opener.open(&mut self.transport, &f) {
                    Ok(Some(bytes)) => self.inbox.extend(serde_json::from_slice::<Value>(&bytes).ok()),
                    Ok(None) => {}
                    Err(_) => return true,
                },
                Ok(Some(Ok(_))) => continue,
                Err(_) => return false,
            }
        }
    }

    pub fn notices(&self) -> Vec<String> {
        self.inbox.iter().filter(|m| m["method"] == "gateway").map(|m| m["params"]["state"].as_str().unwrap_or_default().to_string()).collect()
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

pub fn uuid() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!("req-{}-{}", now_ms(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
}

/// A port nothing listens on right now.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

/// Turns phone access on at a fixed free port, so it comes back at the same port after a restart.
pub fn enable(d: &Daemon) -> u16 {
    let port = free_port();
    let status = d.call("gateway.enable", json!({"port": port}));
    assert_eq!(status["enabled"], true, "{status}");
    assert_eq!(status["port"], json!(port));
    port
}

/// Pairs a phone the way the owner does: open pairing on the Mac, present the code, confirm on the Mac.
pub async fn pair(d: &Daemon, name: &str) -> (Phone, Paired) {
    let started = d.call("gateway.pair_start", json!({}));
    let code = parse_code(started["code"].as_str().unwrap());
    let keys = Keys::new();
    let port = code.port;
    let task = {
        let (keys, name, gateway) = (keys.clone(), name.to_string(), code.gateway_public.clone());
        let code = Code { gateway_public: gateway, secret: code.secret.clone(), port, addresses: code.addresses.clone() };
        tokio::spawn(async move { Phone::pair_on(port, &code, &keys, &name, Duration::from_secs(20)).await })
    };
    let request = waiting_request(d, Duration::from_secs(10)).await.expect("the Mac sees the phone that asks to pair");
    assert_eq!(request["name"], json!(name));
    d.call("gateway.pair_confirm", json!({"request": request["request"], "accept": true}));
    let phone = task.await.unwrap().expect("paired");
    let device = phone.hello["device"].as_str().unwrap().to_string();
    let paired = Paired { keys, device, gateway_public: code.gateway_public, port, counter: now_ms(), name: name.to_string() };
    (phone, paired)
}

pub async fn waiting_request(d: &Daemon, wait: Duration) -> Option<Value> {
    let end = Instant::now() + wait;
    while Instant::now() < end {
        let status = d.call("gateway.status", json!({}));
        if let Some(first) = status["pairing"]["waiting"].as_array().and_then(|w| w.first()) {
            return Some(first.clone());
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    None
}

/// A TCP forwarder that records every byte in both directions: a packet capture of a session.
pub struct Tap {
    pub port: u16,
    pub bytes: Arc<Mutex<Vec<u8>>>,
}

pub async fn tap(target: u16) -> Tap {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let seen = bytes.clone();
    tokio::spawn(async move {
        while let Ok((inbound, _)) = listener.accept().await {
            let Ok(outbound) = TcpStream::connect(("127.0.0.1", target)).await else { continue };
            let (mut ir, mut iw) = inbound.into_split();
            let (mut or, mut ow) = outbound.into_split();
            let up = seen.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16 * 1024];
                while let Ok(n) = ir.read(&mut buf).await {
                    if n == 0 || ow.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                    up.lock().unwrap().extend_from_slice(&buf[..n]);
                }
                let _ = ow.shutdown().await;
            });
            let down = seen.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16 * 1024];
                while let Ok(n) = or.read(&mut buf).await {
                    if n == 0 || iw.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                    down.lock().unwrap().extend_from_slice(&buf[..n]);
                }
                let _ = iw.shutdown().await;
            });
        }
    });
    Tap { port, bytes }
}

pub fn contains(haystack: &[u8], needle: &str) -> bool {
    let n = needle.as_bytes();
    !n.is_empty() && haystack.windows(n.len()).any(|w| w == n)
}
