//! Finding the same `overseerd` VS Code uses, and its socket.
//!
//! Order: `--daemon PATH` / `OVERSEERD`, the binary next to this one (a workspace build), `PATH`,
//! then the binary bundled in the installed VS Code extension. The socket path comes from the
//! daemon itself (`overseerd socket-path`), so `OVERSEER_HOME` and the long-path fallback match
//! whatever VS Code connects to.
//!
//! Production never points at a dev version (AC-212). A TUI is dev only when the dev marker file
//! (`overseer-dev-instance`, written by `scripts/dev`) sits next to its binary. Otherwise it is
//! production: it ignores `OVERSEER_HOME`, `OVERSEER_SOCKET` and `OVERSEER_INSTANCE` leaked into its
//! environment (only `--home` chooses another data folder), never runs a daemon binary marked dev,
//! and refuses a daemon that reports a dev instance (see `client.rs`).

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The file next to dev binaries that marks them dev.
pub const DEV_MARKER: &str = "overseer-dev-instance";
/// Variables that point a daemon at another instance: production drops them.
pub const LEAKY: [&str; 3] = ["OVERSEER_HOME", "OVERSEER_SOCKET", "OVERSEER_INSTANCE"];

/// The dev instance a binary is marked with (the file next to it), if any.
pub fn dev_marker(binary: &Path) -> Option<String> {
    let text = std::fs::read_to_string(binary.parent()?.join(DEV_MARKER)).ok()?;
    Some(Some(text.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| "dev".into()))
}

/// Whether this TUI is production (no dev marker next to its own binary).
pub fn production() -> bool {
    !std::env::current_exe().ok().is_some_and(|me| dev_marker(&me).is_some())
}

#[derive(Debug, Clone)]
pub struct Daemon {
    pub binary: PathBuf,
    /// Data directory to start it with (None: inherit `OVERSEER_HOME`, like `--home` sets).
    pub home: Option<PathBuf>,
}

impl Daemon {
    pub fn find(explicit: Option<&Path>) -> Result<Daemon> {
        Self::find_as(explicit, production())
    }

    /// `production`: a binary named by `--daemon` or `OVERSEERD` that is marked dev is refused;
    /// marked binaries found on their own are passed over.
    pub fn find_as(explicit: Option<&Path>, production: bool) -> Result<Daemon> {
        let candidates = candidates(explicit);
        let named = explicit.map(Path::to_path_buf).or_else(|| std::env::var_os("OVERSEERD").map(PathBuf::from));
        for p in candidates.iter().filter(|p| p.is_file()) {
            if production {
                if let Some(marker) = dev_marker(p) {
                    if named.as_deref() == Some(p.as_path()) {
                        return Err(anyhow!("refusing {}: it is a dev build ({marker}). This overseer-tui is the installed one and uses only the standard daemon; dev instances have their own TUI (scripts/dev tui).", p.display()));
                    }
                    continue;
                }
            }
            return Ok(Daemon { binary: p.clone(), home: None });
        }
        Err(anyhow!("overseerd not found (looked in: {}). Pass --daemon PATH or set OVERSEERD.", candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")))
    }

    /// A command of this daemon binary, with the environment production allows.
    pub fn command(&self, production: bool) -> Command {
        let mut cmd = Command::new(&self.binary);
        if production {
            for key in LEAKY {
                cmd.env_remove(key);
            }
        }
        if let Some(h) = &self.home {
            cmd.env("OVERSEER_HOME", h);
        }
        cmd
    }

    pub fn socket_path(&self) -> Result<PathBuf> {
        let out = self.command(production()).arg("socket-path").output().with_context(|| format!("running {} socket-path", self.binary.display()))?;
        if !out.status.success() {
            return Err(anyhow!("{} socket-path failed", self.binary.display()));
        }
        Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
    }

    /// Starts `overseerd serve` detached, so it outlives this TUI (as it outlives VS Code).
    pub fn spawn_serve(&self) -> Result<()> {
        use std::os::unix::process::CommandExt;
        let mut cmd = self.command(production());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_bin(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let bin = dir.join("overseerd");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        bin
    }

    #[test]
    fn ac212_production_drops_leaked_variables_and_keeps_an_explicit_home() {
        let d = Daemon { binary: PathBuf::from("/bin/true"), home: None };
        let removed: Vec<String> = d.command(true).get_envs().filter(|(_, v)| v.is_none()).map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        for key in LEAKY {
            assert!(removed.contains(&key.to_string()), "{key} removed");
        }
        assert_eq!(d.command(false).get_envs().count(), 0, "a dev TUI passes its environment on");
        let with_home = Daemon { binary: PathBuf::from("/bin/true"), home: Some(PathBuf::from("/tmp/h")) };
        let home: Vec<_> = with_home.command(true).get_envs().filter(|(k, _)| *k == "OVERSEER_HOME").map(|(_, v)| v.map(|v| v.to_owned())).collect();
        assert_eq!(home, vec![Some(std::ffi::OsString::from("/tmp/h"))], "--home is deliberate and kept");
    }

    #[test]
    fn ac212_production_refuses_a_daemon_binary_marked_dev() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = fake_bin(&tmp.path().join("bin"));
        std::fs::write(tmp.path().join("bin").join(DEV_MARKER), "dev-a\n").unwrap();
        assert_eq!(dev_marker(&bin).as_deref(), Some("dev-a"));
        let err = Daemon::find_as(Some(&bin), true).unwrap_err().to_string();
        assert!(err.contains("it is a dev build (dev-a)"), "{err}");
        assert_eq!(Daemon::find_as(Some(&bin), false).unwrap().binary, bin, "a dev TUI may use it");
        let plain = fake_bin(&tmp.path().join("plain"));
        assert_eq!(Daemon::find_as(Some(&plain), true).unwrap().binary, plain);
    }
}
