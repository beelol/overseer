//! Durable state in SQLite. The daemon is the only writer.

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 4;
/// Retained normalized events per run before older ones are pruned (with a marker).
pub const EVENTS_PER_RUN: i64 = 5000;

pub struct Store {
    pub conn: Connection,
}

#[derive(Serialize, Clone, Debug)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub repo_root: String,
    pub target_ref: Option<String>,
    pub workspace_id: String,
    pub start_snapshot: Option<String>,
    pub fork_commit: Option<String>,
    pub fork_provenance: Option<String>,
    pub created_ms: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Workspace {
    pub id: String,
    pub path: String,
    pub repo_root: String,
    pub common_dir: String,
    pub kind: String,
    pub branch: Option<String>,
    pub owner_run_id: Option<String>,
    pub initial_dirty: Value,
    pub created_ms: i64,
    pub removed_ms: Option<i64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub parent_run_id: Option<String>,
    pub harness: String,
    pub harness_version: Option<String>,
    pub profile_id: Option<String>,
    pub model: Option<String>,
    pub workspace_id: String,
    pub native_id: Option<String>,
    pub status: String,
    pub exit_reason: Option<String>,
    pub created_ms: i64,
    pub ended_ms: Option<i64>,
    pub title: String,
    pub relation_source: Option<String>,
    pub relation_confidence: Option<String>,
    pub capabilities: Value,
    pub process_generation: i64,
    pub attention: Option<Value>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Turn {
    pub id: String,
    pub run_id: String,
    pub n: i64,
    pub prompt: String,
    pub snapshot_id: Option<String>,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub status: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Snapshot {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub head: Option<String>,
    pub index_tree: String,
    pub worktree_tree: String,
    pub commit_sha: String,
    pub index_commit: Option<String>,
    pub created_ms: i64,
    pub dirty: Value,
}

#[derive(Serialize, Clone, Debug)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub harness: String,
    pub home: Option<String>,
    pub is_system: bool,
    pub created_ms: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Event {
    pub seq: i64,
    pub ts: i64,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub kind: String,
    pub source: String,
    pub confidence: String,
    pub payload: Value,
}

fn json_col(row: &Row, idx: &str) -> rusqlite::Result<Value> {
    let text: Option<String> = row.get(idx)?;
    Ok(text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null))
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
            CREATE TABLE IF NOT EXISTS workspaces(
              id TEXT PRIMARY KEY, path TEXT NOT NULL, repo_root TEXT NOT NULL, common_dir TEXT NOT NULL,
              kind TEXT NOT NULL, branch TEXT, owner_run_id TEXT, initial_dirty TEXT, created_ms INTEGER NOT NULL,
              removed_ms INTEGER);
            CREATE TABLE IF NOT EXISTS tasks(
              id TEXT PRIMARY KEY, title TEXT NOT NULL, prompt TEXT NOT NULL, repo_root TEXT NOT NULL, target_ref TEXT,
              workspace_id TEXT NOT NULL REFERENCES workspaces(id), start_snapshot TEXT, fork_commit TEXT,
              fork_provenance TEXT, created_ms INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS runs(
              id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id), parent_run_id TEXT REFERENCES runs(id),
              harness TEXT NOT NULL, harness_version TEXT, profile_id TEXT, model TEXT, workspace_id TEXT NOT NULL,
              native_id TEXT, status TEXT NOT NULL, exit_reason TEXT, created_ms INTEGER NOT NULL, ended_ms INTEGER,
              title TEXT NOT NULL, relation_source TEXT, relation_confidence TEXT, capabilities TEXT,
              process_generation INTEGER NOT NULL DEFAULT 0, run_dir TEXT, segment INTEGER NOT NULL DEFAULT 0,
              seg_offset INTEGER NOT NULL DEFAULT 0, attention TEXT, launch TEXT);
            CREATE UNIQUE INDEX IF NOT EXISTS runs_native ON runs(parent_run_id, native_id) WHERE parent_run_id IS NOT NULL;
            CREATE TABLE IF NOT EXISTS turns(
              id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id), n INTEGER NOT NULL, prompt TEXT NOT NULL,
              snapshot_id TEXT, started_ms INTEGER NOT NULL, ended_ms INTEGER, status TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS snapshots(
              id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, kind TEXT NOT NULL, head TEXT, index_tree TEXT NOT NULL,
              worktree_tree TEXT NOT NULL, commit_sha TEXT NOT NULL, index_commit TEXT, created_ms INTEGER NOT NULL, dirty TEXT);
            CREATE TABLE IF NOT EXISTS profiles(
              id TEXT PRIMARY KEY, name TEXT NOT NULL, harness TEXT NOT NULL, home TEXT, is_system INTEGER NOT NULL,
              created_ms INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS events(
              seq INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, task_id TEXT, run_id TEXT, kind TEXT NOT NULL,
              source TEXT NOT NULL, confidence TEXT NOT NULL, payload TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS events_run ON events(run_id, seq);
            "#,
        )?;
        let old_version: Option<String> = self.conn.query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        ).optional()?;
        if let Some(version) = old_version {
            let version: i64 = version.parse()?;
            if version > SCHEMA_VERSION {
                bail!("database schema version {version} is newer than this daemon supports");
            }
        }
        let has_pending: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('runs') WHERE name='pending_parent_native'")?.exists([])?;
        if !has_pending {
            self.conn.execute_batch("ALTER TABLE runs ADD COLUMN pending_parent_native TEXT;")?;
        }
        crate::swarm::schema::migrate(&self.conn)?;
        self.conn.execute("INSERT INTO meta(key, value) VALUES('schema_version', ?1)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![SCHEMA_VERSION.to_string()])?;
        Ok(())
    }

    // ---- workspaces
    pub fn insert_workspace(&self, w: &Workspace) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workspaces(id,path,repo_root,common_dir,kind,branch,owner_run_id,initial_dirty,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![w.id, w.path, w.repo_root, w.common_dir, w.kind, w.branch, w.owner_run_id, w.initial_dirty.to_string(), w.created_ms],
        )?;
        Ok(())
    }

    fn map_workspace(row: &Row) -> rusqlite::Result<Workspace> {
        Ok(Workspace {
            id: row.get("id")?,
            path: row.get("path")?,
            repo_root: row.get("repo_root")?,
            common_dir: row.get("common_dir")?,
            kind: row.get("kind")?,
            branch: row.get("branch")?,
            owner_run_id: row.get("owner_run_id")?,
            initial_dirty: json_col(row, "initial_dirty")?,
            created_ms: row.get("created_ms")?,
            removed_ms: row.get("removed_ms")?,
        })
    }

    pub fn workspace(&self, id: &str) -> Result<Option<Workspace>> {
        Ok(self.conn.query_row("SELECT * FROM workspaces WHERE id=?1", params![id], Self::map_workspace).optional()?)
    }

    pub fn workspaces(&self) -> Result<Vec<Workspace>> {
        let mut stmt = self.conn.prepare("SELECT * FROM workspaces ORDER BY created_ms")?;
        let rows = stmt.query_map([], Self::map_workspace)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn workspace_by_path(&self, path: &str) -> Result<Vec<Workspace>> {
        let mut stmt = self.conn.prepare("SELECT * FROM workspaces WHERE path=?1 AND removed_ms IS NULL")?;
        let rows = stmt.query_map(params![path], Self::map_workspace)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_workspace_owner(&self, id: &str, run: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET owner_run_id=?2 WHERE id=?1", params![id, run])?;
        Ok(())
    }

    pub fn mark_workspace_removed(&self, id: &str, ms: i64) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET removed_ms=?2 WHERE id=?1", params![id, ms])?;
        Ok(())
    }

    // ---- tasks
    pub fn insert_task(&self, t: &Task) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tasks(id,title,prompt,repo_root,target_ref,workspace_id,start_snapshot,fork_commit,fork_provenance,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![t.id, t.title, t.prompt, t.repo_root, t.target_ref, t.workspace_id, t.start_snapshot, t.fork_commit, t.fork_provenance, t.created_ms],
        )?;
        Ok(())
    }

    fn map_task(row: &Row) -> rusqlite::Result<Task> {
        Ok(Task {
            id: row.get("id")?,
            title: row.get("title")?,
            prompt: row.get("prompt")?,
            repo_root: row.get("repo_root")?,
            target_ref: row.get("target_ref")?,
            workspace_id: row.get("workspace_id")?,
            start_snapshot: row.get("start_snapshot")?,
            fork_commit: row.get("fork_commit")?,
            fork_provenance: row.get("fork_provenance")?,
            created_ms: row.get("created_ms")?,
        })
    }

    pub fn task(&self, id: &str) -> Result<Option<Task>> {
        Ok(self.conn.query_row("SELECT * FROM tasks WHERE id=?1", params![id], Self::map_task).optional()?)
    }

    pub fn tasks(&self) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare("SELECT * FROM tasks ORDER BY created_ms")?;
        let rows = stmt.query_map([], Self::map_task)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_task_start_snapshot(&self, id: &str, snap: &str) -> Result<()> {
        self.conn.execute("UPDATE tasks SET start_snapshot=?2 WHERE id=?1", params![id, snap])?;
        Ok(())
    }

    // ---- runs
    pub fn insert_run(&self, r: &Run) -> Result<()> {
        Self::insert_run_row(&self.conn, r)
    }

    fn insert_run_row(conn: &Connection, r: &Run) -> Result<()> {
        conn.execute(
            "INSERT INTO runs(id,task_id,parent_run_id,harness,harness_version,profile_id,model,workspace_id,native_id,status,exit_reason,created_ms,ended_ms,title,relation_source,relation_confidence,capabilities,process_generation)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            params![r.id, r.task_id, r.parent_run_id, r.harness, r.harness_version, r.profile_id, r.model, r.workspace_id, r.native_id,
                r.status, r.exit_reason, r.created_ms, r.ended_ms, r.title, r.relation_source, r.relation_confidence, r.capabilities.to_string(), r.process_generation],
        )?;
        Ok(())
    }

    pub fn insert_run_for_swarm(&self, r: &Run, attempt_id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        Self::insert_run_row(&tx, r)?;
        let linked = tx.execute(
            "UPDATE swarm_worker_launches SET overseer_run_id=?2 WHERE attempt_id=?1 AND overseer_run_id IS NULL",
            params![attempt_id, r.id],
        )?;
        if linked != 1 {
            anyhow::bail!("swarm launch intent is missing or already linked");
        }
        tx.commit()?;
        Ok(())
    }

    fn map_run(row: &Row) -> rusqlite::Result<Run> {
        Ok(Run {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            parent_run_id: row.get("parent_run_id")?,
            harness: row.get("harness")?,
            harness_version: row.get("harness_version")?,
            profile_id: row.get("profile_id")?,
            model: row.get("model")?,
            workspace_id: row.get("workspace_id")?,
            native_id: row.get("native_id")?,
            status: row.get("status")?,
            exit_reason: row.get("exit_reason")?,
            created_ms: row.get("created_ms")?,
            ended_ms: row.get("ended_ms")?,
            title: row.get("title")?,
            relation_source: row.get("relation_source")?,
            relation_confidence: row.get("relation_confidence")?,
            capabilities: json_col(row, "capabilities")?,
            process_generation: row.get("process_generation")?,
            attention: {
                let v = json_col(row, "attention")?;
                if v.is_null() { None } else { Some(v) }
            },
        })
    }

    pub fn run(&self, id: &str) -> Result<Option<Run>> {
        Ok(self.conn.query_row("SELECT * FROM runs WHERE id=?1", params![id], Self::map_run).optional()?)
    }

    pub fn runs(&self) -> Result<Vec<Run>> {
        let mut stmt = self.conn.prepare("SELECT * FROM runs ORDER BY created_ms, id")?;
        let rows = stmt.query_map([], Self::map_run)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn child_by_native(&self, parent: &str, native: &str) -> Result<Option<Run>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM runs WHERE parent_run_id=?1 AND native_id=?2", params![parent, native], Self::map_run)
            .optional()?)
    }

    pub fn children(&self, parent: &str) -> Result<Vec<Run>> {
        let mut stmt = self.conn.prepare("SELECT * FROM runs WHERE parent_run_id=?1 ORDER BY created_ms, id")?;
        let rows = stmt.query_map(params![parent], Self::map_run)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn update_run_status(&self, id: &str, status: &str, reason: Option<&str>, ended: Option<i64>) -> Result<()> {
        self.conn.execute(
            "UPDATE runs SET status=?2, exit_reason=COALESCE(?3, exit_reason), ended_ms=?4 WHERE id=?1",
            params![id, status, reason, ended],
        )?;
        Ok(())
    }

    pub fn set_run_native(&self, id: &str, native: &str) -> Result<()> {
        self.conn.execute("UPDATE runs SET native_id=?2 WHERE id=?1 AND native_id IS NULL", params![id, native])?;
        Ok(())
    }

    pub fn set_run_attention(&self, id: &str, attention: Option<&Value>) -> Result<()> {
        self.conn.execute("UPDATE runs SET attention=?2 WHERE id=?1", params![id, attention.map(|v| v.to_string())])?;
        Ok(())
    }

    pub fn set_run_process(&self, id: &str, run_dir: &str, generation: i64, launch: &Value) -> Result<()> {
        self.conn.execute(
            "UPDATE runs SET run_dir=?2, process_generation=?3, segment=0, seg_offset=0, launch=?4 WHERE id=?1",
            params![id, run_dir, generation, launch.to_string()],
        )?;
        Ok(())
    }

    pub fn run_process(&self, id: &str) -> Result<Option<(String, i64, i64)>> {
        Ok(self
            .conn
            .query_row("SELECT run_dir, segment, seg_offset FROM runs WHERE id=?1 AND run_dir IS NOT NULL", params![id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?)
    }

    pub fn set_run_cursor(&self, id: &str, segment: i64, offset: i64) -> Result<()> {
        self.conn.execute("UPDATE runs SET segment=?2, seg_offset=?3 WHERE id=?1", params![id, segment, offset])?;
        Ok(())
    }

    // ---- turns
    pub fn insert_turn(&self, t: &Turn) -> Result<()> {
        self.conn.execute(
            "INSERT INTO turns(id,run_id,n,prompt,snapshot_id,started_ms,ended_ms,status) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![t.id, t.run_id, t.n, t.prompt, t.snapshot_id, t.started_ms, t.ended_ms, t.status],
        )?;
        Ok(())
    }

    fn map_turn(row: &Row) -> rusqlite::Result<Turn> {
        Ok(Turn {
            id: row.get("id")?,
            run_id: row.get("run_id")?,
            n: row.get("n")?,
            prompt: row.get("prompt")?,
            snapshot_id: row.get("snapshot_id")?,
            started_ms: row.get("started_ms")?,
            ended_ms: row.get("ended_ms")?,
            status: row.get("status")?,
        })
    }

    pub fn turns(&self, run: &str) -> Result<Vec<Turn>> {
        let mut stmt = self.conn.prepare("SELECT * FROM turns WHERE run_id=?1 ORDER BY n")?;
        let rows = stmt.query_map(params![run], Self::map_turn)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn finish_open_turns(&self, run: &str, status: &str, ms: i64) -> Result<()> {
        self.conn.execute("UPDATE turns SET status=?2, ended_ms=?3 WHERE run_id=?1 AND ended_ms IS NULL", params![run, status, ms])?;
        Ok(())
    }

    // ---- snapshots
    pub fn insert_snapshot(&self, s: &Snapshot) -> Result<()> {
        self.conn.execute(
            "INSERT INTO snapshots(id,workspace_id,kind,head,index_tree,worktree_tree,commit_sha,index_commit,created_ms,dirty) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![s.id, s.workspace_id, s.kind, s.head, s.index_tree, s.worktree_tree, s.commit_sha, s.index_commit, s.created_ms, s.dirty.to_string()],
        )?;
        Ok(())
    }

    pub fn snapshot(&self, id: &str) -> Result<Option<Snapshot>> {
        Ok(self
            .conn
            .query_row("SELECT * FROM snapshots WHERE id=?1", params![id], |row| {
                Ok(Snapshot {
                    id: row.get("id")?,
                    workspace_id: row.get("workspace_id")?,
                    kind: row.get("kind")?,
                    head: row.get("head")?,
                    index_tree: row.get("index_tree")?,
                    worktree_tree: row.get("worktree_tree")?,
                    commit_sha: row.get("commit_sha")?,
                    index_commit: row.get("index_commit")?,
                    created_ms: row.get("created_ms")?,
                    dirty: json_col(row, "dirty")?,
                })
            })
            .optional()?)
    }

    // ---- profiles
    pub fn insert_profile(&self, p: &Profile) -> Result<()> {
        self.conn.execute(
            "INSERT INTO profiles(id,name,harness,home,is_system,created_ms) VALUES(?1,?2,?3,?4,?5,?6)",
            params![p.id, p.name, p.harness, p.home, p.is_system as i64, p.created_ms],
        )?;
        Ok(())
    }

    pub fn profiles(&self) -> Result<Vec<Profile>> {
        let mut stmt = self.conn.prepare("SELECT * FROM profiles ORDER BY created_ms, id")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Profile {
                    id: row.get("id")?,
                    name: row.get("name")?,
                    harness: row.get("harness")?,
                    home: row.get("home")?,
                    is_system: row.get::<_, i64>("is_system")? != 0,
                    created_ms: row.get("created_ms")?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn profile(&self, id: &str) -> Result<Option<Profile>> {
        Ok(self.profiles()?.into_iter().find(|p| p.id == id))
    }

    pub fn rename_profile(&self, id: &str, name: &str) -> Result<()> {
        self.conn.execute("UPDATE profiles SET name=?2 WHERE id=?1", params![id, name])?;
        Ok(())
    }

    // ---- events
    pub fn insert_event(&self, ts: i64, task: Option<&str>, run: Option<&str>, kind: &str, source: &str, confidence: &str, payload: &Value) -> Result<Event> {
        self.conn.execute(
            "INSERT INTO events(ts,task_id,run_id,kind,source,confidence,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![ts, task, run, kind, source, confidence, payload.to_string()],
        )?;
        Ok(Event {
            seq: self.conn.last_insert_rowid(),
            ts,
            task_id: task.map(str::to_string),
            run_id: run.map(str::to_string),
            kind: kind.to_string(),
            source: source.to_string(),
            confidence: confidence.to_string(),
            payload: payload.clone(),
        })
    }

    fn map_event(row: &Row) -> rusqlite::Result<Event> {
        Ok(Event {
            seq: row.get("seq")?,
            ts: row.get("ts")?,
            task_id: row.get("task_id")?,
            run_id: row.get("run_id")?,
            kind: row.get("kind")?,
            source: row.get("source")?,
            confidence: row.get("confidence")?,
            payload: json_col(row, "payload")?,
        })
    }

    pub fn events_after(&self, after: i64, run: Option<&str>, limit: i64) -> Result<Vec<Event>> {
        let mut out = Vec::new();
        if let Some(run) = run {
            let mut stmt = self.conn.prepare("SELECT * FROM events WHERE seq>?1 AND run_id=?2 ORDER BY seq LIMIT ?3")?;
            for e in stmt.query_map(params![after, run, limit], Self::map_event)? {
                out.push(e?);
            }
        } else {
            let mut stmt = self.conn.prepare("SELECT * FROM events WHERE seq>?1 ORDER BY seq LIMIT ?2")?;
            for e in stmt.query_map(params![after, limit], Self::map_event)? {
                out.push(e?);
            }
        }
        Ok(out)
    }

    pub fn max_seq(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))?)
    }

    pub fn oldest_retained(&self, run: &str) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT MIN(seq) FROM events WHERE run_id=?1", params![run], |r| r.get(0))?)
    }

    /// Prune a run's oldest events beyond the retention bound. Returns the highest pruned seq.
    pub fn prune_run_events(&self, run: &str, keep: i64) -> Result<Option<i64>> {
        let count: i64 = self.conn.query_row("SELECT COUNT(*) FROM events WHERE run_id=?1", params![run], |r| r.get(0))?;
        if count <= keep {
            return Ok(None);
        }
        let cutoff: i64 = self.conn.query_row(
            "SELECT seq FROM events WHERE run_id=?1 ORDER BY seq DESC LIMIT 1 OFFSET ?2",
            params![run, keep],
            |r| r.get(0),
        )?;
        self.conn.execute("DELETE FROM events WHERE run_id=?1 AND seq<=?2 AND kind<>'retention'", params![run, cutoff])?;
        Ok(Some(cutoff))
    }
}
