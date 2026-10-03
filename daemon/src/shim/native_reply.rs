//! Generation-qualified typed reply receipts. Native bytes never enter segments.
//! A receipt is prepared durably before the first pipe write; replay queries only.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

const WRITE_WAIT: Duration = Duration::from_secs(2);

pub(super) struct Replies {
    generation: Option<i64>,
    dir: PathBuf,
    serial: Mutex<()>,
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    generation: i64,
    delivery_token: String,
    answer_digest: String,
    state: String,
}
impl Receipt {
    fn public(&self) -> Value {
        // A crash can strand prepared before or during a write. It is never
        // evidence of no effect, and never permission to replay bytes.
        json!({"ok":true,"generation":self.generation,"delivery_token":self.delivery_token,
            "answer_digest":self.answer_digest,"state":if self.state=="prepared" {"uncertain"} else {&self.state}})
    }
}
fn refusal(code: &'static str) -> Value {
    json!({"ok":false,"code":code})
}
fn lock_until<T>(lock: &Mutex<T>, deadline: Instant) -> Option<MutexGuard<'_, T>> {
    loop {
        match lock.try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::Poisoned(_)) => return None,
            Err(TryLockError::WouldBlock) => {}
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
impl Replies {
    pub(super) fn new(dir: &Path, generation: Option<i64>) -> Self {
        Self {
            generation,
            dir: dir.join("native-receipts"),
            serial: Mutex::new(()),
        }
    }
    fn save(&self, receipt: &Receipt) -> std::io::Result<()> {
        crate::paths::ensure_private_dir(&self.dir).map_err(std::io::Error::other)?;
        let path = self.dir.join(format!("{}.json", receipt.delivery_token));
        let temporary = self.dir.join(format!("{}.tmp", receipt.delivery_token));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(receipt)?)?;
        file.sync_all()?;
        std::fs::rename(temporary, path)?;
        std::fs::File::open(&self.dir)?.sync_all()
    }
    pub(super) fn handle(
        &self,
        msg: &Value,
        stdin: &Mutex<Option<std::process::ChildStdin>>,
    ) -> Value {
        let Some(expected) = self.generation.filter(|g| *g > 0) else {
            return refusal("native_unqualified");
        };
        if msg["generation"].as_i64() != Some(expected) {
            return refusal("stale_generation");
        }
        let Some(token) = msg["delivery_token"].as_str().filter(|s| {
            s.starts_with("delivery-")
                && (16..=96).contains(&s.len())
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) else {
            return refusal("invalid_receipt");
        };
        let Some(digest) = msg["answer_digest"].as_str().filter(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) else {
            return refusal("invalid_receipt");
        };
        let write = msg["op"] == "request_reply";
        let allowed = if write {
            vec![
                "op",
                "generation",
                "delivery_token",
                "answer_digest",
                "data",
            ]
        } else {
            vec!["op", "generation", "delivery_token", "answer_digest"]
        };
        if !msg.as_object().is_some_and(|o| {
            o.len() == allowed.len() && o.keys().all(|k| allowed.contains(&k.as_str()))
        }) {
            return refusal("invalid_receipt");
        }
        let data = if write {
            let Some(data) = msg["data"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= super::MAX_LINE_BYTES && s.ends_with('\n'))
            else {
                return refusal("invalid_receipt");
            };
            if format!("{:x}", Sha256::digest(data.as_bytes())) != digest {
                return refusal("invalid_receipt");
            }
            Some(data)
        } else {
            None
        };
        // One deadline includes receipt serialization and stdin mutex waiting.
        // Caller control I/O has its own aggregate bound if disk I/O stalls.
        let deadline = Instant::now() + WRITE_WAIT;
        let Some(_serial) = lock_until(&self.serial, deadline) else {
            return refusal("receipt_unavailable");
        };
        let path = self.dir.join(format!("{token}.json"));
        match std::fs::read(&path) {
            Ok(bytes) => {
                let Ok(receipt) = serde_json::from_slice::<Receipt>(&bytes) else {
                    return refusal("receipt_unavailable");
                };
                if !matches!(
                    receipt.state.as_str(),
                    "prepared" | "written" | "not_written" | "uncertain"
                ) {
                    return refusal("receipt_unavailable");
                }
                if receipt.generation != expected
                    || receipt.delivery_token != token
                    || receipt.answer_digest != digest
                {
                    return refusal("receipt_conflict");
                }
                return receipt.public(); // Existing token never takes the write path.
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return refusal("receipt_unavailable"),
        }
        let mut receipt = Receipt {
            generation: expected,
            delivery_token: token.into(),
            answer_digest: digest.into(),
            state: "not_written".into(),
        };
        let Some(data) = data else {
            // Freeze absence under the same serial gate. A delayed original
            // request for this token must never write after authoritative
            // not_written lets the daemon restore/retry with a new token.
            return if self.save(&receipt).is_ok() {
                receipt.public()
            } else {
                refusal("receipt_unavailable")
            };
        };
        let Some(mut guard) = lock_until(stdin, deadline) else {
            return if self.save(&receipt).is_ok() {
                receipt.public()
            } else {
                refusal("receipt_unavailable")
            };
        };
        let Some(pipe) = guard.as_mut() else {
            return if self.save(&receipt).is_ok() {
                receipt.public()
            } else {
                refusal("receipt_unavailable")
            };
        };
        let fd = pipe.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return if self.save(&receipt).is_ok() {
                receipt.public()
            } else {
                refusal("receipt_unavailable")
            };
        }
        struct Restore(i32, i32);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    libc::fcntl(self.0, libc::F_SETFL, self.1);
                }
            }
        }
        let _restore = Restore(fd, flags);
        receipt.state = "prepared".into();
        if self.save(&receipt).is_err() {
            return refusal("receipt_unavailable");
        }
        let mut written = 0;
        while written < data.len() && Instant::now() < deadline {
            match pipe.write(&data.as_bytes()[written..]) {
                Ok(0) => break,
                Ok(n) => written += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
        }
        receipt.state = if written == data.len() && pipe.flush().is_ok() {
            "written"
        } else if written == 0 {
            "not_written"
        } else {
            "uncertain"
        }
        .into();
        if self.save(&receipt).is_err() {
            return refusal("receipt_unavailable");
        }
        receipt.public()
    }
}

/// The final authority gate must not be held behind a blocking connect/write.
/// Nonblocking socket + one aggregate deadline covers all control operations.
pub(crate) fn control(socket: &Path, msg: &Value) -> anyhow::Result<Value> {
    use std::os::fd::FromRawFd;
    let deadline = Instant::now() + Duration::from_secs(3);
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(socket.as_os_str());
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= address.sun_path.len() {
        anyhow::bail!("native reply control unavailable");
    }
    address.sun_family = libc::AF_UNIX as _;
    #[cfg(not(target_os = "linux"))]
    {
        address.sun_len = std::mem::size_of_val(&address) as _;
    }
    for (dest, source) in address.sun_path.iter_mut().zip(bytes) {
        *dest = *source as _;
    }
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        anyhow::bail!("native reply control unavailable");
    }
    let mut stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream
        .set_nonblocking(true)
        .map_err(|_| anyhow::anyhow!("native reply control unavailable"))?;
    let rc = unsafe {
        libc::connect(
            fd,
            &address as *const _ as *const libc::sockaddr,
            std::mem::size_of_val(&address) as _,
        )
    };
    if rc < 0 {
        let error = std::io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(libc::EINPROGRESS) | Some(libc::EAGAIN)
        ) {
            anyhow::bail!("native reply control unavailable");
        }
        loop {
            if Instant::now() >= deadline {
                anyhow::bail!("native reply control unavailable");
            }
            let mut poll = libc::pollfd {
                fd,
                events: libc::POLLOUT,
                revents: 0,
            };
            if unsafe { libc::poll(&mut poll, 1, 5) } > 0 {
                if stream
                    .take_error()
                    .map_err(|_| anyhow::anyhow!("native reply control unavailable"))?
                    .is_some()
                {
                    anyhow::bail!("native reply control unavailable");
                }
                break;
            }
        }
    }
    let data = format!("{msg}\n");
    let mut offset = 0;
    let mut reply = Vec::new();
    loop {
        if Instant::now() >= deadline {
            anyhow::bail!("native reply control unavailable");
        }
        if offset < data.len() {
            match stream.write(&data.as_bytes()[offset..]) {
                Ok(0) => anyhow::bail!("native reply control unavailable"),
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => anyhow::bail!("native reply control unavailable"),
            }
        } else {
            let mut chunk = [0u8; 512];
            match stream.read(&mut chunk) {
                Ok(0) => anyhow::bail!("native reply control unavailable"),
                Ok(n) => {
                    reply.extend_from_slice(&chunk[..n]);
                    if reply.len() > 4096 {
                        anyhow::bail!("native reply control unavailable");
                    }
                    if let Some(end) = reply.iter().position(|b| *b == b'\n') {
                        return serde_json::from_slice(&reply[..end])
                            .map_err(|_| anyhow::anyhow!("native reply control unavailable"));
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => anyhow::bail!("native reply control unavailable"),
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
