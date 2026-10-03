//! Platform boundary for filesystem locations. Everything platform-specific about
//! where Overseer keeps state lives here so Linux support only touches this module.

use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Root for durable state (database, run directories, worktrees, profiles).
/// `OVERSEER_HOME` overrides the platform default (used by tests and fixtures).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OVERSEER_HOME") {
        return PathBuf::from(dir);
    }
    standard_data_dir()
}

pub fn mods_dir() -> PathBuf { data_dir().join("mods") }

/// The standard (production) data folder: the platform default, whatever `OVERSEER_HOME` says.
pub fn standard_data_dir() -> PathBuf {
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
    standard_runtime_dir()
}

fn standard_runtime_dir() -> PathBuf {
    if cfg!(target_os = "linux") {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            return PathBuf::from(dir).join("overseer");
        }
    }
    standard_data_dir().join("run")
}

/// The per-user folder for sockets whose preferred path is too long.
pub fn fallback_socket_dir() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/overseer-{uid}"))
}

/// Unix socket paths are limited to ~104 bytes on macOS. Long state directories
/// fall back to a per-user private directory keyed by the state directory.
fn socket_in(runtime: &Path, data: &Path, name: &str) -> PathBuf {
    let preferred = runtime.join(name);
    if preferred.as_os_str().len() < 100 {
        return preferred;
    }
    let key = crate::daemon::fingerprint(&data.display().to_string());
    fallback_socket_dir().join(format!("{}-{name}", &key[..8]))
}

pub fn short_socket(name: &str) -> PathBuf {
    let path = socket_in(&runtime_dir(), &data_dir(), name);
    if path.starts_with(fallback_socket_dir()) {
        let _ = ensure_private_dir(&fallback_socket_dir());
    }
    path
}

pub fn socket_path() -> PathBuf {
    std::env::var_os("OVERSEER_SOCKET").map(PathBuf::from).unwrap_or_else(|| short_socket("overseerd.sock"))
}

/// The standard (production) daemon's socket, whatever `OVERSEER_HOME` and `OVERSEER_SOCKET` say.
pub fn standard_socket_path() -> PathBuf {
    socket_in(&standard_runtime_dir(), &standard_data_dir(), "overseerd.sock")
}

// ------------------------------------------------------------------ dev instances (AC-212)

/// The file next to a dev build's binaries that marks them dev (`scripts/dev` writes it).
pub const DEV_MARKER: &str = "overseer-dev-instance";

/// This daemon's dev instance (`OVERSEER_INSTANCE=dev-<name>`), or None for the standard one.
pub fn instance() -> Option<String> {
    std::env::var("OVERSEER_INSTANCE").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The dev marker file next to this binary, if any (its content: the instance name).
fn marker_instance() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let text = std::fs::read_to_string(exe.parent()?.join(DEV_MARKER)).ok()?;
    Some(text.trim().to_string())
}

/// `dev-` then 1-32 of `[a-z0-9-]`, starting with a letter or digit.
pub fn valid_instance(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("dev-") else { return false };
    !rest.is_empty() && rest.len() <= 32 && rest.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !rest.starts_with('-')
}

/// A path compared the way the file system sees it (`/tmp` and `/private/tmp` are one folder).
fn resolved(p: &Path) -> PathBuf {
    if let Ok(real) = std::fs::canonicalize(p) {
        return real;
    }
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) => resolved(parent).join(name),
        _ => p.to_path_buf(),
    }
}

/// A dev daemon never opens the standard instance (AC-212). Standard daemons pass (Ok(None));
/// a dev one passes (Ok(Some(instance))) only with its own data folder and socket.
pub fn dev_guard() -> Result<Option<String>, String> {
    let env = instance();
    let file = marker_instance();
    let name = match (&env, &file) {
        (None, None) => return Ok(None),
        (None, Some(f)) => return Err(format!("this overseerd is a dev build (its {DEV_MARKER} file says {f:?}); start it with OVERSEER_INSTANCE={f} and its own OVERSEER_HOME (scripts/dev up), never as the standard Overseer")),
        (Some(e), Some(f)) if e != f => return Err(format!("OVERSEER_INSTANCE={e} does not match this dev build's {DEV_MARKER} file ({f})")),
        (Some(e), _) => e.clone(),
    };
    if !valid_instance(&name) {
        return Err(format!("OVERSEER_INSTANCE={name:?} is not a dev instance name (dev-<name>, lower-case letters, digits and hyphens)"));
    }
    if std::env::var_os("OVERSEER_HOME").is_none() {
        return Err(format!("dev instance {name} needs its own OVERSEER_HOME; it never opens the standard data folder"));
    }
    if resolved(&data_dir()) == resolved(&standard_data_dir()) {
        return Err(format!("dev instance {name} refuses the standard data folder {}", standard_data_dir().display()));
    }
    let socket = resolved(&socket_path());
    if socket == resolved(&standard_socket_path()) {
        return Err(format!("dev instance {name} refuses the standard socket {}", standard_socket_path().display()));
    }
    if socket.starts_with(resolved(&fallback_socket_dir())) {
        return Err(format!("dev instance {name} refuses a socket in the standard socket folder {}; set OVERSEER_SOCKET inside its own folder", fallback_socket_dir().display()));
    }
    Ok(Some(name))
}

pub fn db_path() -> PathBuf {
    data_dir().join("overseer.sqlite")
}

pub fn learning_db_path(database: &std::path::Path) -> PathBuf {
    let mut name = database.as_os_str().to_os_string();
    name.push(".learning");
    PathBuf::from(name)
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
