//! Agents start with what Overseer knows (AC-231). An agent Overseer starts gets, after the
//! request's own words, what the conversation with the owner holds that the request may not say:
//! the files named (in this repository or another), the repositories named, where the other
//! agents' work is, and what the owner said. The daemon builds it from its own records with no
//! model, so a short request ("Please add the login page.") still carries the path the owner gave
//! three messages earlier. It is shown in the agent's chat as a briefing, like AC-190's.

use crate::daemon::Daemon;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// At most this much is added to a start's prompt.
pub const CONTEXT_BYTES: usize = 3 * 1024;
/// The conversation's last messages read for names.
const MESSAGES_READ: i64 = 60;
/// The owner's last messages quoted, each at most `SAID_CHARS`.
const SAID: usize = 6;
const SAID_CHARS: usize = 300;
/// Other agents listed under "where the work is", and paths per agent.
const AGENTS: usize = 8;
const PATHS_PER_AGENT: usize = 5;

/// A file the conversation named, found on disk: its repository and its path inside it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Named {
    pub repo: String,
    pub path: String,
}

fn canon(p: &Path) -> String {
    std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .display()
        .to_string()
}

fn base(root: &str) -> &str {
    root.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(root)
}

/// The Git repository a path is in: the nearest folder upwards with a `.git`.
fn git_root(p: &Path) -> Option<PathBuf> {
    let mut at = if p.is_dir() { Some(p) } else { p.parent() };
    while let Some(dir) = at {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        at = dir.parent();
    }
    None
}

/// The words of a message that look like paths: "/a/b.rs", "~/x/y", "api/src/login.rs",
/// "login.tsx", without the quotes and punctuation around them.
pub fn path_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '`' | '"' | '“' | '”' | '‘' | '’' | '(' | ')' | '[' | ']' | '<' | '>' | ',' | ';'
            )
    }) {
        let w = raw.trim_matches(|c: char| matches!(c, '\'' | ':' | '.' | '!' | '?' | '*'));
        if w.len() < 3 || w.contains("://") || w.starts_with('@') {
            continue;
        }
        let file_like = w.rsplit('/').next().is_some_and(|last| {
            let mut parts = last.rsplitn(2, '.');
            let ext = parts.next().unwrap_or("");
            parts.next().is_some_and(|stem| stem.chars().count() >= 2)
                && (1..=6).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.chars().any(|c| c.is_ascii_alphabetic())
        });
        if w.contains('/') || file_like {
            out.push(w.to_string());
        }
    }
    out
}

/// Resolves the conversation's path words against the disk: absolute (or `~/`) paths as they
/// are, relative ones inside the start's repository, then the known repositories, then as
/// "<repository name>/<path>". Only what exists is kept, with the repository it is in.
pub fn resolve(words: &[String], start_repo: &str, known: &[String]) -> Vec<Named> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut roots: Vec<String> = vec![start_repo.to_string()];
    roots.extend(known.iter().filter(|k| k.as_str() != start_repo).cloned());
    let mut out = BTreeSet::new();
    for w in words {
        let absolute = if let Some(rest) = w.strip_prefix("~/") {
            Some(Path::new(&home).join(rest))
        } else if w.starts_with('/') {
            Some(PathBuf::from(w))
        } else {
            None
        };
        let found: Option<(String, String)> = match absolute {
            Some(p) => p.exists().then(|| git_root(&p)).flatten().and_then(|root| {
                let root = canon(&root);
                let full = canon(&p);
                full.strip_prefix(&root)
                    .map(|rel| (root.clone(), rel.trim_start_matches('/').to_string()))
            }),
            None => {
                let rel = w.trim_start_matches("./");
                roots
                    .iter()
                    .find(|r| Path::new(r).join(rel).exists())
                    .map(|r| (r.clone(), rel.to_string()))
                    .or_else(|| {
                        let (first, rest) = rel.split_once('/')?;
                        roots
                            .iter()
                            .find(|r| {
                                base(r) == first
                                    && !rest.is_empty()
                                    && Path::new(r).join(rest).exists()
                            })
                            .map(|r| (r.clone(), rest.to_string()))
                    })
            }
        };
        if let Some((repo, path)) = found {
            out.insert(Named {
                repo,
                path: if path.is_empty() { ".".into() } else { path },
            });
        }
    }
    out.into_iter().collect()
}

impl Daemon {
    /// What Overseer knows that the start's prompt (`prompt`) does not already say, or empty when
    /// there is nothing.
    pub(crate) fn start_context(&self, repo: &str, prompt: &str) -> String {
        let start_repo = canon(Path::new(repo));
        let sid = self
            .overseer_session()
            .ok()
            .and_then(|s| s["id"].as_str().map(str::to_string))
            .unwrap_or_default();
        // The conversation, oldest first.
        let messages: Vec<(String, String)> = {
            let store = self.store.lock().unwrap();
            let mut stmt = match store.conn.prepare("SELECT source, text FROM overseer_messages WHERE session_id=?1 AND source IN ('owner', 'overseer') ORDER BY seq DESC LIMIT ?2") {
                Ok(s) => s,
                Err(_) => return String::new(),
            };
            let rows: Vec<(String, String)> = stmt
                .query_map(rusqlite::params![sid, MESSAGES_READ], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default();
            rows.into_iter().rev().collect()
        };
        let (tasks, runs) = {
            let store = self.store.lock().unwrap();
            (
                store.tasks().unwrap_or_default(),
                store.runs().unwrap_or_default(),
            )
        };
        // The repositories agents ran in (not Overseer's own empty folder).
        let own_task = self
            .overseer_session()
            .ok()
            .and_then(|s| s["task_id"].as_str().map(str::to_string));
        let mut known: Vec<String> = Vec::new();
        for t in &tasks {
            let r = canon(Path::new(&t.repo_root));
            if !known.contains(&r) && own_task.as_deref() != Some(t.id.as_str()) {
                known.push(r);
            }
        }
        let all_text: String = messages
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let words = path_words(&all_text);
        // Absolute paths name repositories too (one Overseer has never run an agent in).
        for w in &words {
            if w.starts_with('/') || w.starts_with("~/") {
                let p = if let Some(rest) = w.strip_prefix("~/") {
                    Path::new(&std::env::var("HOME").unwrap_or_default()).join(rest)
                } else {
                    PathBuf::from(w)
                };
                if let Some(root) = p.exists().then(|| git_root(&p)).flatten() {
                    let root = canon(&root);
                    if !known.contains(&root) {
                        known.push(root);
                    }
                }
            }
        }
        let files: Vec<Named> = resolve(&words, &start_repo, &known)
            .into_iter()
            .filter(|n| n.path != ".")
            .collect();

        // Repositories named: by a file in them, by their path or by their name as a word.
        let lower = all_text.to_lowercase();
        let named_word = |name: &str| {
            let name = name.to_lowercase();
            lower.match_indices(&name).any(|(at, _)| {
                let before = lower[..at].chars().next_back();
                let after = lower[at + name.len()..].chars().next();
                !before.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
                    && !after.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
            })
        };
        let mut repos: Vec<String> = Vec::new();
        for f in &files {
            if !repos.contains(&f.repo) {
                repos.push(f.repo.clone());
            }
        }
        for k in &known {
            if !repos.contains(k)
                && (all_text.contains(k.as_str()) || (base(k).len() >= 3 && named_word(base(k))))
            {
                repos.push(k.clone());
            }
        }

        let this = |r: &str| {
            if r == start_repo {
                "this repository".to_string()
            } else {
                "another repository".to_string()
            }
        };
        let mut lines: Vec<String> = Vec::new();
        if !files.is_empty() {
            lines.push(format!(
                "Files named: {}.",
                files
                    .iter()
                    .map(|f| format!(
                        "{}/{} ({}, {})",
                        f.repo,
                        f.path,
                        base(&f.repo),
                        this(&f.repo)
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !repos.is_empty() {
            lines.push(format!(
                "Repositories named: {}.",
                repos
                    .iter()
                    .map(|r| format!("{} at {} ({})", base(r), r, this(r)))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }

        // Where the work is: the other agents, their repository, area and the files they worked on.
        let mut work = Vec::new();
        let mut agents: Vec<&crate::store::Run> = runs
            .iter()
            .filter(|r| {
                r.parent_run_id.is_none()
                    && self.run_role(&r.id) == "agent"
                    && r.status != crate::handoff::HANDED_OFF
            })
            .collect();
        agents.sort_by_key(|r| std::cmp::Reverse(r.created_ms));
        for r in agents.into_iter().take(AGENTS) {
            let Ok(d) = self.digest(&r.id) else { continue };
            let mut paths: Vec<String> = d.area.iter().map(|a| format!("claimed {a}")).collect();
            paths.extend(d.changed.iter().map(|c| c.path.clone()));
            paths.truncate(PATHS_PER_AGENT);
            if paths.is_empty() {
                continue;
            }
            work.push(format!(
                "“{}” in {}: {}",
                d.title,
                d.repository,
                paths.join(", ")
            ));
        }
        if !work.is_empty() {
            lines.push(format!("Where the work is: {}.", work.join("; ")));
        }

        let said: Vec<String> = messages
            .iter()
            .filter(|(s, _)| s == "owner")
            .map(|(_, t)| t.trim())
            .filter(|t| !t.is_empty() && !prompt.contains(*t))
            .map(|t| {
                let mut s: String = t.chars().take(SAID_CHARS).collect();
                if t.chars().count() > SAID_CHARS {
                    s.push('…');
                }
                format!("“{}”", s.replace('\n', " "))
            })
            .collect();
        let said: Vec<String> = said[said.len().saturating_sub(SAID)..].to_vec();
        if !said.is_empty() {
            lines.push(format!("The owner said: {}", said.join(" · ")));
        }
        if lines.is_empty() {
            return String::new();
        }
        let text = format!("[What Overseer knows from its conversation with the owner (use what helps; ask Overseer for more):\n{}]", lines.join("\n"));
        let text = crate::redact::redact(&text);
        if text.len() <= CONTEXT_BYTES {
            return text;
        }
        let mut end = CONTEXT_BYTES - 2;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…]", &text[..end])
    }

    /// Shows what was added to a started agent's task in its chat (as a briefing) and keeps it
    /// with its briefings.
    pub(crate) fn record_start_context(&self, run_id: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        let Ok(run) = self.run(run_id) else { return };
        let _ = self.store.lock().unwrap().conn.execute(
            "INSERT INTO briefings(run_id, ts, text, how) VALUES(?1, ?2, ?3, 'context')",
            rusqlite::params![run_id, crate::daemon::now(), text],
        );
        let _ = self.emit(Some(&run.task_id), Some(run_id), "briefing", "overseer", "exact", serde_json::json!({"text": text, "how": "context", "line": "Overseer added what it knows"}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_words_find_paths_and_files() {
        let w = path_words("The endpoint is in /tmp/api/src/login.rs, and `web/page.tsx`; see login.tsx. Not a URL: https://x.y/z.js or 3.5 or e.g.");
        assert_eq!(
            w,
            vec!["/tmp/api/src/login.rs", "web/page.tsx", "login.tsx"]
        );
    }

    #[test]
    fn resolve_keeps_what_exists_with_its_repository() {
        let t = tempfile::tempdir().unwrap();
        let api = t.path().join("api");
        std::fs::create_dir_all(api.join(".git")).unwrap();
        std::fs::create_dir_all(api.join("src")).unwrap();
        std::fs::write(api.join("src/login.rs"), "").unwrap();
        let site = t.path().join("site");
        std::fs::create_dir_all(site.join(".git")).unwrap();
        let api_s = canon(&api);
        let site_s = canon(&site);
        let words = vec![
            format!("{api_s}/src/login.rs"),
            "api/src/login.rs".into(),
            "src/login.rs".into(),
            "nope/x.rs".into(),
        ];
        let found = resolve(&words, &site_s, &[api_s.clone()]);
        assert_eq!(
            found,
            vec![Named {
                repo: api_s,
                path: "src/login.rs".into()
            }]
        );
    }
}
