//! Overseer checks finished work (AC-238). When an agent finishes, the check-in that follows
//! reads its final message in full, its whole diff and its last test output (all bounded), so
//! Overseer can say whether it did what was asked and propose the next step: the merge, a pull
//! request, or a message with the fix. The daemon finds the test output with no model: the last
//! command the agent ran that looks like a test run, with its outcome.

use crate::daemon::Daemon;
use anyhow::Result;
use serde_json::{json, Value};

/// What one finished agent's work may take of a check-in turn, at most.
pub const WORK_BYTES: usize = 12 * 1024;
const TEST_OUTPUT_CHARS: usize = 1500;

/// A command that runs tests (or a check that stands for them).
pub fn is_test_command(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    ["test", "pytest", "jest", "vitest", "mocha", "rspec", "phpunit", "ctest", "make check", "cargo check", "tox"].iter().any(|w| c.contains(w))
}

/// Whether test output reports a failure, when the harness did not say.
fn output_fails(output: &str) -> bool {
    let o = output.to_ascii_lowercase();
    o.contains("test result: failed") || o.contains(" failing") || o.contains("failures:") || o.contains("tests failed") || o.contains("assertionerror")
}

fn tail(text: &str, chars: usize) -> String {
    let n = text.chars().count();
    if n <= chars {
        return text.to_string();
    }
    format!("…{}", text.chars().skip(n - chars).collect::<String>())
}

impl Daemon {
    /// The last test run of an agent: its command, whether it passed and the end of its output.
    pub(crate) fn last_test_run(&self, run_id: &str) -> Result<Option<Value>> {
        let events = self.store.lock().unwrap().events_after(0, Some(run_id), crate::store::EVENTS_PER_RUN)?;
        let mut commands: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut last: Option<Value> = None;
        for e in &events {
            let p = &e.payload;
            let id = p["id"].as_str().unwrap_or("").to_string();
            match e.kind.as_str() {
                "tool_result" => {
                    if let Some(c) = p["input"]["command"].as_str().or(p["input"]["cmd"].as_str()) {
                        commands.insert(id.clone(), c.to_string());
                    }
                    let done = matches!(p["status"].as_str(), Some("completed") | Some("failed")) || (!p["output"].is_null() && p["status"] != "started");
                    if !done {
                        continue;
                    }
                    let Some(command) = commands.get(&id).cloned() else { continue };
                    if !is_test_command(&command) {
                        continue;
                    }
                    let output = p["output"].as_str().map(str::to_string).unwrap_or_else(|| p["output"].to_string());
                    let ok = p["is_error"] != true && p["status"] != "failed" && !output_fails(&output);
                    last = Some(json!({"command": command, "ok": ok, "output": tail(&crate::redact::redact(&output), TEST_OUTPUT_CHARS)}));
                }
                // Codex reports a shell command as one tool line: "npm test [completed, exit 1]".
                "tool" if matches!(p["name"].as_str(), Some("shell") | Some("exec_command")) => {
                    let summary = p["summary"].as_str().unwrap_or("");
                    let command = summary.rsplit_once(" [").map(|(c, _)| c).unwrap_or(summary).to_string();
                    commands.insert(id, command.clone());
                    if is_test_command(&command) && summary.contains("exit ") {
                        let ok = summary.contains("exit 0]");
                        last = Some(json!({"command": command, "ok": ok, "output": ""}));
                    }
                }
                _ => {}
            }
        }
        Ok(last)
    }

    /// An agent's finished work for a check-in: its final message in full, its whole diff and its
    /// last test run, within `budget` bytes.
    pub(crate) fn finished_work(&self, run_id: &str, budget: usize) -> Result<Value> {
        let budget = budget.clamp(2048, WORK_BYTES);
        let final_message: String = {
            let store = self.store.lock().unwrap();
            store.conn.query_row("SELECT json_extract(payload, '$.text') FROM events WHERE run_id=?1 AND kind='output' AND json_extract(payload, '$.role')='assistant' ORDER BY seq DESC LIMIT 1", [run_id], |r| r.get::<_, Option<String>>(0)).ok().flatten().unwrap_or_default()
        };
        let tests = self.last_test_run(run_id)?;
        let diff = self.whole_diff_text(run_id).unwrap_or_else(|e| format!("(no diff: {e})"));
        let message_room = budget / 3;
        let final_message = super::bound(&crate::redact::redact(&final_message), message_room);
        let diff_room = budget.saturating_sub(final_message.len() + tests.as_ref().map(|t| t.to_string().len()).unwrap_or(0)).max(1024);
        Ok(json!({"final_message": final_message, "diff": super::bound(&crate::redact::redact(&diff), diff_room), "tests": tests}))
    }

    /// Every change in an agent's worktree against its task's base, as one diff.
    fn whole_diff_text(&self, run_id: &str) -> Result<String> {
        let run = self.run(run_id)?;
        let ws = self.workspace(&run.workspace_id)?;
        if ws.removed_ms.is_some() {
            anyhow::bail!("the worktree was removed");
        }
        let root = std::fs::canonicalize(&ws.path)?;
        let task = self.task(&run.task_id)?;
        let base = {
            let store = self.store.lock().unwrap();
            task.start_snapshot.as_deref().and_then(|id| store.snapshot(id).ok().flatten()).map(|s| s.commit_sha)
        }
        .or_else(|| task.fork_commit.clone())
        .or_else(|| crate::git::head(&root))
        .ok_or_else(|| anyhow::anyhow!("no base to diff against"))?;
        let trees = crate::git::capture_trees(&root, &crate::paths::data_dir().join("tmp"))?;
        let diff = crate::git::git(&root, &["diff", "--no-color", &base, &trees.worktree_tree])?;
        Ok(if diff.is_empty() { "No changes against the task's base.".to_string() } else { diff })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_and_failures_are_recognised() {
        assert!(is_test_command("npm test") && is_test_command("cargo test -p x") && is_test_command("python -m pytest -q"));
        assert!(!is_test_command("ls -la") && !is_test_command("git status"));
        assert!(output_fails("2 passing\n1 failing") && output_fails("test result: FAILED. 1 passed; 1 failed"));
        assert!(!output_fails("3 passing") && !output_fails("test result: ok. 3 passed"));
    }
}
