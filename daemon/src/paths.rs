//! Platform boundary for filesystem locations. Everything platform-specific about
//! where Overseer keeps state lives here so Linux support only touches this module.

use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Root for durable state (database, run directories, worktrees, profiles).
/// `OVERSEER_HOME` overrides the platform default (used by tests and fixtures).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OVERSEER_HOME") {
        return PathBuf::from(dir);
    }
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Overseer")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("overseer")
    }
}

/// Directory holding the control socket. Owner-only (0700).
pub fn runtime_dir() -> PathBuf {
    if std::env::var_os("OVERSEER_HOME").is_some() {
        return data_dir().join("run");
    }
    if cfg!(target_os = "linux") {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            return PathBuf::from(dir).join("overseer");
        }
    }
    data_dir().join("run")
}

/// Unix socket paths are limited to ~104 bytes on macOS. Long state directories
/// fall back to a per-user private directory keyed by the state directory.
pub fn short_socket(name: &str) -> PathBuf {
    let preferred = runtime_dir().join(name);
    if preferred.as_os_str().len() < 100 {
        return preferred;
    }
    let uid = unsafe { libc::getuid() };
    let dir = PathBuf::from(format!("/tmp/overseer-{uid}"));
    let _ = ensure_private_dir(&dir);
    let key = crate::daemon::fingerprint(&data_dir().display().to_string());
    dir.join(format!("{}-{name}", &key[..8]))
}

pub fn socket_path() -> PathBuf {
    std::env::var_os("OVERSEER_SOCKET").map(PathBuf::from).unwrap_or_else(|| short_socket("overseerd.sock"))
}

pub fn db_path() -> PathBuf {
    data_dir().join("overseer.sqlite")
}

pub fn runs_dir() -> PathBuf {
    data_dir().join("runs")
}

pub fn worktrees_dir() -> PathBuf {
    data_dir().join("worktrees")
}

pub fn profiles_dir() -> PathBuf {
    data_dir().join("profiles")
}

pub fn log_path() -> PathBuf {
    data_dir().join("overseerd.log")
}

/// Create a directory (and parents) readable only by the current user.
pub fn ensure_private_dir(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}
