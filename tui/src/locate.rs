//! Finding the same `overseerd` VS Code uses, and its socket.
//!
//! Order: `--daemon PATH` / `OVERSEERD`, the binary next to this one (a workspace build), `PATH`,
//! then the binary bundled in the installed VS Code extension. The socket path comes from the
//! daemon itself (`overseerd socket-path`), so `OVERSEER_HOME` and the long-path fallback match
//! whatever VS Code connects to.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct Daemon {
    pub binary: PathBuf,
}

impl Daemon {
    pub fn find(explicit: Option<&Path>) -> Result<Daemon> {
        let candidates = candidates(explicit);
        candidates
            .iter()
            .find(|p| p.is_file())
            .map(|p| Daemon { binary: p.clone() })
            .ok_or_else(|| anyhow!("overseerd not found (looked in: {}). Pass --daemon PATH or set OVERSEERD.", candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")))
    }

    pub fn socket_path(&self) -> Result<PathBuf> {
        let out = Command::new(&self.binary).arg("socket-path").output().with_context(|| format!("running {} socket-path", self.binary.display()))?;
        if !out.status.success() {
            return Err(anyhow!("{} socket-path failed", self.binary.display()));
        }
        Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
    }

    /// Starts `overseerd serve` detached, so it outlives this TUI (as it outlives VS Code).
    pub fn spawn_serve(&self) -> Result<()> {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new(&self.binary);
        cmd.arg("serve").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // New session: not in this terminal's process group, so Ctrl-C here never reaches it.
        unsafe {
            cmd.pre_exec(|| {
                libc_setsid();
                Ok(())
            });
        }
        cmd.spawn().with_context(|| format!("starting {} serve", self.binary.display()))?;
        Ok(())
    }
}

fn libc_setsid() {
    extern "C" {
        fn setsid() -> i32;
    }
    unsafe {
        setsid();
    }
}

fn candidates(explicit: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = explicit {
        out.push(p.to_path_buf());
        return out;
    }
    if let Some(p) = std::env::var_os("OVERSEERD") {
        out.push(PathBuf::from(p));
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            out.push(dir.join("overseerd"));
            // Test binaries live one level down (target/debug/deps).
            if let Some(up) = dir.parent() {
                out.push(up.join("overseerd"));
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            out.push(dir.join("overseerd"));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let ext = PathBuf::from(home).join(".vscode/extensions");
        if let Ok(entries) = std::fs::read_dir(&ext) {
            let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("beelol.overseer-"))).collect();
            dirs.sort();
            for d in dirs.into_iter().rev() {
                out.push(d.join("bin").join(format!("overseerd-{}-{}", platform(), arch())));
                out.push(d.join("bin").join("overseerd"));
            }
        }
    }
    out
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}
