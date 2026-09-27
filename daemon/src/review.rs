//! Review through the daemon (Gate N, AC-126): file contents, hunks, reviewed marks and rejecting a
//! hunk, confined to the workspace under the rules of AC-34. A phone has no file system of its
//! own, so everything it reviews comes from here; VS Code uses the same marks, so a mark made on
//! one surface shows on the other.

use crate::daemon::Daemon;
use crate::git;
use crate::server::ProtoError;
use crate::store::Workspace;
use anyhow::{anyhow, bail, Result};
use rusqlite::params;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Larger files are described, not sent.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Reviewed marks kept per run (the newest).
pub const MAX_MARKS_PER_RUN: i64 = 2000;

pub fn migrate(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS review_marks(run_id TEXT NOT NULL, key TEXT NOT NULL, path TEXT NOT NULL, at_ms INTEGER NOT NULL, by TEXT NOT NULL, PRIMARY KEY(run_id, key));",
    )?;
    Ok(())
}

/// The key of a hunk: the same function the review in VS Code uses (extension/branch-diff/review/
/// browser.js, `hunkHash`), over UTF-16 code units, so both surfaces name a hunk the same way.
pub fn hunk_key(path: &str, base: &[String], modified: &[String]) -> String {
    let text = format!("{path}\u{0}{}\u{0}{}", base.join("\n"), modified.join("\n"));
    let (mut h1, mut h2) = (0x811c_9dc5u32, 0x0100_0193u32);
    for c in text.encode_utf16() {
        h1 = (h1 ^ c as u32).wrapping_mul(16_777_619);
        h2 = (h2 ^ c as u32).wrapping_mul(2_246_822_519);
    }
    format!("{h1:08x}{h2:08x}")
}

/// Lines as an editor counts them: split on `\r\n`, `\n` or `\r`, without the line ends.
pub fn lines_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(std::mem::take(&mut cur));
            }
            '\n' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// True for a name that a file system can take for `.git`: any case (APFS and HFS+ ignore case
/// by default), with trailing dots or spaces, with characters HFS+ ignores, or its short name.
pub fn is_git_name(name: &std::ffi::OsStr) -> bool {
    let text = name.to_string_lossy();
    let ignorable = |c: char| matches!(c, '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}');
    let plain: String = text.chars().filter(|c| !ignorable(*c)).collect::<String>().trim_end_matches(['.', ' ']).to_lowercase();
    plain == ".git" || plain == "git~1"
}

fn refuse(message: impl Into<String>) -> anyhow::Error {
    ProtoError::new("outside_workspace", message).into()
}

/// A path inside the workspace, as given: no absolute path, no `..`, nothing under `.git`.
fn clean_rel(rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel.trim_start_matches("./"));
    if rel.is_empty() || rel.contains('\0') || p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(refuse("the path must be inside the workspace"));
    }
    if p.components().any(|c| is_git_name(c.as_os_str())) {
        return Err(refuse("files under .git are not shown"));
    }
    Ok(p.to_path_buf())
}

/// A comparison that names a commit or a ref, and nothing a shell or Git would read as an option.
fn clean_base(base: &str) -> Result<&str> {
    let ok = !base.is_empty() && base.len() <= 200 && !base.starts_with('-') && !base.contains("..") && base.chars().all(|c| c.is_ascii_alphanumeric() || "._/-@{}^~".contains(c));
    if !ok {
        bail!("not a comparison: {base}");
    }
    Ok(base)
}

struct Side {
    exists: bool,
    size: u64,
    kind: &'static str,
    text: Option<String>,
    note: Option<String>,
}

impl Side {
    fn missing() -> Self {
        Self { exists: false, size: 0, kind: "missing", text: None, note: None }
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        let size = bytes.len() as u64;
        if size > MAX_FILE_BYTES {
            return Self { exists: true, size, kind: "too_large", text: None, note: Some(format!("larger than {} MB", MAX_FILE_BYTES / 1024 / 1024)) };
        }
        if bytes.iter().take(8000).any(|b| *b == 0) {
            return Self { exists: true, size, kind: "binary", text: None, note: Some("a binary file".into()) };
        }
        match String::from_utf8(bytes) {
            Ok(text) => Self { exists: true, size, kind: "text", text: Some(text), note: None },
            Err(_) => Self { exists: true, size, kind: "binary", text: None, note: Some("not UTF-8 text".into()) },
        }
    }

    fn json(&self) -> Value {
        json!({"exists": self.exists, "size": self.size, "kind": self.kind, "text": self.text, "note": self.note})
    }
}

impl Daemon {
    fn live_workspace(&self, workspace_id: &str) -> Result<(Workspace, PathBuf)> {
        let ws = self.workspace(workspace_id)?;
        if ws.removed_ms.is_some() {
            bail!("this workspace was removed");
        }
        let root = std::fs::canonicalize(&ws.path).map_err(|_| anyhow!("this workspace is not on disk"))?;
        Ok((ws, root))
    }

    /// The working copy of a file, never through a link and never outside the workspace.
    fn working_side(root: &Path, rel: &Path) -> Result<Side> {
        let target = root.join(rel);
        // Every directory on the way must really be inside the workspace (a linked directory is not).
        if let Some(parent) = target.parent() {
            if let Ok(real) = std::fs::canonicalize(parent) {
                inside(root, &real)?;
            }
        }
        let meta = match std::fs::symlink_metadata(&target) {
            Ok(m) => m,
            Err(_) => return Ok(Side::missing()),
        };
        if meta.file_type().is_symlink() {
            let to = std::fs::read_link(&target).map(|p| p.display().to_string()).unwrap_or_default();
            return Ok(Side { exists: true, size: 0, kind: "link", text: None, note: Some(format!("a link to {to}; links are not followed")) });
        }
        if meta.is_dir() {
            return Ok(Side { exists: true, size: 0, kind: "directory", text: None, note: Some("a folder".into()) });
        }
        if !meta.is_file() {
            return Ok(Side { exists: true, size: 0, kind: "special", text: None, note: Some("not a regular file".into()) });
        }
        if meta.len() > MAX_FILE_BYTES {
            return Ok(Side { exists: true, size: meta.len(), kind: "too_large", text: None, note: Some(format!("larger than {} MB", MAX_FILE_BYTES / 1024 / 1024)) });
        }
        Ok(Side::from_bytes(std::fs::read(&target)?))
    }

    fn base_side(root: &Path, base: &str, rel: &Path) -> Result<Side> {
        let spec = format!("{base}:{}", rel.to_string_lossy());
        let size = match git::git(root, &["cat-file", "-s", &spec]) {
            Ok(s) => s.trim().parse::<u64>().unwrap_or(0),
            Err(_) => return Ok(Side::missing()),
        };
        if size > MAX_FILE_BYTES {
            return Ok(Side { exists: true, size, kind: "too_large", text: None, note: Some(format!("larger than {} MB", MAX_FILE_BYTES / 1024 / 1024)) });
        }
        let out = std::process::Command::new("git").current_dir(root).args(["cat-file", "blob", &spec]).env("GIT_TERMINAL_PROMPT", "0").output()?;
        if !out.status.success() {
            return Ok(Side::missing());
        }
        Ok(Side::from_bytes(out.stdout))
    }

    fn sides(&self, workspace_id: &str, path: &str, base: Option<&str>) -> Result<(Workspace, PathBuf, PathBuf, Side, Side)> {
        let (ws, root) = self.live_workspace(workspace_id)?;
        let rel = clean_rel(path)?;
        let before = match base {
            Some(b) => {
                let b = clean_base(b)?;
                if git::rev_parse(&root, b).is_none() {
                    bail!("comparison base {b} is not available in this repository");
                }
                Self::base_side(&root, b, &rel)?
            }
            None => Side::missing(),
        };
        let now = Self::working_side(&root, &rel)?;
        Ok((ws, root, rel, before, now))
    }

    /// One file at a comparison and in the working copy.
    pub fn workspace_file(&self, workspace_id: &str, path: &str, base: Option<&str>) -> Result<Value> {
        let (ws, _, rel, before, now) = self.sides(workspace_id, path, base)?;
        Ok(json!({"workspace_id": ws.id, "path": rel.to_string_lossy(), "base": base, "before": before.json(), "now": now.json()}))
    }

    /// The changes of one file as hunks, each with its key and whether it is marked reviewed.
    pub fn workspace_hunks(&self, workspace_id: &str, path: &str, base: &str, run_id: Option<&str>) -> Result<Value> {
        let (ws, _, rel, before, now) = self.sides(workspace_id, path, Some(base))?;
        let rel_text = rel.to_string_lossy().to_string();
        let marks: Vec<String> = match run_id {
            Some(run) => self.store.lock().unwrap().review_keys(run)?,
            None => Vec::new(),
        };
        let readable = |s: &Side| s.kind == "text" || s.kind == "missing";
        if !readable(&before) || !readable(&now) {
            let why = [&before, &now].iter().find_map(|s| s.note.clone()).unwrap_or_default();
            return Ok(json!({"workspace_id": ws.id, "path": rel_text, "base": base, "hunks": [], "shown": false, "why": why, "before": {"exists": before.exists, "kind": before.kind}, "now": {"exists": now.exists, "kind": now.kind}}));
        }
        let old = before.text.clone().unwrap_or_default();
        let new = now.text.clone().unwrap_or_default();
        let hunks = hunks_between(&rel_text, &old, &new)?;
        let list: Vec<Value> = hunks
            .into_iter()
            .map(|h| {
                let reviewed = marks.contains(&h.key);
                json!({"key": h.key, "base_start": h.base_start, "base_lines": h.base_lines, "modified_start": h.modified_start, "modified_lines": h.modified_lines, "reviewed": reviewed})
            })
            .collect();
        Ok(json!({
            "workspace_id": ws.id, "path": rel_text, "base": base, "shown": true, "hunks": list,
            "before": {"exists": before.exists, "kind": before.kind, "lines": if before.exists { lines_of(&old).len() } else { 0 }},
            "now": {"exists": now.exists, "kind": now.kind, "lines": if now.exists { lines_of(&new).len() } else { 0 }},
        }))
    }

    pub fn review_marks(&self, run_id: &str) -> Result<Value> {
        self.run(run_id)?;
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT key, path, at_ms, by FROM review_marks WHERE run_id=?1 ORDER BY at_ms")?;
        let marks = stmt
            .query_map(params![run_id], |r| Ok(json!({"key": r.get::<_, String>(0)?, "path": r.get::<_, String>(1)?, "at_ms": r.get::<_, i64>(2)?, "by": r.get::<_, String>(3)?})))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"run_id": run_id, "keys": marks.iter().map(|m| m["key"].clone()).collect::<Vec<_>>(), "marks": marks}))
    }

    /// Marks a hunk reviewed. The hunk is checked against the file as it is now: when the agent
    /// changed it meanwhile, that is a conflict and nothing is marked (the rule of AC-42).
    pub fn review_accept(&self, run_id: &str, p: &Value) -> Result<Value> {
        let run = self.run(run_id)?;
        let path = p["path"].as_str().ok_or_else(|| anyhow!("missing string parameter path"))?;
        let key = p["key"].as_str().filter(|k| k.len() == 16 && k.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())).ok_or_else(|| anyhow!("not a hunk key"))?;
        let strings = |v: &Value| -> Vec<String> { v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default() };
        let modified = strings(&p["modified_lines"]);
        let start = p["modified_start"].as_u64().unwrap_or(0) as usize;
        let (_, root) = self.live_workspace(&run.workspace_id)?;
        let rel = clean_rel(path)?;
        let now = Self::working_side(&root, &rel)?;
        let lines = lines_of(now.text.as_deref().unwrap_or(""));
        let same = if !modified.is_empty() {
            start >= 1 && lines.get(start - 1..start - 1 + modified.len()).is_some_and(|slice| slice == modified.as_slice())
        } else {
            // A hunk that only removes lines: the line it sits at must still be the one it was shown at.
            match p["anchor"].as_str() {
                Some(anchor) => start == 0 || lines.get(start.saturating_sub(1)).is_some_and(|l| l == anchor),
                None => true,
            }
        };
        if !same {
            return Err(ProtoError::new("conflict", format!("Not marked reviewed: {path} changed while you were accepting this hunk (conflict). Review the current content.")).into());
        }
        if let Some(base_lines) = p.get("base_lines").filter(|v| v.is_array()) {
            if hunk_key(path, &strings(base_lines), &modified) != key {
                bail!("the key does not name this hunk");
            }
        }
        let by = crate::server::actor().unwrap_or_else(|| "the Mac".into());
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("INSERT OR REPLACE INTO review_marks(run_id, key, path, at_ms, by) VALUES(?1, ?2, ?3, ?4, ?5)", params![run_id, key, path, crate::daemon::now(), by])?;
            store.conn.execute(
                "DELETE FROM review_marks WHERE run_id=?1 AND key NOT IN (SELECT key FROM review_marks WHERE run_id=?1 ORDER BY at_ms DESC LIMIT ?2)",
                params![run_id, MAX_MARKS_PER_RUN],
            )?;
        }
        self.emit(Some(&run.task_id), Some(run_id), "review_mark", "user", "exact", json!({"key": key, "path": path, "reviewed": true}))?;
        Ok(json!({"key": key, "reviewed": true}))
    }

    pub fn review_unaccept(&self, run_id: &str, key: &str) -> Result<Value> {
        let run = self.run(run_id)?;
        let removed: Option<String> = {
            let store = self.store.lock().unwrap();
            use rusqlite::OptionalExtension;
            let path = store.conn.query_row("SELECT path FROM review_marks WHERE run_id=?1 AND key=?2", params![run_id, key], |r| r.get(0)).optional()?;
            store.conn.execute("DELETE FROM review_marks WHERE run_id=?1 AND key=?2", params![run_id, key])?;
            path
        };
        if let Some(path) = &removed {
            self.emit(Some(&run.task_id), Some(run_id), "review_mark", "user", "exact", json!({"key": key, "path": path, "reviewed": false}))?;
        }
        Ok(json!({"key": key, "reviewed": false, "changed": removed.is_some()}))
    }

    /// Marks that were kept by a surface before the daemon kept them (VS Code's own store).
    pub fn review_import(&self, run_id: &str, marks: &Value) -> Result<Value> {
        self.run(run_id)?;
        let store = self.store.lock().unwrap();
        let mut added = 0;
        for m in marks.as_array().cloned().unwrap_or_default().iter().take(MAX_MARKS_PER_RUN as usize) {
            let (Some(key), Some(path)) = (m["key"].as_str(), m["path"].as_str()) else { continue };
            if key.len() != 16 || !key.chars().all(|c| c.is_ascii_hexdigit()) || clean_rel(path).is_err() {
                continue;
            }
            added += store.conn.execute("INSERT OR IGNORE INTO review_marks(run_id, key, path, at_ms, by) VALUES(?1, ?2, ?3, ?4, 'the Mac')", params![run_id, key, path, m["at"].as_i64().unwrap_or_else(crate::daemon::now)])?;
        }
        Ok(json!({"added": added}))
    }

    /// Rejects a hunk: puts the comparison's lines back in the worktree. Refused when the file is
    /// not what the hunk was shown against, or when it is not a regular text file.
    pub fn review_reject(self: &Arc<Self>, workspace_id: &str, p: &Value) -> Result<Value> {
        let path = p["path"].as_str().ok_or_else(|| anyhow!("missing string parameter path"))?;
        let base = clean_base(p["base"].as_str().ok_or_else(|| anyhow!("missing string parameter base"))?)?;
        let key = p["key"].as_str().ok_or_else(|| anyhow!("missing string parameter key"))?;
        let (ws, root, rel, before, now) = self.sides(workspace_id, path, Some(base))?;
        for side in [&before, &now] {
            if !matches!(side.kind, "text" | "missing") {
                return Err(ProtoError::new("not_editable", format!("{path} cannot be changed here: {}", side.note.clone().unwrap_or_default())).into());
            }
        }
        let old = before.text.clone().unwrap_or_default();
        let new = now.text.clone().unwrap_or_default();
        let rel_text = rel.to_string_lossy().to_string();
        let hunks = hunks_between(&rel_text, &old, &new)?;
        let Some(hunk) = hunks.iter().find(|h| h.key == key) else {
            return Err(ProtoError::new("conflict", format!("Not rejected: {path} changed while you were rejecting this hunk (conflict). Review the current content.")).into());
        };
        let target = root.join(&rel);
        let mut lines = if now.exists { lines_of(&new) } else { Vec::new() };
        let ending = if new.contains("\r\n") { "\r\n" } else { "\n" };
        if hunk.modified_lines.is_empty() {
            let at = hunk.modified_start.min(lines.len());
            lines.splice(at..at, hunk.base_lines.iter().cloned());
        } else {
            let from = hunk.modified_start - 1;
            lines.splice(from..from + hunk.modified_lines.len(), hunk.base_lines.iter().cloned());
        }
        // A file that is gone comes back exactly as it was at the comparison.
        let restored = if now.exists { lines.join(ending) } else { old.clone() };
        let deleted = !before.exists && restored.is_empty();
        if deleted {
            // The file did not exist at the comparison and nothing of it is left.
            std::fs::remove_file(&target)?;
        } else {
            write_inside(&root, &target, restored.as_bytes())?;
        }
        let (task_id, run_id) = {
            let store = self.store.lock().unwrap();
            let run = store.runs()?.into_iter().find(|r| r.workspace_id == ws.id && r.parent_run_id.is_none());
            (run.as_ref().map(|r| r.task_id.clone()), run.map(|r| r.id))
        };
        if let Some(run) = &run_id {
            let _ = self.store.lock().unwrap().conn.execute("DELETE FROM review_marks WHERE run_id=?1 AND key=?2", params![run, key]);
        }
        self.emit(task_id.as_deref(), run_id.as_deref(), "review_reject", "user", "exact", json!({"path": rel_text, "key": key, "base": base, "lines_restored": hunk.base_lines.len(), "lines_removed": hunk.modified_lines.len(), "file_removed": deleted}))?;
        Ok(json!({"path": rel_text, "rejected": true, "file_removed": deleted}))
    }
}

/// `real` (a resolved path) must be inside `root` and nowhere under `.git`.
fn inside(root: &Path, real: &Path) -> Result<()> {
    let Ok(rest) = real.strip_prefix(root) else { return Err(refuse("the path leaves the workspace through a link")) };
    if rest.components().any(|c| is_git_name(c.as_os_str())) {
        return Err(refuse("files under .git are not shown"));
    }
    Ok(())
}

/// Writes `bytes` to `target`, which must end up inside `root`. The folder is checked before
/// anything is created in it. The new content is written to a file that did not exist, under a
/// name nobody can know beforehand and never through a link, and then takes the target's place.
fn write_inside(root: &Path, target: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let parent = target.parent().ok_or_else(|| refuse("the path must be inside the workspace"))?;
    // The nearest folder that exists decides, before any folder is made.
    let mut existing = parent;
    while !existing.exists() {
        existing = existing.parent().ok_or_else(|| refuse("the path must be inside the workspace"))?;
    }
    inside(root, &std::fs::canonicalize(existing)?)?;
    std::fs::create_dir_all(parent)?;
    let parent = std::fs::canonicalize(parent)?;
    inside(root, &parent)?;
    let name = target.file_name().ok_or_else(|| refuse("the path must be inside the workspace"))?;
    let target = parent.join(name);
    if std::fs::symlink_metadata(&target).is_ok_and(|m| !m.is_file()) {
        return Err(ProtoError::new("not_editable", "this is not a regular file").into());
    }
    let tmp = parent.join(format!(".overseer-{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true).custom_flags(libc::O_NOFOLLOW).mode(0o600);
    let written = (|| -> Result<()> {
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        if let Ok(meta) = std::fs::metadata(&target) {
            file.set_permissions(meta.permissions())?;
        } else {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
        }
        file.sync_all()?;
        std::fs::rename(&tmp, &target)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

pub struct Hunk {
    pub key: String,
    /// First line of the comparison this hunk replaces; for an insertion, the line it follows.
    pub base_start: usize,
    pub base_lines: Vec<String>,
    /// First line of the working copy this hunk holds; for a removal, the line it follows.
    pub modified_start: usize,
    pub modified_lines: Vec<String>,
}

/// Hunks without context between two texts, computed by Git (`git diff --no-index`).
pub fn hunks_between(path: &str, old: &str, new: &str) -> Result<Vec<Hunk>> {
    if old == new {
        return Ok(Vec::new());
    }
    let dir = crate::paths::data_dir().join("tmp");
    crate::paths::ensure_private_dir(&dir)?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (a, b) = (dir.join(format!("{id}.before")), dir.join(format!("{id}.now")));
    std::fs::write(&a, old)?;
    std::fs::write(&b, new)?;
    let out = std::process::Command::new("git")
        .current_dir(&dir)
        .args(["-c", "core.quotepath=off", "diff", "--no-index", "--no-color", "--no-ext-diff", "--no-textconv", "-U0", "--"])
        .arg(&a)
        .arg(&b)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output();
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    let out = out?;
    if out.status.code().is_none_or(|c| c > 1) {
        bail!("git diff failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let (old_lines, new_lines) = (lines_of(old), lines_of(new));
    let header = regex::Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@").unwrap();
    let mut hunks = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some(c) = header.captures(line) else { continue };
        let num = |i: usize, default: usize| c.get(i).and_then(|m| m.as_str().parse::<usize>().ok()).unwrap_or(default);
        let (a_start, a_count, b_start, b_count) = (num(1, 0), num(2, 1), num(3, 0), num(4, 1));
        let base_lines: Vec<String> = if a_count == 0 { Vec::new() } else { old_lines.get(a_start - 1..(a_start - 1 + a_count).min(old_lines.len())).unwrap_or(&[]).to_vec() };
        let modified_lines: Vec<String> = if b_count == 0 { Vec::new() } else { new_lines.get(b_start - 1..(b_start - 1 + b_count).min(new_lines.len())).unwrap_or(&[]).to_vec() };
        hunks.push(Hunk { key: hunk_key(path, &base_lines, &modified_lines), base_start: a_start, base_lines, modified_start: b_start, modified_lines });
    }
    Ok(hunks)
}

impl crate::store::Store {
    pub fn review_keys(&self, run_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT key FROM review_marks WHERE run_id=?1")?;
        let keys = stmt.query_map(params![run_id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hunk_key_is_the_one_vs_code_computes() {
        // Values from extension/branch-diff/review/browser.js hunkHash, run in Node for these inputs.
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(hunk_key("a.txt", &s(&["a"]), &s(&["b"])), expected("a.txt\u{0}a\u{0}b"));
        assert_eq!(hunk_key("src/é.ts", &[], &s(&["let x = '😀';", "}"])), expected("src/é.ts\u{0}\u{0}let x = '😀';\n}"));
    }

    /// The JavaScript function, transcribed: 32-bit multiply over UTF-16 code units.
    fn expected(text: &str) -> String {
        let (mut h1, mut h2) = (0x811c9dc5u32, 0x01000193u32);
        for c in text.encode_utf16() {
            h1 = ((h1 ^ c as u32) as u64 * 16777619u64) as u32;
            h2 = ((h2 ^ c as u32) as u64 * 2246822519u64) as u32;
        }
        format!("{h1:08x}{h2:08x}")
    }

    #[test]
    fn lines_split_like_an_editor() {
        assert_eq!(lines_of("a\nb"), vec!["a", "b"]);
        assert_eq!(lines_of("a\r\nb\r\n"), vec!["a", "b", ""]);
        assert_eq!(lines_of("a\rb"), vec!["a", "b"]);
        assert_eq!(lines_of(""), vec![""]);
    }

    #[test]
    fn paths_and_comparisons_are_confined() {
        for bad in ["", "/etc/passwd", "../x", "a/../../x", ".git/config", "a/.git/HEAD", "a\0b"] {
            assert!(clean_rel(bad).is_err(), "{bad:?}");
        }
        for bad in [".GIT/config", ".Git/HEAD", "a/.GiT/x", ".git./config", ".git /config", ".g\u{200c}it/config", "GIT~1/config", "git~1/x"] {
            assert!(clean_rel(bad).is_err(), "{bad:?} can be .git to a file system");
        }
        for ok in ["a.txt", "src/a b.ts", "./x", ".github/x", ".gitignore", ".gitattributes", "git/x", "x.git/y", ".gitmodules"] {
            assert!(clean_rel(ok).is_ok(), "{ok:?}");
        }
        for bad in ["", "-x", "--output=/tmp/x", "a..b", "a b", "a;b", "$(x)", "a:b"] {
            assert!(clean_base(bad).is_err(), "{bad:?}");
        }
        for ok in ["HEAD", "main", "origin/main", "0123abcd", "HEAD~2", "refs/overseer/snapshots/s-1"] {
            assert!(clean_base(ok).is_ok(), "{ok:?}");
        }
    }
}
