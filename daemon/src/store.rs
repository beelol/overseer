//! Durable state in SQLite. The daemon is the only writer.

use anyhow::{anyhow, Result};
use crate::auto_telemetry::{Measurement, StoredMeasurement};
use crate::auto_quota::{QuotaSnapshot, StoredQuotaObservation};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 11;
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
    pub effort: Option<String>,
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
pub struct AutoDailyAggregate {
    pub day_ms: i64,
    pub harness: String,
    pub profile_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub samples: i64,
    pub input_observations: i64,
    pub input_tokens: Option<i64>,
    pub output_observations: i64,
    pub output_tokens: Option<i64>,
    pub cached_input_observations: i64,
    pub cached_input_tokens: Option<i64>,
    pub reasoning_output_observations: i64,
    pub reasoning_output_tokens: Option<i64>,
    pub cost_observations: i64,
    pub cost_usd: Option<f64>,
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
              harness TEXT NOT NULL, harness_version TEXT, profile_id TEXT, model TEXT, effort TEXT, workspace_id TEXT NOT NULL,
              native_id TEXT, status TEXT NOT NULL, exit_reason TEXT, created_ms INTEGER NOT NULL, ended_ms INTEGER,
              title TEXT NOT NULL, relation_source TEXT, relation_confidence TEXT, capabilities TEXT,
              process_generation INTEGER NOT NULL DEFAULT 0, run_dir TEXT, segment INTEGER NOT NULL DEFAULT 0,
              seg_offset INTEGER NOT NULL DEFAULT 0, attention TEXT, launch TEXT);
            CREATE UNIQUE INDEX IF NOT EXISTS runs_native ON runs(parent_run_id, native_id) WHERE parent_run_id IS NOT NULL;
            CREATE INDEX IF NOT EXISTS runs_recent_completion ON runs(status, ended_ms);
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
            CREATE INDEX IF NOT EXISTS events_health_recent ON events(kind, ts);
            CREATE TABLE IF NOT EXISTS auto_measurements(
              event_seq INTEGER PRIMARY KEY, observed_ms INTEGER NOT NULL, task_id TEXT NOT NULL,
              run_id TEXT NOT NULL, harness TEXT NOT NULL, profile_id TEXT, model TEXT, effort TEXT,
              input_tokens INTEGER, output_tokens INTEGER, cached_input_tokens INTEGER,
              reasoning_output_tokens INTEGER, cost_usd REAL);
            CREATE INDEX IF NOT EXISTS auto_measurements_observed ON auto_measurements(observed_ms);
            CREATE TABLE IF NOT EXISTS auto_daily_aggregates(
              day_ms INTEGER NOT NULL, harness TEXT NOT NULL, profile_id TEXT NOT NULL,
              model TEXT NOT NULL, effort TEXT NOT NULL, last_observed_ms INTEGER NOT NULL, samples INTEGER NOT NULL,
              input_observations INTEGER NOT NULL, input_tokens INTEGER NOT NULL,
              output_observations INTEGER NOT NULL, output_tokens INTEGER NOT NULL,
              cached_input_observations INTEGER NOT NULL, cached_input_tokens INTEGER NOT NULL,
              reasoning_output_observations INTEGER NOT NULL, reasoning_output_tokens INTEGER NOT NULL,
              cost_observations INTEGER NOT NULL, cost_usd REAL NOT NULL,
              PRIMARY KEY(day_ms,harness,profile_id,model,effort));
            CREATE INDEX IF NOT EXISTS auto_daily_last_observed ON auto_daily_aggregates(last_observed_ms);
            CREATE TABLE IF NOT EXISTS auto_quota_observations(
              event_seq INTEGER PRIMARY KEY, pool_id TEXT NOT NULL, source TEXT NOT NULL,
              observed_ms INTEGER NOT NULL, snapshot TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS auto_quota_observed ON auto_quota_observations(observed_ms);
            CREATE TABLE IF NOT EXISTS auto_account_identity(
              profile_id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, generation INTEGER NOT NULL,
              observed_ms INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS auto_model_catalogs(
              profile_id TEXT PRIMARY KEY, observed_ms INTEGER NOT NULL, catalog TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS auto_thread_usage_observations(
              id INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL,
              profile_id TEXT NOT NULL, read_account_generation INTEGER NOT NULL,
              attribution TEXT NOT NULL, observed_ms INTEGER NOT NULL,
              source TEXT NOT NULL, estimate TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS auto_thread_usage_observed ON auto_thread_usage_observations(observed_ms);
            CREATE TABLE IF NOT EXISTS auto_run_account_evidence(
              run_id TEXT PRIMARY KEY REFERENCES runs(id), profile_id TEXT NOT NULL,
              first_generation INTEGER NOT NULL, last_generation INTEGER NOT NULL,
              observed_turns INTEGER NOT NULL, consistent INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS managed_work_units(
              work_unit_id TEXT PRIMARY KEY, parent_run_id TEXT NOT NULL REFERENCES runs(id),
              child_run_id TEXT NOT NULL UNIQUE REFERENCES runs(id),
              request_hash TEXT NOT NULL, created_ms INTEGER NOT NULL);
            "#,
        )?;
        let has_pending: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('runs') WHERE name='pending_parent_native'")?.exists([])?;
        if !has_pending {
            self.conn.execute_batch("ALTER TABLE runs ADD COLUMN pending_parent_native TEXT;")?;
        }
        let has_effort: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('runs') WHERE name='effort'")?.exists([])?;
        if !has_effort {
            self.conn.execute_batch("ALTER TABLE runs ADD COLUMN effort TEXT;")?;
        }
        let has_measurement_effort: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('auto_measurements') WHERE name='effort'")?.exists([])?;
        if !has_measurement_effort {
            self.conn.execute_batch("ALTER TABLE auto_measurements ADD COLUMN effort TEXT;")?;
        }
        let has_aggregate_effort: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('auto_daily_aggregates') WHERE name='effort'")?.exists([])?;
        if !has_aggregate_effort {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch("CREATE TABLE auto_daily_aggregates_v2(
                day_ms INTEGER NOT NULL, harness TEXT NOT NULL, profile_id TEXT NOT NULL,
                model TEXT NOT NULL, effort TEXT NOT NULL, last_observed_ms INTEGER NOT NULL,
                samples INTEGER NOT NULL, input_observations INTEGER NOT NULL, input_tokens INTEGER NOT NULL,
                output_observations INTEGER NOT NULL, output_tokens INTEGER NOT NULL,
                cached_input_observations INTEGER NOT NULL, cached_input_tokens INTEGER NOT NULL,
                reasoning_output_observations INTEGER NOT NULL, reasoning_output_tokens INTEGER NOT NULL,
                cost_observations INTEGER NOT NULL, cost_usd REAL NOT NULL,
                PRIMARY KEY(day_ms,harness,profile_id,model,effort));
                INSERT INTO auto_daily_aggregates_v2 SELECT day_ms,harness,profile_id,model,'',
                    last_observed_ms,samples,input_observations,input_tokens,output_observations,
                    output_tokens,cached_input_observations,cached_input_tokens,
                    reasoning_output_observations,reasoning_output_tokens,cost_observations,cost_usd
                    FROM auto_daily_aggregates;
                DROP TABLE auto_daily_aggregates;
                ALTER TABLE auto_daily_aggregates_v2 RENAME TO auto_daily_aggregates;
                CREATE INDEX auto_daily_last_observed ON auto_daily_aggregates(last_observed_ms);")?;
            tx.commit()?;
        }
        self.conn.execute("INSERT INTO meta(key, value) VALUES('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![SCHEMA_VERSION.to_string()])?;
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
        self.conn.execute(
            "INSERT INTO runs(id,task_id,parent_run_id,harness,harness_version,profile_id,model,effort,workspace_id,native_id,status,exit_reason,created_ms,ended_ms,title,relation_source,relation_confidence,capabilities,process_generation)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            params![r.id, r.task_id, r.parent_run_id, r.harness, r.harness_version, r.profile_id, r.model, r.effort, r.workspace_id, r.native_id,
                r.status, r.exit_reason, r.created_ms, r.ended_ms, r.title, r.relation_source, r.relation_confidence, r.capabilities.to_string(), r.process_generation],
        )?;
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
            effort: row.get("effort")?,
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

    pub fn managed_work_unit(&self, id: &str) -> Result<Option<(String, String, String)>> {
        Ok(self.conn.query_row(
            "SELECT parent_run_id,child_run_id,request_hash FROM managed_work_units WHERE work_unit_id=?1",
            params![id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?)
    }

    pub fn insert_managed_work_unit(&self, id: &str, parent: &str, child: &str, request_hash: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO managed_work_units(work_unit_id,parent_run_id,child_run_id,request_hash,created_ms) VALUES(?1,?2,?3,?4,?5)",
            params![id, parent, child, request_hash, crate::daemon::now()],
        )?;
        Ok(())
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

    pub fn delete_profile(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM profiles WHERE id=?1 AND is_system=0", params![id])?;
        Ok(())
    }

    pub fn rename_profile(&self, id: &str, name: &str) -> Result<()> {
        self.conn.execute("UPDATE profiles SET name=?2 WHERE id=?1", params![id, name])?;
        Ok(())
    }

    /// A changed login cannot inherit estimates or quota from the prior account.
    /// `fingerprint` is a one-way digest of the provider's account ID, never the ID itself.
    pub fn record_auto_account_identity(&self, profile_id: &str, fingerprint: &str) -> Result<bool> {
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(anyhow!("invalid account fingerprint"));
        }
        // A native app-server reply can arrive while execution events are in
        // their own transaction. A savepoint keeps account invalidation atomic
        // in both that path and standalone metadata refreshes.
        self.conn.execute_batch("SAVEPOINT auto_identity")?;
        let result = (|| -> Result<bool> {
            let previous: Option<(String, i64)> = self.conn.query_row(
                "SELECT fingerprint,generation FROM auto_account_identity WHERE profile_id=?1",
                params![profile_id], |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?;
            let changed = previous.as_ref().is_some_and(|(old, _)| old != fingerprint);
            if changed {
                self.conn.execute("DELETE FROM auto_measurements WHERE profile_id=?1", params![profile_id])?;
                self.conn.execute("DELETE FROM auto_daily_aggregates WHERE profile_id=?1", params![profile_id])?;
                self.conn.execute("DELETE FROM auto_thread_usage_observations WHERE profile_id=?1", params![profile_id])?;
                self.conn.execute("DELETE FROM auto_run_account_evidence WHERE profile_id=?1", params![profile_id])?;
                self.conn.execute("DELETE FROM auto_quota_observations WHERE pool_id=?1", params![profile_id])?;
                self.conn.execute("DELETE FROM auto_model_catalogs WHERE profile_id=?1", params![profile_id])?;
            }
            self.conn.execute(
                "INSERT INTO auto_account_identity(profile_id,fingerprint,generation,observed_ms) VALUES(?1,?2,?3,?4) ON CONFLICT(profile_id) DO UPDATE SET fingerprint=excluded.fingerprint,generation=excluded.generation,observed_ms=excluded.observed_ms",
                params![profile_id, fingerprint, previous.map(|(_, generation)| generation + i64::from(changed)).unwrap_or(1), crate::daemon::now()],
            )?;
            Ok(changed)
        })();
        match result {
            Ok(changed) => { self.conn.execute_batch("RELEASE auto_identity")?; Ok(changed) }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK TO auto_identity; RELEASE auto_identity");
                Err(error)
            }
        }
    }

    /// A failed account-scoped metadata read cannot leave prior capacity or
    /// catalog observations available for a new Auto admission.
    pub fn invalidate_auto_profile_evidence(&self, profile_id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM auto_model_catalogs WHERE profile_id=?1", params![profile_id])?;
        tx.execute("DELETE FROM auto_quota_observations WHERE pool_id=?1", params![profile_id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn auto_account_generation(&self, profile_id: &str) -> Result<Option<i64>> {
        Ok(self.conn.query_row(
            "SELECT generation FROM auto_account_identity WHERE profile_id=?1",
            params![profile_id], |row| row.get(0),
        ).optional()?)
    }

    /// A same-process Codex app-server metadata reply observed before a turn.
    /// Every turn of the thread must be stamped before its cumulative usage can
    /// be tied to one account generation.
    pub fn record_auto_run_account(&self, run_id: &str, profile_id: &str, generation: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO auto_run_account_evidence(run_id,profile_id,first_generation,last_generation,observed_turns,consistent) VALUES(?1,?2,?3,?3,1,1) \
             ON CONFLICT(run_id) DO UPDATE SET last_generation=excluded.last_generation,observed_turns=observed_turns+1, \
             consistent=CASE WHEN profile_id=excluded.profile_id AND first_generation=excluded.first_generation AND consistent=1 THEN 1 ELSE 0 END",
            params![run_id, profile_id, generation],
        )?;
        Ok(())
    }

    pub fn auto_run_account_matches(&self, run_id: &str, profile_id: &str, generation: i64) -> Result<bool> {
        let evidence: Option<(String, i64, i64, i64, bool)> = self.conn.query_row(
            "SELECT profile_id,first_generation,last_generation,observed_turns,consistent FROM auto_run_account_evidence WHERE run_id=?1",
            params![run_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get::<_, i64>(4)? != 0)),
        ).optional()?;
        let Some((profile, first, last, observed_turns, consistent)) = evidence else { return Ok(false); };
        let actual_turns: i64 = self.conn.query_row("SELECT COUNT(*) FROM turns WHERE run_id=?1", params![run_id], |row| row.get(0))?;
        Ok(consistent && profile == profile_id && first == generation && last == generation && observed_turns == actual_turns)
    }

    pub fn insert_auto_thread_usage(
        &self, run_id: &str, profile_id: &str, generation: i64,
        source: &str, estimate: &crate::auto_consumption::ThreadUsageEstimate,
    ) -> Result<i64> {
        let attribution = if self.auto_run_account_matches(run_id, profile_id, generation)? {
            "same_account_generation"
        } else {
            "unverified_run_account"
        };
        self.conn.execute(
            "INSERT INTO auto_thread_usage_observations(run_id,profile_id,read_account_generation,attribution,observed_ms,source,estimate) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![run_id, profile_id, generation, attribution, estimate.observed_ms, source, serde_json::to_string(estimate)?],
        )?;
        let id = self.conn.last_insert_rowid();
        const THIRTY_DAYS_MS: i64 = 30 * 86_400_000;
        self.conn.execute("DELETE FROM auto_thread_usage_observations WHERE observed_ms < ?1", params![crate::daemon::now().saturating_sub(THIRTY_DAYS_MS)])?;
        self.conn.execute("DELETE FROM auto_thread_usage_observations WHERE id NOT IN (SELECT id FROM auto_thread_usage_observations ORDER BY id DESC LIMIT 5000)", [])?;
        Ok(id)
    }

    pub fn auto_thread_usage_observations(&self, limit: i64) -> Result<Vec<crate::auto_consumption::StoredThreadUsageObservation>> {
        use crate::auto_consumption::{StoredThreadUsageObservation, ThreadUsageEstimate};
        let mut stmt = self.conn.prepare("SELECT id,run_id,profile_id,read_account_generation,attribution,source,estimate FROM auto_thread_usage_observations ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit.clamp(1, 5000)], |row| {
            let encoded: String = row.get(6)?;
            let estimate: ThreadUsageEstimate = serde_json::from_str(&encoded).map_err(|error| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error)))?;
            Ok(StoredThreadUsageObservation { id: row.get(0)?, run_id: row.get(1)?, profile_id: row.get(2)?, read_account_generation: row.get(3)?, attribution: row.get(4)?, subscription_window_relation: "unverified".into(), source: row.get(5)?, estimate })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- allowlisted, account-scoped model discovery
    pub fn put_auto_model_catalog(&self, profile_id: &str, catalog: &crate::auto_route::ModelCatalog) -> Result<()> {
        self.conn.execute(
            "INSERT INTO auto_model_catalogs(profile_id,observed_ms,catalog) VALUES(?1,?2,?3) ON CONFLICT(profile_id) DO UPDATE SET observed_ms=excluded.observed_ms,catalog=excluded.catalog",
            params![profile_id, catalog.observed_ms, serde_json::to_string(catalog)?],
        )?;
        Ok(())
    }

    pub fn auto_model_catalog(&self, profile_id: &str) -> Result<Option<crate::auto_route::ModelCatalog>> {
        let json: Option<String> = self.conn.query_row(
            "SELECT catalog FROM auto_model_catalogs WHERE profile_id=?1", params![profile_id], |row| row.get(0),
        ).optional()?;
        json.map(|value| serde_json::from_str(&value).map_err(Into::into)).transpose()
    }

    // ---- normalized, account-scoped allowance observations
    pub fn insert_auto_quota(&self, event_seq: i64, pool_id: &str, source: &str, snapshot: &QuotaSnapshot) -> Result<bool> {
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO auto_quota_observations(event_seq,pool_id,source,observed_ms,snapshot) VALUES(?1,?2,?3,?4,?5)",
            params![event_seq, pool_id, source, snapshot.observed_ms, serde_json::to_string(snapshot)?],
        )?;
        if inserted == 1 && event_seq % 100 == 0 {
            self.prune_auto_quotas(crate::daemon::now(), 5000)?;
        }
        Ok(inserted == 1)
    }

    /// Keep at most 30 days and 5,000 normalized quota observations.
    pub fn prune_auto_quotas(&self, now_ms: i64, cap: i64) -> Result<usize> {
        const THIRTY_DAYS_MS: i64 = 30 * 86_400_000;
        let expired = self.conn.execute(
            "DELETE FROM auto_quota_observations WHERE observed_ms < ?1",
            params![now_ms.saturating_sub(THIRTY_DAYS_MS)],
        )?;
        let over_cap = self.conn.execute(
            "DELETE FROM auto_quota_observations WHERE event_seq NOT IN (SELECT event_seq FROM auto_quota_observations ORDER BY event_seq DESC LIMIT ?1)",
            params![cap.clamp(1, 5000)],
        )?;
        Ok(expired + over_cap)
    }

    pub fn latest_auto_quota(&self, pool_id: &str) -> Result<Option<StoredQuotaObservation>> {
        let mut rows = self.conn.prepare(
            "SELECT event_seq,pool_id,source,snapshot FROM auto_quota_observations WHERE pool_id=?1 ORDER BY event_seq DESC LIMIT 1"
        )?;
        let found = rows.query_row(params![pool_id], |row| {
            let snapshot_text: String = row.get(3)?;
            let snapshot: QuotaSnapshot = serde_json::from_str(&snapshot_text).map_err(|error| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error)))?;
            Ok(StoredQuotaObservation { event_seq: row.get(0)?, pool_id: row.get(1)?, source: row.get(2)?, snapshot })
        }).optional()?;
        Ok(found)
    }

    pub fn auto_quotas(&self, limit: i64) -> Result<Vec<StoredQuotaObservation>> {
        let mut stmt = self.conn.prepare("SELECT event_seq,pool_id,source,snapshot FROM auto_quota_observations ORDER BY event_seq DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit.clamp(1, 5000)], |row| {
            let snapshot_text: String = row.get(3)?;
            let snapshot: QuotaSnapshot = serde_json::from_str(&snapshot_text).map_err(|error| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error)))?;
            Ok(StoredQuotaObservation { event_seq: row.get(0)?, pool_id: row.get(1)?, source: row.get(2)?, snapshot })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- local Auto learning. This table never stores raw harness output.
    pub fn insert_auto_measurement(&self, event_seq: i64, m: &Measurement) -> Result<bool> {
        const DAY_MS: i64 = 86_400_000;
        if !crate::auto_telemetry::valid_for_store(m) {
            return Err(anyhow!("invalid Auto measurement"));
        }
        // The daemon already batches normalized events in an outer transaction.
        // A savepoint keeps detail and aggregate writes atomic in either context.
        self.conn.execute_batch("SAVEPOINT auto_measurement_write")?;
        let result = (|| -> Result<(usize, i64)> {
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO auto_measurements(event_seq,observed_ms,task_id,run_id,harness,profile_id,model,effort,input_tokens,output_tokens,cached_input_tokens,reasoning_output_tokens,cost_usd) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![event_seq, m.observed_ms, m.task_id, m.run_id, m.harness, m.profile_id, m.model, m.effort,
                m.input_tokens, m.output_tokens, m.cached_input_tokens, m.reasoning_output_tokens, m.cost_usd],
        )?;
        if inserted == 1 {
            let day_ms = m.observed_ms.div_euclid(DAY_MS) * DAY_MS;
            self.conn.execute(
                "INSERT INTO auto_daily_aggregates(day_ms,harness,profile_id,model,effort,last_observed_ms,samples,input_observations,input_tokens,output_observations,output_tokens,cached_input_observations,cached_input_tokens,reasoning_output_observations,reasoning_output_tokens,cost_observations,cost_usd) VALUES(?1,?2,?3,?4,?5,?6,1,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) ON CONFLICT(day_ms,harness,profile_id,model,effort) DO UPDATE SET last_observed_ms=MAX(last_observed_ms,excluded.last_observed_ms),samples=samples+1,input_observations=input_observations+excluded.input_observations,input_tokens=input_tokens+excluded.input_tokens,output_observations=output_observations+excluded.output_observations,output_tokens=output_tokens+excluded.output_tokens,cached_input_observations=cached_input_observations+excluded.cached_input_observations,cached_input_tokens=cached_input_tokens+excluded.cached_input_tokens,reasoning_output_observations=reasoning_output_observations+excluded.reasoning_output_observations,reasoning_output_tokens=reasoning_output_tokens+excluded.reasoning_output_tokens,cost_observations=cost_observations+excluded.cost_observations,cost_usd=cost_usd+excluded.cost_usd",
                params![day_ms, m.harness, m.profile_id.as_deref().unwrap_or(""), m.model.as_deref().unwrap_or(""), m.effort.as_deref().unwrap_or(""), m.observed_ms,
                    i64::from(m.input_tokens.is_some()), m.input_tokens.unwrap_or(0),
                    i64::from(m.output_tokens.is_some()), m.output_tokens.unwrap_or(0),
                    i64::from(m.cached_input_tokens.is_some()), m.cached_input_tokens.unwrap_or(0),
                    i64::from(m.reasoning_output_tokens.is_some()), m.reasoning_output_tokens.unwrap_or(0),
                    i64::from(m.cost_usd.is_some()), m.cost_usd.unwrap_or(0.0)],
            )?;
        }
        let count = if inserted == 1 {
            self.conn.execute("INSERT INTO meta(key,value) VALUES('auto_learning_samples_inserted','1') ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1", [])?;
            self.conn.query_row("SELECT CAST(value AS INTEGER) FROM meta WHERE key='auto_learning_samples_inserted'", [], |row| row.get(0))?
        } else { 0 };
        Ok((inserted, count))
        })();
        let (inserted, count) = match result {
            Ok(value) => {
                self.conn.execute_batch("RELEASE auto_measurement_write")?;
                value
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK TO auto_measurement_write; RELEASE auto_measurement_write");
                return Err(error);
            }
        };
        if inserted == 1 && count % 100 == 0 {
            self.prune_auto_measurements(crate::daemon::now(), 50_000)?;
            self.prune_auto_daily_aggregates(crate::daemon::now(), 10_000)?;
        }
        Ok(inserted == 1)
    }

    pub fn auto_measurements(&self, limit: i64) -> Result<Vec<StoredMeasurement>> {
        let mut stmt = self.conn.prepare("SELECT * FROM auto_measurements ORDER BY event_seq DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit.clamp(1, 50_000)], |row| Ok(StoredMeasurement {
            event_seq: row.get("event_seq")?,
            measurement: Measurement {
                observed_ms: row.get("observed_ms")?, task_id: row.get("task_id")?,
                run_id: row.get("run_id")?, harness: row.get("harness")?,
                profile_id: row.get("profile_id")?, model: row.get("model")?, effort: row.get("effort")?,
                input_tokens: row.get("input_tokens")?, output_tokens: row.get("output_tokens")?,
                cached_input_tokens: row.get("cached_input_tokens")?,
                reasoning_output_tokens: row.get("reasoning_output_tokens")?, cost_usd: row.get("cost_usd")?,
            },
        }))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Keep detailed observations for 30 days and at most 50,000 rows. A caller
    /// may pass a lower cap for bounded fixture verification.
    pub fn prune_auto_measurements(&self, now_ms: i64, cap: i64) -> Result<usize> {
        const THIRTY_DAYS_MS: i64 = 30 * 86_400_000;
        let expired = self.conn.execute(
            "DELETE FROM auto_measurements WHERE observed_ms < ?1",
            params![now_ms.saturating_sub(THIRTY_DAYS_MS)],
        )?;
        let over_cap = self.conn.execute(
            "DELETE FROM auto_measurements WHERE event_seq NOT IN (SELECT event_seq FROM auto_measurements ORDER BY event_seq DESC LIMIT ?1)",
            params![cap.clamp(1, 50_000)],
        )?;
        Ok(expired + over_cap)
    }

    pub fn auto_daily_aggregates(&self, limit: i64) -> Result<Vec<AutoDailyAggregate>> {
        let mut stmt = self.conn.prepare("SELECT * FROM auto_daily_aggregates ORDER BY day_ms DESC,harness,profile_id,model,effort LIMIT ?1")?;
        let rows = stmt.query_map(params![limit.clamp(1, 10_000)], |row| {
            let profile: String = row.get("profile_id")?;
            let model: String = row.get("model")?;
            let effort: String = row.get("effort")?;
            let input_observations: i64 = row.get("input_observations")?;
            let output_observations: i64 = row.get("output_observations")?;
            let cached_input_observations: i64 = row.get("cached_input_observations")?;
            let reasoning_output_observations: i64 = row.get("reasoning_output_observations")?;
            let cost_observations: i64 = row.get("cost_observations")?;
            Ok(AutoDailyAggregate {
                day_ms: row.get("day_ms")?, harness: row.get("harness")?,
                profile_id: (!profile.is_empty()).then_some(profile), model: (!model.is_empty()).then_some(model), effort: (!effort.is_empty()).then_some(effort),
                samples: row.get("samples")?,
                input_observations, input_tokens: (input_observations > 0).then(|| row.get("input_tokens")).transpose()?,
                output_observations, output_tokens: (output_observations > 0).then(|| row.get("output_tokens")).transpose()?,
                cached_input_observations, cached_input_tokens: (cached_input_observations > 0).then(|| row.get("cached_input_tokens")).transpose()?,
                reasoning_output_observations, reasoning_output_tokens: (reasoning_output_observations > 0).then(|| row.get("reasoning_output_tokens")).transpose()?,
                cost_observations, cost_usd: (cost_observations > 0).then(|| row.get("cost_usd")).transpose()?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Keep compact summaries for 90 days and at most 10,000 scoped rows.
    pub fn prune_auto_daily_aggregates(&self, now_ms: i64, cap: i64) -> Result<usize> {
        const NINETY_DAYS_MS: i64 = 90 * 86_400_000;
        let expired = self.conn.execute(
            "DELETE FROM auto_daily_aggregates WHERE last_observed_ms < ?1",
            params![now_ms.saturating_sub(NINETY_DAYS_MS)],
        )?;
        let over_cap = self.conn.execute(
            "DELETE FROM auto_daily_aggregates WHERE rowid NOT IN (SELECT rowid FROM auto_daily_aggregates ORDER BY last_observed_ms DESC LIMIT ?1)",
            params![cap.clamp(1, 10_000)],
        )?;
        Ok(expired + over_cap)
    }

    pub fn clear_auto_learning(&self) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let deleted = tx.execute("DELETE FROM auto_measurements", [])?;
        tx.execute("DELETE FROM auto_daily_aggregates", [])?;
        tx.execute("DELETE FROM auto_thread_usage_observations", [])?;
        tx.execute("DELETE FROM auto_run_account_evidence", [])?;
        tx.execute("DELETE FROM meta WHERE key='auto_learning_samples_inserted'", [])?;
        tx.commit()?;
        Ok(deleted)
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

#[cfg(test)]
mod auto_effort_migration_tests {
    use super::*;

    #[test]
    fn existing_run_table_gains_effort_column_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE runs(
            id TEXT PRIMARY KEY, task_id TEXT NOT NULL, parent_run_id TEXT,
            harness TEXT NOT NULL, harness_version TEXT, profile_id TEXT, model TEXT,
            workspace_id TEXT NOT NULL, native_id TEXT, status TEXT NOT NULL,
            exit_reason TEXT, created_ms INTEGER NOT NULL, ended_ms INTEGER,
            title TEXT NOT NULL, relation_source TEXT, relation_confidence TEXT,
            capabilities TEXT, process_generation INTEGER NOT NULL DEFAULT 0,
            run_dir TEXT, segment INTEGER NOT NULL DEFAULT 0,
            seg_offset INTEGER NOT NULL DEFAULT 0, attention TEXT, launch TEXT,
            pending_parent_native TEXT);").unwrap();
        let store = Store { conn };
        store.migrate().unwrap();
        store.migrate().unwrap();
        let has_effort = store.conn.prepare("SELECT 1 FROM pragma_table_info('runs') WHERE name='effort'")
            .unwrap().exists([]).unwrap();
        assert!(has_effort);
    }

    #[test]
    fn prior_daily_summary_keeps_its_data_with_unknown_effort() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE auto_daily_aggregates(
            day_ms INTEGER NOT NULL, harness TEXT NOT NULL, profile_id TEXT NOT NULL,
            model TEXT NOT NULL, last_observed_ms INTEGER NOT NULL, samples INTEGER NOT NULL,
            input_observations INTEGER NOT NULL, input_tokens INTEGER NOT NULL,
            output_observations INTEGER NOT NULL, output_tokens INTEGER NOT NULL,
            cached_input_observations INTEGER NOT NULL, cached_input_tokens INTEGER NOT NULL,
            reasoning_output_observations INTEGER NOT NULL, reasoning_output_tokens INTEGER NOT NULL,
            cost_observations INTEGER NOT NULL, cost_usd REAL NOT NULL,
            PRIMARY KEY(day_ms,harness,profile_id,model));
            INSERT INTO auto_daily_aggregates VALUES(0,'codex','system-codex','gpt-6-sol',1000,1,1,42,0,0,0,0,0,0,0,0);").unwrap();
        let store = Store { conn };
        store.migrate().unwrap();
        store.migrate().unwrap();
        let rows = store.auto_daily_aggregates(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].samples, 1);
        assert_eq!(rows[0].input_tokens, Some(42));
        assert_eq!(rows[0].effort, None);
    }
}

#[cfg(test)]
mod auto_measurement_tests {
    use super::*;
    use crate::auto_telemetry::{from_usage, from_usage_with_effort};
    use serde_json::json;

    #[test]
    fn local_measurements_deduplicate_without_copying_event_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        let event = store.insert_event(1000, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &json!({
            "input_tokens": 42, "prompt": "secret-prompt-sentinel"
        })).unwrap();
        let measurement = from_usage(1000, "t-1", "r-1", "codex", Some("system-codex"), Some("model-1"), &event.payload).unwrap();
        assert!(store.insert_auto_measurement(event.seq, &measurement).unwrap());
        assert!(!store.insert_auto_measurement(event.seq, &measurement).unwrap());
        let rows = store.auto_measurements(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_seq, event.seq);
        assert_eq!(rows[0].measurement.input_tokens, Some(42));
        assert!(!serde_json::to_string(&rows).unwrap().contains("secret-prompt-sentinel"));
    }

    #[test]
    fn daily_summaries_do_not_mix_model_efforts() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        for effort in ["medium", "high"] {
            let event = store.insert_event(1000, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &json!({"input_tokens":42})).unwrap();
            let measurement = from_usage_with_effort(1000, "t-1", "r-1", "codex", Some("system-codex"), Some("gpt-6-sol"), Some(effort), &event.payload).unwrap();
            store.insert_auto_measurement(event.seq, &measurement).unwrap();
        }
        let summaries = store.auto_daily_aggregates(10).unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].effort.as_deref(), Some("high"));
        assert_eq!(summaries[1].effort.as_deref(), Some("medium"));
        assert!(summaries.iter().all(|row| row.samples == 1 && row.input_tokens == Some(42)));
    }

    #[test]
    fn quota_observation_history_is_bounded_without_erasing_execution_events() {
        const DAY: i64 = 86_400_000;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        for (n, day) in [(1, 69), (2, 70), (3, 98), (4, 99), (5, 100)] {
            let event = store.insert_event(day * DAY, Some("t-1"), Some("r-1"), "quota", "fixture", "reported", &json!({"n":n})).unwrap();
            let quota = crate::auto_quota::parse_codex_rate_limits(&json!({"rateLimits":{"primary":{"usedPercent":50,"resetsAt":1800003600}}}), "system-codex", day * DAY).unwrap();
            store.insert_auto_quota(event.seq, "system-codex", "fixture", &quota).unwrap();
        }
        assert_eq!(store.prune_auto_quotas(100 * DAY, 2).unwrap(), 3);
        assert_eq!(store.auto_quotas(10).unwrap().len(), 2);
        assert_eq!(store.events_after(0, Some("r-1"), 10).unwrap().len(), 5);
    }

    #[test]
    fn account_identity_change_invalidates_old_profile_learning_and_quota() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        assert!(!store.record_auto_account_identity("system-codex", &"a".repeat(64)).unwrap());
        let event = store.insert_event(1000, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &json!({"input_tokens": 42})).unwrap();
        let measurement = from_usage(1000, "t-1", "r-1", "codex", Some("system-codex"), None, &event.payload).unwrap();
        store.insert_auto_measurement(event.seq, &measurement).unwrap();
        let quota = crate::auto_quota::parse_codex_rate_limits(&json!({"rateLimits":{"primary":{"usedPercent":50,"resetsAt":1800003600}}}), "system-codex", 1000).unwrap();
        store.insert_auto_quota(event.seq, "system-codex", "fixture", &quota).unwrap();
        assert!(!store.record_auto_account_identity("system-codex", &"a".repeat(64)).unwrap());
        assert_eq!(store.auto_measurements(10).unwrap().len(), 1);
        assert_eq!(store.auto_daily_aggregates(10).unwrap().len(), 1);
        assert!(store.record_auto_account_identity("system-codex", &"b".repeat(64)).unwrap());
        assert!(store.auto_measurements(10).unwrap().is_empty());
        assert!(store.auto_daily_aggregates(10).unwrap().is_empty());
        assert!(store.auto_quotas(10).unwrap().is_empty());
        assert_eq!(store.events_after(0, Some("r-1"), 10).unwrap().len(), 1);
    }

    #[test]
    fn local_learning_prunes_expired_samples_and_enforces_a_row_cap() {
        const DAY: i64 = 86_400_000;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        for (n, day) in [(1, 69), (2, 70), (3, 98), (4, 99), (5, 100)] {
            let event = store.insert_event(day * DAY, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &json!({"input_tokens": n})).unwrap();
            let measurement = from_usage(day * DAY, "t-1", "r-1", "codex", None, None, &event.payload).unwrap();
            store.insert_auto_measurement(event.seq, &measurement).unwrap();
        }
        // The 30-day boundary is inclusive; a small test cap then keeps newest rows.
        assert_eq!(store.prune_auto_measurements(100 * DAY, 2).unwrap(), 3);
        let rows = store.auto_measurements(10).unwrap();
        assert_eq!(rows.iter().map(|r| r.measurement.input_tokens.unwrap()).collect::<Vec<_>>(), vec![5, 4]);
        assert_eq!(store.events_after(0, Some("r-1"), 10).unwrap().len(), 5);
    }

    #[test]
    fn local_daily_aggregates_preserve_missing_values_and_expire_after_ninety_days() {
        const DAY: i64 = 86_400_000;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        for (n, day, payload) in [
            (1, 9, json!({"input_tokens": 1})),
            (2, 10, json!({"input_tokens": 2})),
            (3, 100, json!({"input_tokens": 42})),
            (4, 100, json!({"output_tokens": 7})),
        ] {
            let event = store.insert_event(day * DAY, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &payload).unwrap();
            let measurement = from_usage(day * DAY, "t-1", "r-1", "codex", Some("system-codex"), Some("model-1"), &event.payload).unwrap();
            assert!(store.insert_auto_measurement(event.seq, &measurement).unwrap());
            assert!(!store.insert_auto_measurement(event.seq, &measurement).unwrap(), "aggregate must deduplicate by event");
            assert_eq!(n as usize, store.auto_measurements(10).unwrap().len());
        }
        let rows = store.auto_daily_aggregates(10).unwrap();
        let newest = rows.iter().find(|r| r.day_ms == 100 * DAY).unwrap();
        assert_eq!(newest.samples, 2);
        assert_eq!(newest.input_observations, 1);
        assert_eq!(newest.input_tokens, Some(42));
        assert_eq!(newest.output_observations, 1);
        assert_eq!(newest.output_tokens, Some(7));
        assert_eq!(store.prune_auto_daily_aggregates(100 * DAY, 2).unwrap(), 1);
        assert_eq!(store.auto_daily_aggregates(10).unwrap().len(), 2);
        assert_eq!(store.events_after(0, Some("r-1"), 10).unwrap().len(), 4);
    }

    #[test]
    fn daily_summary_keeps_a_late_day_observation_for_its_full_ninety_days() {
        const DAY: i64 = 86_400_000;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        let observed = 10 * DAY + 18 * 3_600_000;
        let event = store.insert_event(observed, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &json!({"input_tokens":1})).unwrap();
        let measurement = from_usage(observed, "t-1", "r-1", "codex", None, None, &event.payload).unwrap();
        store.insert_auto_measurement(event.seq, &measurement).unwrap();
        assert_eq!(store.prune_auto_daily_aggregates(100 * DAY + 12 * 3_600_000, 10).unwrap(), 0);
        assert_eq!(store.auto_daily_aggregates(10).unwrap().len(), 1);
        assert_eq!(store.prune_auto_daily_aggregates(observed + 90 * DAY + 1, 10).unwrap(), 1);
    }

    #[test]
    fn maintenance_count_tracks_measurements_even_when_event_ids_skip_multiples_of_one_hundred() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        for n in 0..100 {
            let event = store.insert_event(1000, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &json!({"input_tokens":n+1})).unwrap();
            let measurement = from_usage(1000, "t-1", "r-1", "codex", None, None, &event.payload).unwrap();
            store.insert_auto_measurement(event.seq, &measurement).unwrap();
            store.insert_event(1000, Some("t-1"), Some("r-1"), "filler", "fixture", "exact", &json!({})).unwrap();
        }
        assert!(store.auto_measurements(100).unwrap().is_empty(), "the 100th sample triggers 30-day maintenance even though its event sequence is odd");
        assert!(store.auto_daily_aggregates(100).unwrap().is_empty(), "the 100th sample also triggers 90-day maintenance");
    }

    #[test]
    fn storage_rejects_unbounded_measurement_fields() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        let event = store.insert_event(1000, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &json!({"input_tokens":1})).unwrap();
        let mut measurement = from_usage(1000, "t-1", "r-1", "codex", None, None, &event.payload).unwrap();
        measurement.model = Some("x".repeat(121));
        assert!(store.insert_auto_measurement(event.seq, &measurement).is_err());
        assert!(store.auto_measurements(10).unwrap().is_empty());
        assert!(store.auto_daily_aggregates(10).unwrap().is_empty());
    }

    #[test]
    fn clear_drops_aggregates_without_rebuilding_from_old_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        let event = store.insert_event(1000, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &json!({"input_tokens":42})).unwrap();
        let measurement = from_usage(1000, "t-1", "r-1", "codex", None, None, &event.payload).unwrap();
        store.insert_auto_measurement(event.seq, &measurement).unwrap();
        assert_eq!(store.auto_daily_aggregates(10).unwrap().len(), 1);
        store.clear_auto_learning().unwrap();
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert!(reopened.auto_daily_aggregates(10).unwrap().is_empty());
        assert_eq!(reopened.events_after(0, Some("r-1"), 10).unwrap().len(), 1);
    }

    #[test]
    fn clearing_learning_preserves_execution_events() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        let event = store.insert_event(1000, Some("t-1"), Some("r-1"), "usage", "harness", "exact", &json!({"input_tokens": 42})).unwrap();
        let measurement = from_usage(1000, "t-1", "r-1", "codex", None, None, &event.payload).unwrap();
        store.insert_auto_measurement(event.seq, &measurement).unwrap();
        assert_eq!(store.clear_auto_learning().unwrap(), 1);
        assert!(store.auto_measurements(10).unwrap().is_empty());
        assert_eq!(store.events_after(0, Some("r-1"), 10).unwrap().len(), 1);
    }
}
