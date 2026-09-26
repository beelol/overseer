use anyhow::Result;
use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS swarm_runs(
          id TEXT PRIMARY KEY,
          category TEXT NOT NULL,
          category_key TEXT NOT NULL,
          objective TEXT NOT NULL,
          status TEXT NOT NULL,
          generation INTEGER NOT NULL,
          revision INTEGER NOT NULL,
          allowed_targets TEXT NOT NULL,
          policy TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS swarm_active_category
          ON swarm_runs(category_key)
          WHERE status IN ('planning','running','paused','stalled','stopping');
        CREATE TABLE IF NOT EXISTS swarm_jobs(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id) ON DELETE CASCADE,
          id TEXT NOT NULL,
          plan_revision INTEGER NOT NULL,
          title TEXT NOT NULL,
          acceptance TEXT NOT NULL,
          deps TEXT NOT NULL,
          status TEXT NOT NULL,
          attempt_count INTEGER NOT NULL DEFAULT 0,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,id)
        );
        CREATE INDEX IF NOT EXISTS swarm_jobs_page ON swarm_jobs(run_id,id);
        CREATE TABLE IF NOT EXISTS swarm_attempts(
          id TEXT PRIMARY KEY,
          run_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          revision INTEGER NOT NULL,
          token_sha256 TEXT NOT NULL,
          status TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          FOREIGN KEY(run_id,job_id) REFERENCES swarm_jobs(run_id,id)
        );
        CREATE INDEX IF NOT EXISTS swarm_attempts_job ON swarm_attempts(run_id,job_id);
        CREATE TABLE IF NOT EXISTS swarm_messages(
          seq INTEGER PRIMARY KEY AUTOINCREMENT,
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          message_id TEXT NOT NULL,
          job_id TEXT,
          attempt_id TEXT,
          sender TEXT NOT NULL,
          recipient TEXT NOT NULL,
          kind TEXT NOT NULL,
          revision INTEGER NOT NULL,
          payload TEXT NOT NULL,
          phase TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL,
          UNIQUE(run_id,message_id)
        );
        CREATE INDEX IF NOT EXISTS swarm_inbox ON swarm_messages(run_id,recipient,seq);
        CREATE TABLE IF NOT EXISTS swarm_claims(
          resource TEXT NOT NULL,
          run_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          mode TEXT NOT NULL CHECK(mode IN ('read','write')),
          status TEXT NOT NULL CHECK(status IN ('active','released')),
          revision INTEGER NOT NULL,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL,
          PRIMARY KEY(resource,run_id,job_id),
          FOREIGN KEY(run_id,job_id) REFERENCES swarm_jobs(run_id,id)
        );
        CREATE INDEX IF NOT EXISTS swarm_claims_active ON swarm_claims(resource,status);
        CREATE TABLE IF NOT EXISTS swarm_artifacts(
          id TEXT NOT NULL,
          run_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          attempt_id TEXT NOT NULL REFERENCES swarm_attempts(id),
          source_revision INTEGER NOT NULL,
          kind TEXT NOT NULL,
          content TEXT NOT NULL,
          sha256 TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,id),
          FOREIGN KEY(run_id,job_id) REFERENCES swarm_jobs(run_id,id)
        );
        CREATE TABLE IF NOT EXISTS swarm_decisions(
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          run_id TEXT NOT NULL,
          job_id TEXT NOT NULL,
          attempt_id TEXT NOT NULL,
          revision INTEGER NOT NULL,
          decision TEXT NOT NULL,
          evidence TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          UNIQUE(run_id,job_id,attempt_id,decision)
        );
        CREATE TABLE IF NOT EXISTS swarm_director_turns(
          id TEXT PRIMARY KEY,
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          generation INTEGER NOT NULL,
          revision INTEGER NOT NULL,
          token_sha256 TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('active','complete')),
          created_ms INTEGER NOT NULL,
          completed_ms INTEGER
        );
        CREATE UNIQUE INDEX IF NOT EXISTS swarm_one_director_turn
          ON swarm_director_turns(run_id) WHERE status='active';
        CREATE TABLE IF NOT EXISTS swarm_director_turn_messages(
          turn_id TEXT NOT NULL REFERENCES swarm_director_turns(id),
          seq INTEGER NOT NULL REFERENCES swarm_messages(seq),
          PRIMARY KEY(turn_id,seq)
        );
        CREATE TABLE IF NOT EXISTS swarm_policy_settings(
          scope TEXT NOT NULL CHECK(scope IN ('application','category')),
          scope_key TEXT NOT NULL,
          policy TEXT NOT NULL,
          allowed_targets TEXT NOT NULL,
          updated_ms INTEGER NOT NULL,
          PRIMARY KEY(scope,scope_key)
        );
        "#,
    )?;
    Ok(())
}
