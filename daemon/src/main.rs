mod accounts;
mod adapters;
mod background;
mod daemon;
mod files;
mod usage;
mod git;
mod merge;
mod paths;
mod pr;
mod redact;
mod server;
mod shim;
mod store;
mod swarm;

use std::io::{BufRead, BufReader, Write};

pub fn log(msg: &str) {
    let line = format!("{} [{}] {msg}\n", shim::now_ms(), std::process::id());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(paths::log_path()) {
        let _ = f.write_all(line.as_bytes());
    }
    if std::env::var_os("OVERSEER_LOG_STDERR").is_some() {
        eprint!("{line}");
    }
}

fn usage() -> ! {
    eprintln!("usage: overseerd serve | overseerd ctl <method> [json-params] | overseerd shim <run-dir> | overseerd version");
    std::process::exit(2);
}

/// Hold an exclusive lock for the daemon's lifetime so only one instance runs.
fn single_instance() -> anyhow::Result<std::fs::File> {
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(paths::data_dir().join("overseerd.lock"))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        anyhow::bail!("overseerd is already running");
    }
    Ok(file)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("version") | Some("--version") => println!("overseerd {} (protocol {})", env!("CARGO_PKG_VERSION"), server::PROTOCOL_VERSION),
        Some("socket-path") => println!("{}", paths::socket_path().display()),
        Some("shim") => {
            let dir = args.get(2).unwrap_or_else(|| usage());
            if let Err(e) = shim::run(dir.into()) {
                eprintln!("shim error: {e:#}");
                std::process::exit(1);
            }
        }
        Some("serve") => {
            if let Err(e) = paths::ensure_private_dir(&paths::data_dir()) {
                eprintln!("cannot create data dir: {e}");
                std::process::exit(1);
            }
            let _lock = match single_instance() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(3);
                }
            };
            unsafe {
                libc::signal(libc::SIGPIPE, libc::SIG_IGN);
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
            }
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
            let result: anyhow::Result<()> = rt.block_on(async {
                let d = daemon::Daemon::open()?;
                log(&format!("overseerd {} starting, data dir {}", env!("CARGO_PKG_VERSION"), paths::data_dir().display()));
                let report = d.reconcile()?;
                log(&format!("reconcile: {report}"));
                let dispatches = swarm::recover_pending_dispatches(&d)?;
                log(&format!("swarm dispatch recovery: {dispatches}"));
                server::serve(d).await
            });
            if let Err(e) = result {
                log(&format!("fatal: {e:#}"));
                eprintln!("overseerd: {e:#}");
                std::process::exit(1);
            }
        }
        Some("ctl") => {
            let method = args.get(2).unwrap_or_else(|| usage());
            let params = args.get(3).map(|s| serde_json::from_str::<serde_json::Value>(s).expect("params must be JSON")).unwrap_or(serde_json::json!({}));
            let mut conn = match std::os::unix::net::UnixStream::connect(paths::socket_path()) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("cannot connect to {}: {e}", paths::socket_path().display());
                    std::process::exit(1);
                }
            };
            let msg = serde_json::json!({"id": 1, "method": method, "params": params});
            conn.write_all(format!("{msg}\n").as_bytes()).unwrap();
            let reader = BufReader::new(conn);
            for line in reader.lines() {
                let line = line.unwrap();
                println!("{line}");
                if method != "events.subscribe" {
                    break;
                }
            }
        }
        _ => usage(),
    }
}
