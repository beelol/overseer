//! Durable state in SQLite. The daemon is the only writer.

use anyhow::{anyhow, Result};
use crate::auto_telemetry::{Measurement, StoredMeasurement};
use crate::auto_quota::{QuotaSnapshot, StoredQuotaObservation};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 17;
/// Retained normalized events per run before older ones are pruned (with a marker).
pub const EVENTS_PER_RUN: i64 = 5000;

pub struct Store {
    pub conn: Connection,
    pub learning_conn: Connection,
    pub learning_persistent: bool,
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
        let learning = (|| -> Result<Connection> {
            let learning_conn = if path == Path::new(":memory:") {
                Connection::open_in_memory()?
            } else {
                let learning_path = crate::paths::learning_db_path(path);
                let learning = Connection::open(&learning_path)?;
                #[cfg(unix)] {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&learning_path,
                        std::fs::Permissions::from_mode(0o600))?;
                }
                learning
            };
            learning_conn.pragma_update(None, "journal_mode", "DELETE")?;
            learning_conn.pragma_update(None, "synchronous", "FULL")?;
            // Learning storage is separate from execution. A busy learning
            // file must fail quickly while callers hold the shared Store lock.
            learning_conn.busy_timeout(std::time::Duration::from_millis(25))?;
            let page_size: i64 = learning_conn.pragma_query_value(None, "page_size", |row| row.get(0))?;
            let max_pages = (128 * 1024 * 1024 / page_size).max(1);
            learning_conn.pragma_update(None, "max_page_count", max_pages)?;
            // SQLite cannot lower this limit beneath an existing file's page count.
            // Treat an oversized file as unavailable instead of silently bypassing the cap.
            let applied_max_pages: i64 = learning_conn.pragma_query_value(
                None, "max_page_count", |row| row.get(0))?;
            if applied_max_pages > max_pages {
                return Err(anyhow!("Auto learning database exceeds its page cap"));
            }
            Ok(learning_conn)
        })();
        let (learning_conn, learning_persistent) = match learning {
            Ok(conn) => (conn, true),
            Err(_) => (Connection::open_in_memory()?, false),
        };
        let mut store = Self { conn, learning_conn, learning_persistent };
        store.migrate_main()?;
        if store.migrate_learning().and_then(|_| store.prune_auto_learning_history(crate::daemon::now())).is_err() {
            store.learning_conn = Connection::open_in_memory()?;
            store.learning_persistent = false;
            store.migrate_learning()?;
        }
        Ok(store)
    }

    #[cfg(test)]
    fn migrate(&self) -> Result<()> {
        self.migrate_main()?;
        self.migrate_learning()
    }

    fn migrate_main(&self) -> Result<()> {
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
              observed_turns INTEGER NOT NULL, consistent INTEGER NOT NULL,
              plan_type TEXT, plan_consistent INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS managed_work_units(
              work_unit_id TEXT PRIMARY KEY, parent_run_id TEXT NOT NULL REFERENCES runs(id),
              child_run_id TEXT NOT NULL UNIQUE REFERENCES runs(id),
              request_hash TEXT NOT NULL, created_ms INTEGER NOT NULL,
              result_event_seq INTEGER);
            CREATE TABLE IF NOT EXISTS auto_launch_intents(
              work_unit_id TEXT PRIMARY KEY, parent_run_id TEXT NOT NULL REFERENCES runs(id),
              requirements_hash TEXT NOT NULL, route_id TEXT NOT NULL,
              account_generation INTEGER, phase TEXT NOT NULL, created_ms INTEGER NOT NULL,
              planned_branch TEXT, planned_path TEXT, snapshot_id TEXT, snapshot_commit TEXT,
              decision_event_seq INTEGER REFERENCES events(seq) ON DELETE SET NULL);
            CREATE TABLE IF NOT EXISTS auto_pool_claims(
              work_unit_id TEXT PRIMARY KEY REFERENCES auto_launch_intents(work_unit_id),
              pool_id TEXT NOT NULL, account_generation INTEGER,
              state TEXT NOT NULL CHECK(state IN ('active','uncertain','released')),
              created_ms INTEGER NOT NULL, released_ms INTEGER);
            CREATE INDEX IF NOT EXISTS auto_pool_claims_active
              ON auto_pool_claims(pool_id,state);
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
        let has_run_plan: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('auto_run_account_evidence') WHERE name='plan_type'")?.exists([])?;
        if !has_run_plan {
            self.conn.execute_batch("ALTER TABLE auto_run_account_evidence ADD COLUMN plan_type TEXT;")?;
        }
        let has_plan_consistency: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('auto_run_account_evidence') WHERE name='plan_consistent'")?.exists([])?;
        if !has_plan_consistency {
            self.conn.execute_batch("ALTER TABLE auto_run_account_evidence ADD COLUMN plan_consistent INTEGER NOT NULL DEFAULT 0;")?;
        }
        let has_result_notice: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('managed_work_units') WHERE name='result_event_seq'")?.exists([])?;
        if !has_result_notice {
            self.conn.execute_batch("ALTER TABLE managed_work_units ADD COLUMN result_event_seq INTEGER;")?;
        }
        for column in ["planned_branch", "planned_path", "snapshot_id", "snapshot_commit"] {
            let present = self.conn.prepare("SELECT 1 FROM pragma_table_info('auto_launch_intents') WHERE name=?1")?
                .exists([column])?;
            if !present {
                self.conn.execute_batch(&format!("ALTER TABLE auto_launch_intents ADD COLUMN {column} TEXT;"))?;
            }
        }
        let has_decision_event: bool = self.conn.prepare("SELECT 1 FROM pragma_table_info('auto_launch_intents') WHERE name='decision_event_seq'")?.exists([])?;
        if !has_decision_event {
            self.conn.execute_batch("ALTER TABLE auto_launch_intents ADD COLUMN decision_event_seq INTEGER REFERENCES events(seq) ON DELETE SET NULL;")?;
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
        // A pre-v15 daemon may have admitted a child without a pool claim.
        // Its old route/account evidence cannot prove a shared pool after
        // restart, so occupy a single conservative legacy pool until every
        // such child settles. This is idempotent across interrupted upgrades.
        self.conn.execute(
            "INSERT OR IGNORE INTO auto_pool_claims
                (work_unit_id,pool_id,account_generation,state,created_ms)
             SELECT i.work_unit_id,'legacy/unresolved',i.account_generation,'uncertain',i.created_ms
             FROM auto_launch_intents i
             JOIN managed_work_units m ON m.work_unit_id=i.work_unit_id
             JOIN runs r ON r.id=m.child_run_id
             WHERE NOT (r.status IN ('completed','failed','interrupted') AND r.ended_ms IS NOT NULL)",
            [],
        )?;
        self.conn.execute("INSERT INTO meta(key, value) VALUES('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![SCHEMA_VERSION.to_string()])?;
        Ok(())
    }

    fn migrate_learning(&self) -> Result<()> {
        self.learning_conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
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
            CREATE TABLE IF NOT EXISTS auto_thread_usage_observations(
              id INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL,
              profile_id TEXT NOT NULL, read_account_generation INTEGER NOT NULL,
              attribution TEXT NOT NULL, observed_ms INTEGER NOT NULL,
              source TEXT NOT NULL, estimate TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS auto_thread_usage_observed ON auto_thread_usage_observations(observed_ms);
            CREATE TABLE IF NOT EXISTS auto_work_observations(
              work_unit_id TEXT PRIMARY KEY, run_id TEXT NOT NULL,
              profile_id TEXT, observed_ms INTEGER NOT NULL, record TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS auto_work_observations_recent
              ON auto_work_observations(observed_ms,work_unit_id);
            CREATE TABLE IF NOT EXISTS auto_allowance_estimates(
              profile_id TEXT NOT NULL, account_generation INTEGER NOT NULL,
              scope_key TEXT NOT NULL, observed_ms INTEGER NOT NULL, estimate TEXT NOT NULL,
              PRIMARY KEY(profile_id,account_generation,scope_key));
            CREATE INDEX IF NOT EXISTS auto_allowance_estimates_recent
              ON auto_allowance_estimates(observed_ms,profile_id);
            CREATE INDEX IF NOT EXISTS auto_measurements_run_observed
              ON auto_measurements(run_id,observed_ms);
        "#)?;
        if !self.learning_persistent { return Ok(()); }
        if self.learning_reset_pending()? {
            let tx = self.learning_conn.unchecked_transaction()?;
            tx.execute("DELETE FROM auto_measurements", [])?;
            tx.execute("DELETE FROM auto_daily_aggregates", [])?;
            tx.execute("DELETE FROM auto_thread_usage_observations", [])?;
            tx.execute("DELETE FROM auto_work_observations", [])?;
            tx.execute("DELETE FROM auto_allowance_estimates", [])?;
            tx.execute("DELETE FROM meta WHERE key='auto_learning_samples_inserted'", [])?;
            tx.commit()?;
            let tx = self.conn.unchecked_transaction()?;
            tx.execute("DELETE FROM auto_measurements", [])?;
            tx.execute("DELETE FROM auto_daily_aggregates", [])?;
            tx.execute("DELETE FROM auto_thread_usage_observations", [])?;
            tx.execute("INSERT OR IGNORE INTO meta(key,value) VALUES('auto_learning_split_migrated','1')", [])?;
            tx.execute("DELETE FROM meta WHERE key='auto_learning_reset_required'", [])?;
            tx.commit()?;
            return Ok(());
        }
        let migrated: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE key='auto_learning_split_migrated')",
            [], |row| row.get(0))?;
        if migrated { return Ok(()); }
        self.learning_conn.execute_batch("SAVEPOINT auto_learning_migration")?;
        let copy = (|| -> Result<()> {
            for table in ["auto_measurements", "auto_daily_aggregates", "auto_thread_usage_observations"] {
                let mut stmt = self.conn.prepare(&format!("SELECT * FROM {table}"))?;
                let column_count = stmt.column_count();
                let placeholders = (1..=column_count).map(|n| format!("?{n}"))
                    .collect::<Vec<_>>().join(",");
                let insert = format!("INSERT OR IGNORE INTO {table} VALUES({placeholders})");
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let values = (0..column_count).map(|index| row.get::<_, rusqlite::types::Value>(index))
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    self.learning_conn.execute(&insert, rusqlite::params_from_iter(values))?;
                }
            }
            let count: Option<String> = self.conn.query_row(
                "SELECT value FROM meta WHERE key='auto_learning_samples_inserted'", [], |row| row.get(0)).optional()?;
            if let Some(count) = count {
                self.learning_conn.execute("INSERT OR IGNORE INTO meta(key,value) VALUES('auto_learning_samples_inserted',?1)", [count])?;
            }
            Ok(())
        })();
        if let Err(error) = copy {
            let _ = self.learning_conn.execute_batch("ROLLBACK TO auto_learning_migration; RELEASE auto_learning_migration");
            return Err(error);
        }
        self.learning_conn.execute_batch("RELEASE auto_learning_migration")?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("INSERT OR IGNORE INTO meta(key,value) VALUES('auto_learning_split_migrated','1')", [])?;
        tx.execute("DELETE FROM auto_measurements", [])?;
        tx.execute("DELETE FROM auto_daily_aggregates", [])?;
        tx.execute("DELETE FROM auto_thread_usage_observations", [])?;
        tx.execute("DELETE FROM meta WHERE key='auto_learning_samples_inserted'", [])?;
        tx.commit()?;
        Ok(())
    }

    fn learning_reset_pending(&self) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE key='auto_learning_reset_required')",
            [], |row| row.get(0))?)
    }

    pub fn auto_learning_is_paused(&self) -> Result<bool> {
        Ok(!self.learning_persistent || self.learning_reset_pending()?)
    }

    /// Auto is a user-visible mode switch, not a routing-policy file.
    /// Absent or unexpected values fail closed after an upgrade or restart.
    pub fn auto_mode_enabled(&self) -> Result<bool> {
        let value: Option<String> = self.conn.query_row(
            "SELECT value FROM meta WHERE key='auto_mode_enabled'", [], |row| row.get(0),
        ).optional()?;
        Ok(value.as_deref() == Some("1"))
    }

    pub fn set_auto_mode_enabled(&self, enabled: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key,value) VALUES('auto_mode_enabled',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [if enabled { "1" } else { "0" }],
        )?;
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

    pub fn auto_launch_intent(&self, id: &str) -> Result<Option<(String, String, String, Option<i64>, String)>> {
        Ok(self.conn.query_row(
            "SELECT parent_run_id,requirements_hash,route_id,account_generation,phase FROM auto_launch_intents WHERE work_unit_id=?1",
            params![id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional()?)
    }

    pub fn insert_auto_launch_intent(&self, id: &str, parent: &str, requirements_hash: &str,
        route_id: &str, account_generation: Option<i64>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO auto_launch_intents(work_unit_id,parent_run_id,requirements_hash,route_id,account_generation,phase,created_ms) VALUES(?1,?2,?3,?4,?5,'preparing',?6)",
            params![id, parent, requirements_hash, route_id, account_generation, crate::daemon::now()],
        )?;
        Ok(())
    }

    pub fn insert_auto_selected_decision(&self, work_unit_id: &str, parent: &Run,
        requirements_hash: &str, route_id: &str, pool_id: &str,
        account_generation: Option<i64>, payload: &Value) -> Result<Option<Event>> {
        if pool_id.is_empty() || pool_id.len() > 256 {
            return Err(anyhow!("automatic quota pool identity is invalid"));
        }
        let tx = self.conn.unchecked_transaction()?;
        if self.auto_pool_claimed(pool_id)? {
            return Ok(None);
        }
        self.insert_auto_launch_intent(work_unit_id, &parent.id, requirements_hash,
            route_id, account_generation)?;
        self.conn.execute(
            "INSERT INTO auto_pool_claims(work_unit_id,pool_id,account_generation,state,created_ms)
             VALUES(?1,?2,?3,'active',?4)",
            params![work_unit_id, pool_id, account_generation, crate::daemon::now()],
        )?;
        let event = self.insert_event(crate::daemon::now(), Some(&parent.task_id), Some(&parent.id),
            "auto_decision", "daemon", "exact", payload)?;
        self.conn.execute("UPDATE auto_launch_intents SET decision_event_seq=?2 WHERE work_unit_id=?1",
            params![work_unit_id, event.seq])?;
        tx.commit()?;
        Ok(Some(event))
    }

    /// An unknown subscription draw occupies its shared pool until a child
    /// settles. A paused intent without a child remains effects-uncertain and
    /// cannot be released merely because this daemon restarted.
    pub fn auto_pool_claimed(&self, pool_id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM auto_pool_claims WHERE (pool_id=?1 OR pool_id='legacy/unresolved')
                AND state IN ('active','uncertain'))",
            [pool_id], |row| row.get(0))?)
    }

    /// Called in the terminal run transaction. Unknown or still-running
    /// children retain their claim; only a confirmed settled child releases it.
    pub fn release_settled_auto_pool_claim(&self, child_run_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE auto_pool_claims SET state='released',released_ms=?2
             WHERE work_unit_id=(SELECT m.work_unit_id FROM managed_work_units m
                 JOIN runs r ON r.id=m.child_run_id WHERE m.child_run_id=?1
                 AND r.status IN ('completed','failed','interrupted') AND r.ended_ms IS NOT NULL)
               AND state IN ('active','uncertain')",
            params![child_run_id, crate::daemon::now()],
        )?;
        Ok(())
    }

    /// Without a committed child row, no model turn can have started. Release
    /// the *allowance* claim after a stopped worker or at startup, while the
    /// launch intent and any uncertain Git resource remain untouched. A child
    /// row, even with no observed process, keeps its claim until settlement.
    pub fn release_unstarted_auto_pool_claim(&self, work_unit_id: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE auto_pool_claims SET state='released',released_ms=?2
             WHERE work_unit_id=?1 AND state IN ('active','uncertain')
               AND NOT EXISTS(SELECT 1 FROM managed_work_units m WHERE m.work_unit_id=?1)",
            params![work_unit_id, crate::daemon::now()],
        )? == 1)
    }

    /// Startup reconciliation runs after the old daemon's workers are gone.
    /// A persisted intent with no child may own an uncertain Git worktree,
    /// but it cannot have spent model allowance through this launch path.
    pub fn release_stale_unstarted_auto_pool_claims(&self) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE auto_pool_claims SET state='released',released_ms=?1
             WHERE state IN ('active','uncertain')
               AND NOT EXISTS(SELECT 1 FROM managed_work_units m
                   WHERE m.work_unit_id=auto_pool_claims.work_unit_id)",
            [crate::daemon::now()],
        )?)
    }

    pub fn set_auto_launch_intent_phase(&self, id: &str, phase: &str) -> Result<()> {
        if phase != "paused" {
            return Err(anyhow!("unsupported automatic launch phase"));
        }
        let tx = self.conn.unchecked_transaction()?;
        self.conn.execute("UPDATE auto_launch_intents SET phase=?2 WHERE work_unit_id=?1", params![id, phase])?;
        self.conn.execute("UPDATE auto_pool_claims SET state='uncertain' WHERE work_unit_id=?1 AND state='active'", [id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn mark_auto_child_created(&self, id: &str) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE auto_launch_intents SET phase='child_created' WHERE work_unit_id=?1 AND phase='preparing'",
            [id],
        )?;
        if updated != 1 { return Err(anyhow!("automatic child launch intent is unavailable")); }
        Ok(())
    }

    pub fn journal_auto_launch_worktree(&self, id: &str, branch: &str, path: &str,
        snapshot_id: &str, snapshot_commit: &str) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE auto_launch_intents SET planned_branch=?2,planned_path=?3,snapshot_id=?4,snapshot_commit=?5 WHERE work_unit_id=?1 AND phase='preparing' AND planned_path IS NULL",
            params![id, branch, path, snapshot_id, snapshot_commit],
        )?;
        if updated != 1 { return Err(anyhow!("automatic launch worktree plan was already recorded")); }
        Ok(())
    }

    pub fn auto_launch_resources(&self, id: &str) -> Result<Option<(String, String, String, String)>> {
        Ok(self.conn.query_row(
            "SELECT planned_branch,planned_path,snapshot_id,snapshot_commit FROM auto_launch_intents WHERE work_unit_id=?1 AND planned_branch IS NOT NULL AND planned_path IS NOT NULL AND snapshot_id IS NOT NULL AND snapshot_commit IS NOT NULL",
            params![id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?)
    }

    pub fn insert_managed_work_unit(&self, id: &str, parent: &str, child: &str, request_hash: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO managed_work_units(work_unit_id,parent_run_id,child_run_id,request_hash,created_ms) VALUES(?1,?2,?3,?4,?5)",
            params![id, parent, child, request_hash, crate::daemon::now()],
        )?;
        Ok(())
    }

    /// Recover the last turn's completed signal when a tail was restarted
    /// after its raw-output cursor had already advanced.
    pub fn last_turn_completion(&self, run_id: &str) -> Result<Option<bool>> {
        let status: Option<(String, Option<i64>)> = self.conn.query_row(
            "SELECT status,ended_ms FROM turns WHERE run_id=?1 ORDER BY n DESC LIMIT 1",
            params![run_id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        Ok(match status {
            Some((status, Some(_))) if status == "completed" => Some(true),
            Some((status, Some(_))) if status == "failed" => Some(false),
            _ => None,
        })
    }

    /// Called inside the same transaction as the child's terminal status.
    /// The parent receives a stable handle, never a second copy of child output.
    pub fn publish_managed_result_notice(&self, child: &Run, observed_ms: i64) -> Result<Option<Event>> {
        let record: Option<(String, String, Option<i64>)> = self.conn.query_row(
            "SELECT work_unit_id,parent_run_id,result_event_seq FROM managed_work_units WHERE child_run_id=?1",
            params![child.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        let Some((work_unit_id, parent_id, prior_notice)) = record else { return Ok(None) };
        if prior_notice.is_some() { return Ok(None) }

        let mut source_event_seq = None;
        let mut outputs = self.conn.prepare(
            "SELECT seq,payload FROM events WHERE run_id=?1 AND kind='output' ORDER BY seq DESC LIMIT 5000")?;
        let rows = outputs.query_map(params![child.id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
        for row in rows {
            let (seq, payload) = row?;
            let Ok(value) = serde_json::from_str::<Value>(&payload) else { continue };
            if value["role"] == "assistant" && value["text"].as_str().is_some() {
                source_event_seq = Some(seq);
                break;
            }
        }
        let state = if source_event_seq.is_some() { "ready" } else { "completed_without_text" };
        let event = self.insert_event(observed_ms, Some(&child.task_id), Some(&parent_id),
            "managed_child_result_available", "daemon", "exact",
            &serde_json::json!({"work_unit_id":work_unit_id,"child_run_id":child.id,
                "source_event_seq":source_event_seq,"state":state,"workspace_id":child.workspace_id}))?;
        let changed = self.conn.execute(
            "UPDATE managed_work_units SET result_event_seq=?2 WHERE child_run_id=?1 AND result_event_seq IS NULL",
            params![child.id, event.seq])?;
        if changed != 1 { return Err(anyhow!("managed result notice lost its exclusive claim")) }
        Ok(Some(event))
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
                // If the learning file is unavailable, retain a durable reset
                // marker. Recovery clears stale account data before exposing it.
                self.conn.execute("INSERT OR REPLACE INTO meta(key,value) VALUES('auto_learning_reset_required','1')", [])?;
                if self.learning_persistent {
                    let removed = (|| -> Result<()> {
                        let tx = self.learning_conn.unchecked_transaction()?;
                        tx.execute("DELETE FROM auto_measurements WHERE profile_id=?1", params![profile_id])?;
                        tx.execute("DELETE FROM auto_daily_aggregates WHERE profile_id=?1", params![profile_id])?;
                        tx.execute("DELETE FROM auto_thread_usage_observations WHERE profile_id=?1", params![profile_id])?;
                        tx.execute("DELETE FROM auto_work_observations WHERE profile_id=?1", params![profile_id])?;
                        tx.execute("DELETE FROM auto_allowance_estimates WHERE profile_id=?1", params![profile_id])?;
                        tx.commit()?;
                        Ok(())
                    })();
                    if removed.is_ok() {
                        self.conn.execute("DELETE FROM meta WHERE key='auto_learning_reset_required'", [])?;
                    }
                }
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

    /// The fingerprint is already provider-domain-separated and one-way. Use
    /// it as a shared quota-pool key across profiles signed into one account;
    /// a profile ID by itself is not an account identity.
    pub fn auto_account_pool_id(&self, profile_id: &str) -> Result<Option<String>> {
        let fingerprint: Option<String> = self.conn.query_row(
            "SELECT fingerprint FROM auto_account_identity WHERE profile_id=?1",
            params![profile_id], |row| row.get(0),
        ).optional()?;
        Ok(fingerprint.map(|value| format!("account/{value}")))
    }

    /// Read each profile's newest quota observation for this authenticated
    /// account, including profiles omitted from the current allowed routes.
    /// The bounded lookup fails closed instead of silently dropping a block.
    pub fn auto_account_quota_observations(&self, profile_id: &str) -> Result<Vec<StoredQuotaObservation>> {
        let profile_ids = {
            let mut stmt = self.conn.prepare(
                "SELECT profile_id FROM auto_account_identity WHERE fingerprint=(SELECT fingerprint FROM auto_account_identity WHERE profile_id=?1) ORDER BY profile_id LIMIT 129"
            )?;
            let ids = stmt.query_map(params![profile_id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            ids
        };
        if profile_ids.len() > 128 { return Err(anyhow!("shared account profile bound exceeded")); }
        let mut observations = Vec::new();
        for id in profile_ids {
            if let Some(observation) = self.latest_auto_quota(&id)? {
                observations.push(observation);
            }
        }
        Ok(observations)
    }

    /// A same-process Codex app-server metadata reply observed before a turn.
    /// Every turn of the thread must be stamped before its cumulative usage can
    /// be tied to one account generation.
    pub fn record_auto_run_account(&self, run_id: &str, profile_id: &str, generation: i64,
        plan_type: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO auto_run_account_evidence(run_id,profile_id,first_generation,last_generation,observed_turns,consistent,plan_type,plan_consistent) \
             VALUES(?1,?2,?3,?3,1,1,?4,CASE WHEN ?4 IS NULL THEN 0 ELSE 1 END) \
             ON CONFLICT(run_id) DO UPDATE SET last_generation=excluded.last_generation,observed_turns=observed_turns+1, \
             consistent=CASE WHEN profile_id=excluded.profile_id AND first_generation=excluded.first_generation AND consistent=1 THEN 1 ELSE 0 END, \
             plan_consistent=CASE WHEN plan_consistent=1 AND plan_type IS NOT NULL AND plan_type=excluded.plan_type THEN 1 ELSE 0 END",
            params![run_id, profile_id, generation, plan_type],
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

    pub fn auto_run_plan_matches(&self, run_id: &str, profile_id: &str, generation: i64,
        plan_type: Option<&str>) -> Result<bool> {
        let Some(plan_type) = plan_type else { return Ok(false); };
        if !self.auto_run_account_matches(run_id, profile_id, generation)? { return Ok(false); }
        let evidence: Option<(Option<String>, bool)> = self.conn.query_row(
            "SELECT plan_type,plan_consistent FROM auto_run_account_evidence WHERE run_id=?1",
            params![run_id], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
        ).optional()?;
        Ok(evidence.is_some_and(|(first, consistent)| consistent && first.as_deref() == Some(plan_type)))
    }

    pub fn auto_thread_usage_attribution(&self, run_id: &str, profile_id: &str,
        generation: i64, plan_type: Option<&str>) -> Result<&'static str> {
        if !self.auto_run_account_matches(run_id, profile_id, generation)? {
            Ok("unverified_run_account")
        } else if !self.auto_run_plan_matches(run_id, profile_id, generation, plan_type)? {
            Ok("unverified_plan_scope")
        } else {
            Ok("same_account_generation_and_reported_plan")
        }
    }

    pub fn insert_auto_thread_usage(
        &self, run_id: &str, profile_id: &str, generation: i64,
        source: &str, estimate: &crate::auto_consumption::ThreadUsageEstimate,
    ) -> Result<i64> {
        if !self.learning_persistent || self.learning_reset_pending()? { return Err(anyhow!("Auto learning storage unavailable")); }
        let attribution = self.auto_thread_usage_attribution(run_id, profile_id,
            generation, estimate.plan_type.as_deref())?;
        // This is cumulative thread metadata, not a new charge on every read.
        // Keep one latest sample per run/account generation, including later
        // provider corrections, so refreshes cannot inflate learned draw.
        let encoded = serde_json::to_string(estimate)?;
        let tx = self.learning_conn.unchecked_transaction()?;
        let existing: Option<i64> = tx.query_row(
            "SELECT id FROM auto_thread_usage_observations
             WHERE run_id=?1 AND profile_id=?2 AND read_account_generation=?3
             ORDER BY id DESC LIMIT 1",
            params![run_id, profile_id, generation], |row| row.get(0),
        ).optional()?;
        let id = if let Some(id) = existing {
            tx.execute(
                "UPDATE auto_thread_usage_observations
                 SET attribution=?2,observed_ms=?3,source=?4,estimate=?5 WHERE id=?1",
                params![id, attribution, estimate.observed_ms, source, encoded],
            )?;
            tx.execute(
                "DELETE FROM auto_thread_usage_observations
                 WHERE run_id=?1 AND profile_id=?2 AND read_account_generation=?3 AND id<>?4",
                params![run_id, profile_id, generation, id],
            )?;
            id
        } else {
            tx.execute(
                "INSERT INTO auto_thread_usage_observations(run_id,profile_id,read_account_generation,attribution,observed_ms,source,estimate) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![run_id, profile_id, generation, attribution, estimate.observed_ms, source, encoded],
            )?;
            tx.last_insert_rowid()
        };
        const THIRTY_DAYS_MS: i64 = 30 * 86_400_000;
        tx.execute("DELETE FROM auto_thread_usage_observations WHERE observed_ms < ?1", params![crate::daemon::now().saturating_sub(THIRTY_DAYS_MS)])?;
        tx.execute("DELETE FROM auto_thread_usage_observations WHERE id NOT IN (SELECT id FROM auto_thread_usage_observations ORDER BY observed_ms DESC,id DESC LIMIT 5000)", [])?;
        tx.commit()?;
        Ok(id)
    }

    pub fn auto_thread_usage_observations(&self, limit: i64) -> Result<Vec<crate::auto_consumption::StoredThreadUsageObservation>> {
        if self.learning_reset_pending()? { return Ok(Vec::new()); }
        use crate::auto_consumption::{StoredThreadUsageObservation, ThreadUsageEstimate};
        let mut stmt = self.learning_conn.prepare("SELECT id,run_id,profile_id,read_account_generation,attribution,source,estimate FROM auto_thread_usage_observations ORDER BY id DESC LIMIT ?1")?;
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
    pub fn auto_run_pre_turn_quota(&self, run_id: &str) -> Result<Option<QuotaSnapshot>> {
        let encoded: Option<String> = self.conn.query_row(
            "SELECT q.snapshot FROM auto_quota_observations q JOIN events e ON e.seq=q.event_seq \
             WHERE e.run_id=?1 AND q.source='codex-app/managed-pre-turn' \
             ORDER BY q.observed_ms DESC,q.event_seq DESC LIMIT 1",
            params![run_id], |row| row.get(0),
        ).optional()?;
        encoded.map(|value| serde_json::from_str(&value).map_err(Into::into)).transpose()
    }

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
            "DELETE FROM auto_quota_observations WHERE event_seq NOT IN (SELECT event_seq FROM auto_quota_observations ORDER BY observed_ms DESC,event_seq DESC LIMIT ?1)",
            params![cap.clamp(1, 5000)],
        )?;
        Ok(expired + over_cap)
    }

    pub fn latest_auto_quota(&self, pool_id: &str) -> Result<Option<StoredQuotaObservation>> {
        let mut rows = self.conn.prepare(
            "SELECT event_seq,pool_id,source,snapshot FROM auto_quota_observations WHERE pool_id=?1 ORDER BY observed_ms DESC,event_seq DESC LIMIT 1"
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
        if !self.learning_persistent || self.learning_reset_pending()? { return Err(anyhow!("Auto learning storage unavailable")); }
        if !crate::auto_telemetry::valid_for_store(m) {
            return Err(anyhow!("invalid Auto measurement"));
        }
        // Keep detail and aggregate writes atomic on the learning connection.
        // Callers may invoke this after the execution event has committed.
        self.learning_conn.execute_batch("SAVEPOINT auto_measurement_write")?;
        let result = (|| -> Result<usize> {
        let inserted = self.learning_conn.execute(
            "INSERT OR IGNORE INTO auto_measurements(event_seq,observed_ms,task_id,run_id,harness,profile_id,model,effort,input_tokens,output_tokens,cached_input_tokens,reasoning_output_tokens,cost_usd) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![event_seq, m.observed_ms, m.task_id, m.run_id, m.harness, m.profile_id, m.model, m.effort,
                m.input_tokens, m.output_tokens, m.cached_input_tokens, m.reasoning_output_tokens, m.cost_usd],
        )?;
        if inserted == 1 {
            let day_ms = m.observed_ms.div_euclid(DAY_MS) * DAY_MS;
            self.learning_conn.execute(
                "INSERT INTO auto_daily_aggregates(day_ms,harness,profile_id,model,effort,last_observed_ms,samples,input_observations,input_tokens,output_observations,output_tokens,cached_input_observations,cached_input_tokens,reasoning_output_observations,reasoning_output_tokens,cost_observations,cost_usd) VALUES(?1,?2,?3,?4,?5,?6,1,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) ON CONFLICT(day_ms,harness,profile_id,model,effort) DO UPDATE SET last_observed_ms=MAX(last_observed_ms,excluded.last_observed_ms),samples=samples+1,input_observations=input_observations+excluded.input_observations,input_tokens=input_tokens+excluded.input_tokens,output_observations=output_observations+excluded.output_observations,output_tokens=output_tokens+excluded.output_tokens,cached_input_observations=cached_input_observations+excluded.cached_input_observations,cached_input_tokens=cached_input_tokens+excluded.cached_input_tokens,reasoning_output_observations=reasoning_output_observations+excluded.reasoning_output_observations,reasoning_output_tokens=reasoning_output_tokens+excluded.reasoning_output_tokens,cost_observations=cost_observations+excluded.cost_observations,cost_usd=cost_usd+excluded.cost_usd",
                params![day_ms, m.harness, m.profile_id.as_deref().unwrap_or(""), m.model.as_deref().unwrap_or(""), m.effort.as_deref().unwrap_or(""), m.observed_ms,
                    i64::from(m.input_tokens.is_some()), m.input_tokens.unwrap_or(0),
                    i64::from(m.output_tokens.is_some()), m.output_tokens.unwrap_or(0),
                    i64::from(m.cached_input_tokens.is_some()), m.cached_input_tokens.unwrap_or(0),
                    i64::from(m.reasoning_output_tokens.is_some()), m.reasoning_output_tokens.unwrap_or(0),
                    i64::from(m.cost_usd.is_some()), m.cost_usd.unwrap_or(0.0)],
            )?;
        }
        let count: i64 = if inserted == 1 {
            self.learning_conn.execute("INSERT INTO meta(key,value) VALUES('auto_learning_samples_inserted','1') ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1", [])?;
            self.learning_conn.query_row("SELECT CAST(value AS INTEGER) FROM meta WHERE key='auto_learning_samples_inserted'", [], |row| row.get(0))?
        } else { 0 };
        if inserted == 1 {
            let now = crate::daemon::now();
            // Exact row caps are enforced in the same savepoint as the write.
            // A count query is cheap at these bounded table sizes and avoids
            // overshoot between periodic age-maintenance passes.
            let details: i64 = self.learning_conn.query_row("SELECT COUNT(*) FROM auto_measurements", [], |row| row.get(0))?;
            if details > 50_000 || count % 100 == 0 {
                self.prune_auto_measurements(now, 50_000)?;
            }
            let summaries: i64 = self.learning_conn.query_row("SELECT COUNT(*) FROM auto_daily_aggregates", [], |row| row.get(0))?;
            if summaries > 10_000 || count % 100 == 0 {
                self.prune_auto_daily_aggregates(now, 10_000)?;
            }
        }
        Ok(inserted)
        })();
        let inserted = match result {
            Ok(value) => {
                self.learning_conn.execute_batch("RELEASE auto_measurement_write")?;
                value
            }
            Err(error) => {
                let _ = self.learning_conn.execute_batch("ROLLBACK TO auto_measurement_write; RELEASE auto_measurement_write");
                return Err(error);
            }
        };
        Ok(inserted == 1)
    }

    pub fn auto_measurements(&self, limit: i64) -> Result<Vec<StoredMeasurement>> {
        if self.learning_reset_pending()? { return Ok(Vec::new()); }
        let mut stmt = self.learning_conn.prepare("SELECT * FROM auto_measurements ORDER BY event_seq DESC LIMIT ?1")?;
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

    /// Build a content-free record from execution and normalized meter rows.
    /// A later metadata read may update an existing record, but may not
    /// recreate history after the user has cleared learning.
    fn auto_work_observation(&self, run_id: &str) -> Result<Option<(String, Option<String>, i64, Value)>> {
        let Some(run) = self.run(run_id)? else { return Ok(None); };
        let work: Option<(String, Option<i64>, Option<i64>, Option<String>)> = self.conn.query_row(
            "SELECT m.work_unit_id,i.created_ms,i.account_generation,c.pool_id
             FROM managed_work_units m
             LEFT JOIN auto_launch_intents i ON i.work_unit_id=m.work_unit_id
             LEFT JOIN auto_pool_claims c ON c.work_unit_id=m.work_unit_id
             WHERE m.child_run_id=?1", [run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        let Some((work_unit_id, intent_ms, account_generation, pool_id)) = work else {
            return Ok(None);
        };
        // An account switch invalidates the old account's learning. An Auto
        // child can still finish after that switch, so its launch generation
        // must match the account currently attached to the selected profile.
        if let Some(expected) = account_generation {
            let Some(profile_id) = run.profile_id.as_deref() else { return Ok(None); };
            if self.auto_account_generation(profile_id)? != Some(expected) {
                return Ok(None);
            }
        }
        let Some(ended_ms) = run.ended_ms else { return Ok(None); };
        let decision_payload: Option<String> = self.conn.query_row(
            "SELECT e.payload FROM auto_launch_intents i JOIN events e ON e.seq=i.decision_event_seq
             WHERE i.work_unit_id=?1 AND e.kind='auto_decision'",
            [&work_unit_id], |row| row.get(0),
        ).optional()?;
        let task_requirements = decision_payload
            .as_deref()
            .and_then(|encoded| serde_json::from_str::<Value>(encoded).ok())
            .and_then(|payload| serde_json::from_value::<crate::auto_select::WorkUnit>(
                payload["selection_input"]["work"].clone()).ok())
            .filter(|work| work.id == work_unit_id)
            .map(|work| serde_json::json!({
                "source":"auto_decision", "min_tier":work.min_tier,
                "required_tools":work.required_tools, "context_needed":work.context_needed,
                "requires_approvals":work.requires_approvals,
                "sandbox":work.min_sandbox,
            }));
        let started_ms: Option<i64> = self.conn.query_row(
            "SELECT MIN(started_ms) FROM turns WHERE run_id=?1", [run_id], |row| row.get(0))?;
        let quota_ref = |source: &str| -> Result<Option<Value>> {
            let row: Option<(i64, i64, String)> = self.conn.query_row(
                "SELECT q.event_seq,q.observed_ms,q.snapshot FROM auto_quota_observations q
                 JOIN events e ON e.seq=q.event_seq
                 WHERE e.run_id=?1 AND q.source=?2
                 ORDER BY q.observed_ms DESC,q.event_seq DESC LIMIT 1",
                params![run_id, source], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).optional()?;
            row.map(|(event_seq, observed_ms, encoded)| -> Result<Value> {
                let snapshot: QuotaSnapshot = serde_json::from_str(&encoded)?;
                Ok(serde_json::json!({"event_seq":event_seq,"source":source,
                    "observed_ms":observed_ms,"expires_ms":snapshot.expires_ms,
                    "plan_type":snapshot.reported_plan_type(),
                    "windows":snapshot.windows.iter().map(|window| serde_json::json!({
                        "bucket_id":window.bucket_id,"window":window.window,
                        "model":window.model,"model_family":window.model_family,
                        "reset_ms":window.reset_ms})).collect::<Vec<_>>() }))
            }).transpose()
        };
        let quota_before = quota_ref("codex-app/managed-pre-turn")?;
        let quota_after = quota_ref("codex-app/metadata-read")?;
        let usage_count: i64 = self.learning_conn.query_row(
            "SELECT COUNT(*) FROM auto_measurements WHERE run_id=?1", [run_id], |row| row.get(0))?;
        let usage: Option<Value> = if usage_count == 1 {
            self.learning_conn.query_row(
                "SELECT observed_ms,input_tokens,output_tokens,cached_input_tokens,
                        reasoning_output_tokens,cost_usd FROM auto_measurements WHERE run_id=?1",
                [run_id], |row| Ok(serde_json::json!({"observed_ms":row.get::<_,i64>(0)?,
                    "input_tokens":row.get::<_,Option<i64>>(1)?,
                    "output_tokens":row.get::<_,Option<i64>>(2)?,
                    "cached_input_tokens":row.get::<_,Option<i64>>(3)?,
                    "reasoning_output_tokens":row.get::<_,Option<i64>>(4)?,
                    "cost_usd":row.get::<_,Option<f64>>(5)?})),
            ).optional()?
        } else { None };
        let record = serde_json::json!({
            "work_unit_id":work_unit_id,"run_id":run.id,"harness":run.harness,
            "harness_version":run.harness_version,"profile_id":run.profile_id,
            "pool_id":pool_id,"account_generation":account_generation,
            "model":run.model,"effort":run.effort,"status":run.status,
            "task_requirements":task_requirements,
            "started_ms":started_ms,"ended_ms":ended_ms,
            "launch_overhead_ms":intent_ms.zip(started_ms).and_then(|(intent, start)|
                start.checked_sub(intent).filter(|value| *value >= 0)),
            "execution_ms":started_ms.and_then(|start|
                ended_ms.checked_sub(start).filter(|value| *value >= 0)),
            "quota_before":quota_before,"quota_after":quota_after,
            "usage_observations":usage_count,"usage":usage,
            "usage_semantics":"single_harness_observation_only",
            "subscription_window_draw":"unverified"
        });
        Ok(Some((work_unit_id, run.profile_id, ended_ms, record)))
    }

    pub fn record_auto_work_observation(&self, run_id: &str) -> Result<bool> {
        if !self.learning_persistent || self.learning_reset_pending()? {
            return Err(anyhow!("Auto learning storage unavailable"));
        }
        let Some((work_unit_id, profile_id, observed_ms, record)) = self.auto_work_observation(run_id)? else {
            return Ok(false);
        };
        self.learning_conn.execute_batch("SAVEPOINT auto_work_measurement_write")?;
        let write = (|| -> Result<bool> {
        let inserted = self.learning_conn.execute(
            "INSERT OR IGNORE INTO auto_work_observations(work_unit_id,run_id,profile_id,observed_ms,record)
             VALUES(?1,?2,?3,?4,?5)",
            params![work_unit_id,run_id,profile_id,observed_ms,record.to_string()])?;
        if inserted == 1 {
            self.prune_auto_work_observations(crate::daemon::now(), 5000)?;
        }
        Ok(inserted == 1)
        })();
        match write {
            Ok(inserted) => {
                self.learning_conn.execute_batch("RELEASE auto_work_measurement_write")?;
                Ok(inserted)
            }
            Err(error) => {
                let _ = self.learning_conn.execute_batch(
                    "ROLLBACK TO auto_work_measurement_write; RELEASE auto_work_measurement_write");
                Err(error)
            }
        }
    }

    pub fn refresh_auto_work_observation(&self, run_id: &str) -> Result<bool> {
        if !self.learning_persistent || self.learning_reset_pending()? {
            return Err(anyhow!("Auto learning storage unavailable"));
        }
        let existing: bool = self.learning_conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM auto_work_observations WHERE run_id=?1)",
            [run_id], |row| row.get(0))?;
        if !existing { return Ok(false); }
        if let Some((work_unit_id, _, _, record)) = self.auto_work_observation(run_id)? {
            let updated = self.learning_conn.execute("UPDATE auto_work_observations SET record=?2 WHERE work_unit_id=?1",
                params![work_unit_id,record.to_string()])?;
            return Ok(updated == 1);
        }
        Ok(false)
    }

    pub fn auto_work_observations(&self, limit: i64) -> Result<Vec<Value>> {
        if self.learning_reset_pending()? { return Ok(Vec::new()); }
        self.prune_auto_work_observations(crate::daemon::now(), 5000)?;
        let mut stmt = self.learning_conn.prepare(
            "SELECT record FROM auto_work_observations ORDER BY observed_ms DESC,work_unit_id DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit.clamp(1, 5000)], |row| {
            let encoded: String = row.get(0)?;
            serde_json::from_str::<Value>(&encoded).map_err(|error|
                rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error)))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Store only a bounded, fully scoped estimate for the account generation
    /// that produced it. This is local learning, not execution authority; a
    /// caller must first establish actual-work attribution or a direct
    /// provider-reported subscription charge.
    pub fn put_auto_allowance_estimate(&self, profile_id: &str, generation: i64,
        estimate: &crate::auto_select::AllowanceEstimate, now_ms: i64) -> Result<bool> {
        use crate::auto_consumption::EstimateKey;
        if !self.learning_persistent || self.learning_reset_pending()? {
            return Err(anyhow!("Auto learning storage unavailable"));
        }
        let key = EstimateKey::from_estimate(estimate)
            .ok_or_else(|| anyhow!("allowance estimate scope is incomplete"))?;
        const AGE_MS: i64 = 30 * 86_400_000;
        if generation <= 0 || self.auto_account_generation(profile_id)? != Some(generation)
            || self.auto_account_pool_id(profile_id)?.as_deref() != Some(key.pool_id.as_str())
            || estimate.observed_ms > now_ms
            || now_ms.saturating_sub(estimate.observed_ms) >= AGE_MS
            || estimate.windows.is_empty() || estimate.windows.len() > 16 {
            return Err(anyhow!("allowance estimate account, age, or windows invalid"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for draw in &estimate.windows {
            if draw.bucket_id.is_empty() || draw.window.is_empty()
                || !draw.upper_percent.is_finite()
                || !(0.0..=100.0).contains(&draw.upper_percent)
                || draw.upper_percent == 0.0
                || !seen.insert((&draw.bucket_id, &draw.window)) {
                return Err(anyhow!("allowance estimate window is invalid"));
            }
        }
        let scope_key = serde_json::to_string(&key)?;
        let encoded = serde_json::to_string(estimate)?;
        if profile_id.is_empty() || profile_id.len() > 120
            || scope_key.len() > 4096 || encoded.len() > 8192 {
            return Err(anyhow!("allowance estimate exceeds storage bound"));
        }
        self.learning_conn.execute_batch("SAVEPOINT auto_allowance_estimate_write")?;
        let write = (|| -> Result<bool> {
            let changed = self.learning_conn.execute(
                "INSERT INTO auto_allowance_estimates(profile_id,account_generation,scope_key,observed_ms,estimate)
                 VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(profile_id,account_generation,scope_key) DO UPDATE
                 SET observed_ms=excluded.observed_ms,estimate=excluded.estimate
                 WHERE excluded.observed_ms>=auto_allowance_estimates.observed_ms",
                params![profile_id,generation,scope_key,estimate.observed_ms,encoded],
            )?;
            self.prune_auto_allowance_estimates(now_ms, 5000)?;
            Ok(changed != 0)
        })();
        match write {
            Ok(changed) => {
                self.learning_conn.execute_batch("RELEASE auto_allowance_estimate_write")?;
                Ok(changed)
            }
            Err(error) => {
                let _ = self.learning_conn.execute_batch(
                    "ROLLBACK TO auto_allowance_estimate_write; RELEASE auto_allowance_estimate_write");
                Err(error)
            }
        }
    }

    pub fn auto_allowance_estimate(&self, profile_id: &str, generation: i64,
        key: &crate::auto_consumption::EstimateKey, now_ms: i64)
        -> Result<Option<crate::auto_select::AllowanceEstimate>> {
        if !self.learning_persistent || self.learning_reset_pending()?
            || self.auto_account_generation(profile_id)? != Some(generation)
            || self.auto_account_pool_id(profile_id)?.as_deref() != Some(key.pool_id.as_str()) {
            return Ok(None);
        }
        let scope_key = serde_json::to_string(key)?;
        let encoded: Option<String> = self.learning_conn.query_row(
            "SELECT estimate FROM auto_allowance_estimates
             WHERE profile_id=?1 AND account_generation=?2 AND scope_key=?3
               AND observed_ms<=?4 AND observed_ms>?5",
            params![profile_id,generation,scope_key,now_ms,
                now_ms.saturating_sub(30 * 86_400_000_i64)], |row| row.get(0),
        ).optional()?;
        let Some(encoded) = encoded else { return Ok(None); };
        let estimate: crate::auto_select::AllowanceEstimate = serde_json::from_str(&encoded)?;
        if crate::auto_consumption::EstimateKey::from_estimate(&estimate).as_ref() != Some(key) {
            return Err(anyhow!("stored allowance estimate scope changed"));
        }
        Ok(Some(estimate))
    }

    pub fn prune_auto_allowance_estimates(&self, now_ms: i64, cap: i64) -> Result<usize> {
        let expired = self.learning_conn.execute(
            "DELETE FROM auto_allowance_estimates WHERE observed_ms < ?1",
            [now_ms.saturating_sub(30 * 86_400_000_i64)],
        )?;
        let cutoff: Option<(i64, i64)> = self.learning_conn.query_row(
            "SELECT observed_ms,rowid FROM auto_allowance_estimates
             ORDER BY observed_ms DESC,rowid DESC LIMIT 1 OFFSET ?1",
            [cap.clamp(1, 5000)], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let over_cap = match cutoff {
            Some((observed_ms, rowid)) => self.learning_conn.execute(
                "DELETE FROM auto_allowance_estimates WHERE observed_ms < ?1
                 OR (observed_ms=?1 AND rowid<=?2)", params![observed_ms,rowid],
            )?,
            None => 0,
        };
        Ok(expired + over_cap)
    }

    /// Work-level learning has the same 30-day detail horizon and an exact
    /// 5,000-row cap. Indexed cutoff deletion keeps maintenance bounded.
    pub fn prune_auto_work_observations(&self, now_ms: i64, cap: i64) -> Result<usize> {
        const THIRTY_DAYS_MS: i64 = 30 * 86_400_000;
        let expired = self.learning_conn.execute(
            "DELETE FROM auto_work_observations WHERE observed_ms < ?1",
            [now_ms.saturating_sub(THIRTY_DAYS_MS)])?;
        let cutoff: Option<(i64, String)> = self.learning_conn.query_row(
            "SELECT observed_ms,work_unit_id FROM auto_work_observations
             ORDER BY observed_ms DESC,work_unit_id DESC LIMIT 1 OFFSET ?1",
            [cap.clamp(1, 5000)], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let over_cap = match cutoff {
            Some((observed_ms, work_unit_id)) => self.learning_conn.execute(
                "DELETE FROM auto_work_observations WHERE observed_ms < ?1
                 OR (observed_ms=?1 AND work_unit_id<=?2)",
                params![observed_ms,work_unit_id])?,
            None => 0,
        };
        Ok(expired + over_cap)
    }

    /// Apply age and row bounds when the daemon opens or a user inspects learning,
    /// including periods with no new usage measurements.
    pub fn prune_auto_learning_history(&self, now_ms: i64) -> Result<()> {
        self.prune_auto_measurements(now_ms, 50_000)?;
        self.prune_auto_daily_aggregates(now_ms, 10_000)?;
        self.learning_conn.execute(
            "DELETE FROM auto_thread_usage_observations WHERE observed_ms < ?1",
            params![now_ms.saturating_sub(30 * 86_400_000_i64)],
        )?;
        self.prune_auto_work_observations(now_ms, 5000)?;
        self.prune_auto_allowance_estimates(now_ms, 5000)?;
        Ok(())
    }

    /// Keep detailed observations for 30 days and at most 50,000 rows. A caller
    /// may pass a lower cap for bounded fixture verification.
    pub fn prune_auto_measurements(&self, now_ms: i64, cap: i64) -> Result<usize> {
        const THIRTY_DAYS_MS: i64 = 30 * 86_400_000;
        let expired = self.learning_conn.execute(
            "DELETE FROM auto_measurements WHERE observed_ms < ?1",
            params![now_ms.saturating_sub(THIRTY_DAYS_MS)],
        )?;
        let cutoff: Option<i64> = self.learning_conn.query_row(
            "SELECT event_seq FROM auto_measurements ORDER BY event_seq DESC LIMIT 1 OFFSET ?1",
            params![cap.clamp(1, 50_000)], |row| row.get(0),
        ).optional()?;
        let over_cap = match cutoff {
            Some(event_seq) => self.learning_conn.execute(
                "DELETE FROM auto_measurements WHERE event_seq<=?1", params![event_seq])?,
            None => 0,
        };
        Ok(expired + over_cap)
    }

    pub fn auto_daily_aggregates(&self, limit: i64) -> Result<Vec<AutoDailyAggregate>> {
        if self.learning_reset_pending()? { return Ok(Vec::new()); }
        let mut stmt = self.learning_conn.prepare("SELECT * FROM auto_daily_aggregates ORDER BY day_ms DESC,harness,profile_id,model,effort LIMIT ?1")?;
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
        let expired = self.learning_conn.execute(
            "DELETE FROM auto_daily_aggregates WHERE last_observed_ms < ?1",
            params![now_ms.saturating_sub(NINETY_DAYS_MS)],
        )?;
        // Rowid breaks equal-timestamp ties, so the cutoff always leaves
        // exactly the newest `cap` scoped summaries.
        let cutoff: Option<(i64, i64)> = self.learning_conn.query_row(
            "SELECT last_observed_ms,rowid FROM auto_daily_aggregates
             ORDER BY last_observed_ms DESC,rowid DESC LIMIT 1 OFFSET ?1",
            params![cap.clamp(1, 10_000)], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let over_cap = match cutoff {
            Some((observed_ms, rowid)) => self.learning_conn.execute(
                "DELETE FROM auto_daily_aggregates WHERE last_observed_ms < ?1
                 OR (last_observed_ms=?1 AND rowid<=?2)", params![observed_ms, rowid])?,
            None => 0,
        };
        Ok(expired + over_cap)
    }

    pub fn clear_auto_learning(&self) -> Result<usize> {
        if !self.learning_persistent {
            self.conn.execute("INSERT OR REPLACE INTO meta(key,value) VALUES('auto_learning_reset_required','1')", [])?;
            self.conn.execute("DELETE FROM auto_run_account_evidence", [])?;
            return Err(anyhow!("Auto learning storage unavailable; deletion queued for recovery"));
        }
        let tx = self.learning_conn.unchecked_transaction()?;
        let deleted = tx.execute("DELETE FROM auto_measurements", [])?;
        tx.execute("DELETE FROM auto_daily_aggregates", [])?;
        tx.execute("DELETE FROM auto_thread_usage_observations", [])?;
        tx.execute("DELETE FROM auto_work_observations", [])?;
        tx.execute("DELETE FROM auto_allowance_estimates", [])?;
        tx.execute("DELETE FROM meta WHERE key='auto_learning_samples_inserted'", [])?;
        tx.commit()?;
        self.conn.execute("DELETE FROM auto_run_account_evidence", [])?;
        self.conn.execute("DELETE FROM meta WHERE key='auto_learning_reset_required'", [])?;
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
mod schema_migration_tests {
    use super::*;

    fn scoped_estimate(pool_id: &str, model_version: &str, observed_ms: i64)
        -> crate::auto_select::AllowanceEstimate {
        serde_json::from_value(serde_json::json!({
            "pool_id":pool_id,"model":"sol","effort":"medium",
            "model_version":model_version,"plan_type":"pro",
            "task_signature":{"min_tier":"general","required_tools":["browser"],
                "context_needed":1000,"requires_approvals":false,
                "min_sandbox":"read_only","max_sandbox":"workspace_write"},
            "source":"attributed_actual_work","observed_ms":observed_ms,
            "windows":[{"bucket_id":"codex","window":"primary","upper_percent":2.2}]
        })).unwrap()
    }

    #[test]
    fn learned_allowance_estimate_is_bound_to_account_generation_and_clear() {
        use crate::auto_consumption::EstimateKey;
        let store = Store::open(Path::new(":memory:")).unwrap();
        let now = crate::daemon::now();
        store.record_auto_account_identity("profile", &"a".repeat(64)).unwrap();
        let pool = store.auto_account_pool_id("profile").unwrap().unwrap();
        let estimate = scoped_estimate(&pool, "sol-v1", now);
        let key = EstimateKey::from_estimate(&estimate).unwrap();
        assert!(store.put_auto_allowance_estimate("profile", 1, &estimate, now).unwrap());
        let stored = store.auto_allowance_estimate("profile", 1, &key, now).unwrap().unwrap();
        assert_eq!(stored.windows[0].upper_percent, 2.2);
        assert!(store.auto_allowance_estimate("profile", 2, &key, now).unwrap().is_none());
        store.record_auto_account_identity("profile", &"b".repeat(64)).unwrap();
        assert!(store.auto_allowance_estimate("profile", 1, &key, now).unwrap().is_none());
        assert!(store.put_auto_allowance_estimate("profile", 2, &estimate, now).is_err(),
            "the old account pool cannot seed the new login");
        let new_pool = store.auto_account_pool_id("profile").unwrap().unwrap();
        let next = scoped_estimate(&new_pool, "sol-v1", now);
        let next_key = EstimateKey::from_estimate(&next).unwrap();
        assert!(store.put_auto_allowance_estimate("profile", 2, &next, now).unwrap());
        store.clear_auto_learning().unwrap();
        assert!(store.auto_allowance_estimate("profile", 2, &next_key, now).unwrap().is_none());
    }

    #[test]
    fn learned_allowance_estimates_obey_age_and_row_caps() {
        use crate::auto_consumption::EstimateKey;
        let store = Store::open(Path::new(":memory:")).unwrap();
        let now = crate::daemon::now();
        store.record_auto_account_identity("profile", &"a".repeat(64)).unwrap();
        let pool = store.auto_account_pool_id("profile").unwrap().unwrap();
        let mut keys = Vec::new();
        for index in 0..4 {
            let estimate = scoped_estimate(&pool, &format!("sol-v{index}"), now - 4 + index);
            keys.push(EstimateKey::from_estimate(&estimate).unwrap());
            store.put_auto_allowance_estimate("profile", 1, &estimate, now).unwrap();
        }
        assert_eq!(store.prune_auto_allowance_estimates(now, 2).unwrap(), 2);
        assert!(store.auto_allowance_estimate("profile", 1, &keys[0], now).unwrap().is_none());
        assert!(store.auto_allowance_estimate("profile", 1, &keys[3], now).unwrap().is_some());
        store.learning_conn.execute("UPDATE auto_allowance_estimates SET observed_ms=?1",
            [now - 31 * 86_400_000]).unwrap();
        assert!(store.auto_allowance_estimate("profile", 1, &keys[3], now).unwrap().is_none());
        assert_eq!(store.prune_auto_allowance_estimates(now, 2).unwrap(), 2);
    }

    #[test]
    fn work_learning_uses_fake_clock_expiry_and_exact_row_cap() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let now = 1_800_000_000_000_i64;
        for (id, observed) in [
            ("expired", now - 31 * 86_400_000),
            ("older", now - 5_000),
            ("middle", now - 3_000),
            ("newest", now - 1_000),
        ] {
            store.learning_conn.execute(
                "INSERT INTO auto_work_observations(work_unit_id,run_id,profile_id,observed_ms,record)
                 VALUES(?1,?2,'profile',?3,?4)",
                params![id,id,observed,serde_json::json!({"work_unit_id":id}).to_string()],
            ).unwrap();
        }
        assert_eq!(store.prune_auto_work_observations(now, 2).unwrap(), 2);
        let rows = store.auto_work_observations(10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["work_unit_id"], "newest");
        assert_eq!(rows[1]["work_unit_id"], "middle");
    }

    #[test]
    fn work_history_read_expires_rows_without_a_new_work_unit_and_reopen_prunes_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        let expired = crate::daemon::now() - 31 * 86_400_000;
        store.learning_conn.execute(
            "INSERT INTO auto_work_observations(work_unit_id,run_id,profile_id,observed_ms,record)
             VALUES('old','old','profile',?1,'{\"work_unit_id\":\"old\"}')",
            [expired],
        ).unwrap();
        assert!(store.auto_work_observations(10).unwrap().is_empty());
        let remaining: i64 = store.learning_conn.query_row(
            "SELECT COUNT(*) FROM auto_work_observations", [], |row| row.get(0)).unwrap();
        assert_eq!(remaining, 0, "a read must remove expired rows from disk");
        store.learning_conn.execute(
            "INSERT INTO auto_work_observations(work_unit_id,run_id,profile_id,observed_ms,record)
             VALUES('old','old','profile',?1,'{\"work_unit_id\":\"old\"}')",
            [expired],
        ).unwrap();
        drop(store);
        let reopened = Store::open(&path).unwrap();
        let remaining: i64 = reopened.learning_conn.query_row(
            "SELECT COUNT(*) FROM auto_work_observations", [], |row| row.get(0)).unwrap();
        assert_eq!(remaining, 0, "startup must enforce retention even without a read");
    }

    #[test]
    fn oversized_existing_learning_database_pauses_learning_without_stopping_execution() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        drop(Store::open(&path).unwrap());
        let learning_path = dir.path().join("state.sqlite.learning");
        let learning = Connection::open(&learning_path).unwrap();
        learning.execute_batch("CREATE TABLE oversized_payload(value BLOB)").unwrap();
        learning.execute(
            "INSERT INTO oversized_payload(value) VALUES(zeroblob(?1))",
            params![129 * 1024 * 1024],
        ).unwrap();
        let page_size: i64 = learning.pragma_query_value(None, "page_size", |row| row.get(0)).unwrap();
        let page_count: i64 = learning.pragma_query_value(None, "page_count", |row| row.get(0)).unwrap();
        assert!(page_size * page_count > 128 * 1024 * 1024);
        drop(learning);

        let store = Store::open(&path).unwrap();
        assert!(!store.learning_persistent, "an oversized file must not silently bypass the cap");
        assert!(store.auto_learning_is_paused().unwrap());
        store.conn.execute("INSERT INTO meta(key,value) VALUES('execution-after-oversize','ok')", []).unwrap();
        assert_eq!(store.conn.query_row(
            "SELECT value FROM meta WHERE key='execution-after-oversize'", [], |row| row.get::<_, String>(0)
        ).unwrap(), "ok");
    }

    #[test]
    fn failed_account_cleanup_hides_learning_and_reports_pause_until_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        store.record_auto_account_identity("p", &"a".repeat(64)).unwrap();
        let measurement = crate::auto_telemetry::from_usage(1000, "task", "run", "codex",
            Some("p"), None, &serde_json::json!({"input_tokens":1})).unwrap();
        store.insert_auto_measurement(1, &measurement).unwrap();
        store.learning_conn.execute_batch("CREATE TRIGGER block_learning_delete BEFORE DELETE ON auto_measurements
            BEGIN SELECT RAISE(FAIL,'learning cleanup blocked'); END;").unwrap();
        assert!(store.record_auto_account_identity("p", &"b".repeat(64)).unwrap());
        assert!(store.auto_learning_is_paused().unwrap());
        assert!(store.auto_measurements(10).unwrap().is_empty(),
            "old-account measurements must be hidden while cleanup is pending");
        drop(store);
        let learning = Connection::open(dir.path().join("state.sqlite.learning")).unwrap();
        learning.execute_batch("DROP TRIGGER block_learning_delete").unwrap();
        drop(learning);
        let recovered = Store::open(&path).unwrap();
        assert!(!recovered.auto_learning_is_paused().unwrap());
        assert!(recovered.auto_measurements(10).unwrap().is_empty());
    }

    #[test]
    fn account_switch_while_learning_is_unavailable_cannot_restore_old_samples() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let learning_path = dir.path().join("state.sqlite.learning");
        let saved_path = dir.path().join("saved-learning.sqlite");
        let store = Store::open(&path).unwrap();
        store.record_auto_account_identity("p", &"a".repeat(64)).unwrap();
        let measurement = crate::auto_telemetry::from_usage(1000, "task", "run", "codex",
            Some("p"), None, &serde_json::json!({"input_tokens":1})).unwrap();
        store.insert_auto_measurement(1, &measurement).unwrap();
        drop(store);
        std::fs::rename(&learning_path, &saved_path).unwrap();
        std::fs::create_dir(&learning_path).unwrap();
        let unavailable = Store::open(&path).unwrap();
        assert!(unavailable.record_auto_account_identity("p", &"b".repeat(64)).unwrap());
        drop(unavailable);
        std::fs::remove_dir(&learning_path).unwrap();
        std::fs::rename(&saved_path, &learning_path).unwrap();
        let recovered = Store::open(&path).unwrap();
        assert!(recovered.auto_measurements(10).unwrap().is_empty(),
            "the old account's learning must not return after storage recovers");
    }

    #[test]
    fn unavailable_learning_file_does_not_stop_execution_storage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        drop(Store::open(&path).unwrap());
        let learning_path = dir.path().join("state.sqlite.learning");
        std::fs::remove_file(&learning_path).unwrap();
        std::fs::create_dir(&learning_path).unwrap();
        let store = Store::open(&path).expect("learning storage failure must not block execution startup");
        let event = store.insert_event(1000, None, None, "status", "test", "exact",
            &serde_json::json!({"message":"execution survived"})).unwrap();
        assert_eq!(event.seq, 1);
        let measurement = crate::auto_telemetry::from_usage(1000, "task", "run", "codex",
            None, None, &serde_json::json!({"input_tokens":1})).unwrap();
        assert!(store.insert_auto_measurement(2, &measurement).is_err(),
            "unavailable learning file must not silently become an ephemeral estimate");
        assert!(store.auto_measurements(10).unwrap().is_empty());
    }

    #[test]
    fn legacy_learning_rows_move_once_and_clear_cannot_reimport_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let legacy = Connection::open(&path).unwrap();
        let observed = crate::daemon::now();
        let day = observed.div_euclid(86_400_000) * 86_400_000;
        legacy.execute_batch(&format!("CREATE TABLE auto_measurements(
            event_seq INTEGER PRIMARY KEY, observed_ms INTEGER NOT NULL, task_id TEXT NOT NULL,
            run_id TEXT NOT NULL, harness TEXT NOT NULL, profile_id TEXT, model TEXT,
            input_tokens INTEGER, output_tokens INTEGER, cached_input_tokens INTEGER,
            reasoning_output_tokens INTEGER, cost_usd REAL);
            INSERT INTO auto_measurements VALUES(7,{observed},'t','r','codex','p','model',42,1,NULL,NULL,NULL);
            CREATE TABLE auto_daily_aggregates(
            day_ms INTEGER NOT NULL, harness TEXT NOT NULL, profile_id TEXT NOT NULL,
            model TEXT NOT NULL, last_observed_ms INTEGER NOT NULL, samples INTEGER NOT NULL,
            input_observations INTEGER NOT NULL, input_tokens INTEGER NOT NULL,
            output_observations INTEGER NOT NULL, output_tokens INTEGER NOT NULL,
            cached_input_observations INTEGER NOT NULL, cached_input_tokens INTEGER NOT NULL,
            reasoning_output_observations INTEGER NOT NULL, reasoning_output_tokens INTEGER NOT NULL,
            cost_observations INTEGER NOT NULL, cost_usd REAL NOT NULL,
            PRIMARY KEY(day_ms,harness,profile_id,model));
            INSERT INTO auto_daily_aggregates VALUES({day},'codex','p','model',{observed},1,1,42,1,1,0,0,0,0,0,0);")).unwrap();
        drop(legacy);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.auto_measurements(10).unwrap().len(), 1);
        assert_eq!(store.auto_daily_aggregates(10).unwrap()[0].input_tokens, Some(42));
        assert_eq!(store.conn.query_row("SELECT COUNT(*) FROM auto_measurements", [],
            |row| row.get::<_, i64>(0)).unwrap(), 0);
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.auto_daily_aggregates(10).unwrap()[0].samples, 1);
        assert_eq!(store.clear_auto_learning().unwrap(), 1);
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert!(reopened.auto_measurements(10).unwrap().is_empty());
        assert!(reopened.auto_daily_aggregates(10).unwrap().is_empty());
    }

    #[test]
    fn later_thread_credit_correction_replaces_the_run_sample() {
        use crate::auto_consumption::{CreditGroup, ThreadUsageEstimate};
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        let group = CreditGroup { model:Some("gpt-6-sol".into()), effort:Some("medium".into()),
            estimated_credits_micros:1_000, input_tokens:None, output_tokens:None,
            cached_input_tokens:None, net_new_input_tokens:None, total_tokens:None };
        let now = crate::daemon::now();
        let first = ThreadUsageEstimate { observed_ms:now, plan_type:None,
            estimated_credits_micros:1_000, groups:vec![group.clone()] };
        let first_id = store.insert_auto_thread_usage("run", "profile", 1, "codex-app/account-usage-read", &first).unwrap();
        let corrected = ThreadUsageEstimate { observed_ms:now + 1, plan_type:None,
            estimated_credits_micros:2_000,
            groups:vec![CreditGroup { estimated_credits_micros:2_000, ..group }] };
        let corrected_id = store.insert_auto_thread_usage("run", "profile", 1,
            "codex-app/account-usage-read", &corrected).unwrap();
        assert_eq!(first_id, corrected_id, "one run and account generation has one cumulative sample");
        let rows = store.auto_thread_usage_observations(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].estimate.estimated_credits_micros, 2_000);
        assert_eq!(rows[0].estimate.observed_ms, now + 1);
    }

    #[test]
    fn corrected_thread_credit_sample_survives_the_next_row_cap_insert() {
        use crate::auto_consumption::ThreadUsageEstimate;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        let now = crate::daemon::now();
        let estimate = ThreadUsageEstimate { observed_ms:now, plan_type:None,
            estimated_credits_micros:0, groups:Vec::new() };
        let encoded = serde_json::to_string(&estimate).unwrap();
        store.learning_conn.execute(
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<5000)
             INSERT INTO auto_thread_usage_observations
             (run_id,profile_id,read_account_generation,attribution,observed_ms,source,estimate)
             SELECT CASE WHEN x=1 THEN 'target' ELSE 'other-'||x END,
               'profile',1,'unverified_run_account',?1+x,'fixture',?2 FROM n",
            params![now, encoded],
        ).unwrap();
        let corrected = ThreadUsageEstimate { observed_ms:now+6_000, plan_type:None,
            estimated_credits_micros:0, groups:Vec::new() };
        store.insert_auto_thread_usage("target", "profile", 1,
            "codex-app/account-usage-read", &corrected).unwrap();
        let fresh = ThreadUsageEstimate { observed_ms:now+6_001, plan_type:None,
            estimated_credits_micros:0, groups:Vec::new() };
        store.insert_auto_thread_usage("fresh", "profile", 1,
            "codex-app/account-usage-read", &fresh).unwrap();
        let count: i64 = store.learning_conn.query_row(
            "SELECT COUNT(*) FROM auto_thread_usage_observations", [], |row| row.get(0)).unwrap();
        let target: i64 = store.learning_conn.query_row(
            "SELECT COUNT(*) FROM auto_thread_usage_observations WHERE run_id='target'", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 5_000);
        assert_eq!(target, 1, "a freshly corrected sample cannot be evicted as the oldest ID");
    }

    #[test]
    fn full_execution_store_rolls_back_auto_admission_before_any_launch() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        store.conn.execute_batch("INSERT INTO workspaces
            (id,path,repo_root,common_dir,kind,initial_dirty,created_ms)
            VALUES('w','/tmp','/tmp','/tmp','current','{}',0);
            INSERT INTO tasks(id,title,prompt,repo_root,workspace_id,created_ms)
            VALUES('t','task','prompt','/tmp','w',0);
            INSERT INTO runs(id,task_id,harness,workspace_id,status,created_ms,title,capabilities)
            VALUES('parent','t','codex-app','w','completed',0,'parent','{}');").unwrap();
        let parent = store.run("parent").unwrap().unwrap();
        let original_pages: i64 = store.conn.pragma_query_value(None, "page_count", |row| row.get(0)).unwrap();
        store.conn.pragma_update(None, "max_page_count", original_pages).unwrap();
        // Prove the first admission write still fits: the full error must
        // occur after an intent was inserted into the selected transaction.
        store.conn.execute_batch("SAVEPOINT admission_probe").unwrap();
        store.insert_auto_launch_intent("probe", "parent", "hash", "route", None).unwrap();
        store.conn.execute_batch("ROLLBACK TO admission_probe; RELEASE admission_probe").unwrap();
        let oversized_trace = serde_json::json!({"selection_input":"x".repeat(1024 * 1024)});
        let error = store.insert_auto_selected_decision("unit", &parent, "hash", "route", "pool", None,
            &oversized_trace).unwrap_err();
        assert!(matches!(error.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::SqliteFailure(code, _)) if code.code == rusqlite::ErrorCode::DiskFull),
            "physical main-store pressure must return SQLITE_FULL: {error:#}");
        assert!(store.auto_launch_intent("unit").unwrap().is_none(),
            "a failed decision write cannot leave a launch claim");
        assert!(!store.auto_pool_claimed("pool").unwrap(),
            "a failed decision write cannot reserve allowance");
        let decisions: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM events WHERE kind='auto_decision'", [], |row| row.get(0)).unwrap();
        assert_eq!(decisions, 0);

        store.conn.pragma_update(None, "max_page_count", original_pages + 1024).unwrap();
        store.insert_auto_selected_decision("unit", &parent, "hash", "route", "pool", None,
            &serde_json::json!({"selected":"route"})).unwrap();
        assert!(store.auto_launch_intent("unit").unwrap().is_some());
        let decisions: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM events WHERE kind='auto_decision'", [], |row| row.get(0)).unwrap();
        assert_eq!(decisions, 1, "recovery must not replay a failed admission");
    }

    #[test]
    fn unknown_pool_claim_is_atomic_persistent_and_released_only_after_settlement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        store.conn.execute_batch("INSERT INTO workspaces
            (id,path,repo_root,common_dir,kind,initial_dirty,created_ms)
            VALUES('w','/tmp','/tmp','/tmp','current','{}',0);
            INSERT INTO tasks(id,title,prompt,repo_root,workspace_id,created_ms)
            VALUES('t','task','prompt','/tmp','w',0);
            INSERT INTO runs(id,task_id,harness,workspace_id,status,created_ms,title,capabilities)
            VALUES('parent','t','codex-app','w','completed',0,'parent','{}');").unwrap();
        let parent = store.run("parent").unwrap().unwrap();
        assert!(store.insert_auto_selected_decision("one", &parent, "hash", "route-a",
            "account/shared", None, &serde_json::json!({"unit":"one"})).unwrap().is_some());
        assert!(store.auto_pool_claimed("account/shared").unwrap());
        assert!(store.insert_auto_selected_decision("two", &parent, "hash", "route-b",
            "account/shared", None, &serde_json::json!({"unit":"two"})).unwrap().is_none());
        assert!(store.auto_launch_intent("two").unwrap().is_none());
        assert!(store.insert_auto_selected_decision("three", &parent, "hash", "route-c",
            "account/independent", None, &serde_json::json!({"unit":"three"})).unwrap().is_some());
        store.set_auto_launch_intent_phase("one", "paused").unwrap();
        drop(store);

        let reopened = Store::open(&path).unwrap();
        assert!(reopened.auto_pool_claimed("account/shared").unwrap(),
            "a launch with uncertain effects must continue blocking after restart");
        assert!(reopened.insert_auto_selected_decision("two", &parent, "hash", "route-b",
            "account/shared", None, &serde_json::json!({"unit":"two"})).unwrap().is_none());
        reopened.journal_auto_launch_worktree("three", "overseer/test", "/tmp/test",
            "snapshot", "commit").unwrap();
        assert!(reopened.release_unstarted_auto_pool_claim("three").unwrap(),
            "uncertain Git state without a child cannot have consumed model allowance");
        assert!(!reopened.auto_pool_claimed("account/independent").unwrap());
        assert!(reopened.auto_launch_resources("three").unwrap().is_some(),
            "releasing allowance must retain the uncertain Git resource for inspection");
        reopened.conn.execute_batch("INSERT INTO runs(id,task_id,parent_run_id,harness,workspace_id,
            status,created_ms,ended_ms,title,capabilities)
            VALUES('child','t','parent','codex-app','w','completed',1,2,'child','{}');
            INSERT INTO managed_work_units(work_unit_id,parent_run_id,child_run_id,request_hash,created_ms)
            VALUES('one','parent','child','request-hash',1);").unwrap();
        reopened.release_settled_auto_pool_claim("child").unwrap();
        assert!(!reopened.auto_pool_claimed("account/shared").unwrap());
        assert!(reopened.insert_auto_selected_decision("two", &parent, "hash", "route-b",
            "account/shared", None, &serde_json::json!({"unit":"two"})).unwrap().is_some());
        assert!(reopened.release_unstarted_auto_pool_claim("two").unwrap(),
            "a worker proven never spawned may release its claim");
    }

    #[test]
    fn interrupted_learning_migration_retries_without_doubling_aggregates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        let observed = crate::daemon::now();
        let day = observed.div_euclid(86_400_000) * 86_400_000;
        store.conn.execute_batch(&format!("DELETE FROM meta WHERE key='auto_learning_split_migrated';
            INSERT INTO auto_measurements(event_seq,observed_ms,task_id,run_id,harness,profile_id,model,effort,input_tokens)
              VALUES(7,{observed},'t','r','codex','p','model','',42);
            INSERT INTO auto_daily_aggregates VALUES({day},'codex','p','model','',{observed},1,1,42,1,1,0,0,0,0,0,0);
            CREATE TRIGGER stop_learning_marker BEFORE INSERT ON meta
              WHEN NEW.key='auto_learning_split_migrated'
              BEGIN SELECT RAISE(FAIL,'injected migration marker failure'); END;")).unwrap();
        drop(store);

        let interrupted = Store::open(&path).unwrap();
        assert!(interrupted.auto_learning_is_paused().unwrap());
        assert_eq!(interrupted.conn.query_row("SELECT COUNT(*) FROM auto_measurements", [],
            |row| row.get::<_, i64>(0)).unwrap(), 1,
            "legacy rows cannot be deleted before the durable marker commits");
        drop(interrupted);
        let learning = Connection::open(dir.path().join("state.sqlite.learning")).unwrap();
        assert_eq!(learning.query_row("SELECT COUNT(*) FROM auto_measurements", [],
            |row| row.get::<_, i64>(0)).unwrap(), 1,
            "the learning copy commits before the main marker");
        drop(learning);

        let main = Connection::open(&path).unwrap();
        main.execute_batch("DROP TRIGGER stop_learning_marker").unwrap();
        drop(main);
        for _ in 0..2 {
            let recovered = Store::open(&path).unwrap();
            assert!(!recovered.auto_learning_is_paused().unwrap());
            assert_eq!(recovered.auto_measurements(10).unwrap().len(), 1);
            assert_eq!(recovered.auto_daily_aggregates(10).unwrap()[0].samples, 1,
                "retry must not double an already copied aggregate");
            assert_eq!(recovered.conn.query_row("SELECT COUNT(*) FROM auto_measurements", [],
                |row| row.get::<_, i64>(0)).unwrap(), 0);
        }
    }

    #[test]
    fn prior_auto_launch_intent_gains_a_journal_without_losing_the_claim() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE auto_launch_intents(
            work_unit_id TEXT PRIMARY KEY, parent_run_id TEXT NOT NULL,
            requirements_hash TEXT NOT NULL, route_id TEXT NOT NULL,
            account_generation INTEGER, phase TEXT NOT NULL, created_ms INTEGER NOT NULL);
            INSERT INTO auto_launch_intents VALUES('browser-1','parent-1','hash-1',
                'system-codex/gpt-6-sol/medium',4,'preparing',1000);").unwrap();
        let store = Store { conn, learning_conn: Connection::open_in_memory().unwrap(), learning_persistent: true };
        store.migrate().unwrap();
        store.migrate().unwrap();
        assert_eq!(store.auto_launch_intent("browser-1").unwrap().unwrap().3, Some(4));
        store.journal_auto_launch_worktree("browser-1", "overseer/browser-1",
            "/tmp/browser-1", "snapshot-1", "commit-1").unwrap();
        assert_eq!(store.auto_launch_resources("browser-1").unwrap(),
            Some(("overseer/browser-1".into(), "/tmp/browser-1".into(),
                "snapshot-1".into(), "commit-1".into())));
    }

    #[test]
    fn upgrade_conservatively_claims_active_legacy_auto_children_until_settlement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        store.conn.execute_batch("INSERT INTO workspaces
            (id,path,repo_root,common_dir,kind,initial_dirty,created_ms)
            VALUES('w','/tmp','/tmp','/tmp','current','{}',0);
            INSERT INTO tasks(id,title,prompt,repo_root,workspace_id,created_ms)
            VALUES('t','task','prompt','/tmp','w',0);
            INSERT INTO runs(id,task_id,harness,workspace_id,status,created_ms,title,capabilities)
            VALUES('parent','t','codex-app','w','completed',0,'parent','{}');
            INSERT INTO runs(id,task_id,parent_run_id,harness,profile_id,workspace_id,status,
                created_ms,title,capabilities,process_generation)
            VALUES('child','t','parent','codex-app','profile-a','w','running',1,'child','{}',1);
            INSERT INTO auto_launch_intents(work_unit_id,parent_run_id,requirements_hash,route_id,
                account_generation,phase,created_ms)
            VALUES('legacy-unit','parent','hash','profile-a/gpt-6-sol/medium',1,'child_created',1);
            INSERT INTO managed_work_units(work_unit_id,parent_run_id,child_run_id,request_hash,created_ms)
            VALUES('legacy-unit','parent','child','request-hash',1);
            DROP TABLE auto_pool_claims;
            UPDATE meta SET value='14' WHERE key='schema_version';").unwrap();
        drop(store);

        let upgraded = Store::open(&path).unwrap();
        assert!(upgraded.auto_pool_claimed("account/shared").unwrap(),
            "a pre-upgrade active child must block new unknown-draw admissions");
        upgraded.migrate().unwrap();
        let claims: i64 = upgraded.conn.query_row("SELECT COUNT(*) FROM auto_pool_claims",
            [], |row| row.get(0)).unwrap();
        assert_eq!(claims, 1, "backfill must be idempotent");
        upgraded.conn.execute("UPDATE runs SET status='completed',ended_ms=2 WHERE id='child'", []).unwrap();
        upgraded.release_settled_auto_pool_claim("child").unwrap();
        assert!(!upgraded.auto_pool_claimed("account/shared").unwrap());
    }

    #[test]
    fn prior_run_account_evidence_gains_unknown_plan_without_losing_account_history() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE auto_run_account_evidence(
            run_id TEXT PRIMARY KEY, profile_id TEXT NOT NULL,
            first_generation INTEGER NOT NULL, last_generation INTEGER NOT NULL,
            observed_turns INTEGER NOT NULL, consistent INTEGER NOT NULL);
            INSERT INTO auto_run_account_evidence VALUES('run-1','profile-1',2,2,1,1);").unwrap();
        let store = Store { conn, learning_conn: Connection::open_in_memory().unwrap(), learning_persistent: true };
        store.migrate().unwrap();
        store.migrate().unwrap();
        let row: (i64, Option<String>, i64) = store.conn.query_row(
            "SELECT first_generation,plan_type,plan_consistent FROM auto_run_account_evidence WHERE run_id='run-1'",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(row, (2, None, 0), "old account evidence must not become proven plan evidence");
    }

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
        let store = Store { conn, learning_conn: Connection::open_in_memory().unwrap(), learning_persistent: true };
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
        let store = Store { conn, learning_conn: Connection::open_in_memory().unwrap(), learning_persistent: true };
        store.migrate().unwrap();
        store.migrate().unwrap();
        let rows = store.auto_daily_aggregates(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].samples, 1);
        assert_eq!(rows[0].input_tokens, Some(42));
        assert_eq!(rows[0].effort, None);
    }

    #[test]
    fn existing_managed_work_units_gain_result_marker_without_losing_identity() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE managed_work_units(
            work_unit_id TEXT PRIMARY KEY, parent_run_id TEXT NOT NULL,
            child_run_id TEXT NOT NULL UNIQUE, request_hash TEXT NOT NULL,
            created_ms INTEGER NOT NULL);
            INSERT INTO managed_work_units VALUES('browser-1','parent-1','child-1','hash-1',1000);").unwrap();
        let store = Store { conn, learning_conn: Connection::open_in_memory().unwrap(), learning_persistent: true };
        store.migrate().unwrap();
        store.migrate().unwrap();
        let (child, marker): (String, Option<i64>) = store.conn.query_row(
            "SELECT child_run_id,result_event_seq FROM managed_work_units WHERE work_unit_id='browser-1'",
            [], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(child, "child-1");
        assert_eq!(marker, None);
    }

    #[test]
    fn result_publication_marker_does_not_block_bounded_parent_event_retention() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        // Only the event/marker relationship matters here; run foreign keys
        // are disabled while inserting the minimal fixture identities.
        store.conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        store.conn.execute("INSERT INTO managed_work_units(
            work_unit_id,parent_run_id,child_run_id,request_hash,created_ms)
            VALUES('browser-1','parent-1','child-1','hash-1',1000)", []).unwrap();
        store.conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        let notice = store.insert_event(1000, None, Some("parent-1"),
            "managed_child_result_available", "daemon", "exact", &serde_json::json!({})).unwrap();
        store.conn.execute("UPDATE managed_work_units SET result_event_seq=?1 WHERE work_unit_id='browser-1'",
            params![notice.seq]).unwrap();
        assert_eq!(store.prune_run_events("parent-1", 0).unwrap(), Some(notice.seq));
        assert!(store.events_after(0, Some("parent-1"), 10).unwrap().is_empty());
        let marker: Option<i64> = store.conn.query_row(
            "SELECT result_event_seq FROM managed_work_units WHERE work_unit_id='browser-1'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(marker, Some(notice.seq), "publication remains idempotent after event retention");
    }
}

#[cfg(test)]
mod auto_measurement_tests {
    use super::*;
    use crate::auto_telemetry::{from_usage, from_usage_with_effort};
    use serde_json::json;

    #[test]
    fn reopening_after_inactivity_expires_learning_without_touching_execution_history() {
        const DAY: i64 = 86_400_000;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let now = crate::daemon::now();
        let store = Store::open(&path).unwrap();
        store.insert_event(now - 91 * DAY, None, None, "usage", "fixture", "exact", &json!({"input_tokens": 1})).unwrap();
        for (id, observed) in [(1, now - 31 * DAY), (2, now - 29 * DAY)] {
            store.learning_conn.execute(
                "INSERT INTO auto_measurements(event_seq,observed_ms,task_id,run_id,harness) VALUES(?1,?2,'task','run','codex')",
                params![id, observed],
            ).unwrap();
            store.learning_conn.execute(
                "INSERT INTO auto_thread_usage_observations(run_id,profile_id,read_account_generation,attribution,observed_ms,source,estimate) VALUES('run','profile',1,'unverified',?1,'fixture','{}')",
                params![observed],
            ).unwrap();
        }
        for (day, observed) in [(1, now - 91 * DAY), (2, now - 89 * DAY)] {
            store.learning_conn.execute(
                "INSERT INTO auto_daily_aggregates(day_ms,harness,profile_id,model,effort,last_observed_ms,samples,input_observations,input_tokens,output_observations,output_tokens,cached_input_observations,cached_input_tokens,reasoning_output_observations,reasoning_output_tokens,cost_observations,cost_usd) VALUES(?1,'codex','','','',?2,1,0,0,0,0,0,0,0,0,0,0)",
                params![day, observed],
            ).unwrap();
        }
        drop(store);

        let reopened = Store::open(&path).unwrap();
        for table in ["auto_measurements", "auto_daily_aggregates", "auto_thread_usage_observations"] {
            let count: i64 = reopened.learning_conn.query_row(
                &format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0),
            ).unwrap();
            assert_eq!(count, 1, "{table} must retain only its unexpired row");
        }
        assert_eq!(reopened.events_after(0, None, 10).unwrap().len(), 1);
    }

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
    fn late_quota_event_cannot_replace_newer_account_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        let pool = "system-codex";
        let newer_ms = 1_800_000_000_000_i64;
        let newer = crate::auto_quota::parse_codex_rate_limits(&json!({
            "ordinaryUsageAllowed":true,
            "rateLimits":{"primary":{"usedPercent":20,"resetsAt":1800003600}}
        }), pool, newer_ms).unwrap();
        let newer_event = store.insert_event(newer_ms, None, None, "quota", "fixture", "reported", &json!({})).unwrap();
        store.insert_auto_quota(newer_event.seq, pool, "fixture", &newer).unwrap();

        let older_ms = newer_ms - 10_000;
        let older = crate::auto_quota::parse_codex_rate_limits(&json!({
            "ordinaryUsageAllowed":false,
            "rateLimits":{"primary":{"usedPercent":100,"resetsAt":1800003600}}
        }), pool, older_ms).unwrap();
        let late_event = store.insert_event(newer_ms + 1, None, None, "quota", "fixture", "reported", &json!({})).unwrap();
        store.insert_auto_quota(late_event.seq, pool, "fixture", &older).unwrap();

        let current = store.latest_auto_quota(pool).unwrap().unwrap();
        assert_eq!(current.event_seq, newer_event.seq);
        assert_eq!(current.snapshot.state_for("gpt-6-sol", newer_ms),
            crate::auto_quota::QuotaState::ObservedNonExhausted);
        assert_eq!(store.auto_quotas(10).unwrap().len(), 2,
            "late evidence remains inspectable without controlling admission");
        assert_eq!(store.prune_auto_quotas(newer_ms + 1, 1).unwrap(), 1);
        assert_eq!(store.latest_auto_quota(pool).unwrap().unwrap().event_seq, newer_event.seq,
            "bounded retention must keep the latest observation time");
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
    fn inserted_learning_never_exceeds_its_detail_or_summary_row_limits() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
        let now = crate::daemon::now();
        store.learning_conn.execute(
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<50000)
             INSERT INTO auto_measurements(event_seq,observed_ms,task_id,run_id,harness,input_tokens)
             SELECT x,?1,'task','run','codex',1 FROM n",
            params![now],
        ).unwrap();
        store.learning_conn.execute(
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10000)
             INSERT INTO auto_daily_aggregates(day_ms,harness,profile_id,model,effort,last_observed_ms,samples,
               input_observations,input_tokens,output_observations,output_tokens,cached_input_observations,
               cached_input_tokens,reasoning_output_observations,reasoning_output_tokens,cost_observations,cost_usd)
             SELECT 0,'codex','','model-'||x,'',?1,1,1,1,0,0,0,0,0,0,0,0 FROM n",
            params![now],
        ).unwrap();
        let measurement = from_usage(now, "task", "run", "codex", None, Some("new-model"),
            &json!({"input_tokens": 1})).unwrap();
        let maintenance_start = std::time::Instant::now();
        assert!(store.insert_auto_measurement(50_001, &measurement).unwrap());
        eprintln!("at-cap Auto learning write: {:?}", maintenance_start.elapsed());
        let details: i64 = store.learning_conn.query_row("SELECT COUNT(*) FROM auto_measurements", [], |row| row.get(0)).unwrap();
        let summaries: i64 = store.learning_conn.query_row("SELECT COUNT(*) FROM auto_daily_aggregates", [], |row| row.get(0)).unwrap();
        assert_eq!(details, 50_000);
        assert_eq!(summaries, 10_000);
        assert!(store.auto_measurements(1).unwrap().iter().any(|row| row.event_seq == 50_001));
        assert!(store.auto_daily_aggregates(10_000).unwrap().iter().any(|row| row.model.as_deref() == Some("new-model")));
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
