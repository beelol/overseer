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
          stop_reason TEXT,
          stalled_from TEXT,
          stall_reason TEXT,
          no_progress_turns INTEGER NOT NULL DEFAULT 0,
          failed_planning_turns INTEGER NOT NULL DEFAULT 0,
          generation INTEGER NOT NULL,
          revision INTEGER NOT NULL,
          allowed_targets TEXT NOT NULL,
          policy TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          updated_ms INTEGER NOT NULL
        );
        DROP INDEX IF EXISTS swarm_active_category;
        CREATE UNIQUE INDEX swarm_active_category
          ON swarm_runs(category_key)
          WHERE status IN ('planning','running','paused','stalled','draining','stopping');
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
          reviewed_message_seq INTEGER NOT NULL DEFAULT 0,
          created_ms INTEGER NOT NULL,
          UNIQUE(run_id,job_id,attempt_id,decision)
        );
        CREATE TABLE IF NOT EXISTS swarm_completions(
          run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id),
          request_sha256 TEXT NOT NULL,
          generation INTEGER NOT NULL,
          revision INTEGER NOT NULL,
          summary TEXT NOT NULL,
          verification TEXT NOT NULL,
          checks TEXT NOT NULL,
          created_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_director_turns(
          id TEXT PRIMARY KEY,
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          generation INTEGER NOT NULL,
          revision INTEGER NOT NULL,
          token_sha256 TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('active','complete')),
          accepted_decision_id_at_claim INTEGER NOT NULL DEFAULT 0,
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
        CREATE TABLE IF NOT EXISTS swarm_allocations(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          pool_id TEXT NOT NULL,
          window_id TEXT NOT NULL,
          unit TEXT NOT NULL,
          allocation_milli INTEGER NOT NULL,
          reserve_milli INTEGER NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,pool_id,window_id)
        );
        CREATE TABLE IF NOT EXISTS swarm_admissions(
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          request_id TEXT NOT NULL,
          request_sha256 TEXT NOT NULL,
          job_id TEXT NOT NULL,
          attempt_id TEXT NOT NULL REFERENCES swarm_attempts(id),
          target_id TEXT NOT NULL,
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(run_id,request_id)
        );
        CREATE TABLE IF NOT EXISTS swarm_reservations(
          attempt_id TEXT NOT NULL REFERENCES swarm_attempts(id),
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          pool_id TEXT NOT NULL,
          window_id TEXT NOT NULL,
          unit TEXT NOT NULL,
          amount_milli INTEGER NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('active','uncertain','reconciled')),
          created_ms INTEGER NOT NULL,
          PRIMARY KEY(attempt_id,pool_id,window_id)
        );
        CREATE INDEX IF NOT EXISTS swarm_reservations_pool ON swarm_reservations(pool_id,window_id,status);
        CREATE TABLE IF NOT EXISTS swarm_growth(
          run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id),
          wave_start_ms INTEGER NOT NULL,
          admitted_count INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_scheduler_cursor(
          id INTEGER PRIMARY KEY CHECK(id=1),
          last_category_key TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_scheduler_admissions(
          request_id TEXT PRIMARY KEY,
          request_sha256 TEXT NOT NULL,
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          job_id TEXT NOT NULL,
          attempt_id TEXT NOT NULL UNIQUE REFERENCES swarm_attempts(id),
          target_id TEXT NOT NULL,
          created_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_dispatch_intents(
          request_id TEXT PRIMARY KEY,
          request_sha256 TEXT NOT NULL,
          request_json TEXT NOT NULL,
          failure_injected INTEGER NOT NULL DEFAULT 0 CHECK(failure_injected IN (0,1)),
          created_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_review_gate(
          run_id TEXT PRIMARY KEY REFERENCES swarm_runs(id),
          held INTEGER NOT NULL CHECK(held IN (0,1))
        );
        CREATE TABLE IF NOT EXISTS swarm_worker_launches(
          attempt_id TEXT PRIMARY KEY REFERENCES swarm_attempts(id),
          run_id TEXT NOT NULL REFERENCES swarm_runs(id),
          job_id TEXT NOT NULL,
          request_sha256 TEXT NOT NULL,
          overseer_run_id TEXT UNIQUE REFERENCES runs(id),
          created_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS swarm_worker_liveness(
          attempt_id TEXT PRIMARY KEY REFERENCES swarm_attempts(id),
          state TEXT NOT NULL CHECK(state IN ('reachable','suspect','unknown')),
          unreachable_since_ms INTEGER,
          last_sample_ms INTEGER NOT NULL
        );
        "#,
    )?;
    let has_stop_reason = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='stop_reason'")?
        .exists([])?;
    if !has_stop_reason {
        conn.execute_batch("ALTER TABLE swarm_runs ADD COLUMN stop_reason TEXT;")?;
    }
    let has_stalled_from = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='stalled_from'")?
        .exists([])?;
    if !has_stalled_from {
        conn.execute_batch("ALTER TABLE swarm_runs ADD COLUMN stalled_from TEXT;")?;
    }
    let has_stall_reason = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='stall_reason'")?
        .exists([])?;
    if !has_stall_reason {
        conn.execute_batch("ALTER TABLE swarm_runs ADD COLUMN stall_reason TEXT;")?;
    }
    let has_no_progress_turns = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='no_progress_turns'")?
        .exists([])?;
    if !has_no_progress_turns {
        conn.execute_batch(
            "ALTER TABLE swarm_runs ADD COLUMN no_progress_turns INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    let has_failed_planning_turns = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_runs') WHERE name='failed_planning_turns'")?
        .exists([])?;
    if !has_failed_planning_turns {
        conn.execute_batch("ALTER TABLE swarm_runs ADD COLUMN failed_planning_turns INTEGER NOT NULL DEFAULT 0;")?;
    }
    let has_decision_snapshot = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_director_turns') WHERE name='accepted_decision_id_at_claim'")?
        .exists([])?;
    if !has_decision_snapshot {
        conn.execute_batch("ALTER TABLE swarm_director_turns ADD COLUMN accepted_decision_id_at_claim INTEGER NOT NULL DEFAULT 0;")?;
    }
    let has_reviewed_message_seq = conn
        .prepare("SELECT 1 FROM pragma_table_info('swarm_decisions') WHERE name='reviewed_message_seq'")?
        .exists([])?;
    if !has_reviewed_message_seq {
        conn.execute_batch("ALTER TABLE swarm_decisions ADD COLUMN reviewed_message_seq INTEGER NOT NULL DEFAULT 0;")?;
    }
    Ok(())
}
