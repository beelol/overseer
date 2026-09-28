#![allow(dead_code)]
//! A network the tests control (Continuity, AC-83 and AC-97). SYNTHETIC: it stands in for the
//! link between this machine and the internet, so that "Wi-Fi goes off" can be produced without
//! touching the machine's own network.
//!
//! One listener on loopback plays either part:
//! - **the internet** for the daemon's real probes (`OVERSEER_TEST_PROBE_URLS` points them here):
//!   online, every request is answered `204`;
//! - **the way out** for a real harness (`HTTPS_PROXY` points it here): online, a `CONNECT` is
//!   tunnelled to the real host.
//!
//! Cut, it behaves the way a dead link does, in one of three ways: nothing listens and every
//! connection is refused (the interface is down), connections are accepted and never answered (the
//! link is up and nothing comes back), or they are accepted and closed at once. Every cut also
//! ends the connections that are open, as Wi-Fi going off ends a stream in flight.
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Answers: a request gets `204`; a `CONNECT` is tunnelled to the real host.
    Online,
    /// Nothing listens: every connection is refused at once (the interface is down).
    Refuse,
    /// Connections are accepted and never answered (the link is up, nothing comes back).
    Blackhole,
    /// Connections are accepted and closed at once.
    Drop,
}

impl Mode {
    pub fn parse(word: &str) -> Option<Mode> {
        match word.trim() {
            "online" => Some(Mode::Online),
            "refuse" | "cut" | "off" => Some(Mode::Refuse),
            "blackhole" => Some(Mode::Blackhole),
            "drop" => Some(Mode::Drop),
            _ => None,
        }
    }
}

struct Shared {
    mode: Mutex<Mode>,
    /// The listener, closed at the moment the link is cut (not when the accept loop next wakes, so
    /// a connection made right after `set` is refused, however loaded the machine is).
    listener: Mutex<Option<TcpListener>>,
    open: Mutex<Vec<TcpStream>>,
    log: Mutex<Vec<String>>,
    stop: AtomicBool,
    started: Instant,
}

pub struct NetSim {
    pub addr: SocketAddr,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl NetSim {
    /// Starts online on a free loopback port.
    pub fn start() -> NetSim {
        NetSim::start_at("127.0.0.1:0")
    }

    pub fn start_at(at: &str) -> NetSim {
        let listener = TcpListener::bind(at).expect("bind the simulated network");
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let shared = Arc::new(Shared { mode: Mutex::new(Mode::Online), listener: Mutex::new(Some(listener)), open: Mutex::new(Vec::new()), log: Mutex::new(Vec::new()), stop: AtomicBool::new(false), started: Instant::now() });
        let s = shared.clone();
        let thread = std::thread::spawn(move || accept_loop(addr, s));
        NetSim { addr, shared, thread: Some(thread) }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn mode(&self) -> Mode {
        *self.shared.mode.lock().unwrap()
    }

    /// Changes the link. Anything but online also ends every open connection.
    pub fn set(&self, mode: Mode) {
        *self.shared.mode.lock().unwrap() = mode;
        // The listener closes or opens here and now, so the link is what was set as soon as this
        // returns (the accept loop only reopens it if binding failed here).
        let mut slot = self.shared.listener.lock().unwrap();
        if mode == Mode::Refuse {
            slot.take();
        } else if slot.is_none() {
            if let Ok(l) = TcpListener::bind(self.addr) {
                l.set_nonblocking(true).unwrap();
                *slot = Some(l);
            }
        }
        drop(slot);
        note(&self.shared, format!("link {mode:?}"));
        if mode != Mode::Online {
            for s in self.shared.open.lock().unwrap().drain(..) {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
    }

    /// What happened, one line per connection or change, with the time since the start.
    pub fn log(&self) -> Vec<String> {
        self.shared.log.lock().unwrap().clone()
    }
}

impl Drop for NetSim {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        for s in self.shared.open.lock().unwrap().drain(..) {
            let _ = s.shutdown(Shutdown::Both);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn note(shared: &Shared, line: String) {
    let at = shared.started.elapsed().as_millis();
    shared.log.lock().unwrap().push(format!("{at:>7} ms  {line}"));
}

fn accept_loop(addr: SocketAddr, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::SeqCst) {
        let mode = *shared.mode.lock().unwrap();
        if mode == Mode::Refuse {
            // Nothing listens while the link is down: connections are refused by the kernel.
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let accepted = {
            let mut slot = shared.listener.lock().unwrap();
            if slot.is_none() {
                match TcpListener::bind(addr) {
                    Ok(l) => {
                        l.set_nonblocking(true).unwrap();
                        *slot = Some(l);
                    }
                    Err(_) => {
                        drop(slot);
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                }
            }
            slot.as_ref().unwrap().accept()
        };
        match accepted {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                match mode {
                    Mode::Blackhole => {
                        note(&shared, "accepted and never answered".into());
                        shared.open.lock().unwrap().push(stream);
                    }
                    Mode::Drop => {
                        note(&shared, "accepted and closed".into());
                        let _ = stream.shutdown(Shutdown::Both);
                    }
                    _ => {
                        let s = shared.clone();
                        std::thread::spawn(move || serve(stream, s));
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// Reads a request head, at most 16 KiB.
fn head(stream: &mut TcpStream) -> Option<String> {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while buf.len() < 16 * 1024 {
        match stream.read(&mut byte) {
            Ok(1) => {
                buf.push(byte[0]);
                if buf.ends_with(b"\r\n\r\n") {
                    return String::from_utf8(buf).ok();
                }
            }
            _ => return None,
        }
    }
    None
}

fn serve(mut stream: TcpStream, shared: Arc<Shared>) {
    let Some(head) = head(&mut stream) else { return };
    let first = head.lines().next().unwrap_or("").to_string();
    if *shared.mode.lock().unwrap() != Mode::Online {
        note(&shared, format!("{first}: cut while it was being read"));
        return;
    }
    let Some(target) = first.strip_prefix("CONNECT ").and_then(|r| r.split_whitespace().next()).map(|t| t.to_string()) else {
        // The internet, for the probes: any request is answered.
        note(&shared, format!("{first}: 204"));
        let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    };
    // The way out, for a harness: a tunnel to the real host while the link is up.
    let upstream = target.to_socket_addrs().ok().and_then(|mut a| a.next()).and_then(|a| TcpStream::connect_timeout(&a, Duration::from_secs(10)).ok());
    let Some(upstream) = upstream else {
        note(&shared, format!("CONNECT {target}: the host could not be reached"));
        let _ = stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
        return;
    };
    if stream.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").is_err() {
        return;
    }
    note(&shared, format!("CONNECT {target}: tunnelled"));
    let _ = stream.set_read_timeout(None);
    {
        let mut open = shared.open.lock().unwrap();
        open.push(stream.try_clone().unwrap());
        open.push(upstream.try_clone().unwrap());
    }
    let (mut a_in, mut a_out) = (stream.try_clone().unwrap(), stream);
    let (mut b_in, mut b_out) = (upstream.try_clone().unwrap(), upstream);
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut a_in, &mut b_out);
        let _ = b_out.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut b_in, &mut a_out);
    let _ = a_out.shutdown(Shutdown::Write);
    let _ = up.join();
}
