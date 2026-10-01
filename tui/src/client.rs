//! Client for the overseerd local protocol: newline-delimited JSON over a Unix socket, the same
//! protocol the VS Code extension speaks. One connection at a time; a reader thread turns
//! responses and events into [`Msg`]s for the app loop. Reconnects (starting the daemon when it
//! is gone) and resumes the event stream from the last seen cursor, so a dropped connection
//! neither loses nor repeats events.

use crate::locate::Daemon;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Messages delivered to the app loop.
#[derive(Debug, Clone)]
pub enum Msg {
    /// Connected (or reconnected); the app reloads `state`.
    Connected,
    /// Connection lost; the client is reconnecting.
    Disconnected(String),
    /// The daemon is a dev instance and this TUI is the installed one (AC-212): never used, no retry.
    Refused(String),
    /// A daemon event (never delivered twice).
    Event(Value),
    /// The replay of retained events after the subscription cursor finished.
    Replayed,
    /// Voice Mode's live channel (`voice.subscribe`): states, heard words, requests (T-35).
    Voice(Value),
    /// Reply to a request made with [`Requests::request`].
    Reply { id: u64, result: Result<Value, String> },
}

/// What the app needs from a daemon connection. The real one is [`Client`]; tests use a fake.
pub trait Requests: Send + Sync {
    /// Sends a request; its reply arrives later as [`Msg::Reply`] with the returned id.
    fn request(&self, method: &str, params: Value) -> u64;
    fn connected(&self) -> bool;
    /// Sets the event cursor to resume from, if none is set yet (the first `state`'s cursor).
    fn set_cursor_if_unset(&self, cursor: i64);
    /// Subscribes to live events from the cursor (after `state` was loaded on a connection).
    fn subscribe(&self);
    /// After "stop agents and daemon": do not start the daemon again until cleared.
    fn set_stopped(&self, _stopped: bool) {}
}

pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    daemon: Option<Daemon>,
    socket: std::path::PathBuf,
    tx: Sender<Msg>,
    writer: Mutex<Option<UnixStream>>,
    next_id: AtomicU64,
    /// Highest event sequence number delivered.
    cursor: AtomicI64,
    connected: AtomicBool,
    closed: AtomicBool,
    /// Request ids the client itself made (hello, subscribe); their replies are not forwarded.
    internal: Mutex<HashSet<u64>>,
    generation: AtomicU64,
    /// Stopped on purpose: keep trying to connect (another UI may start it) but never spawn.
    stopped: AtomicBool,
    /// The installed TUI: refuses a daemon whose hello reports a dev instance (AC-212).
    production: bool,
    /// The id of this connection's hello; its answer says which daemon this is.
    hello: AtomicU64,
    refused: AtomicBool,
}

impl Client {
    /// Connects in the background (starting the daemon when needed) and keeps reconnecting.
    /// `after` is the event cursor to resume from (0 = only new events after the first state).
    pub fn start(daemon: Option<Daemon>, socket: std::path::PathBuf, tx: Sender<Msg>) -> Client {
        Self::start_as(daemon, socket, tx, crate::locate::production())
    }

    /// `production`: refuse a daemon that reports a dev instance (the installed TUI does).
    pub fn start_as(daemon: Option<Daemon>, socket: std::path::PathBuf, tx: Sender<Msg>, production: bool) -> Client {
        let inner = Arc::new(Inner {
            daemon,
            socket,
            tx,
            writer: Mutex::new(None),
            next_id: AtomicU64::new(1),
            cursor: AtomicI64::new(-1),
            connected: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            internal: Mutex::new(HashSet::new()),
            generation: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            production,
            hello: AtomicU64::new(0),
            refused: AtomicBool::new(false),
        });
        let bg = inner.clone();
        std::thread::spawn(move || bg.connect_loop());
        Client { inner }
    }

    pub fn cursor(&self) -> i64 {
        self.inner.cursor.load(Ordering::SeqCst)
    }

    /// Drops the current connection (tests use it to exercise resume-from-cursor).
    pub fn drop_connection(&self) {
        if let Some(s) = self.inner.writer.lock().unwrap().as_ref() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }

    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
        self.drop_connection();
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.close();
    }
}

impl Requests for Client {
    fn request(&self, method: &str, params: Value) -> u64 {
        self.inner.send(method, params)
    }
    fn connected(&self) -> bool {
        self.inner.connected.load(Ordering::SeqCst)
    }
    /// Only takes effect before the first subscription (later ones resume from what was delivered).
    fn set_cursor_if_unset(&self, cursor: i64) {
        let _ = self.inner.cursor.compare_exchange(-1, cursor, Ordering::SeqCst, Ordering::SeqCst);
    }
    /// Called after `state` was loaded on each (re)connection, so no event between the state
    /// and the stream is lost.
    fn subscribe(&self) {
        let after = self.inner.cursor.load(Ordering::SeqCst).max(0);
        let id = self.inner.send("events.subscribe", json!({ "after": after }));
        self.inner.internal.lock().unwrap().insert(id);
    }
    fn set_stopped(&self, stopped: bool) {
        self.inner.stopped.store(stopped, Ordering::SeqCst);
    }
}

impl Inner {
    fn send(&self, method: &str, params: Value) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let line = json!({ "id": id, "method": method, "params": params }).to_string() + "\n";
        let mut guard = self.writer.lock().unwrap();
        let ok = match guard.as_mut() {
            Some(s) => s.write_all(line.as_bytes()).is_ok(),
            None => false,
        };
        drop(guard);
        if !ok {
            let _ = self.tx.send(Msg::Reply { id, result: Err("not connected to overseerd".into()) });
        }
        id
    }

    fn connect_loop(self: Arc<Self>) {
        let mut spawned = false;
        while !self.closed.load(Ordering::SeqCst) && !self.refused.load(Ordering::SeqCst) {
            match UnixStream::connect(&self.socket) {
                Ok(stream) => {
                    spawned = false;
                    self.run_connection(stream);
                    if self.closed.load(Ordering::SeqCst) {
                        return;
                    }
                }
                Err(_) => {
                    // Like VS Code: start the daemon (detached, it outlives this TUI) once per outage.
                    if !spawned && !self.stopped.load(Ordering::SeqCst) {
                        if let Some(d) = &self.daemon {
                            let _ = d.spawn_serve();
                        }
                        spawned = true;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    fn run_connection(&self, stream: UnixStream) {
        let Ok(read) = stream.try_clone() else { return };
        *self.writer.lock().unwrap() = Some(stream);
        self.generation.fetch_add(1, Ordering::SeqCst);
        // Identify as a watching UI (the daemon counts it like a VS Code window). Connected is
        // announced when its answer says the daemon may be used.
        let hello = self.send("hello", json!({ "client": "tui" }));
        self.hello.store(hello, Ordering::SeqCst);
        let mut reader = BufReader::new(read);
        let mut line = String::new();
        let reason = loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break "the daemon closed the connection".to_string(),
                Ok(_) => {}
                Err(e) => break e.to_string(),
            }
            let Ok(msg) = serde_json::from_str::<Value>(line.trim_end()) else { continue };
            self.dispatch(msg);
        };
        *self.writer.lock().unwrap() = None;
        self.connected.store(false, Ordering::SeqCst);
        if !self.closed.load(Ordering::SeqCst) && !self.refused.load(Ordering::SeqCst) {
            let _ = self.tx.send(Msg::Disconnected(reason));
        }
    }

    fn dispatch(&self, msg: Value) {
        match msg["method"].as_str() {
            Some("event") => {
                let seq = msg["params"]["seq"].as_i64().unwrap_or(0);
                // Replay overlap after a reconnect: never apply an event twice.
                if seq <= self.cursor.load(Ordering::SeqCst) {
                    return;
                }
                self.cursor.store(seq, Ordering::SeqCst);
                let _ = self.tx.send(Msg::Event(msg["params"].clone()));
            }
            Some("replayed") => {
                let _ = self.tx.send(Msg::Replayed);
            }
            Some("voice") => {
                let _ = self.tx.send(Msg::Voice(msg["params"].clone()));
            }
            Some("resync") => {
                // The live stream lagged: resume from what was delivered.
                let after = self.cursor.load(Ordering::SeqCst).max(0);
                let id = self.send("events.subscribe", json!({ "after": after }));
                self.internal.lock().unwrap().insert(id);
            }
            _ => {
                let Some(id) = msg["id"].as_u64() else { return };
                if id == self.hello.load(Ordering::SeqCst) {
                    let instance = msg["result"]["instance"].as_str().filter(|_| self.production);
                    if let Some(instance) = instance {
                        self.refused.store(true, Ordering::SeqCst);
                        let _ = self.tx.send(Msg::Refused(format!("This overseer-tui is the installed one and refuses dev instance {instance} ({}). Dev instances have their own TUI: scripts/dev tui.", self.socket.display())));
                        if let Some(s) = self.writer.lock().unwrap().as_ref() {
                            let _ = s.shutdown(std::net::Shutdown::Both);
                        }
                    } else {
                        self.connected.store(true, Ordering::SeqCst);
                        let _ = self.tx.send(Msg::Connected);
                    }
                    return;
                }
                if self.internal.lock().unwrap().remove(&id) {
                    return;
                }
                let result = if let Some(err) = msg.get("error").filter(|e| !e.is_null()) {
                    Err(err["message"].as_str().or(err["code"].as_str()).unwrap_or("error").to_string())
                } else {
                    Ok(msg["result"].clone())
                };
                let _ = self.tx.send(Msg::Reply { id, result });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A fake daemon answering hello with `instance`; returns how many connections it took.
    fn fake(path: &std::path::Path, instance: Option<&'static str>) -> Arc<AtomicU64> {
        let listener = UnixListener::bind(path).unwrap();
        let count = Arc::new(AtomicU64::new(0));
        let seen = count.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                seen.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    let mut w = stream.try_clone().unwrap();
                    for line in BufReader::new(stream).lines().map_while(Result::ok) {
                        let msg: Value = serde_json::from_str(&line).unwrap();
                        let result = if msg["method"] == "hello" { json!({ "instance": instance }) } else { json!({}) };
                        let _ = w.write_all(format!("{}\n", json!({ "id": msg["id"], "result": result })).as_bytes());
                    }
                });
            }
        });
        count
    }

    #[test]
    fn ac212_the_installed_tui_refuses_a_dev_daemon_and_does_not_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let sock = tmp.path().join("d.sock");
        let seen = fake(&sock, Some("dev-a"));
        let (tx, rx) = std::sync::mpsc::channel();
        let c = Client::start_as(None, sock, tx, true);
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            Msg::Refused(why) => assert!(why.contains("refuses dev instance dev-a"), "{why}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
        std::thread::sleep(Duration::from_millis(1200));
        assert!(!c.connected());
        assert_eq!(seen.load(Ordering::SeqCst), 1, "no retry into the dev daemon");
        assert!(rx.try_iter().all(|m| !matches!(m, Msg::Connected)), "never announced as connected");
    }

    #[test]
    fn ac212_a_dev_tui_and_a_standard_daemon_connect() {
        let tmp = tempfile::tempdir().unwrap();
        for (name, instance, production) in [("std.sock", None, true), ("dev.sock", Some("dev-b"), false)] {
            let sock = tmp.path().join(name);
            fake(&sock, instance);
            let (tx, rx) = std::sync::mpsc::channel();
            let c = Client::start_as(None, sock, tx, production);
            assert!(matches!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Msg::Connected), "{name}");
            assert!(c.connected());
        }
    }
}
