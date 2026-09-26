//! Worktree file tree for the Overseer view (AC-51): one directory at a time, so very large
//! repositories stay responsive, with each entry marked when it (or anything below it) changed
//! since the task started. Paths are confined to the workspace; `.git` is never listed.

use crate::daemon::Daemon;
use crate::git;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Component, Path};

/// Entries returned per directory; the rest are counted and reported as truncated.
pub const MAX_ENTRIES: usize = 5000;

impl Daemon {
    pub fn workspace_tree(&self, workspace_id: &str, dir: &str) -> Result<Value> {
        let ws = self.workspace(workspace_id)?;
        if ws.removed_ms.is_some() {
            bail!("this workspace was removed");
        }
        let rel = Path::new(dir.trim_start_matches("./"));
        if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) || rel.components().any(|c| c.as_os_str() == ".git") {
            bail!("path must be inside the workspace");
        }
        let root = std::fs::canonicalize(&ws.path)?;
        let target = std::fs::canonicalize(root.join(rel)).map_err(|_| anyhow!("no such directory: {dir}"))?;
        if !target.starts_with(&root) || !target.is_dir() {
            bail!("path must be a directory inside the workspace");
        }
        // Changes since the task started (task-start snapshot), falling back to HEAD.
        let base = {
            let store = self.store.lock().unwrap();
            let task = store.tasks()?.into_iter().find(|t| t.workspace_id == ws.id);
            task.and_then(|t| t.start_snapshot).and_then(|id| store.snapshot(&id).ok().flatten()).map(|s| s.commit_sha)
        }
        .or_else(|| git::head(&root));
        let mut changed: HashMap<String, String> = HashMap::new();
        let mut base_label = Value::Null;
        if let Some(base) = &base {
            if let Ok(diff) = self.workspace_diff_opts(&ws.id, base, false) {
                for c in diff["changes"].as_array().cloned().unwrap_or_default() {
                    if let (Some(p), Some(st)) = (c["path"].as_str(), c["status"].as_str()) {
                        changed.insert(p.to_string(), st.to_string());
                    }
                }
                base_label = json!(base);
            }
        }
        // Changed-file counts per ancestor directory, computed once (large repositories).
        let mut inside_counts: HashMap<String, usize> = HashMap::new();
        for p in changed.keys() {
            let mut cur = Path::new(p.as_str()).parent();
            while let Some(d) = cur.filter(|d| !d.as_os_str().is_empty()) {
                *inside_counts.entry(d.to_string_lossy().to_string()).or_default() += 1;
                cur = d.parent();
            }
        }
        let prefix = target.strip_prefix(&root).unwrap_or(Path::new("")).to_string_lossy().to_string();
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        let mut total = 0usize;
        for entry in std::fs::read_dir(&target)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            total += 1;
            let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let ft = entry.file_type()?;
            let is_dir = ft.is_dir();
            let status = changed.get(&path).cloned();
            let inside = if is_dir { inside_counts.get(&path).copied().unwrap_or(0) } else { 0 };
            let item = json!({"name": name, "path": path, "dir": is_dir, "symlink": ft.is_symlink(), "status": status, "changes_inside": inside});
            if is_dir { dirs.push(item) } else { files.push(item) }
        }
        let key = |v: &Value| v["name"].as_str().unwrap_or_default().to_lowercase();
        dirs.sort_by_key(key);
        files.sort_by_key(key);
        // Deleted files are not on disk; list the ones that belonged directly in this directory.
        let deleted: Vec<Value> = changed
            .iter()
            .filter(|(p, st)| st.as_str() == "D" && Path::new(p.as_str()).parent().map(|d| d.to_string_lossy() == prefix).unwrap_or(false))
            .map(|(p, _)| json!({"name": Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()), "path": p, "dir": false, "status": "D", "deleted": true, "changes_inside": 0}))
            .collect();
        let mut entries: Vec<Value> = dirs.into_iter().chain(files).collect();
        let truncated = entries.len() > MAX_ENTRIES;
        entries.truncate(MAX_ENTRIES);
        entries.extend(deleted);
        Ok(json!({"workspace_id": ws.id, "root": ws.path, "dir": prefix, "base": base_label, "entries": entries, "total": total, "truncated": truncated, "changed_total": changed.len()}))
    }
}
